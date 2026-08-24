use crate::catalog::RuntimeCatalogEntry;
use crate::store::StorePaths;

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
