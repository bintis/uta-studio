use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerCommand {
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

pub fn emit(frame: WorkerFrame<'_>) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &frame).map_err(|error| error.to_string())?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
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
