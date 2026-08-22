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
    sha256: String,
    source_commit: String,
    runtime_recipe_digest: String,
}

pub struct ValidatedRuntime {
    pub engine: PathBuf,
    pub manifest_sha256: String,
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest)
        .map_err(|error| format!("could not hash {}: {error}", path.display()))?;
    Ok(format!("{:x}", digest.finalize()))
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
        || engine.runtime_recipe_digest != kind.recipe_digest()
    {
        return Err("Qwen runtime identity does not match the selected recipe".to_string());
    }
    let path = root.join(relative);
    if sha256(&path)? != engine.sha256 {
        return Err("Qwen native engine hash mismatch".to_string());
    }
    if manifest.libraries.len() != 4 {
        return Err("Qwen runtime manifest has an unexpected library set".to_string());
    }
    for (relative, expected) in manifest.libraries {
        let relative = Path::new(&relative);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || sha256(&root.join(relative))? != expected
        {
            return Err("Qwen native runtime library validation failed".to_string());
        }
    }
    Ok(ValidatedRuntime {
        engine: path,
        manifest_sha256,
    })
}
