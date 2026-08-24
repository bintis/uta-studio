use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::SourceIdentity;
use crate::resource::ResourceRef;
use crate::state::InstallState;

pub const INSTALL_MANIFEST_SCHEMA: &str = "uta.runtime.install-manifest";
pub const INSTALL_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallManifest {
    pub schema: String,
    pub schema_version: u32,
    pub resource: ResourceRef,
    pub catalog_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_recipe_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversion_recipe_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
    pub files: Vec<InstalledFile>,
    pub created_timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledFile {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationVerification {
    pub state: InstallState,
    pub content_digest: Option<String>,
}

impl GenerationVerification {
    fn incomplete() -> Self {
        Self {
            state: InstallState::Incomplete,
            content_digest: None,
        }
    }

    fn corrupt() -> Self {
        Self {
            state: InstallState::Corrupt,
            content_digest: None,
        }
    }
}

/// Verify a published immutable generation without creating or modifying any
/// path. Generation IDs are the full SHA-256 of the exact canonical manifest
/// bytes written by the publisher. This exhaustive form hashes every payload
/// byte and is reserved for explicit verification and execution resolution.
pub fn verify_generation(
    generation_dir: &Path,
    generation: &str,
    expected_resource: &ResourceRef,
) -> GenerationVerification {
    verify_generation_impl(generation_dir, generation, expected_resource, true)
}

/// Validate the immutable generation envelope without re-reading every model
/// byte. Publication already performs exhaustive verification before switching
/// `current.json`; read-only status/preview paths can therefore validate the
/// signed-by-digest manifest, declared file set, file types, and sizes. Explicit
/// `verify` and model resolution still call [`verify_generation`].
pub(crate) fn verify_generation_metadata(
    generation_dir: &Path,
    generation: &str,
    expected_resource: &ResourceRef,
) -> GenerationVerification {
    verify_generation_impl(generation_dir, generation, expected_resource, false)
}

fn verify_generation_impl(
    generation_dir: &Path,
    generation: &str,
    expected_resource: &ResourceRef,
    verify_content: bool,
) -> GenerationVerification {
    if !is_generation_id(generation) || !generation_dir.is_dir() {
        return GenerationVerification::incomplete();
    }
    let manifest_path = generation_dir.join("install-manifest.json");
    let bytes = match std::fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return GenerationVerification::incomplete();
        }
        Err(_) => return GenerationVerification::corrupt(),
    };
    if sha256_bytes(&bytes) != generation {
        return GenerationVerification::corrupt();
    }
    let manifest: InstallManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(_) => return GenerationVerification::corrupt(),
    };
    if manifest.schema != INSTALL_MANIFEST_SCHEMA
        || manifest.schema_version != INSTALL_MANIFEST_SCHEMA_VERSION
        || &manifest.resource != expected_resource
        || manifest.catalog_version.trim().is_empty()
        || manifest.created_timestamp.trim().is_empty()
        || manifest.files.is_empty()
        || manifest
            .source_sha256
            .as_ref()
            .is_some_and(|digest| !is_sha256(digest))
        || manifest.source.as_ref().is_some_and(|source| {
            source.sha256.as_ref().is_some_and(|source_sha256| {
                manifest.source_sha256.as_ref() != Some(source_sha256)
                    && source.converted_artifact.as_ref().is_none_or(|converted| {
                        manifest.source_sha256.as_ref() != Some(&converted.manifest_sha256)
                    })
            })
        })
        || manifest
            .model_recipe_digest
            .as_ref()
            .is_some_and(|digest| digest.trim().is_empty())
        || manifest
            .conversion_recipe_digest
            .as_ref()
            .is_some_and(|digest| digest.trim().is_empty())
        || manifest
            .runtime_recipe_digest
            .as_ref()
            .is_some_and(|digest| digest.trim().is_empty())
        || (expected_resource.kind == crate::resource::ResourceKind::Model
            && manifest.model_recipe_digest.is_none())
    {
        return GenerationVerification::corrupt();
    }

    let mut declared = BTreeSet::new();
    for file in &manifest.files {
        if !safe_relative_path(&file.path)
            || !is_sha256(&file.sha256)
            || !declared.insert(file.path.clone())
        {
            return GenerationVerification::corrupt();
        }
        let path = generation_dir.join(&file.path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return GenerationVerification::incomplete();
            }
            Err(_) => return GenerationVerification::corrupt(),
        };
        if !metadata.file_type().is_file() || metadata.len() != file.size {
            return GenerationVerification::corrupt();
        }
        if verify_content {
            match sha256_file(&path) {
                Ok(digest) if digest == file.sha256 => {}
                _ => return GenerationVerification::corrupt(),
            }
        }
    }

    let mut actual = BTreeSet::new();
    if collect_owned_files(generation_dir, generation_dir, &mut actual).is_err() {
        return GenerationVerification::corrupt();
    }
    actual.remove(Path::new("install-manifest.json"));
    if actual != declared {
        return GenerationVerification::corrupt();
    }

    GenerationVerification {
        state: InstallState::Installed,
        content_digest: Some(generation.to_string()),
    }
}

pub fn read_install_manifest(generation_dir: &Path) -> Option<InstallManifest> {
    let bytes = std::fs::read(generation_dir.join("install-manifest.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn generation_id(manifest_bytes: &[u8]) -> String {
    sha256_bytes(manifest_bytes)
}

pub fn is_generation_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256(value: &str) -> bool {
    is_generation_id(value)
}

pub(crate) fn safe_relative_path(path: &Path) -> bool {
    let portable = path.to_string_lossy();
    !path.as_os_str().is_empty()
        && !portable.contains('\\')
        && !portable.contains(':')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn collect_owned_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "symlinks are not allowed in generations",
            ));
        }
        if metadata.is_dir() {
            collect_owned_files(root, &entry.path(), files)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(std::io::Error::other)?
                .to_path_buf();
            if !safe_relative_path(&relative) || !files.insert(relative) {
                return Err(std::io::Error::other("unsafe or duplicate generation path"));
            }
        } else {
            return Err(std::io::Error::other("unsupported generation file type"));
        }
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_ids_are_full_lowercase_sha256() {
        assert!(is_generation_id(&"a".repeat(64)));
        assert!(!is_generation_id(&"A".repeat(64)));
        assert!(!is_generation_id("../generation"));
        assert!(!is_generation_id(&"a".repeat(63)));
    }

    #[test]
    fn manifest_paths_cannot_escape_a_generation() {
        assert!(safe_relative_path(Path::new("weights/model.bin")));
        for path in [
            "../model.bin",
            "/tmp/model.bin",
            "weights/../model.bin",
            "C:\\models\\model.bin",
            "C:model.bin",
            "",
        ] {
            assert!(!safe_relative_path(Path::new(path)), "{path}");
        }
        assert!(safe_relative_path(Path::new(&format!(
            "weights/{}.bin",
            "a".repeat(180)
        ))));
    }

    #[test]
    fn duplicate_manifest_file_declarations_are_corrupt() {
        let root = std::env::temp_dir().join(format!(
            "uta-runtime-duplicate-manifest-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let resource = ResourceRef::model("rmvpe").unwrap();
        let file = InstalledFile {
            path: PathBuf::from("model.bin"),
            sha256: sha256_bytes(b"model"),
            size: 5,
        };
        let manifest = InstallManifest {
            schema: INSTALL_MANIFEST_SCHEMA.to_string(),
            schema_version: INSTALL_MANIFEST_SCHEMA_VERSION,
            resource: resource.clone(),
            catalog_version: "test".to_string(),
            source: None,
            source_sha256: None,
            model_recipe_digest: Some("recipe".to_string()),
            conversion_recipe_digest: None,
            runtime_recipe_digest: None,
            files: vec![file.clone(), file],
            created_timestamp: "1".to_string(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let generation = generation_id(&bytes);
        let directory = root.join(&generation);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("model.bin"), b"model").unwrap();
        std::fs::write(directory.join("install-manifest.json"), bytes).unwrap();
        assert_eq!(
            verify_generation(&directory, &generation, &resource).state,
            InstallState::Corrupt
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
