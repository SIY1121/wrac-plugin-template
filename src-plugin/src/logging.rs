//! Product-specific wrapper around the shared WRAC logger.

use std::path::{Path, PathBuf};

use wrac_log::{FileLoggerConfig, RtDrainConfig};

pub(crate) fn init_debug_logging_once(app_name: &str) {
    let log_file = default_log_file(app_name);
    wrac_log::init_file_logger_once(
        FileLoggerConfig::new(app_name, log_file).with_stderr_prefix("wrac_gain_plugin"),
    );

    if cfg!(debug_assertions) || std::env::var_os("WRAC_RT_LOG").is_some() {
        wrac_log::init_rt_log_drain_once(RtDrainConfig::default());
    }
}

fn default_log_file(app_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.log")
        .join(format!("{} Latest.log", sanitize_file_stem(app_name)))
}

fn sanitize_file_stem(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized.is_empty() {
        "Plugin".to_string()
    } else {
        sanitized
    }
}
