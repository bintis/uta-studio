//! Persistent, opt-in local diagnostic snapshots and live log capture.
//! No tracing calls here: the app-log writer calls into this module.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::log_storage::{self, LogStorageStats};

static DEBUG_LOGGING: LazyLock<Mutex<DebugLogging>> =
    LazyLock::new(|| Mutex::new(DebugLogging::default()));
static DIRECTORY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct DebugLogging {
    session: Option<DebugSession>,
    error: Option<String>,
}

struct DebugSession {
    directory: PathBuf,
    context: String,
    app_log: File,
    backend_stderr: File,
    app_source: Option<File>,
    app_source_path: Option<PathBuf>,
    app_copied: u64,
}

/// Snapshot the full on-disk app.log and recursive analysis-logs, then mirror
/// subsequent app-log lines and backend stderr into the returned directory.
/// `context` is stored verbatim in context.txt; desktop owns its contents and
/// tracing configuration. Missing logs and skipped links are in snapshot-notes.txt.
/// An already-active session is returned unchanged (no repeated snapshot).
/// After stopping, a new start creates a new directory; previous logs remain.
/// Failure may leave a partial snapshot at the path included in the error.
/// This is synchronous local I/O, with no retention cap or power-loss guarantee.
pub fn start_debug_logging(context: &str) -> Result<PathBuf, String> {
    let _snapshot = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(directory) = DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .active_directory()
    {
        return Ok(directory);
    }
    let root = crate::cache::uta_studio_dir();
    // Bulk copies must not hold up live logging or backend pipe draining.
    let prepared = DebugSession::snapshot(&root, context);
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .activate(prepared)
}

/// Stop mirroring immediately without deleting logs or changing desktop tracing.
/// Serialized with start and cleanup; repeated stops are harmless. Already-running
/// workers retain their level, but future commands no longer inherit DEBUG.
pub fn stop_debug_logging() {
    let _snapshot = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .stop();
}

/// Logical bytes and regular-file count under the app-owned log locations.
/// Links and special files are skipped. I/O failures are returned, not hidden.
/// Active workers can still append analysis logs, so this is a point-in-time view.
pub fn log_storage_stats() -> Result<LogStorageStats, String> {
    let _snapshot = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _logging = DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    log_storage::stats_at(&crate::cache::uta_studio_dir())
}

/// Explicitly clear only regular app-owned logs, truncating app.log in place.
/// Close live handles first; if enabled, resume a fresh capture without copying
/// history, even after a cleanup error. No cached media, models or settings are
/// touched. Directories, links and special files are retained. Errors can mean
/// partial cleanup; a restart failure leaves DEBUG off and is also reported.
/// Returned stats include the fresh session's context/notes when enabled.
pub fn clear_logs() -> Result<LogStorageStats, String> {
    let _snapshot = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear(&crate::cache::uta_studio_dir())
}

/// First live-write failure (or latest start/cleanup failure), retained until a
/// successful new start or cleanup. An idempotent start does not clear errors.
/// Reading does not clear it. No recursive tracing is used.
pub fn debug_logging_error() -> Option<String> {
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .error
        .clone()
}

/// Whether local mirroring is active; desktop owns persistence and tracing.
pub fn debug_logging_enabled() -> bool {
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .session
        .is_some()
}

/// Serialize primary app-log writes with the final snapshot tail and switch,
/// so an app line cannot be both copied and mirrored into the same session.
pub(crate) fn with_app_log(text: &str, record: impl FnOnce()) {
    let mut logging = DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    record();
    logging.record_app(text);
}

pub(crate) fn record_backend_stderr(bytes: &[u8]) {
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .record_stderr(bytes);
}

impl DebugLogging {
    #[cfg(test)]
    fn start(&mut self, root: &Path, context: &str) -> Result<PathBuf, String> {
        if let Some(directory) = self.active_directory() {
            return Ok(directory);
        }
        self.activate(DebugSession::snapshot(root, context))
    }

    fn active_directory(&self) -> Option<PathBuf> {
        self.session
            .as_ref()
            .map(|session| session.directory.clone())
    }

    fn stop(&mut self) {
        self.session = None;
    }

    fn clear(&mut self, root: &Path) -> Result<LogStorageStats, String> {
        let context = self.session.as_ref().map(|session| session.context.clone());
        self.stop(); // Drop all mirror handles before deleting their files (Windows too).
        let cleanup = log_storage::clear_at(root);
        let restart = if let Some(context) = context {
            // Never snapshot here, including when cleanup only partially succeeded.
            self.activate(DebugSession::fresh(root, &context))
                .map(|_| ())
        } else {
            Ok(())
        };
        let result = match (cleanup, restart) {
            (Ok(()), Ok(())) => log_storage::stats_at(root),
            (Err(cleanup), Ok(())) => Err(format!("log cleanup incomplete: {cleanup}")),
            (Ok(()), Err(restart)) => {
                Err(format!("logs cleared but debug restart failed: {restart}"))
            }
            (Err(cleanup), Err(restart)) => Err(format!(
                "log cleanup incomplete: {cleanup}; debug restart failed: {restart}"
            )),
        };
        self.error = result.as_ref().err().cloned();
        result
    }

    fn activate(&mut self, prepared: Result<DebugSession, String>) -> Result<PathBuf, String> {
        let prepared = prepared.and_then(|mut session| {
            session.finish_app_snapshot()?;
            Ok(session)
        });
        match prepared {
            Ok(session) => {
                let directory = session.directory.clone();
                self.session = Some(session);
                self.error = None;
                Ok(directory)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn record_app(&mut self, text: &str) {
        if let Some(session) = &mut self.session {
            let result = writeln!(session.app_log, "{}", text.trim_end_matches('\n'));
            if let Err(error) = result {
                self.error.get_or_insert_with(|| {
                    format!(
                        "write {}: {error}",
                        session.directory.join("app.log").display()
                    )
                });
            }
        }
    }

    fn record_stderr(&mut self, bytes: &[u8]) {
        if let Some(session) = &mut self.session {
            if let Err(error) = session.backend_stderr.write_all(bytes) {
                self.error.get_or_insert_with(|| {
                    format!(
                        "write {}: {error}",
                        session.directory.join("backend-stderr.log").display()
                    )
                });
            }
        }
    }
}

impl DebugSession {
    fn snapshot(root: &Path, context: &str) -> Result<Self, String> {
        let directory = unique_directory(&root.join("debug-logs"))?;
        Self::snapshot_into(root, context, &directory)
            .map_err(|error| format!("debug snapshot {} incomplete: {error}", directory.display()))
    }

    fn fresh(root: &Path, context: &str) -> Result<Self, String> {
        let directory = unique_directory(&root.join("debug-logs"))?;
        Self::create(root, context, &directory, false)
            .map_err(|error| format!("debug capture {} incomplete: {error}", directory.display()))
    }

    fn snapshot_into(root: &Path, context: &str, directory: &Path) -> Result<Self, String> {
        Self::create(root, context, directory, true)
    }

    fn create(
        root: &Path,
        context: &str,
        directory: &Path,
        snapshot: bool,
    ) -> Result<Self, String> {
        let context_path = directory.join("context.txt");
        let mut context_file = log_storage::open_file(
            &context_path,
            OpenOptions::new().write(true).create_new(true),
        )?;
        io_at(
            "write",
            &context_path,
            context_file.write_all(context.as_bytes()),
        )?;
        let notes_path = directory.join("snapshot-notes.txt");
        let mut notes =
            log_storage::open_file(&notes_path, OpenOptions::new().write(true).create_new(true))?;
        io_at(
            "write",
            &notes_path,
            writeln!(
                notes,
                "Uta! Studio debug snapshot\nsource: {}\nprocess: {}\nstarted unix milliseconds: {}\nFiles copied through their length at open; analysis logs may be changing.\nLinks are not followed. Live backend stderr is raw, uncapped and shared by all drains.\nOnly future native commands receive UTA_STUDIO_DEBUG=1.\n",
                root.display(),
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
        )?;
        if snapshot {
            copy_logs(
                &root.join("analysis-logs"),
                &directory.join("analysis-logs"),
                &mut notes,
                &notes_path,
            )?;
        } else {
            io_at(
                "write",
                &notes_path,
                writeln!(
                    notes,
                    "Fresh live capture after explicit log cleanup; no history copied."
                ),
            )?;
        }
        let source_path = root.join("app.log");
        let app_source = if !snapshot {
            None
        } else {
            match fs::symlink_metadata(&source_path) {
                Ok(metadata) if metadata.is_file() => Some(log_storage::open_file(
                    &source_path,
                    OpenOptions::new().read(true),
                )?),
                Ok(_) => {
                    io_at(
                        "write",
                        &notes_path,
                        writeln!(
                            notes,
                            "skipped non-regular app log: {}",
                            source_path.display()
                        ),
                    )?;
                    None
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    io_at(
                        "write",
                        &notes_path,
                        writeln!(notes, "missing: {}", source_path.display()),
                    )?;
                    None
                }
                Err(error) => return Err(format!("inspect {}: {error}", source_path.display())),
            }
        };
        let app_path = directory.join("app.log");
        let stderr_path = directory.join("backend-stderr.log");
        let app_log =
            log_storage::open_file(&app_path, OpenOptions::new().create_new(true).append(true))?;
        let backend_stderr = log_storage::open_file(
            &stderr_path,
            OpenOptions::new().create_new(true).append(true),
        )?;
        let mut session = Self {
            directory: directory.to_path_buf(),
            context: context.to_string(),
            app_log,
            backend_stderr,
            app_source,
            // None on a fresh capture prevents the activation tail from reopening
            // any surviving app history after a partially failed cleanup.
            app_source_path: snapshot.then_some(source_path),
            app_copied: 0,
        };
        session.copy_app_tail()?;
        Ok(session)
    }

    fn copy_app_tail(&mut self) -> Result<(), String> {
        let Some(source) = &mut self.app_source else {
            return Ok(());
        };
        let path = self.directory.join("app.log");
        let length = io_at("inspect app log source for", &path, source.metadata())?.len();
        let remaining = length
            .checked_sub(self.app_copied)
            .ok_or_else(|| format!("copy app log to {}: source shortened", path.display()))?;
        let copied = io_at(
            "copy app log to",
            &path,
            io::copy(&mut source.take(remaining), &mut self.app_log),
        )?;
        if copied != remaining {
            return Err(format!(
                "copy app log to {}: source shortened during copy",
                path.display()
            ));
        }
        self.app_copied = length;
        Ok(())
    }

    fn finish_app_snapshot(&mut self) -> Result<(), String> {
        // The primary app log may have been created while missing at preparation.
        if self.app_source.is_none()
            && let Some(source_path) = &self.app_source_path
        {
            match fs::symlink_metadata(source_path) {
                Ok(metadata) if metadata.is_file() => {
                    self.app_source = Some(log_storage::open_file(
                        source_path,
                        OpenOptions::new().read(true),
                    )?);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "debug snapshot {} incomplete: inspect {}: {error}",
                        self.directory.display(),
                        source_path.display()
                    ));
                }
            }
        }
        // Only bytes appended during the bulk app copy require the live lock.
        self.copy_app_tail().map_err(|error| {
            format!(
                "debug snapshot {} incomplete: {error}",
                self.directory.display()
            )
        })?;
        self.app_source = None;
        Ok(())
    }
}

fn io_at<T>(action: &str, path: &Path, result: io::Result<T>) -> Result<T, String> {
    result.map_err(|error| format!("{action} {}: {error}", path.display()))
}

fn unique_directory(parent: &Path) -> Result<PathBuf, String> {
    log_storage::create_directory(parent)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    loop {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = parent.join(format!(
            "session-{timestamp}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create directory {}: {error}", directory.display())),
        }
    }
}

fn copy_logs(
    source: &Path,
    destination: &Path,
    notes: &mut File,
    notes_path: &Path,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return io_at(
                "write",
                notes_path,
                writeln!(notes, "missing: {}", source.display()),
            );
        }
        Err(error) => return Err(format!("inspect {}: {error}", source.display())),
    };
    if metadata.is_dir() {
        io_at("create directory", destination, fs::create_dir(destination))?;
        for entry in io_at("read directory", source, fs::read_dir(source))? {
            let entry = io_at("read directory entry", source, entry)?;
            copy_logs(
                &entry.path(),
                &destination.join(entry.file_name()),
                notes,
                notes_path,
            )?;
        }
    } else if metadata.is_file() {
        let input = log_storage::open_file(source, OpenOptions::new().read(true))?;
        let length = io_at("inspect open file", source, input.metadata())?.len();
        let mut output = io_at(
            "create",
            destination,
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination),
        )?;
        let copied = io::copy(&mut input.take(length), &mut output).map_err(|error| {
            format!(
                "copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        if copied != length {
            return Err(format!(
                "copy {} to {}: source shortened from {length} to {copied} bytes",
                source.display(),
                destination.display()
            ));
        }
    } else {
        io_at(
            "write",
            notes_path,
            writeln!(notes, "skipped link or special file: {}", source.display()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self(unique_directory(&std::env::temp_dir()).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn copies_full_app_log_and_nested_jsonl_and_context() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        let text = (0..1200)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        fs::write(root.join("app.log"), &text).unwrap();
        fs::create_dir_all(root.join("analysis-logs/run/nodes")).unwrap();
        let jsonl = b"{\"event\":\"start\"}\n{\"event\":\"end\"}\n";
        fs::write(root.join("analysis-logs/run/nodes/events.jsonl"), jsonl).unwrap();
        let mut logging = DebugLogging::default();
        let directory = logging.start(root, "desktop context\nunchanged").unwrap();
        assert_eq!(fs::read_to_string(directory.join("app.log")).unwrap(), text);
        assert_eq!(
            fs::read(directory.join("analysis-logs/run/nodes/events.jsonl")).unwrap(),
            jsonl
        );
        assert_eq!(
            fs::read_to_string(directory.join("context.txt")).unwrap(),
            "desktop context\nunchanged"
        );
        logging.record_app("live\n");
        assert_eq!(
            fs::read_to_string(directory.join("app.log")).unwrap(),
            format!("{text}live\n")
        );
        assert_eq!(fs::read_to_string(root.join("app.log")).unwrap(), text);
    }

    #[test]
    fn activation_copies_appends_during_preparation_exactly_once() {
        let fixture = Fixture::new();
        let path = fixture.0.join("app.log");
        fs::write(&path, "before\n").unwrap();
        let prepared = DebugSession::snapshot(&fixture.0, "").unwrap();
        let mut source = OpenOptions::new().append(true).open(&path).unwrap();
        source.write_all(b"during\n").unwrap();
        let mut logging = DebugLogging::default();
        let directory = logging.activate(Ok(prepared)).unwrap();
        logging.record_app("after");
        assert_eq!(
            fs::read_to_string(directory.join("app.log")).unwrap(),
            "before\nduring\nafter\n"
        );
    }

    #[test]
    fn activation_copies_app_log_created_during_preparation() {
        let fixture = Fixture::new();
        let prepared = DebugSession::snapshot(&fixture.0, "").unwrap();
        fs::write(fixture.0.join("app.log"), "created\n").unwrap();
        let mut logging = DebugLogging::default();
        let directory = logging.activate(Ok(prepared)).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("app.log")).unwrap(),
            "created\n"
        );
    }

    #[test]
    fn missing_logs_are_noted_and_live_files_are_created() {
        let fixture = Fixture::new();
        let mut logging = DebugLogging::default();
        let directory = logging.start(&fixture.0, "").unwrap();
        let notes = fs::read_to_string(directory.join("snapshot-notes.txt")).unwrap();
        assert_eq!(notes.matches("missing:").count(), 2);
        assert!(notes.contains("app.log"));
        assert!(notes.contains("analysis-logs"));
        assert!(directory.join("app.log").is_file());
        assert!(directory.join("backend-stderr.log").is_file());
        assert!(logging.error.is_none());
    }

    #[test]
    fn repeated_start_is_idempotent_and_stop_can_repeat_before_reenabling() {
        let fixture = Fixture::new();
        let mut logging = DebugLogging::default();
        fs::write(fixture.0.join("app.log"), "before\n").unwrap();
        let previous = logging.start(&fixture.0, "previous").unwrap();
        logging.record_stderr(b"previous stderr");
        fs::write(fixture.0.join("app.log"), "before\nafter\n").unwrap();
        let repeated = logging.start(&fixture.0, "not another snapshot").unwrap();
        assert_eq!(previous, repeated);
        assert_eq!(
            fs::read_to_string(previous.join("context.txt")).unwrap(),
            "previous"
        );
        assert_eq!(
            fs::read_dir(fixture.0.join("debug-logs")).unwrap().count(),
            1
        );
        logging.stop();
        logging.stop();
        assert!(logging.active_directory().is_none());
        logging.record_app("not captured");
        logging.record_stderr(b"not captured either");
        let current = logging.start(&fixture.0, "current").unwrap();
        assert_ne!(previous, current);
        logging.record_app("current live");
        logging.record_stderr(b"current stderr");
        assert_eq!(
            fs::read_to_string(previous.join("app.log")).unwrap(),
            "before\n"
        );
        assert_eq!(
            fs::read(previous.join("backend-stderr.log")).unwrap(),
            b"previous stderr"
        );
        assert_eq!(
            fs::read_to_string(current.join("app.log")).unwrap(),
            "before\nafter\ncurrent live\n"
        );
        assert_eq!(
            fs::read(current.join("backend-stderr.log")).unwrap(),
            b"current stderr"
        );
    }

    #[test]
    fn start_failure_is_explicit_and_active_start_does_not_attempt_io() {
        let fixture = Fixture::new();
        let blocked = Fixture::new();
        fs::write(blocked.0.join("debug-logs"), "not a directory").unwrap();
        let mut logging = DebugLogging::default();
        assert!(logging.start(&blocked.0, "").is_err());
        assert!(logging.session.is_none());
        let directory = logging.start(&fixture.0, "").unwrap();
        assert!(logging.error.is_none());
        assert_eq!(logging.start(&blocked.0, "").unwrap(), directory);
        logging.record_stderr(b"still captured");
        assert_eq!(
            fs::read(directory.join("backend-stderr.log")).unwrap(),
            b"still captured"
        );
        logging.stop();
        let error = logging.start(&blocked.0, "").unwrap_err();
        assert!(error.contains("debug-logs"));
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        assert!(logging.active_directory().is_none());
    }

    #[test]
    fn clear_while_off_does_not_enable_capture_or_touch_unrelated_data() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("app.log"), b"history").unwrap();
        fs::create_dir_all(fixture.0.join("analysis-logs/run")).unwrap();
        fs::write(fixture.0.join("analysis-logs/run/events.jsonl"), b"{}\n").unwrap();
        fs::write(fixture.0.join("settings.json"), b"settings").unwrap();
        let mut logging = DebugLogging::default();
        for _ in 0..2 {
            let stats = logging.clear(&fixture.0).unwrap();
            assert_eq!(
                stats,
                LogStorageStats {
                    bytes: 0,
                    file_count: 1
                }
            );
            assert!(logging.active_directory().is_none());
            assert!(!fixture.0.join("debug-logs").exists());
            assert_eq!(
                fs::read(fixture.0.join("settings.json")).unwrap(),
                b"settings"
            );
        }
    }

    #[test]
    fn clear_active_capture_restarts_empty_live_logs_without_copying_history() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("app.log"), b"deleted history\n").unwrap();
        fs::create_dir_all(fixture.0.join("analysis-logs/run")).unwrap();
        fs::write(fixture.0.join("analysis-logs/run/events.jsonl"), b"{}\n").unwrap();
        let mut logging = DebugLogging::default();
        let previous = logging.start(&fixture.0, "desktop context").unwrap();
        logging.record_stderr(b"deleted stderr");
        let stats = logging.clear(&fixture.0).unwrap();
        assert_eq!(stats, log_storage::stats_at(&fixture.0).unwrap());
        assert!(stats.bytes > 0); // Fresh context and notes are deliberately retained.
        assert_eq!(stats.file_count, 5); // Primary log + four fresh session files.
        assert!(!previous.join("app.log").exists());
        assert!(!fixture.0.join("analysis-logs/run/events.jsonl").exists());
        assert_eq!(fs::read(fixture.0.join("app.log")).unwrap(), b"");
        let current = logging.active_directory().unwrap();
        assert_ne!(current, previous);
        assert_eq!(fs::read(current.join("app.log")).unwrap(), b"");
        assert_eq!(fs::read(current.join("backend-stderr.log")).unwrap(), b"");
        assert!(!current.join("analysis-logs").exists());
        assert_eq!(
            fs::read_to_string(current.join("context.txt")).unwrap(),
            "desktop context"
        );
        assert!(
            fs::read_to_string(current.join("snapshot-notes.txt"))
                .unwrap()
                .contains("no history copied")
        );
        logging.record_app("fresh line");
        logging.record_stderr(b"fresh stderr");
        assert_eq!(fs::read(current.join("app.log")).unwrap(), b"fresh line\n");
        assert_eq!(
            fs::read(current.join("backend-stderr.log")).unwrap(),
            b"fresh stderr"
        );
        assert_eq!(logging.start(&fixture.0, "ignored").unwrap(), current);
        logging.clear(&fixture.0).unwrap();
        assert_ne!(logging.active_directory().unwrap(), current);
        assert!(!current.join("app.log").exists());
    }

    #[test]
    fn failed_cleanup_is_reported_and_capture_resumes_without_partial_history() {
        let fixture = Fixture::new();
        let mut logging = DebugLogging::default();
        let previous = logging.start(&fixture.0, "context").unwrap();
        logging.record_app("old mirror");
        // Wrong type causes a deterministic traversal failure without permissions.
        fs::create_dir(fixture.0.join("app.log")).unwrap();
        let error = logging.clear(&fixture.0).unwrap_err();
        assert!(error.contains("log cleanup incomplete"));
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        let current = logging.active_directory().unwrap();
        assert_ne!(current, previous);
        assert_eq!(fs::read(current.join("app.log")).unwrap(), b"");
        assert_eq!(fs::read(previous.join("app.log")).unwrap(), b"old mirror\n");
        logging.record_stderr(b"resumed");
        assert_eq!(
            fs::read(current.join("backend-stderr.log")).unwrap(),
            b"resumed"
        );
    }

    #[test]
    fn cleanup_and_restart_failures_are_both_reported_and_capture_is_off() {
        let fixture = Fixture::new();
        let blocked = Fixture::new();
        let mut logging = DebugLogging::default();
        let previous = logging.start(&fixture.0, "context").unwrap();
        // A changed data root with a wrong-type log directory deterministically
        // fails both traversal and restart, even on privileged test runners.
        fs::write(blocked.0.join("debug-logs"), b"not a directory").unwrap();
        let error = logging.clear(&blocked.0).unwrap_err();
        assert!(error.contains("log cleanup incomplete"));
        assert!(error.contains("debug restart failed"));
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        assert!(logging.active_directory().is_none());
        logging.record_stderr(b"not captured");
        assert_eq!(fs::read(previous.join("backend-stderr.log")).unwrap(), b"");
    }

    #[cfg(unix)]
    #[test]
    fn linked_debug_directory_is_not_followed_on_start_or_restart() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let linked = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("source.flac"), b"media").unwrap();
        symlink(&outside.0, linked.0.join("debug-logs")).unwrap();
        let mut logging = DebugLogging::default();
        assert!(
            logging
                .start(&linked.0, "")
                .unwrap_err()
                .contains("debug-logs")
        );
        logging.start(&fixture.0, "context").unwrap();
        let error = logging.clear(&linked.0).unwrap_err();
        assert!(error.contains("logs cleared but debug restart failed"));
        assert!(logging.active_directory().is_none());
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        assert_eq!(fs::read(outside.0.join("source.flac")).unwrap(), b"media");
        assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    }

    #[test]
    fn stderr_is_raw_and_uncapped_and_write_failures_are_observable() {
        let fixture = Fixture::new();
        let mut logging = DebugLogging::default();
        let directory = logging.start(&fixture.0, "").unwrap();
        let bytes = vec![0xff; 2 * 1024 * 1024];
        for chunk in bytes.chunks(4096) {
            logging.record_stderr(chunk);
        }
        assert_eq!(
            fs::read(directory.join("backend-stderr.log")).unwrap(),
            bytes
        );
        // Read-only handles deterministically fail writes, even when tests run as root.
        logging.session.as_mut().unwrap().backend_stderr =
            File::open(directory.join("backend-stderr.log")).unwrap();
        logging.record_stderr(b"cannot write");
        let error = logging.error.clone().unwrap();
        assert!(error.contains("backend-stderr.log"));
        logging.record_stderr(b"still does not panic");
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        logging.start(&fixture.0, "").unwrap();
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        logging.stop();
        logging.start(&fixture.0, "").unwrap();
        assert!(logging.error.is_none());
        let session = logging.session.as_mut().unwrap();
        session.app_log = File::open(session.directory.join("app.log")).unwrap();
        logging.record_app("cannot write either");
        assert!(logging.error.as_ref().unwrap().contains("app.log"));
    }
}
