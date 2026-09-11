//! Storage operations are limited to the app-owned log locations, not caches.
//! The core logger has no rotations: do not infer ownership from a filename glob.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogStorageStats {
    /// Logical bytes in regular log files, including snapshot context and notes.
    pub bytes: u64,
    pub file_count: u64,
}

pub(crate) fn stats_at(root: &Path) -> Result<LogStorageStats, String> {
    let mut stats = LogStorageStats::default();
    visit_logs(root, &mut |path, metadata| {
        stats.bytes = stats
            .bytes
            .checked_add(metadata.len())
            .ok_or_else(|| format!("log byte count overflow at {}", path.display()))?;
        stats.file_count = stats
            .file_count
            .checked_add(1)
            .ok_or_else(|| format!("log file count overflow at {}", path.display()))?;
        Ok(())
    })?;
    Ok(stats)
}

pub(crate) fn clear_at(root: &Path) -> Result<(), String> {
    let app_log = root.join("app.log");
    visit_logs(root, &mut |path, _| {
        if path == app_log {
            // Keep the inode: an open app-log writer must not retain deleted bytes.
            let file = open_file(path, OpenOptions::new().write(true))?;
            io_at("truncate", path, file.set_len(0))
        } else {
            io_at("remove log", path, fs::remove_file(path))
        }
    })
}

fn visit_logs(
    root: &Path,
    visit: &mut impl FnMut(&Path, &Metadata) -> Result<(), String>,
) -> Result<(), String> {
    let Some(metadata) = metadata_without_links(root)? else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Err(format!("log root is not a directory: {}", root.display()));
    }
    for name in ["app.log", "analysis-logs", "debug-logs"] {
        let path = root.join(name);
        let Some(metadata) = metadata_without_links(&path)? else {
            continue;
        };
        if name == "app.log" {
            if metadata.is_file() {
                visit(&path, &metadata)?;
            } else if metadata.is_dir() {
                return Err(format!("app log is not a regular file: {}", path.display()));
            }
        } else if metadata.is_dir() {
            visit_directory(&path, visit)?;
        } else if metadata.is_file() {
            return Err(format!(
                "log directory is not a directory: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn visit_directory(
    directory: &Path,
    visit: &mut impl FnMut(&Path, &Metadata) -> Result<(), String>,
) -> Result<(), String> {
    for entry in io_at("read log directory", directory, fs::read_dir(directory))? {
        let entry = io_at("read log directory entry", directory, entry)?;
        let path = entry.path();
        // A failed inspection of a discovered entry is an error, even NotFound:
        // the caller must not report a successful partial traversal/cleanup.
        let metadata = io_at("inspect log", &path, fs::symlink_metadata(&path))?;
        if metadata.is_dir() {
            visit_directory(&path, visit)?;
        } else if metadata.is_file() {
            visit(&path, &metadata)?;
        }
        // Symlinks and special files are not logs and remain untouched.
        // Keep directories, including those containing skipped entries.
    }
    Ok(())
}

/// Inspect every path component so a linked data root or ancestor is also skipped.
/// Missing paths and links have no owned regular-file storage to report or clear.
pub(crate) fn metadata_without_links(path: &Path) -> Result<Option<Metadata>, String> {
    let mut metadata = None;
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(value) if value.file_type().is_symlink() => return Ok(None),
            Ok(value) => metadata = Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("inspect {}: {error}", ancestor.display())),
        }
    }
    Ok(metadata)
}

pub(crate) fn create_directory(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "not an unlinked log directory: {}",
                    ancestor.display()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(ancestor) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = io_at("inspect", ancestor, fs::symlink_metadata(ancestor))?;
                        if !metadata.is_dir() {
                            return Err(format!(
                                "not an unlinked log directory: {}",
                                ancestor.display()
                            ));
                        }
                    }
                    Err(error) => {
                        return Err(format!("create directory {}: {error}", ancestor.display()));
                    }
                }
            }
            Err(error) => return Err(format!("inspect {}: {error}", ancestor.display())),
        }
    }
    Ok(())
}

/// Ordinary writes and snapshot reads must not follow a linked log either.
pub(crate) fn open_file(path: &Path, options: &mut OpenOptions) -> Result<File, String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !metadata_without_links(parent)?.is_some_and(|metadata| metadata.is_dir()) {
        return Err(format!(
            "missing or linked log directory: {}",
            parent.display()
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(format!("not a regular log file: {}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("inspect {}: {error}", path.display())),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the link itself, never its target.
        options.custom_flags(0x00200000);
    }
    let file = io_at("open log", path, options.open(path))?;
    if !io_at("inspect open log", path, file.metadata())?.is_file() {
        return Err(format!("not a regular log file: {}", path.display()));
    }
    Ok(file)
}

fn io_at<T>(action: &str, path: &Path, result: io::Result<T>) -> Result<T, String> {
    result.map_err(|error| format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FIXTURE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "uta-studio-log-storage-{timestamp}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accounts_only_owned_logs_and_clears_without_unlinking_app_log() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        fs::write(root.join("app.log"), b"app\n").unwrap();
        fs::create_dir_all(root.join("analysis-logs/run/nodes")).unwrap();
        fs::write(root.join("analysis-logs/run/nodes/events.jsonl"), b"{}\n").unwrap();
        fs::create_dir_all(root.join("debug-logs/session")).unwrap();
        fs::write(root.join("debug-logs/session/context.txt"), b"context").unwrap();
        fs::write(root.join("debug-logs/session/app.log"), b"copy").unwrap();
        for name in ["cache", "models", "media", "vendor"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("app.log"), b"unrelated").unwrap();
        }
        for name in [
            "config.json",
            "library.sqlite",
            "app.log.backup",
            "other.log",
        ] {
            fs::write(root.join(name), b"untouched").unwrap();
        }
        assert_eq!(
            stats_at(root).unwrap(),
            LogStorageStats {
                bytes: 18,
                file_count: 4
            }
        );
        let mut writer = OpenOptions::new()
            .append(true)
            .open(root.join("app.log"))
            .unwrap();
        clear_at(root).unwrap();
        assert_eq!(
            stats_at(root).unwrap(),
            LogStorageStats {
                bytes: 0,
                file_count: 1
            }
        );
        writer.write_all(b"fresh\n").unwrap();
        assert_eq!(fs::read(root.join("app.log")).unwrap(), b"fresh\n");
        assert_eq!(stats_at(root).unwrap().bytes, 6);
        for name in ["cache", "models", "media", "vendor"] {
            assert_eq!(
                fs::read(root.join(name).join("app.log")).unwrap(),
                b"unrelated"
            );
        }
        for name in [
            "config.json",
            "library.sqlite",
            "app.log.backup",
            "other.log",
        ] {
            assert_eq!(fs::read(root.join(name)).unwrap(), b"untouched");
        }
    }

    #[test]
    fn absent_logs_are_empty_without_creating_the_root() {
        let fixture = Fixture::new();
        let root = fixture.0.join("missing");
        assert_eq!(stats_at(&root).unwrap(), LogStorageStats::default());
        clear_at(&root).unwrap();
        assert!(!root.exists());
    }

    #[test]
    fn invalid_log_directory_is_an_error_not_successful_partial_cleanup() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("app.log"), b"history").unwrap();
        fs::write(fixture.0.join("analysis-logs"), b"not a directory").unwrap();
        assert!(stats_at(&fixture.0).unwrap_err().contains("analysis-logs"));
        assert!(clear_at(&fixture.0).unwrap_err().contains("analysis-logs"));
        assert_eq!(fs::read(fixture.0.join("app.log")).unwrap(), b"");
        assert_eq!(
            fs::read(fixture.0.join("analysis-logs")).unwrap(),
            b"not a directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_directories_and_root_ancestors_are_never_followed() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("app.log"), b"outside history").unwrap();
        fs::create_dir(outside.0.join("nested")).unwrap();
        fs::write(outside.0.join("nested/source.flac"), b"source media").unwrap();
        symlink(outside.0.join("app.log"), fixture.0.join("app.log")).unwrap();
        symlink(&outside.0, fixture.0.join("analysis-logs")).unwrap();
        fs::create_dir(fixture.0.join("debug-logs")).unwrap();
        symlink(&outside.0, fixture.0.join("debug-logs/directory-link")).unwrap();
        symlink(
            outside.0.join("app.log"),
            fixture.0.join("debug-logs/file-link"),
        )
        .unwrap();
        symlink(
            outside.0.join("missing"),
            fixture.0.join("debug-logs/dangling"),
        )
        .unwrap();
        assert_eq!(stats_at(&fixture.0).unwrap(), LogStorageStats::default());
        clear_at(&fixture.0).unwrap();
        assert!(open_file(&fixture.0.join("app.log"), OpenOptions::new().append(true)).is_err());
        assert!(create_directory(&fixture.0.join("analysis-logs/new")).is_err());
        for root in [
            fixture.0.join("analysis-logs"),
            fixture.0.join("analysis-logs/nested"),
        ] {
            assert_eq!(stats_at(&root).unwrap(), LogStorageStats::default());
            clear_at(&root).unwrap();
        }
        assert_eq!(
            fs::read(outside.0.join("app.log")).unwrap(),
            b"outside history"
        );
        assert_eq!(
            fs::read(outside.0.join("nested/source.flac")).unwrap(),
            b"source media"
        );
        assert!(
            fs::symlink_metadata(fixture.0.join("debug-logs/file-link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!outside.0.join("new").exists());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_errors_are_propagated() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        let directory = fixture.0.join("analysis-logs");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0)).unwrap();
        // Privileged runners can read mode-zero directories; exercise the actual
        // permission failure only when the filesystem enforces it for this user.
        let unreadable = fs::read_dir(&directory).is_err();
        let stats = stats_at(&fixture.0);
        let cleared = clear_at(&fixture.0);
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        if unreadable {
            assert!(stats.unwrap_err().contains("read log directory"));
            assert!(cleared.unwrap_err().contains("read log directory"));
        }
    }
}
