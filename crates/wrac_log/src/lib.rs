//! Logging utilities for WRAC plugins.
//!
//! Regular logs are written through the `log` facade. Realtime audio threads must use
//! the `rt*` macros with an [`RtLogWriter`], which writes into a fixed-size buffer
//! drained later from a non-realtime worker.

mod file_logger;
mod rt;

pub use file_logger::{
    RecentLogFilesOptions, collect_recent_log_files, current_log_dir, current_log_file, init_impl,
    init_test,
};
pub use rt::{RtDrainConfig, RtLog, RtLogWriter, drain_rt_logs_once, init_rt_log_drain_once};

#[macro_export]
macro_rules! init {
    ($app_name:expr) => {
        $crate::init_impl(option_env!("CARGO_MANIFEST_DIR"), $app_name)
    };
}

#[macro_export]
macro_rules! rttrace {
    ($writer:expr, $block_seq:expr, $sample_time:expr, $($arg:tt)+) => {{
        (&$writer).write_fmt(
            log::Level::Trace,
            module_path!(),
            $block_seq,
            $sample_time,
            format_args!($($arg)+),
        );
    }};
}

#[macro_export]
macro_rules! rtdebug {
    ($writer:expr, $block_seq:expr, $sample_time:expr, $($arg:tt)+) => {{
        (&$writer).write_fmt(
            log::Level::Debug,
            module_path!(),
            $block_seq,
            $sample_time,
            format_args!($($arg)+),
        );
    }};
}

#[macro_export]
macro_rules! rtinfo {
    ($writer:expr, $block_seq:expr, $sample_time:expr, $($arg:tt)+) => {{
        (&$writer).write_fmt(
            log::Level::Info,
            module_path!(),
            $block_seq,
            $sample_time,
            format_args!($($arg)+),
        );
    }};
}

#[macro_export]
macro_rules! rtwarn {
    ($writer:expr, $block_seq:expr, $sample_time:expr, $($arg:tt)+) => {{
        (&$writer).write_fmt(
            log::Level::Warn,
            module_path!(),
            $block_seq,
            $sample_time,
            format_args!($($arg)+),
        );
    }};
}

#[macro_export]
macro_rules! rterror {
    ($writer:expr, $block_seq:expr, $sample_time:expr, $($arg:tt)+) => {{
        (&$writer).write_fmt(
            log::Level::Error,
            module_path!(),
            $block_seq,
            $sample_time,
            format_args!($($arg)+),
        );
    }};
}
