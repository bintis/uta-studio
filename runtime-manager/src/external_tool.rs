use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::store::StorePaths;

pub const FUSION_AGENT_ADAPTER_ID: &str = "fusion_agent_adapter";
pub const FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT: &str = "uta.fusion_agent_adapter";
pub const FUSION_AGENT_PROTOCOL_VERSION: u32 = 3;
const EXTERNAL_TOOLS_CONFIG_VERSION: u32 = 1;
const EXTERNAL_TOOLS_CONFIG_FILE: &str = "external-tools.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FusionAgentAdapterManifestV1 {
    pub contract: String,
    pub version: u32,
    pub adapter_id: String,
    pub adapter_version: String,
    pub fusion_protocol_version: u32,
}

impl FusionAgentAdapterManifestV1 {
    pub fn validate(&self) -> RuntimeManagerResult<()> {
        if self.contract != FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT
            || self.version != 1
            || self.fusion_protocol_version != FUSION_AGENT_PROTOCOL_VERSION
            || self.adapter_id != FUSION_AGENT_ADAPTER_ID
            || self.adapter_version.trim().is_empty()
            || self.adapter_version.len() > 128
        {
            return Err(RuntimeManagerError::new(
                "tool_protocol_mismatch",
                "Fusion Agent Adapter manifest does not declare the supported Uta fusion protocol",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalToolRegistryV1 {
    version: u32,
    #[serde(default)]
    tools: BTreeMap<String, ExternalToolConfigurationV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalToolConfigurationV1 {
    executable: PathBuf,
}

pub(crate) fn configured_tool_path(
    paths: &StorePaths,
    tool_id: &str,
) -> RuntimeManagerResult<Option<PathBuf>> {
    let registry = read_registry(paths)?;
    Ok(registry
        .tools
        .get(tool_id)
        .map(|configuration| configuration.executable.clone()))
}

pub(crate) fn configure_tool_path(
    paths: &StorePaths,
    tool_id: &str,
    executable: &Path,
) -> RuntimeManagerResult<()> {
    if !valid_identity(tool_id) {
        return Err(RuntimeManagerError::new(
            "invalid_resource",
            "external tool id is invalid",
        ));
    }
    if !executable_file(executable) {
        return Err(RuntimeManagerError::new(
            "tool_unusable",
            format!(
                "external tool path is not an executable file: {}",
                executable.display()
            ),
        ));
    }
    let executable = executable.canonicalize().map_err(|error| {
        RuntimeManagerError::new(
            "tool_unusable",
            format!(
                "could not resolve external tool path {}: {error}",
                executable.display()
            ),
        )
    })?;
    let mut registry = read_registry(paths)?;
    registry.version = EXTERNAL_TOOLS_CONFIG_VERSION;
    registry.tools.insert(
        tool_id.to_string(),
        ExternalToolConfigurationV1 { executable },
    );
    write_registry(paths, &registry)
}

pub(crate) fn clear_tool_path(paths: &StorePaths, tool_id: &str) -> RuntimeManagerResult<()> {
    let Some(path) = registry_path(paths) else {
        return Err(RuntimeManagerError::new(
            "runtime_store_unconfigured",
            "runtime store is not configured",
        ));
    };
    let mut registry = read_registry(paths)?;
    registry.tools.remove(tool_id);
    if registry.tools.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                RuntimeManagerError::new(
                    "publish_failed",
                    format!("could not clear {}: {error}", path.display()),
                )
            })?;
        }
        return Ok(());
    }
    write_registry(paths, &registry)
}

pub(crate) fn fusion_adapter_manifest(
    executable: &Path,
) -> RuntimeManagerResult<FusionAgentAdapterManifestV1> {
    let candidates = fusion_adapter_manifest_candidates(executable);
    let path = candidates
        .iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            RuntimeManagerError::new(
                "tool_protocol_mismatch",
                format!(
                    "Fusion Agent Adapter manifest is missing beside {}",
                    executable.display()
                ),
            )
        })?;
    let metadata = std::fs::metadata(path).map_err(|error| {
        RuntimeManagerError::new(
            "tool_protocol_mismatch",
            format!(
                "could not inspect adapter manifest {}: {error}",
                path.display()
            ),
        )
    })?;
    if metadata.len() == 0 || metadata.len() > 64 * 1024 {
        return Err(RuntimeManagerError::new(
            "tool_protocol_mismatch",
            "Fusion Agent Adapter manifest size is invalid",
        ));
    }
    let manifest: FusionAgentAdapterManifestV1 =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| {
            RuntimeManagerError::new(
                "tool_protocol_mismatch",
                format!(
                    "could not read adapter manifest {}: {error}",
                    path.display()
                ),
            )
        })?)
        .map_err(|error| {
            RuntimeManagerError::new(
                "tool_protocol_mismatch",
                format!("Fusion Agent Adapter manifest is invalid: {error}"),
            )
        })?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn fusion_adapter_manifest_path(executable: &Path) -> PathBuf {
    let mut value = OsString::from(executable.as_os_str());
    value.push(".uta-fusion-adapter.json");
    PathBuf::from(value)
}

fn fusion_adapter_manifest_candidates(executable: &Path) -> Vec<PathBuf> {
    // Bind compatibility metadata to the exact selected executable. A shared
    // directory manifest could otherwise bless an unrelated `codex`/`claude`
    // binary that happens to live beside a real Uta adapter.
    vec![fusion_adapter_manifest_path(executable)]
}

pub(crate) fn executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(windows)]
    {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.is_file()
    }
}

fn registry_path(paths: &StorePaths) -> Option<PathBuf> {
    paths
        .store_root
        .as_ref()
        .map(|root| root.join(EXTERNAL_TOOLS_CONFIG_FILE))
}

fn read_registry(paths: &StorePaths) -> RuntimeManagerResult<ExternalToolRegistryV1> {
    let path = registry_path(paths).ok_or_else(|| {
        RuntimeManagerError::new(
            "runtime_store_unconfigured",
            "runtime store is not configured",
        )
    })?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ExternalToolRegistryV1 {
                version: EXTERNAL_TOOLS_CONFIG_VERSION,
                tools: BTreeMap::new(),
            });
        }
        Err(error) => {
            return Err(RuntimeManagerError::new(
                "resource_corrupt",
                format!("could not read {}: {error}", path.display()),
            ));
        }
    };
    let registry: ExternalToolRegistryV1 = serde_json::from_slice(&bytes).map_err(|error| {
        RuntimeManagerError::new(
            "resource_corrupt",
            format!("external tool configuration is invalid: {error}"),
        )
    })?;
    if registry.version != EXTERNAL_TOOLS_CONFIG_VERSION {
        return Err(RuntimeManagerError::new(
            "resource_corrupt",
            "external tool configuration version is unsupported",
        ));
    }
    Ok(registry)
}

fn write_registry(
    paths: &StorePaths,
    registry: &ExternalToolRegistryV1,
) -> RuntimeManagerResult<()> {
    let path = registry_path(paths).ok_or_else(|| {
        RuntimeManagerError::new(
            "runtime_store_unconfigured",
            "runtime store is not configured",
        )
    })?;
    let parent = path.parent().expect("external tool registry has parent");
    std::fs::create_dir_all(parent).map_err(|error| {
        RuntimeManagerError::new(
            "publish_failed",
            format!(
                "could not create runtime store {}: {error}",
                parent.display()
            ),
        )
    })?;
    let bytes = serde_json::to_vec_pretty(registry).map_err(|error| {
        RuntimeManagerError::new(
            "publish_failed",
            format!("could not encode external tool configuration: {error}"),
        )
    })?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{EXTERNAL_TOOLS_CONFIG_FILE}.{}-{nonce}.tmp",
        std::process::id()
    ));
    std::fs::write(&temporary, bytes).map_err(|error| {
        RuntimeManagerError::new(
            "publish_failed",
            format!("could not write {}: {error}", temporary.display()),
        )
    })?;
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| {
            RuntimeManagerError::new(
                "publish_failed",
                format!("could not replace {}: {error}", path.display()),
            )
        })?;
    }
    std::fs::rename(&temporary, &path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        RuntimeManagerError::new(
            "publish_failed",
            format!("could not publish {}: {error}", path.display()),
        )
    })
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_adapter(root: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let executable = root.join(name);
        std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manifest = FusionAgentAdapterManifestV1 {
            contract: FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT.to_string(),
            version: 1,
            adapter_id: FUSION_AGENT_ADAPTER_ID.to_string(),
            adapter_version: "1.2.3".to_string(),
            fusion_protocol_version: FUSION_AGENT_PROTOCOL_VERSION,
        };
        std::fs::write(
            fusion_adapter_manifest_path(&executable),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        executable
    }

    #[test]
    fn manifest_adapter_id_must_match_the_canonical_tool_resource() {
        let manifest = FusionAgentAdapterManifestV1 {
            contract: FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT.to_string(),
            version: 1,
            adapter_id: "provider-specific-name".to_string(),
            adapter_version: "1.2.3".to_string(),
            fusion_protocol_version: FUSION_AGENT_PROTOCOL_VERSION,
        };
        assert_eq!(
            manifest.validate().unwrap_err().code,
            "tool_protocol_mismatch"
        );
    }

    #[cfg(unix)]
    #[test]
    fn configured_path_round_trips_without_executing_the_adapter() {
        let root = std::env::temp_dir().join(format!(
            "uta-runtime-external-tool-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let executable = make_adapter(&root, "uta-fusion-agent-test");
        let canonical = executable.canonicalize().unwrap();
        let paths = StorePaths::new(root.join("store"));
        configure_tool_path(&paths, FUSION_AGENT_ADAPTER_ID, &executable).unwrap();
        assert_eq!(
            configured_tool_path(&paths, FUSION_AGENT_ADAPTER_ID)
                .unwrap()
                .as_deref(),
            Some(canonical.as_path())
        );
        clear_tool_path(&paths, FUSION_AGENT_ADAPTER_ID).unwrap();
        assert!(
            configured_tool_path(&paths, FUSION_AGENT_ADAPTER_ID)
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn runtime_manager_verifies_configures_resolves_and_clears_only_real_adapters() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "uta-runtime-adapter-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let manager =
            crate::RuntimeManager::with_default_catalog(StorePaths::new(root.join("store")))
                .unwrap();
        let resource = crate::ResourceRef::tool(FUSION_AGENT_ADAPTER_ID).unwrap();

        let plain_codex = root.join("codex");
        std::fs::write(&plain_codex, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&plain_codex, std::fs::Permissions::from_mode(0o700)).unwrap();
        let shared_manifest = FusionAgentAdapterManifestV1 {
            contract: FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT.to_string(),
            version: 1,
            adapter_id: FUSION_AGENT_ADAPTER_ID.to_string(),
            adapter_version: "1.2.3".to_string(),
            fusion_protocol_version: FUSION_AGENT_PROTOCOL_VERSION,
        };
        std::fs::write(
            root.join("uta-fusion-agent-adapter.json"),
            serde_json::to_vec(&shared_manifest).unwrap(),
        )
        .unwrap();
        let error = manager
            .configure_external_tool(&resource, &plain_codex)
            .unwrap_err();
        assert_eq!(error.code, "tool_protocol_mismatch");
        assert!(
            !manager
                .status(&resource, crate::RuntimePolicy::Production)
                .unwrap()
                .usable
        );

        let adapter = make_adapter(&root, "uta-fusion-agent-codex");
        let configured = manager
            .configure_external_tool(&resource, &adapter)
            .unwrap();
        assert!(configured.usable);
        assert_eq!(
            configured.tool_identity.as_deref(),
            Some(FUSION_AGENT_ADAPTER_ID)
        );
        let resolved = manager
            .resolve_tool(FUSION_AGENT_ADAPTER_ID, crate::RuntimePolicy::Production)
            .unwrap();
        assert_eq!(resolved.identity, FUSION_AGENT_ADAPTER_ID);
        assert_eq!(resolved.protocol_version, FUSION_AGENT_PROTOCOL_VERSION);

        std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o600)).unwrap();
        let unusable = manager
            .status(&resource, crate::RuntimePolicy::Production)
            .unwrap();
        assert!(!unusable.usable);
        assert!(
            unusable
                .reasons
                .contains(&crate::ReadinessReason::ExecutableMissing)
        );
        let cleared = manager.clear_external_tool(&resource).unwrap();
        assert!(!cleared.usable);
        assert_eq!(cleared.install_state, crate::InstallState::Absent);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_registry_is_never_silently_replaced_or_cleared() {
        let root = std::env::temp_dir().join(format!(
            "uta-runtime-corrupt-tool-registry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = StorePaths::new(root.join("store"));
        let registry = registry_path(&paths).unwrap();
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        std::fs::write(&registry, b"not-json").unwrap();
        let executable = root.join(if cfg!(windows) {
            "adapter.exe"
        } else {
            "adapter"
        });
        std::fs::write(&executable, b"adapter").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        assert_eq!(
            configured_tool_path(&paths, FUSION_AGENT_ADAPTER_ID)
                .unwrap_err()
                .code,
            "resource_corrupt"
        );
        assert_eq!(
            configure_tool_path(&paths, FUSION_AGENT_ADAPTER_ID, &executable)
                .unwrap_err()
                .code,
            "resource_corrupt"
        );
        assert_eq!(
            clear_tool_path(&paths, FUSION_AGENT_ADAPTER_ID)
                .unwrap_err()
                .code,
            "resource_corrupt"
        );
        assert_eq!(std::fs::read(&registry).unwrap(), b"not-json");
        std::fs::remove_dir_all(root).unwrap();
    }
}
