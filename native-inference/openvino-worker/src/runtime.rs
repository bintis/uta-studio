use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::protocol::COMPONENT_RECIPE;

const OPENVINO_VERSION: &str = "2026.3.0";
const OPENVINO_COMMIT: &str = "8a17657b995fd3b4a52f8484acfcf2bb61214623";
const REQUIRED_LIBRARIES: [&str; 5] = [
    "runtime/lib/intel64/libOpenCL.so.1.0.0",
    "runtime/lib/intel64/libopenvino.so.2026.3.0",
    "runtime/lib/intel64/libopenvino_c.so.2026.3.0",
    "runtime/lib/intel64/libopenvino_intel_gpu_plugin.so",
    "runtime/lib/intel64/libopenvino_onnx_frontend.so.2026.3.0",
];

#[derive(Deserialize)]
struct RuntimeManifest {
    schema_version: u32,
    openvino_version: String,
    source_commit: String,
    recipe_sha256: String,
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
    if std::env::var_os("OPENVINO_INSTALL_DIR").is_none() {
        let configured = std::env::var_os("UTA_STUDIO_OPENVINO_INSTALL_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| default_install_dir(&home))
            });
        if let Some(install_dir) = configured.filter(|path| has_runtime(path)) {
            // SAFETY: main calls this before creating threads or initializing
            // OpenVINO. No other code can concurrently inspect the process
            // environment at that point.
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

pub fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("could not open runtime library {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest)
        .map_err(|error| format!("could not hash runtime library {}: {error}", path.display()))?;
    Ok(format!("{:x}", digest.finalize()))
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
        || manifest.recipe_sha256 != COMPONENT_RECIPE
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
        let expected = manifest
            .libraries
            .get(relative)
            .ok_or_else(|| format!("OpenVINO runtime manifest is missing {relative}"))?;
        let actual = sha256(&install_dir.join(path))?;
        if &actual != expected {
            return Err(format!(
                "OpenVINO runtime library hash mismatch: {relative}"
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
}
