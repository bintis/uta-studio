use std::io::Write;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const COMPONENT_RECIPE: &str =
    "bdeac2a4e1299e4bf82cb2d4edf64c7bdbc613fa40f58727c58793cf7f1a4093";

#[cfg(unix)]
static PROTOCOL_STDOUT: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

/// Keep the NDJSON channel on a duplicated stdout descriptor while native
/// OpenVINO/oneDNN diagnostics are redirected to stderr. Some plugins write
/// directly to fd 1 and bypass Rust's stdout handle, so process-level isolation
/// is required to preserve the machine protocol.
#[cfg(unix)]
pub fn isolate_native_stdout() -> Result<(), String> {
    unsafe extern "C" {
        fn dup(old_fd: i32) -> i32;
        fn dup2(old_fd: i32, new_fd: i32) -> i32;
    }
    let protocol_fd = unsafe { dup(1) };
    if protocol_fd < 0 {
        return Err("could not duplicate the OpenVINO Worker protocol fd".to_string());
    }
    if unsafe { dup2(2, 1) } < 0 {
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        let _ = unsafe { close(protocol_fd) };
        return Err("could not isolate native OpenVINO stdout diagnostics".to_string());
    }
    let protocol = unsafe { std::fs::File::from_raw_fd(protocol_fd) };
    PROTOCOL_STDOUT
        .set(Mutex::new(protocol))
        .map_err(|_| "OpenVINO Worker protocol fd was initialized twice".to_string())
}

#[cfg(not(unix))]
pub fn isolate_native_stdout() -> Result<(), String> {
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerCommand {
    Run {
        protocol: u32,
        task_id: String,
        node_id: String,
        model_id: String,
        input_artifacts: Vec<PathBuf>,
        output_dir: PathBuf,
        #[serde(default)]
        config: serde_json::Value,
    },
    Cancel {
        protocol: u32,
        task_id: String,
    },
    Quit {
        protocol: u32,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerFrame<'a> {
    Ready {
        protocol: u32,
        component: &'a str,
        runtime_recipe_digest: &'a str,
    },
    Progress {
        task_id: &'a str,
        fraction: f32,
        message: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        work_units_completed: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        work_units_total: Option<u64>,
    },
    Output {
        task_id: &'a str,
        artifact: &'a str,
        path: &'a std::path::Path,
        media_type: &'a str,
    },
    Done {
        task_id: &'a str,
        status: &'a str,
    },
    Error {
        task_id: Option<&'a str>,
        code: &'a str,
        message: &'a str,
        retryable: bool,
    },
}

pub fn emit(frame: WorkerFrame<'_>) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(protocol) = PROTOCOL_STDOUT.get() {
        let mut protocol = protocol
            .lock()
            .map_err(|_| "OpenVINO Worker protocol fd lock is poisoned".to_string())?;
        serde_json::to_writer(&mut *protocol, &frame).map_err(|error| error.to_string())?;
        protocol
            .write_all(b"\n")
            .map_err(|error| error.to_string())?;
        return protocol.flush().map_err(|error| error.to_string());
    }

    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &frame).map_err(|error| error.to_string())?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

pub fn command_protocol(command: &WorkerCommand) -> u32 {
    match command {
        WorkerCommand::Run { protocol, .. }
        | WorkerCommand::Cancel { protocol, .. }
        | WorkerCommand::Quit { protocol } => *protocol,
    }
}
