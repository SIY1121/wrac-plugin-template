//! WRAC plugin 用の logging 基盤。
//!
//! 通常 logger は非 realtime thread 用の同期 file logger として実装する。
//! realtime thread からは `rtdebug!` 系 macro で固定長 ring buffer へ書き込み、
//! 非 realtime の drain worker が通常 logger へ流す。

use std::array;
use std::fmt::{self, Write as _};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock, Weak};
use std::thread;
use std::time::Duration;

use log::{Level, LevelFilter, Log, Metadata, Record};
use time::{OffsetDateTime, macros::format_description};

const RT_LOG_CAPACITY: usize = 4096;
const RT_MESSAGE_CAPACITY: usize = 256;
const RT_TARGET_CAPACITY: usize = 96;

static FILE_LOGGER_INIT: Once = Once::new();
static FILE_LOGGER: FileLogger = FileLogger {
    file: Mutex::new(None),
    level: Mutex::new(LevelFilter::Debug),
};
static RT_REGISTRY: OnceLock<RtRegistry> = OnceLock::new();
static RT_DRAIN_WORKER: OnceLock<()> = OnceLock::new();

pub struct FileLoggerConfig {
    app_name: String,
    log_file: PathBuf,
    level: LevelFilter,
    stderr_prefix: String,
}

impl FileLoggerConfig {
    pub fn new(app_name: impl Into<String>, log_file: impl Into<PathBuf>) -> Self {
        Self {
            app_name: app_name.into(),
            log_file: log_file.into(),
            level: std::env::var("RUST_LOG")
                .ok()
                .and_then(|value| parse_level_filter(&value))
                .unwrap_or(LevelFilter::Debug),
            stderr_prefix: "wrac".to_string(),
        }
    }

    pub fn with_stderr_prefix(mut self, stderr_prefix: impl Into<String>) -> Self {
        self.stderr_prefix = stderr_prefix.into();
        self
    }

    pub fn with_level(mut self, level: LevelFilter) -> Self {
        self.level = level;
        self
    }
}

pub struct RtDrainConfig {
    interval: Duration,
}

impl Default for RtDrainConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(100),
        }
    }
}

impl RtDrainConfig {
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

pub fn init_file_logger_once(config: FileLoggerConfig) {
    FILE_LOGGER_INIT.call_once(|| {
        let file = open_log_file(&config.log_file, &config.stderr_prefix);

        if let Ok(mut logger_file) = FILE_LOGGER.file.lock() {
            *logger_file = file;
        }
        if let Ok(mut logger_level) = FILE_LOGGER.level.lock() {
            *logger_level = config.level;
        }

        if log::set_logger(&FILE_LOGGER).is_ok() {
            log::set_max_level(config.level);
            eprintln!(
                "[{}] debug log: {}",
                config.stderr_prefix,
                config.log_file.display()
            );
            FILE_LOGGER.write_session_header(&config.app_name);
        }
    });
}

pub fn init_rt_log_drain_once(config: RtDrainConfig) {
    RT_DRAIN_WORKER.get_or_init(|| {
        let interval = config.interval;
        let _ = thread::Builder::new()
            .name("wrac-rt-log-drain".to_string())
            .spawn(move || {
                loop {
                    thread::sleep(interval);
                    drain_rt_logs_once();
                }
            });
    });
}

pub fn drain_rt_logs_once() {
    rt_registry().drain_all();
}

pub struct RtLog {
    inner: Arc<RtLogInner>,
}

impl RtLog {
    pub fn new_registered(name: &'static str) -> Self {
        let inner = Arc::new(RtLogInner::new(name));
        rt_registry().register(&inner);
        Self { inner }
    }

    pub fn writer(&self) -> RtLogWriter {
        RtLogWriter {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for RtLog {
    fn drop(&mut self) {
        self.inner.drain_to_log();
        rt_registry().unregister(&self.inner);
    }
}

#[derive(Clone)]
pub struct RtLogWriter {
    inner: Arc<RtLogInner>,
}

impl RtLogWriter {
    #[doc(hidden)]
    pub fn write_fmt(
        &self,
        level: Level,
        target: &'static str,
        block_seq: u64,
        sample_time: u32,
        args: fmt::Arguments<'_>,
    ) {
        self.inner
            .write_fmt(level, target, block_seq, sample_time, args);
    }
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

struct RtRegistry {
    logs: Mutex<Vec<Weak<RtLogInner>>>,
}

impl RtRegistry {
    fn register(&self, log: &Arc<RtLogInner>) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(Arc::downgrade(log));
        }
    }

    fn unregister(&self, log: &Arc<RtLogInner>) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.retain(|registered| {
                registered
                    .upgrade()
                    .is_some_and(|inner| !Arc::ptr_eq(&inner, log))
            });
        }
    }

    fn drain_all(&self) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.retain(|registered| {
                let Some(log) = registered.upgrade() else {
                    return false;
                };
                log.drain_to_log();
                true
            });
        }
    }
}

fn rt_registry() -> &'static RtRegistry {
    RT_REGISTRY.get_or_init(|| RtRegistry {
        logs: Mutex::new(Vec::new()),
    })
}

struct RtLogInner {
    name: &'static str,
    next_sequence: AtomicU64,
    drain_sequence: AtomicU64,
    dropped: AtomicU64,
    slots: Vec<RtLogSlot>,
}

impl RtLogInner {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            next_sequence: AtomicU64::new(0),
            drain_sequence: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            // 固定長 slot を heap に置き、plugin instance 作成時の stack overflow を避ける。
            slots: (0..RT_LOG_CAPACITY).map(|_| RtLogSlot::new()).collect(),
        }
    }

    fn write_fmt(
        &self,
        level: Level,
        target: &'static str,
        block_seq: u64,
        sample_time: u32,
        args: fmt::Arguments<'_>,
    ) {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let drain_sequence = self.drain_sequence.load(Ordering::Acquire);
        if sequence.saturating_sub(drain_sequence) >= RT_LOG_CAPACITY as u64 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }

        let slot = &self.slots[sequence as usize % RT_LOG_CAPACITY];
        slot.write(sequence, level, target, block_seq, sample_time, args);
    }

    fn drain_to_log(&self) {
        let total = self.next_sequence.load(Ordering::Acquire);
        let retained_start = total.saturating_sub(RT_LOG_CAPACITY as u64);
        let start = self
            .drain_sequence
            .load(Ordering::Acquire)
            .max(retained_start);

        let dropped = self.dropped.swap(0, Ordering::AcqRel);
        if dropped > 0 || start > self.drain_sequence.load(Ordering::Acquire) {
            log::warn!(
                target: "wrac_log::rt",
                "[rt] name={} dropped={} skipped={}",
                self.name,
                dropped,
                start.saturating_sub(self.drain_sequence.load(Ordering::Acquire)),
            );
        }

        let mut drained_until = start;
        for sequence in start..total {
            if let Some(record) = self.slots[sequence as usize % RT_LOG_CAPACITY].read(sequence) {
                log::log!(
                    target: record.target.as_str(),
                    record.level,
                    "[rt] name={} seq={} block={} sample={} {}",
                    self.name,
                    record.sequence,
                    record.block_seq,
                    record.sample_time,
                    record.message.as_str(),
                );
                drained_until = sequence + 1;
            } else {
                // writer は sequence number を先に予約してから slot を publish する。
                // ここで先へ進むと、直後に publish された record を永久に読み飛ばすため、
                // drain_sequence は連続して読めた位置までしか進めない。
                break;
            }
        }
        self.drain_sequence.store(drained_until, Ordering::Release);
    }
}

struct RtLogSlot {
    sequence: AtomicU64,
    level: AtomicU8,
    block_seq: AtomicU64,
    sample_time: AtomicU32,
    target_len: AtomicUsize,
    target: [AtomicU8; RT_TARGET_CAPACITY],
    message_len: AtomicUsize,
    message: [AtomicU8; RT_MESSAGE_CAPACITY],
}

impl RtLogSlot {
    fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            level: AtomicU8::new(level_to_u8(Level::Debug)),
            block_seq: AtomicU64::new(0),
            sample_time: AtomicU32::new(0),
            target_len: AtomicUsize::new(0),
            target: array::from_fn(|_| AtomicU8::new(0)),
            message_len: AtomicUsize::new(0),
            message: array::from_fn(|_| AtomicU8::new(0)),
        }
    }

    fn write(
        &self,
        sequence: u64,
        level: Level,
        target: &str,
        block_seq: u64,
        sample_time: u32,
        args: fmt::Arguments<'_>,
    ) {
        self.sequence.store(0, Ordering::Release);
        self.level.store(level_to_u8(level), Ordering::Relaxed);
        self.block_seq.store(block_seq, Ordering::Relaxed);
        self.sample_time.store(sample_time, Ordering::Relaxed);
        write_atomic_bytes(&self.target, &self.target_len, target.as_bytes());

        let mut message = FixedMessage::new();
        let _ = message.write_fmt(args);
        write_atomic_bytes(&self.message, &self.message_len, message.as_bytes());
        self.sequence.store(sequence + 1, Ordering::Release);
    }

    fn read(&self, sequence: u64) -> Option<RtLogRecord> {
        if self.sequence.load(Ordering::Acquire) != sequence + 1 {
            return None;
        }

        let record = RtLogRecord {
            sequence,
            level: u8_to_level(self.level.load(Ordering::Relaxed)),
            block_seq: self.block_seq.load(Ordering::Relaxed),
            sample_time: self.sample_time.load(Ordering::Relaxed),
            target: read_atomic_string::<RT_TARGET_CAPACITY>(&self.target, &self.target_len),
            message: read_atomic_string::<RT_MESSAGE_CAPACITY>(&self.message, &self.message_len),
        };

        if self.sequence.load(Ordering::Acquire) == sequence + 1 {
            Some(record)
        } else {
            None
        }
    }
}

struct RtLogRecord {
    sequence: u64,
    level: Level,
    block_seq: u64,
    sample_time: u32,
    target: FixedString<RT_TARGET_CAPACITY>,
    message: FixedString<RT_MESSAGE_CAPACITY>,
}

struct FixedMessage {
    bytes: [u8; RT_MESSAGE_CAPACITY],
    len: usize,
}

impl FixedMessage {
    fn new() -> Self {
        Self {
            bytes: [0; RT_MESSAGE_CAPACITY],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl fmt::Write for FixedMessage {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = RT_MESSAGE_CAPACITY.saturating_sub(self.len);
        let count = utf8_boundary_len(value, remaining);
        self.bytes[self.len..self.len + count].copy_from_slice(&value.as_bytes()[..count]);
        self.len += count;
        Ok(())
    }
}

fn utf8_boundary_len(value: &str, limit: usize) -> usize {
    if value.len() <= limit {
        return value.len();
    }
    let mut count = limit.min(value.len());
    while count > 0 && !value.is_char_boundary(count) {
        count -= 1;
    }
    count
}

struct FixedString<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> FixedString<N> {
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or("<invalid utf8>")
    }
}

fn write_atomic_bytes<const N: usize>(target: &[AtomicU8; N], len: &AtomicUsize, bytes: &[u8]) {
    let count = N.min(bytes.len());
    for index in 0..count {
        target[index].store(bytes[index], Ordering::Relaxed);
    }
    len.store(count, Ordering::Relaxed);
}

fn read_atomic_string<const N: usize>(source: &[AtomicU8; N], len: &AtomicUsize) -> FixedString<N> {
    let len = len.load(Ordering::Relaxed).min(N);
    let mut bytes = [0; N];
    for index in 0..len {
        bytes[index] = source[index].load(Ordering::Relaxed);
    }
    FixedString { bytes, len }
}

struct FileLogger {
    file: Mutex<Option<File>>,
    level: Mutex<LevelFilter>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format!(
            "{} [{}] {} - {}\n",
            local_timestamp_millis(),
            record.level(),
            record.target(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());

        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

impl FileLogger {
    fn level(&self) -> LevelFilter {
        self.level
            .lock()
            .map(|level| *level)
            .unwrap_or(LevelFilter::Off)
    }

    fn write_session_header(&self, app_name: &str) {
        let line = format!(
            "\n================ {} session started at {} ================\n",
            app_name,
            local_timestamp_millis()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());

        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}

fn open_log_file(path: &Path, stderr_prefix: &str) -> Option<File> {
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "[{stderr_prefix}] failed to create log directory '{}': {error}",
            parent.display()
        );
        return None;
    }

    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Some(file),
        Err(error) => {
            eprintln!(
                "[{stderr_prefix}] failed to open log file '{}': {error}",
                path.display()
            );
            None
        }
    }
}

fn parse_level_filter(value: &str) -> Option<LevelFilter> {
    let value = value
        .rsplit(',')
        .next()
        .and_then(|directive| directive.rsplit('=').next())
        .unwrap_or(value)
        .trim();

    match value.to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

fn local_timestamp_millis() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let format =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");
    now.format(format)
        .unwrap_or_else(|_| format!("{}.{:03}", now.unix_timestamp(), now.millisecond()))
}

const fn level_to_u8(level: Level) -> u8 {
    match level {
        Level::Error => 1,
        Level::Warn => 2,
        Level::Info => 3,
        Level::Debug => 4,
        Level::Trace => 5,
    }
}

fn u8_to_level(level: u8) -> Level {
    match level {
        1 => Level::Error,
        2 => Level::Warn,
        3 => Level::Info,
        5 => Level::Trace,
        _ => Level::Debug,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_stops_before_unpublished_slot() {
        let log = RtLogInner::new("test");
        log.next_sequence.store(1, Ordering::Release);

        log.drain_to_log();
        assert_eq!(log.drain_sequence.load(Ordering::Acquire), 0);

        log.slots[0].write(0, Level::Debug, "test", 10, 20, format_args!("published"));
        log.drain_to_log();
        assert_eq!(log.drain_sequence.load(Ordering::Acquire), 1);
    }

    #[test]
    fn fixed_message_truncates_at_utf8_boundary() {
        let mut message = FixedMessage::new();
        let value = "a".repeat(RT_MESSAGE_CAPACITY - 1) + "あ";

        message.write_str(&value).unwrap();

        assert_eq!(message.len, RT_MESSAGE_CAPACITY - 1);
        assert_eq!(
            std::str::from_utf8(message.as_bytes()).unwrap().len(),
            message.len
        );
    }
}
