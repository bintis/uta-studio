//! Producer-side native fault records. A pipe flush is not a disk sync.
//!
//! Studio supplies its existing analysis-log directory before spawning workers.
//! Records are written and synchronized on the invoking worker thread, before
//! returning to native code. No GPU API, tracing subscriber, desktop snapshot
//! lock, background logger, inference retry or backend fallback is used.
use std::ffi::{CStr, c_char};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static JOURNAL: OnceLock<Mutex<Option<Journal>>> = OnceLock::new();

struct Journal {
    file: File,
    started: Instant,
    sequence: u64,
    boot_id: Option<String>,
}

fn warning(error: impl std::fmt::Display) {
    // Do not re-enter application logging from an FFI callback. A log failure
    // is visible but does not become an unrelated model eligibility gate.
    let _ = writeln!(
        io::stderr().lock(),
        "[uta-native-diagnostics] capture unavailable: {error}"
    );
}

impl Journal {
    fn create(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = directory.join(format!("native-{}-{timestamp}.jsonl", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.sync_all()?;
        // Persist the directory entry too. A standalone caller may have
        // supplied a not-yet-created nested directory for these logs.
        #[cfg(unix)]
        for parent in directory
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
        {
            File::open(parent)?.sync_all()?;
        }
        let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|value| value.trim().to_owned());
        let mut result = Self {
            file,
            started: Instant::now(),
            sequence: 0,
            boot_id,
        };
        result.record("journal_opened", &path.to_string_lossy())?;
        let _ = writeln!(
            io::stderr().lock(),
            "[uta-native-diagnostics] {}",
            path.display()
        );
        Ok(result)
    }

    fn record(&mut self, phase: &str, detail: &str) -> io::Result<()> {
        let record = serde_json::json!({
            "record_type": "native_execution",
            "pid": std::process::id(),
            "boot_id": self.boot_id,
            "sequence": self.sequence,
            "timestamp_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "elapsed_micros": self.started.elapsed().as_micros(),
            "phase": phase,
            "detail": detail,
        });
        persist(&mut self.file, &record)?;
        self.sequence += 1;
        Ok(())
    }
}

trait DurableWrite: Write {
    fn sync_record(&self) -> io::Result<()>;
}
impl DurableWrite for File {
    fn sync_record(&self) -> io::Result<()> {
        self.sync_data()
    }
}
fn persist(writer: &mut impl DurableWrite, record: &serde_json::Value) -> io::Result<()> {
    // Encode first: a JSON encoding failure must not start a partial record.
    let mut bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    writer.sync_record()
}

pub(crate) fn record(phase: &str, detail: &str) {
    let journal = JOURNAL.get_or_init(|| {
        let value = std::env::var_os("UTA_STUDIO_NATIVE_LOG_DIRECTORY")
            .filter(|value| !value.is_empty())
            .and_then(|directory| match Journal::create(Path::new(&directory)) {
                Ok(value) => Some(value),
                Err(error) => {
                    warning(error);
                    None
                }
            });
        Mutex::new(value)
    });
    let mut guard = journal
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(journal) = guard.as_mut()
        && let Err(error) = journal.record(phase, detail)
    {
        // Do not append later 'complete' records to a broken trace or flood
        // stderr with one failure per tensor. Existing bytes are preserved.
        *guard = None;
        warning(error);
    }
}

/// The native library borrows both C strings for this synchronous callback.
/// The function and its process-owned journal outlive every native model/DSO.
pub(crate) unsafe extern "C" fn native_event(phase: *const c_char, detail: *const c_char) {
    let result = std::panic::catch_unwind(|| {
        if phase.is_null() || detail.is_null() {
            return;
        }
        // SAFETY: api.h requires valid, NUL-terminated strings for the call.
        let phase = unsafe { CStr::from_ptr(phase) }.to_string_lossy();
        let detail = unsafe { CStr::from_ptr(detail) }.to_string_lossy();
        record(&phase, &detail);
    });
    if result.is_err() {
        warning("native diagnostic callback panicked");
    }
}

#[cfg(test)]
mod tests;
