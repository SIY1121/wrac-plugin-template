use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::ptr;

use clap_sys::entry::clap_plugin_entry;
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, CLAP_PARAM_IS_BYPASS, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_HIDDEN,
    CLAP_PARAM_IS_READONLY, CLAP_PARAM_IS_STEPPED, clap_param_info, clap_plugin_params,
};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::host::clap_host;
use clap_sys::version::CLAP_VERSION;
use libloading::Library;

use crate::context::Context;
use crate::metadata::ValidationMetadata;
use crate::profile::BuildProfile;
use crate::targets::{Platform, ValidateTarget};
use crate::{Result, targets::ValidateTarget as Target};

const RULE_FENDER_SINGLE_KNOB: &str = "fender-studio-pro-generic-editor-single-knob";
const RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX: &str = "luna-vst3-param-id-must-match-index";
const RULE_BYPASS_PARAM_SHAPE: &str = "bypass-param-shape";
const RULE_PLUGIN_REQUIRES_BYPASS: &str = "plugin-requires-bypass";

const KNOWN_RULES: &[&str] = &[
    RULE_FENDER_SINGLE_KNOB,
    RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX,
    RULE_BYPASS_PARAM_SHAPE,
    RULE_PLUGIN_REQUIRES_BYPASS,
];

pub(crate) fn validate_wrac_rules(
    ctx: &Context,
    profile: BuildProfile,
    targets: &[ValidateTarget],
) -> Result<()> {
    validate_disabled_rules(&ctx.metadata.validation)?;

    let clap = ctx.clap_bundle(profile);
    let schema = unsafe { read_clap_schema(ctx, profile, &clap)? };
    let results = evaluate_checks(
        &schema,
        targets,
        &ctx.metadata.validation,
        &ctx.plugin_manifest(),
    );
    print_check_results(&results);
    let violations = failed_violations(&results);

    if violations.is_empty() {
        println!("WRAC production-readiness checks: passed");
        return Ok(());
    }

    let mut message = String::from("WRAC production-readiness checks failed:\n");
    for violation in violations {
        let _ = writeln!(
            message,
            "\n{}:\n  error {}\n    {}\n    Fix: {}",
            violation.location.display(),
            violation.rule_id,
            violation.message,
            violation.fix
        );
    }
    Err(message.into())
}

fn validate_disabled_rules(validation: &ValidationMetadata) -> Result<()> {
    for rule_id in validation.disabled_rules.keys() {
        if !KNOWN_RULES.contains(&rule_id.as_str()) {
            return Err(format!(
                "unknown WRAC production-readiness rule in disabled_rules: {rule_id}"
            )
            .into());
        }
    }
    Ok(())
}

fn evaluate_checks(
    schema: &PluginSchema,
    targets: &[ValidateTarget],
    validation: &ValidationMetadata,
    location: &Path,
) -> Vec<CheckResult> {
    let hidden_or_readonly = |param: &&ParameterSchema| {
        param.flags.contains(CLAP_PARAM_IS_HIDDEN) || param.flags.contains(CLAP_PARAM_IS_READONLY)
    };
    let visible_non_bypass_count = schema
        .params
        .iter()
        .filter(|param| !hidden_or_readonly(param) && !param.flags.contains(CLAP_PARAM_IS_BYPASS))
        .count();
    let bypass_params = schema
        .params
        .iter()
        .filter(|param| param.flags.contains(CLAP_PARAM_IS_BYPASS))
        .collect::<Vec<_>>();

    let mut results = Vec::new();

    if targets
        .iter()
        .any(|target| matches!(target, Target::Clap | Target::Vst3))
    {
        let violations = if visible_non_bypass_count == 1 {
            vec![RuleViolation {
                location: location.to_path_buf(),
                rule_id: RULE_FENDER_SINGLE_KNOB,
                message: format!(
                    "Fender Studio Pro generic editors fail to render knobs when exactly one visible non-bypass parameter is exposed. visible_non_bypass_parameter_count={visible_non_bypass_count}"
                ),
                fix: "Expose zero or at least two visible non-bypass parameters, or disable this rule with a documented reason.",
            }]
        } else {
            Vec::new()
        };
        push_check_result(
            &mut results,
            validation,
            RULE_FENDER_SINGLE_KNOB,
            CheckStatus::from_violations(violations),
        );
    } else {
        push_check_result(
            &mut results,
            validation,
            RULE_FENDER_SINGLE_KNOB,
            CheckStatus::Skipped("CLAP or VST3 validation was not requested."),
        );
    }

    if targets.contains(&Target::Vst3) {
        let mut violations = Vec::new();
        for (index, param) in schema.params.iter().enumerate() {
            if param.id != index as u32 {
                violations.push(RuleViolation {
                    location: location.to_path_buf(),
                    rule_id: RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX,
                    message: format!(
                        "LUNA 2.0.3.4381 VST3 automation writes fail when ParamID differs from parameter index. index={index} id={} name=\"{}\"",
                        param.id, param.name
                    ),
                    fix: "Keep public VST3 parameter IDs equal to their parameter-list indices.",
                });
            }
        }
        push_check_result(
            &mut results,
            validation,
            RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX,
            CheckStatus::from_violations(violations),
        );
    } else {
        push_check_result(
            &mut results,
            validation,
            RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX,
            CheckStatus::Skipped("VST3 validation was not requested."),
        );
    }

    let mut bypass_shape_violations = Vec::new();
    if bypass_params.len() > 1 {
        bypass_shape_violations.push(RuleViolation {
            location: location.to_path_buf(),
            rule_id: RULE_BYPASS_PARAM_SHAPE,
            message: format!(
                "Only one bypass parameter may be exposed. bypass_parameter_count={}",
                bypass_params.len()
            ),
            fix: "Expose a single host bypass parameter.",
        });
    }
    for param in bypass_params {
        let stepped = param.flags.contains(CLAP_PARAM_IS_STEPPED);
        let enum_flag = param.flags.contains(CLAP_PARAM_IS_ENUM);
        let default_is_boolean =
            nearly_equal(param.default_value, 0.0) || nearly_equal(param.default_value, 1.0);
        if !stepped
            || !enum_flag
            || !nearly_equal(param.min_value, 0.0)
            || !nearly_equal(param.max_value, 1.0)
            || !default_is_boolean
        {
            bypass_shape_violations.push(RuleViolation {
                location: location.to_path_buf(),
                rule_id: RULE_BYPASS_PARAM_SHAPE,
                message: format!(
                    "Bypass parameter must be a stepped enum with range 0..1 and a boolean default. id={} name=\"{}\" stepped={stepped} enum={enum_flag} min={} max={} default={}",
                    param.id, param.name, param.min_value, param.max_value, param.default_value
                ),
                fix: "Set bypass flags to stepped + enum + bypass, min=0, max=1, and default=0 or 1.",
            });
        }
    }
    push_check_result(
        &mut results,
        validation,
        RULE_BYPASS_PARAM_SHAPE,
        CheckStatus::from_violations(bypass_shape_violations),
    );

    let bypass_required_violations = if schema
        .params
        .iter()
        .all(|param| !param.flags.contains(CLAP_PARAM_IS_BYPASS))
    {
        vec![RuleViolation {
            location: location.to_path_buf(),
            rule_id: RULE_PLUGIN_REQUIRES_BYPASS,
            message: "Production plugins should expose a host bypass parameter.".to_string(),
            fix: "Add one bypass parameter, or disable this rule with a documented reason.",
        }]
    } else {
        Vec::new()
    };
    push_check_result(
        &mut results,
        validation,
        RULE_PLUGIN_REQUIRES_BYPASS,
        CheckStatus::from_violations(bypass_required_violations),
    );

    results
}

fn push_check_result(
    results: &mut Vec<CheckResult>,
    validation: &ValidationMetadata,
    rule_id: &'static str,
    status: CheckStatus,
) {
    if let Some(disabled) = validation.disabled_rules.get(rule_id) {
        results.push(CheckResult {
            rule_id,
            status: CheckStatus::Disabled(disabled.reason.clone()),
        });
        return;
    }
    results.push(CheckResult { rule_id, status });
}

fn print_check_results(results: &[CheckResult]) {
    println!("WRAC production-readiness checks:");
    for result in results {
        match &result.status {
            CheckStatus::Passed => println!("  pass     {}", result.rule_id),
            CheckStatus::Skipped(reason) => {
                println!("  skipped  {}", result.rule_id);
                println!("           reason: {reason}");
            }
            CheckStatus::Disabled(reason) => {
                println!("  disabled {}", result.rule_id);
                println!("           reason: {reason}");
            }
            CheckStatus::Failed(violations) => {
                println!("  fail     {}", result.rule_id);
                for violation in violations {
                    println!("           {}", violation.message);
                    println!("           Fix: {}", violation.fix);
                }
            }
        }
    }
}

fn failed_violations(results: &[CheckResult]) -> Vec<&RuleViolation> {
    results
        .iter()
        .flat_map(|result| match &result.status {
            CheckStatus::Failed(violations) => violations.iter().collect::<Vec<_>>(),
            CheckStatus::Passed | CheckStatus::Skipped(_) | CheckStatus::Disabled(_) => Vec::new(),
        })
        .collect()
}

fn nearly_equal(a: f64, b: f64) -> bool {
    (a - b).abs() < f64::EPSILON
}

#[derive(Debug)]
struct CheckResult {
    rule_id: &'static str,
    status: CheckStatus,
}

#[derive(Debug)]
enum CheckStatus {
    Passed,
    Failed(Vec<RuleViolation>),
    Skipped(&'static str),
    Disabled(String),
}

impl CheckStatus {
    fn from_violations(violations: Vec<RuleViolation>) -> Self {
        if violations.is_empty() {
            Self::Passed
        } else {
            Self::Failed(violations)
        }
    }
}

#[derive(Debug)]
struct RuleViolation {
    location: PathBuf,
    rule_id: &'static str,
    message: String,
    fix: &'static str,
}

#[derive(Debug)]
struct PluginSchema {
    params: Vec<ParameterSchema>,
}

#[derive(Debug)]
struct ParameterSchema {
    id: u32,
    name: String,
    flags: u32,
    min_value: f64,
    max_value: f64,
    default_value: f64,
}

unsafe fn read_clap_schema(
    ctx: &Context,
    profile: BuildProfile,
    clap_bundle: &Path,
) -> Result<PluginSchema> {
    let library_path = clap_library_path(ctx, profile);
    let plugin_path = CString::new(clap_bundle.to_string_lossy().as_bytes())?;
    let library = unsafe { Library::new(&library_path) }?;
    let get_entry = unsafe { library.get::<unsafe extern "C" fn() -> usize>(b"get_clap_entry") }?;
    let entry = unsafe { get_entry() as *const clap_plugin_entry };
    if entry.is_null() {
        return Err("CLAP entry returned a null pointer".into());
    }

    let init = unsafe { (*entry).init }.ok_or("CLAP entry has no init callback")?;
    if !unsafe { init(plugin_path.as_ptr()) } {
        return Err("CLAP entry init failed".into());
    }
    let _entry_guard = ClapEntryGuard { entry };

    let get_factory =
        unsafe { (*entry).get_factory }.ok_or("CLAP entry has no get_factory callback")?;
    let factory =
        unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) as *const clap_plugin_factory };
    if factory.is_null() {
        return Err("CLAP plugin factory is not available".into());
    }

    let descriptor = unsafe { first_plugin_descriptor(factory) }?;
    let plugin_id = unsafe { CStr::from_ptr(descriptor.id) };
    let create_plugin =
        unsafe { (*factory).create_plugin }.ok_or("CLAP factory has no create_plugin callback")?;
    let host = validator_clap_host();
    let plugin = unsafe { create_plugin(factory, &host, plugin_id.as_ptr()) };
    if plugin.is_null() {
        return Err(format!(
            "CLAP factory failed to create plugin id={}",
            plugin_id.to_string_lossy()
        )
        .into());
    }
    let _plugin_guard = ClapPluginGuard { plugin };

    if let Some(init_plugin) = unsafe { (*plugin).init } {
        if !unsafe { init_plugin(plugin) } {
            return Err("CLAP plugin init failed".into());
        }
    }

    let params = unsafe { read_params(plugin) }?;
    Ok(PluginSchema { params })
}

fn validator_clap_host() -> clap_host {
    clap_host {
        clap_version: CLAP_VERSION,
        host_data: ptr::null_mut(),
        name: c"WRAC xtask checks".as_ptr(),
        vendor: c"WRAC".as_ptr(),
        url: c"https://github.com/novonotes/wrac-plugin-template".as_ptr(),
        version: c"0".as_ptr(),
        get_extension: Some(validator_host_get_extension),
        request_restart: Some(validator_host_request_restart),
        request_process: Some(validator_host_request_process),
        request_callback: Some(validator_host_request_callback),
    }
}

unsafe extern "C" fn validator_host_get_extension(
    _host: *const clap_host,
    _extension_id: *const c_char,
) -> *const c_void {
    ptr::null()
}

unsafe extern "C" fn validator_host_request_restart(_host: *const clap_host) {}

unsafe extern "C" fn validator_host_request_process(_host: *const clap_host) {}

unsafe extern "C" fn validator_host_request_callback(_host: *const clap_host) {}

unsafe fn first_plugin_descriptor(
    factory: *const clap_plugin_factory,
) -> Result<&'static clap_sys::plugin::clap_plugin_descriptor> {
    let count = unsafe { (*factory).get_plugin_count }
        .ok_or("CLAP factory has no get_plugin_count callback")?;
    if unsafe { count(factory) } == 0 {
        return Err("CLAP factory contains no plugins".into());
    }
    let get_descriptor = unsafe { (*factory).get_plugin_descriptor }
        .ok_or("CLAP factory has no get_plugin_descriptor callback")?;
    let descriptor = unsafe { get_descriptor(factory, 0) };
    if descriptor.is_null() {
        return Err("CLAP factory returned a null descriptor".into());
    }
    Ok(unsafe { &*descriptor })
}

unsafe fn read_params(
    plugin: *const clap_sys::plugin::clap_plugin,
) -> Result<Vec<ParameterSchema>> {
    let get_extension =
        unsafe { (*plugin).get_extension }.ok_or("CLAP plugin has no get_extension callback")?;
    let params =
        unsafe { get_extension(plugin, CLAP_EXT_PARAMS.as_ptr()) as *const clap_plugin_params };
    if params.is_null() {
        return Ok(Vec::new());
    }
    let count = unsafe { (*params).count }.ok_or("CLAP params extension has no count callback")?;
    let get_info =
        unsafe { (*params).get_info }.ok_or("CLAP params extension has no get_info callback")?;
    let mut result = Vec::new();
    for index in 0..unsafe { count(plugin) } {
        let mut info = clap_param_info {
            id: 0,
            flags: 0,
            cookie: ptr::null_mut(),
            name: [0; clap_sys::string_sizes::CLAP_NAME_SIZE],
            module: [0; clap_sys::string_sizes::CLAP_PATH_SIZE],
            min_value: 0.0,
            max_value: 0.0,
            default_value: 0.0,
        };
        if !unsafe { get_info(plugin, index, &mut info) } {
            return Err(format!("CLAP params.get_info failed for index {index}").into());
        }
        result.push(ParameterSchema {
            id: info.id,
            name: c_char_array_to_string(&info.name),
            flags: info.flags,
            min_value: info.min_value,
            max_value: info.max_value,
            default_value: info.default_value,
        });
    }
    Ok(result)
}

fn c_char_array_to_string(buffer: &[std::ffi::c_char]) -> String {
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

fn clap_library_path(ctx: &Context, profile: BuildProfile) -> PathBuf {
    match ctx.platform {
        Platform::Macos => ctx
            .clap_bundle(profile)
            .join("Contents")
            .join("MacOS")
            .join(&ctx.metadata.bundle_name),
        Platform::Windows | Platform::Linux => ctx.clap_bundle(profile),
    }
}

struct ClapEntryGuard {
    entry: *const clap_plugin_entry,
}

impl Drop for ClapEntryGuard {
    fn drop(&mut self) {
        if let Some(deinit) = unsafe { (*self.entry).deinit } {
            unsafe { deinit() };
        }
    }
}

struct ClapPluginGuard {
    plugin: *const clap_sys::plugin::clap_plugin,
}

impl Drop for ClapPluginGuard {
    fn drop(&mut self) {
        if let Some(destroy) = unsafe { (*self.plugin).destroy } {
            unsafe { destroy(self.plugin) };
        }
    }
}

trait FlagContains {
    fn contains(self, flag: u32) -> bool;
}

impl FlagContains for u32 {
    fn contains(self, flag: u32) -> bool {
        self & flag != 0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use crate::metadata::{DisabledValidationRule, ValidationMetadata};
    use crate::targets::ValidateTarget;

    use super::*;

    fn schema(params: Vec<ParameterSchema>) -> PluginSchema {
        PluginSchema { params }
    }

    fn param(id: u32, flags: u32) -> ParameterSchema {
        ParameterSchema {
            id,
            name: format!("Param {id}"),
            flags,
            min_value: 0.0,
            max_value: 1.0,
            default_value: 0.0,
        }
    }

    fn no_disabled_rules() -> ValidationMetadata {
        ValidationMetadata::default()
    }

    fn status_for<'a>(results: &'a [CheckResult], rule_id: &str) -> &'a CheckStatus {
        &results
            .iter()
            .find(|result| result.rule_id == rule_id)
            .expect("rule result should exist")
            .status
    }

    fn rule_failed(results: &[CheckResult], rule_id: &str) -> bool {
        matches!(status_for(results, rule_id), CheckStatus::Failed(_))
    }

    fn valid_bypass_param(id: u32) -> ParameterSchema {
        param(
            id,
            CLAP_PARAM_IS_BYPASS | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM,
        )
    }

    #[test]
    fn single_visible_non_bypass_parameter_fails_for_clap_and_vst3() {
        let results = evaluate_checks(
            &schema(vec![param(0, 0), param(1, CLAP_PARAM_IS_BYPASS)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_FENDER_SINGLE_KNOB));
    }

    #[test]
    fn single_visible_non_bypass_parameter_is_skipped_for_au_only() {
        let results = evaluate_checks(
            &schema(vec![param(0, 0), valid_bypass_param(1)]),
            &[ValidateTarget::Au],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_FENDER_SINGLE_KNOB),
            CheckStatus::Skipped(_)
        ));
    }

    #[test]
    fn zero_visible_non_bypass_parameters_are_allowed() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_FENDER_SINGLE_KNOB),
            CheckStatus::Passed
        ));
    }

    #[test]
    fn two_visible_non_bypass_parameters_are_allowed() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0), param(1, 0), param(2, 0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_FENDER_SINGLE_KNOB),
            CheckStatus::Passed
        ));
    }

    #[test]
    fn hidden_readonly_and_bypass_parameters_do_not_count_as_visible_knobs() {
        let results = evaluate_checks(
            &schema(vec![
                valid_bypass_param(0),
                param(1, CLAP_PARAM_IS_HIDDEN),
                param(2, CLAP_PARAM_IS_READONLY),
            ]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_FENDER_SINGLE_KNOB),
            CheckStatus::Passed
        ));
    }

    #[test]
    fn disabled_rules_are_reported() {
        let mut disabled_rules = HashMap::new();
        disabled_rules.insert(
            RULE_FENDER_SINGLE_KNOB.to_string(),
            DisabledValidationRule {
                reason: "not a supported host workflow".to_string(),
            },
        );
        let validation = ValidationMetadata { disabled_rules };
        let results = evaluate_checks(
            &schema(vec![param(0, 0), param(1, CLAP_PARAM_IS_BYPASS)]),
            &[ValidateTarget::Clap],
            &validation,
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_FENDER_SINGLE_KNOB),
            CheckStatus::Disabled(reason) if reason == "not a supported host workflow"
        ));
    }

    #[test]
    fn vst3_param_id_must_match_index() {
        let results = evaluate_checks(
            &schema(vec![param(1, 0), param(2, 0)]),
            &[ValidateTarget::Vst3],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX));
    }

    #[test]
    fn vst3_only_rule_is_skipped_without_vst3_target() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX),
            CheckStatus::Skipped(_)
        ));
    }

    #[test]
    fn vst3_param_ids_matching_indices_pass() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0), param(1, 0)]),
            &[ValidateTarget::Vst3],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_LUNA_VST3_PARAM_ID_MATCH_INDEX),
            CheckStatus::Passed
        ));
    }

    #[test]
    fn bypass_shape_requires_stepped_flag() {
        let results = evaluate_checks(
            &schema(vec![param(0, CLAP_PARAM_IS_BYPASS | CLAP_PARAM_IS_ENUM)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_BYPASS_PARAM_SHAPE));
    }

    #[test]
    fn bypass_shape_requires_enum_flag() {
        let results = evaluate_checks(
            &schema(vec![param(0, CLAP_PARAM_IS_BYPASS | CLAP_PARAM_IS_STEPPED)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_BYPASS_PARAM_SHAPE));
    }

    #[test]
    fn bypass_shape_requires_boolean_range() {
        let mut bypass = valid_bypass_param(0);
        bypass.max_value = 2.0;
        let results = evaluate_checks(
            &schema(vec![bypass]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_BYPASS_PARAM_SHAPE));
    }

    #[test]
    fn bypass_shape_requires_boolean_default() {
        let mut bypass = valid_bypass_param(0);
        bypass.default_value = 0.5;
        let results = evaluate_checks(
            &schema(vec![bypass]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_BYPASS_PARAM_SHAPE));
    }

    #[test]
    fn bypass_shape_allows_one_valid_bypass_parameter() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_BYPASS_PARAM_SHAPE),
            CheckStatus::Passed
        ));
    }

    #[test]
    fn bypass_shape_rejects_multiple_bypass_parameters() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0), valid_bypass_param(1)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_BYPASS_PARAM_SHAPE));
    }

    #[test]
    fn plugin_requires_bypass() {
        let results = evaluate_checks(
            &schema(Vec::new()),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_PLUGIN_REQUIRES_BYPASS));
    }

    #[test]
    fn plugin_requires_bypass_when_only_non_bypass_parameters_exist() {
        let results = evaluate_checks(
            &schema(vec![param(0, 0), param(1, 0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(rule_failed(&results, RULE_PLUGIN_REQUIRES_BYPASS));
    }

    #[test]
    fn plugin_requires_bypass_passes_with_valid_bypass_parameter() {
        let results = evaluate_checks(
            &schema(vec![valid_bypass_param(0)]),
            &[ValidateTarget::Clap],
            &no_disabled_rules(),
            Path::new("Cargo.toml"),
        );
        assert!(matches!(
            status_for(&results, RULE_PLUGIN_REQUIRES_BYPASS),
            CheckStatus::Passed
        ));
    }
}
