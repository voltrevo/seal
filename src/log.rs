use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Max size per log file before rotation (10 MB).
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;

/// Number of old log files to keep.
const KEEP_LOGS: usize = 3;

/// A writer that rotates log files when they exceed MAX_LOG_SIZE.
/// Total disk usage is bounded to ~(KEEP_LOGS + 1) * MAX_LOG_SIZE = 40 MB.
pub struct RotatingLog {
    path: PathBuf,
    inner: Mutex<RotatingLogInner>,
}

struct RotatingLogInner {
    file: File,
    written: u64,
}

impl RotatingLog {
    pub fn new(path: PathBuf) -> std::io::Result<Self> {
        rotate(&path, KEEP_LOGS);
        let file = open_log(&path)?;
        Ok(Self {
            path,
            inner: Mutex::new(RotatingLogInner { file, written: 0 }),
        })
    }

    fn write_bytes(&self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        let n = inner.file.write(buf)?;
        inner.written += n as u64;

        if inner.written >= MAX_LOG_SIZE {
            inner.file.flush().ok();
            rotate(&self.path, KEEP_LOGS);
            inner.file = open_log(&self.path)?;
            inner.written = 0;
        }

        Ok(n)
    }

    fn flush_inner(&self) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.file.flush()
    }
}

/// A per-event writer handle returned by MakeWriter.
/// Delegates all writes to the shared RotatingLog.
pub struct LogWriter<'a> {
    log: &'a RotatingLog,
}

impl<'a> Write for LogWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.log.write_bytes(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.log.flush_inner()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RotatingLog {
    type Writer = LogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter { log: self }
    }
}

fn open_log(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

/// Rotate: daemon.log → .1 → .2 → .3 (delete oldest beyond `keep`).
fn rotate(log_path: &Path, keep: usize) {
    let base = log_path.display().to_string();

    // Delete the oldest
    let _ = std::fs::remove_file(format!("{base}.{keep}"));

    // Shift N-1 → N
    for i in (1..keep).rev() {
        let _ = std::fs::rename(format!("{base}.{i}"), format!("{base}.{}", i + 1));
    }

    // Current → .1
    if log_path.exists() {
        let _ = std::fs::rename(log_path, format!("{base}.1"));
    }
}
