//! Last-export tracking and export-node inspection.
//!
//! Destinations are user-chosen package files, not analysis Artifact
//! revisions. Recording a path never moves source media.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cache::uta_studio_dir;
use crate::error::UtaStudioError;

thread_local! {
    static STORE_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPackageKind {
    Utz,
    UltraStar,
}

impl ExportPackageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utz => "utz",
            Self::UltraStar => "ultrastar",
        }
    }

    pub fn from_node_id(id: &str) -> Option<Self> {
        match id {
            "export.utz" => Some(Self::Utz),
            "export.ultrastar" => Some(Self::UltraStar),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Utz => "utz",
            Self::UltraStar => "txt",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ExportDestinationStore {
    songs: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportNodeInspection {
    pub kind: ExportPackageKind,
    pub ready: bool,
    pub missing: Vec<String>,
    pub last_destination: Option<PathBuf>,
    pub last_destination_exists: bool,
}

fn store_path() -> PathBuf {
    STORE_OVERRIDE
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(|| uta_studio_dir().join("export-destinations.json"))
}

#[cfg(test)]
pub fn override_export_destination_store(path: Option<PathBuf>) {
    STORE_OVERRIDE.with(|slot| *slot.borrow_mut() = path);
}

fn load_store() -> ExportDestinationStore {
    std::fs::read(store_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_store(store: &ExportDestinationStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub fn record_last_export(
    file_hash: &str,
    kind: ExportPackageKind,
    path: impl AsRef<Path>,
) -> Result<(), String> {
    let mut store = load_store();
    store
        .songs
        .entry(file_hash.to_string())
        .or_default()
        .insert(
            kind.as_str().to_string(),
            path.as_ref().to_string_lossy().into_owned(),
        );
    save_store(&store)
}

pub fn last_export_destination(file_hash: &str, kind: ExportPackageKind) -> Option<PathBuf> {
    load_store()
        .songs
        .get(file_hash)
        .and_then(|row| row.get(kind.as_str()))
        .map(PathBuf::from)
}

pub fn export_readiness(file_hash: &str) -> Result<(bool, Vec<String>), UtaStudioError> {
    crate::library_db::init_library().map_err(|error| UtaStudioError::Other(error.to_string()))?;
    let cache = crate::cache::CacheDir::new();
    let missing = crate::utz_export::missing_export_assets(&cache, file_hash);
    Ok((missing.is_empty(), missing))
}

pub fn inspect_export_node(
    file_hash: &str,
    kind: ExportPackageKind,
) -> Result<ExportNodeInspection, String> {
    let (ready, missing) = export_readiness(file_hash).map_err(|error| error.to_string())?;
    let last_destination = last_export_destination(file_hash, kind);
    let last_destination_exists = last_destination.as_ref().is_some_and(|path| path.is_file());
    Ok(ExportNodeInspection {
        kind,
        ready,
        missing,
        last_destination,
        last_destination_exists,
    })
}

pub fn validate_export_package(path: &Path, kind: ExportPackageKind) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("export file is not available: {}", path.display()));
    }
    match kind {
        ExportPackageKind::Utz => {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            utz::UtzPackage::from_bytes(&bytes).map_err(|error| error.to_string())?;
            Ok(format!("UTZ package is valid · {}", path.display()))
        }
        ExportPackageKind::UltraStar => {
            crate::ultrastar_export::validate_ultrastar_chart(path)
                .map_err(|error| error.to_string())?;
            Ok(format!("UltraStar chart is valid · {}", path.display()))
        }
    }
}

pub fn validate_export_node(file_hash: &str, kind: ExportPackageKind) -> Result<String, String> {
    let inspection = inspect_export_node(file_hash, kind)?;
    if let Some(path) = inspection.last_destination.as_ref() {
        if path.is_file() {
            return validate_export_package(path, kind);
        }
        return Err(format!(
            "last export is missing: {}. Chart ready: {}. Missing: {}",
            path.display(),
            inspection.ready,
            if inspection.missing.is_empty() {
                "none".to_string()
            } else {
                inspection.missing.join(", ")
            }
        ));
    }
    if inspection.ready {
        Ok("No last export is tracked yet. The authored chart is ready to export.".into())
    } else {
        Err(format!(
            "No last export is tracked, and the chart is not ready: {}",
            inspection.missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_reads_last_export_without_inferring_a_path() {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-export-dest-{}-{}",
            std::process::id(),
            workbench_nonce()
        ));
        std::fs::create_dir_all(&root).unwrap();
        override_export_destination_store(Some(root.join("export-destinations.json")));
        let dest = root.join("song.utz");
        std::fs::write(&dest, b"not-a-real-package").unwrap();
        record_last_export("hash-a", ExportPackageKind::Utz, &dest).unwrap();
        assert_eq!(
            last_export_destination("hash-a", ExportPackageKind::Utz).as_deref(),
            Some(dest.as_path())
        );
        assert!(last_export_destination("hash-a", ExportPackageKind::UltraStar).is_none());
        override_export_destination_store(None);
        let _ = std::fs::remove_dir_all(root);
    }

    fn workbench_nonce() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}
