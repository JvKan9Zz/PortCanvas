//! Silent background runtime debug logger with a strict 10 MiB total cap.
//!
//! Writes under `<install_dir>/debuglogs` on Windows (falling back to the app
//! data directory if the executable directory is not writable) and under
//! `<data_dir>/debuglogs` on macOS. Never logs secrets and never panics on I/O
//! errors.

use anyhow::{Context, Result};
use chrono::Local;
use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub const MAX_TOTAL_BYTES: u64 = 10 * 1024 * 1024;
pub const SEGMENT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARCHIVES: usize = 4;
const MAX_ENTRY_BYTES: usize = 64 * 1024;
const RECENT_EXPORT_THRESHOLD: u64 = 256 * 1024;
const PRIMARY_NAME: &str = "portcanvas.log";

static GLOBAL: OnceLock<DebugLogger> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();

pub struct DebugLogger {
    dir: PathBuf,
    segment_bytes: u64,
    max_total_bytes: u64,
    max_archives: usize,
    lock: Mutex<()>,
}

impl DebugLogger {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_limits(dir, SEGMENT_BYTES, MAX_TOTAL_BYTES, MAX_ARCHIVES)
    }

    pub fn with_limits(
        dir: PathBuf,
        segment_bytes: u64,
        max_total_bytes: u64,
        max_archives: usize,
    ) -> Self {
        let segment = segment_bytes.max(1024).min(max_total_bytes.max(1024));
        let logger = Self {
            dir,
            segment_bytes: segment,
            max_total_bytes: max_total_bytes.max(1024),
            max_archives,
            lock: Mutex::new(()),
        };
        let _guard = logger.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = fs::create_dir_all(&logger.dir);
        logger.enforce_total_limit_locked();
        drop(_guard);
        logger
    }

    pub fn directory(&self) -> &Path {
        &self.dir
    }

    pub fn primary_path(&self) -> PathBuf {
        self.dir.join(PRIMARY_NAME)
    }

    fn archive_path(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{PRIMARY_NAME}.{index}"))
    }

    pub fn write(&self, level: &str, scope: &str, message: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let sanitized = sanitize_message(message);
        let mut line = format!("[{timestamp}] [{level}] [{scope}] {sanitized}\n");
        if line.len() > MAX_ENTRY_BYTES {
            let mut end = MAX_ENTRY_BYTES.saturating_sub(32);
            while end > 0 && !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
            line.push_str(" ... [truncated]\n");
        }
        let bytes = line.as_bytes();

        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let primary = self.primary_path();
        let current_len = fs::metadata(&primary).map(|m| m.len()).unwrap_or(0);
        if current_len > 0 && current_len.saturating_add(bytes.len() as u64) > self.segment_bytes {
            self.rotate_locked();
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&primary) {
            let _ = file.write_all(bytes);
            let _ = file.flush();
        }
        self.enforce_total_limit_locked();
    }

    fn rotate_locked(&self) {
        if self.max_archives == 0 {
            let _ = fs::remove_file(self.primary_path());
            return;
        }
        let oldest = self.archive_path(self.max_archives);
        let _ = fs::remove_file(&oldest);
        for idx in (1..self.max_archives).rev() {
            let from = self.archive_path(idx);
            let to = self.archive_path(idx + 1);
            if from.exists() {
                let _ = fs::remove_file(&to);
                let _ = fs::rename(&from, &to);
            }
        }
        let primary = self.primary_path();
        let first = self.archive_path(1);
        if primary.exists() {
            let _ = fs::remove_file(&first);
            let _ = fs::rename(&primary, &first);
        }
    }

    fn enforce_total_limit_locked(&self) {
        // Delete any stale archives beyond max_archives first.
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(suffix) = name.strip_prefix(&format!("{PRIMARY_NAME}.")) {
                    let keep = suffix
                        .parse::<usize>()
                        .ok()
                        .is_some_and(|n| (1..=self.max_archives).contains(&n));
                    if !keep {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        let mut total = self.total_bytes_locked();
        if total <= self.max_total_bytes {
            return;
        }

        // Remove oldest archives first until total fits within budget.
        for idx in (1..=self.max_archives).rev() {
            if total <= self.max_total_bytes {
                break;
            }
            let path = self.archive_path(idx);
            if let Ok(meta) = fs::metadata(&path) {
                if fs::remove_file(&path).is_ok() {
                    total = total.saturating_sub(meta.len());
                }
            }
        }

        // If the primary file alone still exceeds max_total_bytes, keep only its newest tail.
        let primary = self.primary_path();
        if total > self.max_total_bytes {
            if let Ok(data) = fs::read(&primary) {
                let keep = self.max_total_bytes as usize;
                if data.len() > keep {
                    let mut start = data.len() - keep;
                    if let Some(pos) = data[start..].iter().position(|&b| b == b'\n') {
                        start = (start + pos + 1).min(data.len());
                    }
                    let _ = fs::write(&primary, &data[start..]);
                }
            }
        }
    }

    fn total_bytes_locked(&self) -> u64 {
        let mut total = fs::metadata(self.primary_path()).map(|m| m.len()).unwrap_or(0);
        for idx in 1..=self.max_archives {
            total = total.saturating_add(
                fs::metadata(self.archive_path(idx))
                    .map(|m| m.len())
                    .unwrap_or(0),
            );
        }
        total
    }

    pub fn total_bytes(&self) -> u64 {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.total_bytes_locked()
    }

    /// Export the latest runtime debug log (including recent pre-rotation tail
    /// if the active file is small) to `destination`.
    pub fn export_latest(&self, destination: &Path) -> Result<u64> {
        let (previous, current) = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let current = fs::read(self.primary_path()).unwrap_or_default();
            let previous = if (current.len() as u64) < RECENT_EXPORT_THRESHOLD {
                fs::read(self.archive_path(1)).ok()
            } else {
                None
            };
            (previous, current)
        };

        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("无法创建导出目录：{}", parent.display()))?;
            }
        }

        let header = format!(
            "=== PortCanvas Debug Log Export ===\nVersion: {}\nOS: {} ({})\nExported At: {}\nLog Directory: {}\n===================================\n\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
            self.dir.display(),
        );

        let mut output = Vec::with_capacity(header.len() + current.len() + 64 * 1024);
        output.extend_from_slice(header.as_bytes());

        if let Some(prev) = previous {
            if !prev.is_empty() {
                let tail_limit = RECENT_EXPORT_THRESHOLD as usize;
                let mut start = prev.len().saturating_sub(tail_limit);
                if start > 0 {
                    if let Some(pos) = prev[start..].iter().position(|&b| b == b'\n') {
                        start = (start + pos + 1).min(prev.len());
                    }
                }
                output.extend_from_slice(b"--- Previous Segment Tail ---\n");
                output.extend_from_slice(&prev[start..]);
                if !output.ends_with(b"\n") {
                    output.push(b'\n');
                }
                output.extend_from_slice(b"--- Current Segment ---\n");
            }
        }

        if current.is_empty() {
            output.extend_from_slice(b"(No debug log entries recorded yet.)\n");
        } else {
            output.extend_from_slice(&current);
        }

        fs::write(destination, &output)
            .with_context(|| format!("无法写入导出日志文件：{}", destination.display()))?;
        Ok(output.len() as u64)
    }
}

fn sanitize_message(message: &str) -> String {
    let trimmed = message.trim_end_matches(&['\r', '\n'][..]);
    if !trimmed.contains('\n') && !trimmed.contains('\r') {
        return trimmed.to_owned();
    }
    trimmed
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\n    ")
}

/// Resolve the debug log directory.
/// - `PORTCANVAS_DEBUG_LOG_DIR` (if absolute) takes precedence for isolated tests.
/// - On Windows: `<install_dir>/debuglogs`, falling back to `<data_dir>/debuglogs`
///   only if `<install_dir>/debuglogs` cannot be created or written.
/// - On macOS/other: `<data_dir>/debuglogs` (app bundle binaries are read-only).
pub fn resolve_directory(data_dir: Option<&Path>) -> PathBuf {
    if let Some(explicit) = std::env::var_os("PORTCANVAS_DEBUG_LOG_DIR").map(PathBuf::from) {
        if explicit.is_absolute() {
            return explicit;
        }
    }

    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(install_dir) = exe.parent() {
                let candidate = install_dir.join("debuglogs");
                if is_dir_writable(&candidate) {
                    return candidate;
                }
            }
        }
    }

    if let Some(data) = data_dir {
        let candidate = data.join("debuglogs");
        if is_dir_writable(&candidate) {
            return candidate;
        }
    }

    std::env::temp_dir().join("PortCanvas").join("debuglogs")
}

fn is_dir_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    match OpenOptions::new().create(true).write(true).open(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn init(data_dir: Option<&Path>) -> &'static DebugLogger {
    let logger = GLOBAL.get_or_init(|| {
        let dir = resolve_directory(data_dir);
        let logger = DebugLogger::new(dir);
        logger.write(
            "INFO",
            "startup",
            &format!(
                "PortCanvas v{} started (pid={}, os={}/{}, debuglogs={})",
                env!("CARGO_PKG_VERSION"),
                std::process::id(),
                std::env::consts::OS,
                std::env::consts::ARCH,
                logger.directory().display()
            ),
        );
        logger
    });

    PANIC_HOOK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("unnamed");
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".into());
            let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic payload".into()
            };
            let backtrace = Backtrace::force_capture();
            error(
                "panic",
                &format!(
                    "thread '{thread_name}' panicked at {location}: {payload}\nbacktrace:\n{backtrace}"
                ),
            );
            previous(info);
        }));
    });

    logger
}

pub fn info(scope: &str, message: &str) {
    if let Some(logger) = GLOBAL.get() {
        logger.write("INFO", scope, message);
    }
}

pub fn warn(scope: &str, message: &str) {
    if let Some(logger) = GLOBAL.get() {
        logger.write("WARN", scope, message);
    }
}

pub fn error(scope: &str, message: &str) {
    if let Some(logger) = GLOBAL.get() {
        logger.write("ERROR", scope, message);
    }
}

pub fn export_latest(destination: &Path) -> Result<u64> {
    let logger = GLOBAL.get_or_init(|| DebugLogger::new(resolve_directory(None)));
    logger.export_latest(destination)
}

pub fn default_export_filename() -> String {
    format!(
        "PortCanvas-debug-{}.log",
        Local::now().format("%Y%m%d-%H%M%S")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> Result<PathBuf> {
        let base = std::env::var_os("WCS_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = base.join(format!("debug-log-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn rotates_and_strictly_enforces_total_byte_limit() -> Result<()> {
        let root = temp_root("rotate")?;
        let dir = root.join("debuglogs");
        let max_total = 10 * 1024;
        let segment = 2 * 1024;
        let logger = DebugLogger::with_limits(dir.clone(), segment, max_total, 4);

        let payload = "X".repeat(300);
        for i in 0..80 {
            logger.write("INFO", "test", &format!("entry-{i:03} {payload}"));
            assert!(
                logger.total_bytes() <= max_total,
                "total bytes {} exceeded limit {} at entry {i}",
                logger.total_bytes(),
                max_total
            );
        }

        assert!(logger.primary_path().is_file());
        assert!(logger.archive_path(1).is_file());
        assert!(!logger.archive_path(5).exists());

        let exported = root.join("exported.log");
        let written = logger.export_latest(&exported)?;
        assert!(written > 0);
        let text = fs::read_to_string(&exported)?;
        assert!(text.contains("PortCanvas Debug Log Export"));
        assert!(text.contains("entry-079"));

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn export_includes_previous_segment_tail_when_primary_is_small() -> Result<()> {
        let root = temp_root("export-tail")?;
        let dir = root.join("debuglogs");
        let logger = DebugLogger::with_limits(dir, 1024, 5 * 1024, 4);

        for i in 0..20 {
            logger.write("INFO", "test", &format!("before-rotate-{i:02} {}", "A".repeat(80)));
        }
        logger.write("WARN", "test", "after-rotate-marker");

        let exported = root.join("feedback.log");
        logger.export_latest(&exported)?;
        let content = fs::read_to_string(&exported)?;
        assert!(content.contains("Previous Segment Tail"));
        assert!(content.contains("after-rotate-marker"));

        fs::remove_dir_all(root)?;
        Ok(())
    }
}
