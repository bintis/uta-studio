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
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
        .map(|directory| directory.join(&executable_name))
        .filter(|path| executable_file(path))
    {
        return Ok(path);
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
        PathBuf::from("target/debug").join(&executable_name),
        PathBuf::from("../target/debug").join(&executable_name),
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
    let command = Command::new(program);
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
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let mut output = output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let available = MAX_CAPTURED_STDERR_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..read.min(available)]);
        }
    });
    (captured, handle)
}

pub fn stderr_text(captured: &Arc<Mutex<Vec<u8>>>) -> String {
    let bytes = captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    String::from_utf8_lossy(&bytes).trim().to_string()
}
