//! Producer-side native fault records. A pipe flush is not a disk sync.
//!
//! Studio supplies its existing analysis-log directory before spawning workers.
//! Device/model/error boundaries, Qwen layer/attention boundaries and explicitly
//! focused operator details are synchronized on the invoking thread. Other
//! scheduling details are batched; their unsynchronized tail may be lost.
//! Native device waits and tensor lifetimes are unchanged, but I/O timing changes.
use std::ffi::{CStr, c_char};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod buffering;
use buffering::{RecordBuffer, requires_sync};

static JOURNAL: OnceLock<Mutex<Option<Journal>>> = OnceLock::new();

struct Journal {
    file: File,
    started: Instant,
    sequence: u64,
    boot_id: Option<String>,
    buffer: RecordBuffer,
    focus: Option<String>,
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
        let focus = std::env::var("UTA_STUDIO_NATIVE_TRACE_FOCUS")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let mut result = Self {
            file,
            started: Instant::now(),
            sequence: 0,
            boot_id,
            buffer: RecordBuffer::default(),
            focus,
        };
        result.record("journal_opened", &path.to_string_lossy())?;
        let policy = serde_json::json!({
            "focused_scope": result.focus,
            "durable": "device/model/forward/error, Qwen layer/attention entry, completed attention tiles, and focused events",
            "details": "known Qwen strict operator triplets and other scheduling details use 64 KiB batches; synchronize on next event after one second",
            "limitation": "unfocused tail can be lost before a durable boundary; one second is checked on the next event, not by a timer; a submit marker does not prove device execution",
            "device_completion": "native GPU synchronization and tensor lifetimes are unchanged by journal batching",
        });
        result.record("journal_policy", &policy.to_string())?;
        let _ = writeln!(
            io::stderr().lock(),
            "[uta-native-diagnostics] {}",
            path.display()
        );
        Ok(result)
    }

    fn record(&mut self, phase: &str, detail: &str) -> io::Result<()> {
        let durable = requires_sync(phase, detail, self.focus.as_deref());
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
        self.buffer
            .append(&mut self.file, &record, durable, self.started.elapsed())?;
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
