use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::resolver::RuntimeManager;
use crate::runtime_lock::native_runtime_lock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub schema: String,
    pub schema_version: u32,
    pub os: String,
    pub architecture: String,
    pub checks: Vec<DiagnosticCheck>,
}

pub fn run_doctor(manager: &RuntimeManager) -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(DiagnosticCheck {
        id: "platform".to_string(),
        severity: DiagnosticSeverity::Info,
        message: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
        path: None,
    });
    checks.push(match native_runtime_lock() {
        Ok(lock) => DiagnosticCheck {
            id: "runtime_lock".to_string(),
            severity: DiagnosticSeverity::Ok,
            message: format!(
                "runtime lock {} parsed ({})",
                lock.document_version, lock.status
            ),
            path: None,
        },
        Err(error) => DiagnosticCheck {
            id: "runtime_lock".to_string(),
            severity: DiagnosticSeverity::Error,
            message: format!("runtime lock parse failed: {error}"),
            path: None,
        },
    });

    let paths = manager.paths();
    checks.push(match &paths.store_root {
        Some(root) if root.exists() => DiagnosticCheck {
            id: "store_root".to_string(),
            severity: DiagnosticSeverity::Ok,
            message: "runtime store root exists".to_string(),
            path: Some(root.clone()),
        },
        Some(root) => DiagnosticCheck {
            id: "store_root".to_string(),
            severity: DiagnosticSeverity::Warning,
            message: "runtime store root is configured but does not exist".to_string(),
            path: Some(root.clone()),
        },
        None => DiagnosticCheck {
            id: "store_root".to_string(),
            severity: DiagnosticSeverity::Info,
            message: "managed runtime store root is not configured".to_string(),
            path: None,
        },
    });

    let gpu_devices = visible_gpu_devices();
    checks.push(DiagnosticCheck {
        id: "gpu_devices".to_string(),
        severity: if gpu_devices.is_empty() {
            DiagnosticSeverity::Warning
        } else {
            DiagnosticSeverity::Ok
        },
        message: if gpu_devices.is_empty() {
            "no native GPU device node was detected".to_string()
        } else {
            format!("visible GPU devices: {}", gpu_devices.join(", "))
        },
        path: None,
    });
    let vulkan_loader = vulkan_loader_path();
    checks.push(DiagnosticCheck {
        id: "vulkan_loader".to_string(),
        severity: if vulkan_loader.is_some() {
            DiagnosticSeverity::Ok
        } else {
            DiagnosticSeverity::Warning
        },
        message: if vulkan_loader.is_some() {
            "Vulkan loader is visible".to_string()
        } else {
            "Vulkan loader was not found in configured loader paths".to_string()
        },
        path: vulkan_loader,
    });

    for runtime in manager.catalog().runtimes.values() {
        let executable = crate::platform::executable_for_runtime(runtime, paths);
        checks.push(DiagnosticCheck {
            id: format!("runtime_executable:{}", runtime.id),
            severity: if executable.is_some() {
                DiagnosticSeverity::Ok
            } else {
                DiagnosticSeverity::Warning
            },
            message: if executable.is_some() {
                format!("{} executable is visible", runtime.display_name)
            } else {
                format!("{} executable is not visible", runtime.display_name)
            },
            path: executable,
        });
    }

    let openvino = manager
        .catalog()
        .runtime("openvino_2026_3")
        .and_then(|runtime| crate::platform::executable_for_runtime(runtime, paths));
    checks.push(DiagnosticCheck {
        id: "openvino_gpu".to_string(),
        severity: if openvino.is_some() && !gpu_devices.is_empty() {
            DiagnosticSeverity::Ok
        } else {
            DiagnosticSeverity::Warning
        },
        message: if openvino.is_some() && !gpu_devices.is_empty() {
            "OpenVINO worker and a GPU device are visible; smoke verifies execution".to_string()
        } else {
            "OpenVINO GPU prerequisites are incomplete".to_string()
        },
        path: openvino,
    });

    checks.push(store_permission_check(paths.store_root.as_deref()));
    checks.push(free_disk_check(paths.store_root.as_deref()));

    let ffmpeg = paths.tool_executable("ffmpeg");
    checks.push(DiagnosticCheck {
        id: "tool:ffmpeg".to_string(),
        severity: if ffmpeg.is_some() {
            DiagnosticSeverity::Ok
        } else {
            DiagnosticSeverity::Warning
        },
        message: if ffmpeg.is_some() {
            "ffmpeg executable is visible".to_string()
        } else {
            "ffmpeg executable is not configured in Runtime Manager paths".to_string()
        },
        path: ffmpeg,
    });

    DoctorReport {
        schema: "uta.runtime.doctor".to_string(),
        schema_version: 1,
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        checks,
    }
}

fn visible_gpu_devices() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/dev/dri")
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("renderD") || name.starts_with("card"))
            .collect()
    }
    #[cfg(not(target_os = "linux"))]
    Vec::new()
}

fn vulkan_loader_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UTA_STUDIO_VULKAN_LOADER_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    let filename = if cfg!(windows) {
        "vulkan-1.dll"
    } else {
        "libvulkan.so.1"
    };
    let mut directories = std::env::var_os(if cfg!(windows) {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    })
    .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
    .unwrap_or_default();
    if cfg!(target_os = "linux") {
        directories.extend([PathBuf::from("/usr/lib"), PathBuf::from("/usr/lib64")]);
    }
    directories
        .into_iter()
        .map(|directory| directory.join(filename))
        .find(|path| path.is_file())
}

fn store_permission_check(root: Option<&std::path::Path>) -> DiagnosticCheck {
    match root {
        Some(root) if root.is_dir() => match std::fs::read_dir(root) {
            Ok(_) => DiagnosticCheck {
                id: "store_permissions".to_string(),
                severity: DiagnosticSeverity::Ok,
                message: "runtime store is readable; mutation performs a separate write check"
                    .to_string(),
                path: Some(root.to_path_buf()),
            },
            Err(error) => DiagnosticCheck {
                id: "store_permissions".to_string(),
                severity: DiagnosticSeverity::Error,
                message: format!("runtime store is not readable: {error}"),
                path: Some(root.to_path_buf()),
            },
        },
        Some(root) => DiagnosticCheck {
            id: "store_permissions".to_string(),
            severity: DiagnosticSeverity::Warning,
            message: "runtime store does not exist; doctor will not create it to test writes"
                .to_string(),
            path: Some(root.to_path_buf()),
        },
        None => DiagnosticCheck {
            id: "store_permissions".to_string(),
            severity: DiagnosticSeverity::Info,
            message: "runtime store is not configured".to_string(),
            path: None,
        },
    }
}

fn free_disk_check(root: Option<&std::path::Path>) -> DiagnosticCheck {
    let existing = root.and_then(|root| {
        root.ancestors()
            .find(|candidate| candidate.exists())
            .map(std::path::Path::to_path_buf)
    });
    match existing
        .as_deref()
        .and_then(|path| fs2::available_space(path).ok())
    {
        Some(bytes) => DiagnosticCheck {
            id: "free_disk".to_string(),
            severity: DiagnosticSeverity::Info,
            message: format!("{bytes} bytes are available for explicit runtime mutations"),
            path: root.map(std::path::Path::to_path_buf),
        },
        None => DiagnosticCheck {
            id: "free_disk".to_string(),
            severity: DiagnosticSeverity::Warning,
            message: "free disk space could not be determined without changing the store"
                .to_string(),
            path: root.map(std::path::Path::to_path_buf),
        },
    }
}
