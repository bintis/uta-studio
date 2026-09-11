use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerCommand {
    Prepare {
        model_id: String,
        config: serde_json::Value,
    },
    Run {
        task_id: String,
        node_id: String,
        model_id: String,
        input_artifacts: Vec<PathBuf>,
        output_dir: PathBuf,
        #[serde(default)]
        config: serde_json::Value,
    },
    Cancel {
        task_id: String,
    },
    Quit,
    /// Enumerate the machine's usable GGML devices. Enumeration never creates a
    /// logical device or submits work, so this is safe to ask of a worker that
    /// has not been given a task.
    Devices,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerFrame<'a> {
    Prepared {
        model_id: &'a str,
        status: &'a str,
        message: &'a str,
        device: &'a str,
        free_bytes: Option<u64>,
    },
    Diagnostic {
        task_id: &'a str,
        message: &'a str,
    },
    Ready {
        component: &'a str,
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
    Devices {
        devices: &'a [DeviceReport],
    },
}

/// One enumerated GGML device, as the control plane sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceReport {
    /// Index this device has in the loaded GGML runtime. It is the worker's
    /// address for the device and is only meaningful within one runtime build.
    pub ggml_index: usize,
    pub name: String,
    pub description: String,
    /// `cpu`, `discrete_gpu` or `integrated_gpu`, matching the `device_class`
    /// a `run` command accepts.
    pub kind: String,
}

/// The protocol stream, once claimed: a private duplicate of the original
/// standard output. After the claim, file descriptor 1 is redirected to
/// standard error so that native libraries which print to stdout (oneDNN
/// verbose output, driver build logs) become diagnostics instead of
/// corrupting the NDJSON protocol the Analysis Engine parses.
static PROTOCOL_STREAM: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> =
    std::sync::OnceLock::new();

/// Claims standard output for the protocol before any native library is
/// loaded. Must run once, on the main thread, before the first frame.
#[cfg(unix)]
pub fn claim_protocol_stream() -> Result<(), String> {
    use std::os::unix::io::FromRawFd;
    if PROTOCOL_STREAM.get().is_some() {
        return Ok(());
    }
    // SAFETY: plain POSIX descriptor duplication on the process's own
    // standard streams; the duplicate is owned by the File below and
    // descriptor 1 keeps a valid target (standard error) afterwards.
    let (protocol, redirected) = unsafe { (libc::dup(1), libc::dup2(2, 1)) };
    if protocol < 0 || redirected < 0 {
        return Err("could not claim the worker protocol stream".to_string());
    }
    let file = unsafe { std::fs::File::from_raw_fd(protocol) };
    PROTOCOL_STREAM
        .set(std::sync::Mutex::new(file))
        .map_err(|_| "worker protocol stream was already claimed".to_string())
}

#[cfg(not(unix))]
pub fn claim_protocol_stream() -> Result<(), String> {
    Ok(())
}

pub fn emit(frame: WorkerFrame<'_>) -> Result<(), String> {
    let encoded = serde_json::to_vec(&frame).map_err(|error| error.to_string())?;
    match PROTOCOL_STREAM.get() {
        Some(stream) => {
            let mut stream = stream
                .lock()
                .map_err(|_| "worker protocol stream is poisoned".to_string())?;
            stream
                .write_all(&encoded)
                .and_then(|()| stream.write_all(b"\n"))
                .and_then(|()| stream.flush())
                .map_err(|error| error.to_string())
        }
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&encoded)
                .and_then(|()| stdout.write_all(b"\n"))
                .and_then(|()| stdout.flush())
                .map_err(|error| error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_devices_command_parses_without_protocol_field() {
        let command: WorkerCommand = serde_json::from_str(r#"{"type":"devices"}"#).unwrap();
        assert!(matches!(command, WorkerCommand::Devices));
    }

    #[test]
    fn a_device_report_names_the_class_a_run_command_accepts() {
        let devices = [DeviceReport {
            ggml_index: 0,
            name: "Vulkan0".to_string(),
            description: "Intel(R) Arc(tm) B580 Graphics".to_string(),
            kind: "discrete_gpu".to_string(),
        }];
        let encoded = serde_json::to_string(&WorkerFrame::Devices { devices: &devices }).unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded["type"], "devices");
        assert_eq!(decoded["devices"][0]["kind"], "discrete_gpu");
        assert_eq!(decoded["devices"][0]["ggml_index"], 0);
    }

    #[test]
    fn an_unknown_command_type_is_rejected_rather_than_defaulted() {
        assert!(serde_json::from_str::<WorkerCommand>(r#"{"type":"reboot"}"#).is_err());
    }
}
