use std::path::{Path, PathBuf};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};

pub(super) struct OutputRunGuard {
    root: PathBuf,
    committed: bool,
}

impl OutputRunGuard {
    pub(super) fn new(path: &Path) -> EngineResult<Self> {
        let root = authorize_output_root(path)?;
        let mut entries = std::fs::read_dir(&root).map_err(|error| {
            EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("could not inspect authorized output directory: {error}"),
            )
        })?;
        if entries.next().is_some() {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "authorized analysis output directory must be empty",
            ));
        }
        Ok(Self {
            root,
            committed: false,
        })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for OutputRunGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn authorize_output_root(path: &Path) -> EngineResult<PathBuf> {
    if !path.is_dir() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!(
                "authorized output directory is unavailable: {}",
                path.display()
            ),
        ));
    }
    path.canonicalize().map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not authorize output directory: {error}"),
        )
    })
}
