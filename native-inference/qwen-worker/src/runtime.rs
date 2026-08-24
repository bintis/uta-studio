use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::WorkerKind;

const GGML_COMMIT: &str = "8c63e70982c95ceb862e3a1073a2c1beef75d60a";

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    schema_version: u32,
    ggml_commit: String,
    engines: BTreeMap<String, EngineManifest>,
    libraries: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct EngineManifest {
    path: String,
    #[serde(rename = "sha256")]
    _sha256: String,
    source_commit: String,
    #[serde(rename = "runtime_recipe_digest")]
    _runtime_recipe_digest: String,
}

pub struct ValidatedRuntime {
    pub engine: PathBuf,
    pub manifest_sha256: String,
}

fn default_runtime_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/uta-studio/runtime/qwen-native-v1"))
}

pub fn validate(kind: WorkerKind) -> Result<ValidatedRuntime, String> {
    let root = std::env::var_os("UTA_STUDIO_QWEN_ENGINE_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(default_runtime_dir)
        .ok_or_else(|| "Qwen native runtime location is unavailable".to_string())?;
    let manifest_bytes = std::fs::read(root.join("runtime-manifest.json"))
        .map_err(|error| format!("Qwen native runtime manifest is unavailable: {error}"))?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("Qwen native runtime manifest is invalid: {error}"))?;
    let engine = manifest
        .engines
        .get(kind.engine_id())
        .ok_or_else(|| "Qwen runtime manifest is missing the selected engine".to_string())?;
    let relative = Path::new(&engine.path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Qwen runtime manifest contains an unsafe engine path".to_string());
    }
    if manifest.schema_version != 1
        || manifest.ggml_commit != GGML_COMMIT
        || engine.source_commit != kind.source_commit()
    {
        return Err("Qwen runtime identity does not match the selected recipe".to_string());
    }
    let path = root.join(relative);
    if !path.is_file() {
        return Err("Qwen native engine is unavailable".to_string());
    }
    if manifest.libraries.len() != 4 {
        return Err("Qwen runtime manifest has an unexpected library set".to_string());
    }
    for relative in manifest.libraries.keys() {
        let relative = Path::new(relative);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || !root.join(relative).is_file()
        {
            return Err("Qwen native runtime library validation failed".to_string());
        }
    }
    Ok(ValidatedRuntime {
        engine: path,
        manifest_sha256,
    })
}
