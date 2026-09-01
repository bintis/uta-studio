use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

pub const RECIPE_DIGEST: &str = "4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a";
const GGML_COMMIT: &str = "8c63e70982c95ceb862e3a1073a2c1beef75d60a";

const MODEL_IDENTITIES: [(&str, u64); 6] = [
    ("bs_roformer_leap_xe90_vocals", 267_433_600),
    ("melband_roformer_denoise_aufr33", 457_008_736),
    ("melband_roformer_dereverb_anvuew", 457_008_736),
    ("melband_roformer_inst_v2", 787_918_656),
    ("melband_roformer_harmony", 457_008_736),
    ("bs_polarformer_public_instrumental", 204_237_408),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeManifest {
    schema_version: u32,
    #[serde(rename = "recipe_digest")]
    _recipe_digest: String,
    ggml_commit: String,
    engine: RuntimeFile,
    #[serde(default)]
    libraries: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFile {
    path: String,
    #[serde(rename = "sha256")]
    _sha256: String,
}

pub struct ValidatedRuntime {
    pub engine: PathBuf,
    pub library_dir: PathBuf,
}

fn safe_relative(value: &str) -> Result<&Path, String> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("GGML runtime manifest contains an unsafe path".to_string());
    }
    Ok(path)
}

fn runtime_root() -> Result<PathBuf, String> {
    std::env::var_os("UTA_STUDIO_GGML_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share/uta-studio/runtime/ggml-vulkan-v1"))
        })
        .ok_or_else(|| "GGML Vulkan runtime location is unavailable".to_string())
}

pub fn validate_runtime() -> Result<ValidatedRuntime, String> {
    let root = runtime_root()?;
    let bytes = std::fs::read(root.join("runtime-manifest.json"))
        .map_err(|error| format!("GGML Vulkan runtime manifest is unavailable: {error}"))?;
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("GGML Vulkan runtime manifest is invalid: {error}"))?;
    if manifest.schema_version != 1 || manifest.ggml_commit != GGML_COMMIT {
        return Err("GGML Vulkan runtime identity does not match the worker recipe".to_string());
    }
    let engine = root.join(safe_relative(&manifest.engine.path)?);
    if !engine.is_file() {
        return Err("GGML RoFormer engine is unavailable".to_string());
    }
    for relative in manifest.libraries.keys() {
        if !root.join(safe_relative(relative)?).is_file() {
            return Err(format!("GGML runtime library is unavailable: {relative}"));
        }
    }
    let library_dir = root.join("lib");
    if !library_dir.is_dir() {
        return Err("GGML Vulkan runtime library directory is unavailable".to_string());
    }
    Ok(ValidatedRuntime {
        engine,
        library_dir,
    })
}

pub fn validate_model(model_id: &str, configured: &Path) -> Result<PathBuf, String> {
    let expected_size = MODEL_IDENTITIES
        .iter()
        .find(|(id, _)| *id == model_id)
        .map(|(_, size)| *size)
        .ok_or_else(|| format!("model {model_id} has no GGML Vulkan executor"))?;
    let path = configured_model_path(model_id, configured);
    let metadata = path
        .metadata()
        .map_err(|error| format!("GGUF model is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(format!("GGUF model size mismatch for {model_id}"));
    }
    Ok(path)
}

fn configured_model_path(model_id: &str, configured: &Path) -> PathBuf {
    if configured.is_file() {
        configured.to_path_buf()
    } else if model_id == "bs_roformer_leap_xe90_vocals" {
        configured.join("bs_leap_xe_voc-F32.gguf")
    } else {
        configured.join("model-fp16.gguf")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_model_has_a_full_identity() {
        assert_eq!(MODEL_IDENTITIES.len(), 6);
        for (id, size) in MODEL_IDENTITIES {
            assert!(!id.is_empty());
            assert!(size > 200_000_000);
        }
    }

    #[test]
    fn leap_directory_keeps_the_public_f32_filename() {
        assert_eq!(
            configured_model_path(
                "bs_roformer_leap_xe90_vocals",
                Path::new("managed-generation")
            ),
            Path::new("managed-generation/bs_leap_xe_voc-F32.gguf")
        );
    }
}
