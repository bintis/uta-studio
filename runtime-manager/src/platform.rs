use std::collections::BTreeMap;

use serde::Deserialize;

use crate::catalog::RuntimeCatalogEntry;
use crate::store::StorePaths;

#[derive(Deserialize)]
struct RuntimeManifestEnvironment {
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

/// Process environment the installed native runtime declares for the worker
/// that loads it (device selector, driver library directory, kernel cache).
/// Runtimes without a native library, or without an installed manifest,
/// declare nothing. The Engine applies these when it spawns the worker; the
/// worker cannot change its own library search path after it has started.
pub fn runtime_environment(
    entry: &RuntimeCatalogEntry,
    paths: &StorePaths,
) -> BTreeMap<String, String> {
    if entry.native_library.is_none() {
        return BTreeMap::new();
    }
    paths
        .runtime_library_root(&entry.id)
        .and_then(|root| std::fs::read(root.join("runtime-manifest.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<RuntimeManifestEnvironment>(&bytes).ok())
        .map(|manifest| manifest.environment)
        .unwrap_or_default()
}

pub fn executable_for_runtime(
    entry: &RuntimeCatalogEntry,
    paths: &StorePaths,
) -> Option<std::path::PathBuf> {
    paths.runtime_executable(&entry.id).or_else(|| {
        if entry.executable_component_id != entry.id {
            paths.runtime_executable(&entry.executable_component_id)
        } else {
            None
        }
    })
}

pub fn worker_supports_model(entry: &RuntimeCatalogEntry, model_id: &str) -> bool {
    entry.supported_models.iter().any(|id| id == model_id)
}
