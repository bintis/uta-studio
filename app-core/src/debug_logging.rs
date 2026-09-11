//! Persistent, opt-in local diagnostic snapshots and live log capture.
//! No tracing calls here: the app-log writer calls into this module.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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
    app_log: File,
    backend_stderr: File,
    app_source: Option<File>,
    app_source_path: PathBuf,
    app_copied: u64,
}

/// Snapshot the full on-disk app.log and recursive analysis-logs, then mirror
/// subsequent app-log lines and backend stderr into the returned directory.
/// `context` is stored verbatim in context.txt; desktop owns its contents and
/// tracing configuration. Missing logs and skipped links are in snapshot-notes.txt.
/// Each successful call switches live capture to a new persistent directory;
/// previous directories remain untouched. Failure preserves any active session
/// and may leave a partial snapshot at the path included in the error.
/// This is synchronous local I/O, with no retention cap or power-loss guarantee.
pub fn start_debug_logging(context: &str) -> Result<PathBuf, String> {
    let _snapshot = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = crate::cache::uta_studio_dir();
    // Bulk copies must not hold up live logging or backend pipe draining.
    let prepared = DebugSession::snapshot(&root, context);
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .activate(prepared)
}

/// First live-write failure (or latest start failure), retained until the next
/// successful start. Reading does not clear it. No recursive tracing is used.
pub fn debug_logging_error() -> Option<String> {
    DEBUG_LOGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .error
        .clone()
}

pub(crate) fn enabled() -> bool {
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
        self.activate(DebugSession::snapshot(root, context))
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

    fn snapshot_into(root: &Path, context: &str, directory: &Path) -> Result<Self, String> {
        let context_path = directory.join("context.txt");
        io_at("write", &context_path, fs::write(&context_path, context))?;
        let notes_path = directory.join("snapshot-notes.txt");
        let mut notes = io_at("create", &notes_path, File::create(&notes_path))?;
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
        copy_logs(
            &root.join("analysis-logs"),
            &directory.join("analysis-logs"),
            &mut notes,
            &notes_path,
        )?;
        let source_path = root.join("app.log");
        let app_source = match fs::symlink_metadata(&source_path) {
            Ok(metadata) if metadata.is_file() => {
                Some(io_at("open", &source_path, File::open(&source_path))?)
            }
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
        };
        let app_path = directory.join("app.log");
        let stderr_path = directory.join("backend-stderr.log");
        let app_log = io_at(
            "open live log",
            &app_path,
            OpenOptions::new().create(true).append(true).open(&app_path),
        )?;
        let backend_stderr = io_at(
            "create live log",
            &stderr_path,
            OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(&stderr_path),
        )?;
        let mut session = Self {
            directory: directory.to_path_buf(),
            app_log,
            backend_stderr,
            app_source,
            app_source_path: source_path,
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
        if self.app_source.is_none() {
            match fs::symlink_metadata(&self.app_source_path) {
                Ok(metadata) if metadata.is_file() => {
                    self.app_source = Some(io_at(
                        "open app log for final snapshot",
                        &self.app_source_path,
                        File::open(&self.app_source_path),
                    )?);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "debug snapshot {} incomplete: inspect {}: {error}",
                        self.directory.display(),
                        self.app_source_path.display()
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
    io_at("create directory", parent, fs::create_dir_all(parent))?;
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
        let input = io_at("open", source, File::open(source))?;
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
    fn repeated_start_refreshes_without_touching_previous_directory() {
        let fixture = Fixture::new();
        let mut logging = DebugLogging::default();
        fs::write(fixture.0.join("app.log"), "before\n").unwrap();
        let previous = logging.start(&fixture.0, "previous").unwrap();
        logging.record_stderr(b"previous stderr");
        fs::write(fixture.0.join("app.log"), "before\nafter\n").unwrap();
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
    fn start_failure_is_explicit_and_preserves_active_capture() {
        let fixture = Fixture::new();
        let blocked = Fixture::new();
        fs::write(blocked.0.join("debug-logs"), "not a directory").unwrap();
        let mut logging = DebugLogging::default();
        assert!(logging.start(&blocked.0, "").is_err());
        assert!(logging.session.is_none());
        let directory = logging.start(&fixture.0, "").unwrap();
        assert!(logging.error.is_none());
        let error = logging.start(&blocked.0, "").unwrap_err();
        assert!(error.contains("debug-logs"));
        assert_eq!(logging.error.as_deref(), Some(error.as_str()));
        logging.record_stderr(b"still captured");
        assert_eq!(
            fs::read(directory.join("backend-stderr.log")).unwrap(),
            b"still captured"
        );
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
        assert!(logging.error.is_none());
        let session = logging.session.as_mut().unwrap();
        session.app_log = File::open(session.directory.join("app.log")).unwrap();
        logging.record_app("cannot write either");
        assert!(logging.error.as_ref().unwrap().contains("app.log"));
    }
}
