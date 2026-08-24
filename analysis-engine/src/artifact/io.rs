use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{ArtifactRefV1, EngineError, EngineErrorCode, EngineResult};

const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

pub fn write_json_artifact(
    root: &Path,
    relative: &Path,
    media_type: &str,
    value: &impl Serialize,
) -> EngineResult<ArtifactRefV1> {
    let root = authorize_root(root)?;
    let target = confined_target(&root, relative)?;
    let parent = target
        .parent()
        .ok_or_else(|| output_error("artifact target has no parent"))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| output_error(format!("could not create artifact directory: {error}")))?;
    let parent = parent.canonicalize().map_err(|error| {
        output_error(format!("could not authorize artifact directory: {error}"))
    })?;
    if !parent.starts_with(&root) || target.exists() {
        return Err(output_error(
            "artifact target escaped the output root or already exists",
        ));
    }
    let leaf = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| output_error("artifact filename is invalid"))?;
    let temporary = parent.join(format!(".{leaf}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| output_error(format!("could not stage artifact: {error}")))?;
        serde_json::to_writer(&mut file, value)
            .map_err(|error| output_error(format!("could not encode artifact: {error}")))?;
        file.write_all(b"\n")
            .and_then(|_| file.sync_all())
            .map_err(|error| output_error(format!("could not sync artifact: {error}")))?;
        std::fs::rename(&temporary, &target)
            .map_err(|error| output_error(format!("could not publish artifact: {error}")))?;
        artifact_ref_for_existing(&root, relative, media_type)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn artifact_ref_for_existing(
    root: &Path,
    relative: &Path,
    media_type: &str,
) -> EngineResult<ArtifactRefV1> {
    let root = authorize_root(root)?;
    let target = confined_target(&root, relative)?;
    let metadata = std::fs::symlink_metadata(&target)
        .map_err(|error| output_error(format!("artifact is unavailable: {error}")))?;
    let canonical = target
        .canonicalize()
        .map_err(|error| output_error(format!("artifact cannot be confined: {error}")))?;
    if !metadata.file_type().is_file()
        || !canonical.starts_with(&root)
        || metadata.len() == 0
        || metadata.len() > MAX_ARTIFACT_BYTES
        || media_type.trim().is_empty()
    {
        return Err(output_error(
            "artifact is not a valid confined regular file",
        ));
    }
    let reference = ArtifactRefV1 {
        path: relative.to_path_buf(),
        media_type: media_type.to_string(),
        sha256: sha256_file(&canonical)?,
        bytes: metadata.len(),
    };
    reference.validate()?;
    Ok(reference)
}

fn authorize_root(root: &Path) -> EngineResult<PathBuf> {
    if !root.is_dir() {
        return Err(output_error("authorized artifact root is unavailable"));
    }
    root.canonicalize()
        .map_err(|error| output_error(format!("could not authorize artifact root: {error}")))
}

fn confined_target(root: &Path, relative: &Path) -> EngineResult<PathBuf> {
    let portable = relative.to_string_lossy();
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || portable.contains('\\')
        || portable.contains(':')
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(output_error("artifact path is not a safe relative path"));
    }
    Ok(root.join(relative))
}

fn sha256_file(path: &Path) -> EngineResult<String> {
    let mut file = File::open(path)
        .map_err(|error| output_error(format!("could not open artifact for hashing: {error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| output_error(format!("could not hash artifact: {error}")))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn output_error(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "uta-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn atomic_json_is_hashed_and_never_overwrites() {
        let root = root();
        let relative = Path::new("transcript/transcript.json");
        let reference = write_json_artifact(
            &root,
            relative,
            "application/json",
            &serde_json::json!({"text":"sing"}),
        )
        .unwrap();
        assert_eq!(reference.path, relative);
        assert_eq!(reference.bytes, 16);
        assert_eq!(reference.sha256.len(), 64);
        assert!(write_json_artifact(&root, relative, "application/json", &true).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn traversal_is_rejected() {
        let root = root();
        assert!(write_json_artifact(&root, Path::new("../escape.json"), "x", &true).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
