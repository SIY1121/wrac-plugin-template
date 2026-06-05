mod checks;
mod clap_schema;
mod report;

use crate::Result;
use crate::context::Context;
use crate::profile::BuildProfile;
use crate::targets::ValidateTarget;

pub(crate) fn validate_wrac_rules(
    ctx: &Context,
    profile: BuildProfile,
    targets: &[ValidateTarget],
) -> Result<()> {
    checks::validate_disabled_rules(&ctx.metadata.validation)?;

    let clap = ctx.clap_bundle(profile);
    let schema = unsafe { clap_schema::read_clap_schema(ctx, profile, &clap)? };
    let results = checks::evaluate_checks(
        &schema,
        targets,
        &ctx.metadata.validation,
        &ctx.plugin_manifest(),
    );

    report::print_check_results(&results);
    let violations = report::failed_violations(&results);
    if violations.is_empty() {
        println!("WRAC production-readiness checks: passed");
        return Ok(());
    }

    Err(report::failure_message(&violations).into())
}
