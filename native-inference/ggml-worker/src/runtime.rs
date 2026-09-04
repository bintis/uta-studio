use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const RECIPE_DIGEST: &str = "dd364845b256b8adc04c291e9c79a3426fe960ca1a7beab3990fdbcdc9e7bfd2";
const GGML_COMMIT: &str = "8c63e70982c95ceb862e3a1073a2c1beef75d60a";
pub const RMVPE_SOURCE_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";
pub const RMVPE_GGUF_SHA256: &str =
    "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2";

struct ModelIdentity {
    id: &'static str,
    size: u64,
}

const MODEL_IDENTITIES: [ModelIdentity; 7] = [
    ModelIdentity {
        id: "bs_roformer_leap_xe90_vocals",
        size: 267_433_600,
    },
    ModelIdentity {
        id: "melband_roformer_denoise_aufr33",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "melband_roformer_dereverb_anvuew",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "melband_roformer_inst_v2",
        size: 787_918_656,
    },
    ModelIdentity {
        id: "melband_roformer_harmony",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "bs_polarformer_public_instrumental",
        size: 204_237_408,
    },
    ModelIdentity {
        id: "rmvpe",
        size: 361_625_344,
    },
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeManifest {
    schema_version: u32,
    recipe_digest: String,
    ggml_commit: String,
    engines: BTreeMap<String, RuntimeFile>,
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
    /// Content identity retained as output provenance. It is not used as an
    /// acceptance gate; runtime acceptance uses schema, recipe, commit, safe
    /// paths, declared files, and executable presence.
    pub manifest_content_digest: String,
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

fn engine_key(model_id: &str) -> Result<&'static str, String> {
    if model_id == "rmvpe" {
        Ok("rmvpe")
    } else if MODEL_IDENTITIES
        .iter()
        .any(|identity| identity.id == model_id)
    {
        Ok("roformer")
    } else {
        Err(format!("model {model_id} has no GGML Vulkan executor"))
    }
}

pub fn validate_runtime(model_id: &str) -> Result<ValidatedRuntime, String> {
    let root = runtime_root()?;
    let bytes = std::fs::read(root.join("runtime-manifest.json"))
        .map_err(|error| format!("GGML Vulkan runtime manifest is unavailable: {error}"))?;
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("GGML Vulkan runtime manifest is invalid: {error}"))?;
    if manifest.schema_version != 2
        || manifest.recipe_digest != RECIPE_DIGEST
        || manifest.ggml_commit != GGML_COMMIT
    {
        return Err("GGML Vulkan runtime identity does not match the worker recipe".to_string());
    }
    let key = engine_key(model_id)?;
    let engine_file = manifest
        .engines
        .get(key)
        .ok_or_else(|| format!("GGML runtime does not declare the {key} engine"))?;
    let engine = root.join(safe_relative(&engine_file.path)?);
    if !engine.is_file() {
        return Err(format!("GGML {key} engine is unavailable"));
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
        manifest_content_digest: format!("{:x}", Sha256::digest(&bytes)),
    })
}

pub fn validate_model(model_id: &str, configured: &Path) -> Result<PathBuf, String> {
    let expected_size = MODEL_IDENTITIES
        .iter()
        .find(|identity| identity.id == model_id)
        .map(|identity| identity.size)
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
    } else if model_id == "rmvpe" {
        configured.join("rmvpe-f32.gguf")
    } else {
        configured.join("model-fp16.gguf")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_model_has_a_full_identity() {
        assert_eq!(MODEL_IDENTITIES.len(), 7);
        for identity in MODEL_IDENTITIES {
            assert!(!identity.id.is_empty());
            assert!(identity.size > 200_000_000);
        }
    }

    #[test]
    fn public_f32_models_keep_their_distinct_filenames() {
        assert_eq!(
            configured_model_path(
                "bs_roformer_leap_xe90_vocals",
                Path::new("managed-generation")
            ),
            Path::new("managed-generation/bs_leap_xe_voc-F32.gguf")
        );
        assert_eq!(
            configured_model_path("rmvpe", Path::new("managed-generation")),
            Path::new("managed-generation/rmvpe-f32.gguf")
        );
    }

    #[test]
    fn rmvpe_selects_its_own_engine() {
        assert_eq!(engine_key("rmvpe"), Ok("rmvpe"));
        assert_eq!(engine_key("melband_roformer_inst_v2"), Ok("roformer"));
        assert!(engine_key("unknown").is_err());
    }
}
