use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceDevice {
    Cpu,
    Gpu,
}

impl InferenceDevice {
    pub fn openvino(self) -> openvino::DeviceType<'static> {
        match self {
            Self::Cpu => openvino::DeviceType::CPU,
            Self::Gpu => openvino::DeviceType::GPU,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
        }
    }

    pub fn evidence_backend(self) -> &'static str {
        match self {
            Self::Cpu => "openvino_cpu",
            Self::Gpu => "openvino_gpu",
        }
    }
}

/// Select an OpenVINO device only from an explicit worker command. Missing
/// configuration preserves the established GPU route; malformed or conflicting
/// values fail closed and never become an automatic CPU fallback.
pub fn inference_device(config: &serde_json::Value) -> Result<InferenceDevice, String> {
    let backend = config.get("backend").and_then(serde_json::Value::as_str);
    let device = config.get("device").and_then(serde_json::Value::as_str);
    let from_backend = match backend {
        None => None,
        Some("openvino_cpu") => Some(InferenceDevice::Cpu),
        Some("openvino_gpu") => Some(InferenceDevice::Gpu),
        Some(other) => return Err(format!("unsupported OpenVINO backend: {other}")),
    };
    let from_device = match device {
        None => None,
        Some(value) if value.eq_ignore_ascii_case("cpu") => Some(InferenceDevice::Cpu),
        Some(value) if value.eq_ignore_ascii_case("gpu") => Some(InferenceDevice::Gpu),
        Some(other) => return Err(format!("unsupported OpenVINO device: {other}")),
    };
    if from_backend.is_some() && from_device.is_some() && from_backend != from_device {
        return Err("OpenVINO backend and device selections conflict".to_string());
    }
    Ok(from_backend.or(from_device).unwrap_or(InferenceDevice::Gpu))
}

pub fn configure_inference_core(
    core: &mut openvino::Core,
    device: InferenceDevice,
) -> Result<(), String> {
    let openvino_device = device.openvino();
    if !core
        .available_devices()
        .map_err(|error| format!("could not enumerate OpenVINO devices: {error}"))?
        .contains(&openvino_device)
    {
        return Err(format!(
            "explicit OpenVINO {} device is unavailable; fallback is forbidden",
            device.label()
        ));
    }
    core.set_properties(
        &openvino_device,
        [
            (openvino::RwPropertyKey::HintInferencePrecision, "f32"),
            (openvino::RwPropertyKey::HintExecutionMode, "ACCURACY"),
        ],
    )
    .map_err(|error| {
        format!(
            "could not configure OpenVINO {} accuracy mode: {error}",
            device.label()
        )
    })?;
    if device == InferenceDevice::Gpu {
        configure_low_impact_gpu_queue_for(core, &openvino_device)?;
    }
    Ok(())
}

const OPENVINO_VERSION: &str = "2026.3.0";
const OPENVINO_COMMIT: &str = "8a17657b995fd3b4a52f8484acfcf2bb61214623";
const REQUIRED_LIBRARIES: [&str; 6] = [
    "runtime/lib/intel64/libOpenCL.so.1.0.0",
    "runtime/lib/intel64/libopenvino.so.2026.3.0",
    "runtime/lib/intel64/libopenvino_c.so.2026.3.0",
    "runtime/lib/intel64/libopenvino_intel_cpu_plugin.so",
    "runtime/lib/intel64/libopenvino_intel_gpu_plugin.so",
    "runtime/lib/intel64/libopenvino_onnx_frontend.so.2026.3.0",
];

#[derive(Deserialize)]
struct RuntimeManifest {
    schema_version: u32,
    openvino_version: String,
    source_commit: String,
    #[serde(rename = "recipe_sha256")]
    _recipe_sha256: String,
    libraries: BTreeMap<String, String>,
}

fn default_install_dir(home: &Path) -> PathBuf {
    home.join(".local")
        .join("share")
        .join("uta-studio")
        .join("runtime")
        .join(format!("openvino-{OPENVINO_VERSION}"))
}

fn has_runtime(install_dir: &Path) -> bool {
    if cfg!(target_os = "windows") {
        install_dir
            .join("runtime/bin/intel64/Release/openvino_c.dll")
            .is_file()
    } else {
        install_dir
            .join("runtime/lib/intel64/libopenvino_c.so")
            .is_file()
    }
}

pub fn configure_process_environment() {
    // Match the retired GGML conservative execution profile as closely as the
    // pinned OpenVINO OCL plugin permits. OpenVINO defaults its GPU command
    // queue to out-of-order even for a synchronous InferRequest; force the
    // release-internal in-order option before the plugin is initialized.
    // oneDNN and CM are also excluded at build time, but repeat those gates at
    // runtime and disable low-precision graph transformations so an inherited
    // environment cannot silently re-enable an accelerated implementation.
    for (name, value) in [
        ("OV_GPU_QUEUE_TYPE", "in-order"),
        ("OV_GPU_USE_ONEDNN", "0"),
        ("OV_GPU_USE_CM", "0"),
        ("OV_LP_TRANSFORMS_MODE", "0"),
    ] {
        // SAFETY: main calls this before creating threads or initializing
        // OpenVINO. No other code can concurrently inspect the process
        // environment at that point.
        unsafe { std::env::set_var(name, value) };
    }

    if std::env::var_os("OPENVINO_INSTALL_DIR").is_none() {
        let configured = std::env::var_os("UTA_STUDIO_OPENVINO_INSTALL_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| default_install_dir(&home))
            });
        if let Some(install_dir) = configured.filter(|path| has_runtime(path)) {
            // SAFETY: see the process-startup invariant above.
            unsafe { std::env::set_var("OPENVINO_INSTALL_DIR", install_dir) };
        }
    }

    #[cfg(target_os = "linux")]
    if std::env::var_os("OCL_ICD_VENDORS").is_none() {
        let nixos_vendors = Path::new("/run/opengl-driver/etc/OpenCL/vendors");
        if nixos_vendors.is_dir() {
            // SAFETY: see the process-startup invariant above.
            unsafe { std::env::set_var("OCL_ICD_VENDORS", nixos_vendors) };
        }
    }
}

pub fn configure_low_impact_gpu_queue(core: &mut openvino::Core) -> Result<(), String> {
    configure_low_impact_gpu_queue_for(core, &openvino::DeviceType::GPU)
}

pub fn configure_low_impact_gpu_queue_for(
    core: &mut openvino::Core,
    device: &openvino::DeviceType<'_>,
) -> Result<(), String> {
    core.set_properties(
        device,
        [
            // Keep OpenVINO from choosing throughput-oriented concurrency even
            // though the Worker currently submits inference synchronously.
            (openvino::RwPropertyKey::HintPerformanceMode, "LATENCY"),
            (openvino::RwPropertyKey::NumStreams, "1"),
            (openvino::RwPropertyKey::HintNumRequests, "1"),
            // Apply all three Intel GPU low-impact scheduling hints. These are
            // plugin hints, not hard device power or frequency limits.
            (
                openvino::RwPropertyKey::Other("GPU_QUEUE_THROTTLE".into()),
                "LOW",
            ),
            (
                openvino::RwPropertyKey::Other("GPU_QUEUE_PRIORITY".into()),
                "LOW",
            ),
            (
                openvino::RwPropertyKey::Other("GPU_HOST_TASK_PRIORITY".into()),
                "LOW",
            ),
        ],
    )
    .map_err(|error| format!("could not configure low-impact OpenVINO GPU queue: {error}"))
}

pub fn validate_runtime() -> Result<String, String> {
    let install_dir = std::env::var_os("OPENVINO_INSTALL_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "OpenVINO runtime is not installed; use Settings > Models & runtime".to_string()
        })?;
    let manifest_path = install_dir.join("runtime-manifest.json");
    let bytes = std::fs::read(&manifest_path).map_err(|error| {
        format!(
            "source-built OpenVINO runtime manifest is unavailable at {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("OpenVINO runtime manifest is invalid: {error}"))?;
    if manifest.schema_version != 1
        || manifest.openvino_version != OPENVINO_VERSION
        || manifest.source_commit != OPENVINO_COMMIT
    {
        return Err("OpenVINO runtime identity does not match the worker recipe".to_string());
    }
    if manifest.libraries.len() != REQUIRED_LIBRARIES.len() {
        return Err("OpenVINO runtime manifest has an unexpected library set".to_string());
    }
    for relative in REQUIRED_LIBRARIES {
        let path = Path::new(relative);
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("OpenVINO runtime manifest contains an unsafe path".to_string());
        }
        manifest
            .libraries
            .get(relative)
            .ok_or_else(|| format!("OpenVINO runtime manifest is missing {relative}"))?;
        if !install_dir.join(path).is_file() {
            return Err(format!(
                "OpenVINO runtime library is unavailable: {relative}"
            ));
        }
    }
    Ok(manifest_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_runtime_path_is_product_scoped_and_versioned() {
        assert_eq!(
            default_install_dir(Path::new("/home/tester")),
            PathBuf::from("/home/tester/.local/share/uta-studio/runtime/openvino-2026.3.0")
        );
    }

    #[test]
    fn inference_device_is_explicit_and_never_falls_back() {
        assert_eq!(
            inference_device(&serde_json::json!({})).unwrap(),
            InferenceDevice::Gpu
        );
        assert_eq!(
            inference_device(&serde_json::json!({"backend":"openvino_cpu"})).unwrap(),
            InferenceDevice::Cpu
        );
        assert_eq!(
            inference_device(&serde_json::json!({"device":"CPU"})).unwrap(),
            InferenceDevice::Cpu
        );
        assert!(
            inference_device(&serde_json::json!({"backend":"openvino_cpu","device":"gpu"}))
                .is_err()
        );
        assert!(inference_device(&serde_json::json!({"backend":"auto"})).is_err());
    }
}
