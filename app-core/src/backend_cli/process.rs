use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command};
use std::sync::{Arc, Mutex};

use super::error::BackendCliError;

pub const MAX_MACHINE_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CAPTURED_STDERR_BYTES: usize = 256 * 1024;

pub fn discover_executable(variable: &str, name: &str) -> Result<PathBuf, BackendCliError> {
    if let Some(configured) = std::env::var_os(variable) {
        let path = PathBuf::from(configured);
        return executable_file(&path)
            .then_some(path.clone())
            .ok_or(BackendCliError::ExecutableMissing(path));
    }
    let executable_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let candidate = directory.join(&executable_name);
        if executable_file(&candidate) {
            return Ok(candidate);
        }
        if let Some(parent) = directory.parent() {
            for candidate_directory in [
                parent.join("release"),
                parent.join("debug"),
                parent.join("bin"),
            ] {
                let candidate = candidate_directory.join(&executable_name);
                if executable_file(&candidate) {
                    return candidate.canonicalize().map_err(BackendCliError::from);
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(&executable_name))
        .find(|path| executable_file(path))
    {
        return Ok(path);
    }
    for candidate in [
        PathBuf::from("target/release").join(&executable_name),
        PathBuf::from("../target/release").join(&executable_name),
        PathBuf::from("target/debug").join(&executable_name),
        PathBuf::from("../target/debug").join(&executable_name),
        PathBuf::from("target/bin").join(&executable_name),
        PathBuf::from("../target/bin").join(&executable_name),
        PathBuf::from("result/bin").join(&executable_name),
        PathBuf::from("../result/bin").join(&executable_name),
    ] {
        if executable_file(&candidate) {
            return candidate.canonicalize().map_err(BackendCliError::from);
        }
    }
    Err(BackendCliError::ExecutableMissing(PathBuf::from(
        executable_name,
    )))
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn native_command(program: impl AsRef<OsStr>) -> Command {
    native_command_with_debug(program, crate::debug_logging::debug_logging_enabled())
}

fn native_command_with_debug(program: impl AsRef<OsStr>, debug: bool) -> Command {
    let mut command = Command::new(program);
    if debug {
        command.env("UTA_STUDIO_DEBUG", "1");
    } else {
        // The persisted app setting owns future workers, not the parent's env.
        command.env_remove("UTA_STUDIO_DEBUG");
    }
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000);
        command
    };
    command
}

pub fn read_machine_frame<R: BufRead>(
    reader: &mut R,
) -> Result<Option<serde_json::Value>, BackendCliError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(BackendCliError::from)?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(take) > MAX_MACHINE_FRAME_BYTES {
            let consumed = newline.map_or(available.len(), |index| index + 1);
            reader.consume(consumed);
            if newline.is_none() {
                loop {
                    let remainder = reader.fill_buf().map_err(BackendCliError::from)?;
                    if remainder.is_empty() {
                        break;
                    }
                    if let Some(index) = remainder.iter().position(|byte| *byte == b'\n') {
                        reader.consume(index + 1);
                        break;
                    }
                    let length = remainder.len();
                    reader.consume(length);
                }
            }
            return Err(BackendCliError::FrameTooLarge {
                limit: MAX_MACHINE_FRAME_BYTES,
            });
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(newline.map_or(take, |index| index + 1));
        if newline.is_some() {
            break;
        }
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(BackendCliError::StdoutPollution("empty line".to_string()));
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        let excerpt = String::from_utf8_lossy(&bytes[..bytes.len().min(160)]);
        BackendCliError::StdoutPollution(format!("{error}; frame starts with {excerpt:?}"))
    })
}

pub fn spawn_stderr_drain(
    stderr: ChildStderr,
) -> (Arc<Mutex<Vec<u8>>>, std::thread::JoinHandle<()>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&captured);
    let handle = std::thread::spawn(move || {
        drain_stderr(stderr, &output, crate::debug_logging::record_backend_stderr);
    });
    (captured, handle)
}

fn drain_stderr(stderr: impl Read, output: &Mutex<Vec<u8>>, mut mirror: impl FnMut(&[u8])) {
    let mut reader = BufReader::new(stderr);
    let mut buffer = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        // Mirror every raw byte before applying the existing error-memory cap.
        // The mirror records failures without tracing, panicking or stopping us.
        mirror(&buffer[..read]);
        let mut output = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let available = MAX_CAPTURED_STDERR_BYTES.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(available)]);
    }
}

pub fn stderr_text(captured: &Arc<Mutex<Vec<u8>>>) -> String {
    let bytes = captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    String::from_utf8_lossy(&bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_applied_to_commands_without_changing_process_environment() {
        let command = native_command_with_debug("unused-backend", true);
        assert!(command.get_envs().any(|(key, value)| {
            key == OsStr::new("UTA_STUDIO_DEBUG") && value == Some(OsStr::new("1"))
        }));
        let command = native_command_with_debug("unused-backend", false);
        assert!(
            command
                .get_envs()
                .any(|(key, value)| { key == OsStr::new("UTA_STUDIO_DEBUG") && value.is_none() })
        );
    }

    #[test]
    fn stderr_mirror_receives_every_byte_beyond_bounded_error_memory() {
        let bytes = vec![0xff; MAX_CAPTURED_STDERR_BYTES * 3];
        let output = Mutex::new(Vec::new());
        let mut mirrored = Vec::new();
        drain_stderr(bytes.as_slice(), &output, |chunk| {
            mirrored.extend_from_slice(chunk)
        });
        assert_eq!(mirrored, bytes);
        assert_eq!(*output.lock().unwrap(), bytes[..MAX_CAPTURED_STDERR_BYTES]);
    }

    #[test]
    fn failed_mirror_writes_do_not_stop_stderr_draining() {
        use std::io::Write;

        let bytes = vec![b'x'; MAX_CAPTURED_STDERR_BYTES * 3];
        let output = Mutex::new(Vec::new());
        let mut drained = 0;
        let mut failures = 0;
        drain_stderr(bytes.as_slice(), &output, |chunk| {
            // A zero-capacity writer fails deterministically without touching disk.
            let mut destination: &mut [u8] = &mut [];
            if destination.write_all(chunk).is_err() {
                failures += 1;
            }
            drained += chunk.len();
        });
        assert!(failures > 1);
        assert_eq!(drained, bytes.len());
        assert_eq!(output.lock().unwrap().len(), MAX_CAPTURED_STDERR_BYTES);
    }
}
