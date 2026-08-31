//! Runtime Manager-owned discovery and selection for native Fusion Agent
//! provider integrations. Provider CLIs are observed through PATH only; this
//! module never launches them or reports authentication as ready.

use std::ffi::OsStr;

use serde::{Deserialize, Serialize};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::external_tool::{
    FUSION_AGENT_ADAPTER_ID, FusionAgentAdapterManifestV1, clear_fusion_provider,
    configure_fusion_provider, configured_fusion_provider, fusion_adapter_manifest,
};
use crate::store::StorePaths;

pub const FUSION_PROVIDER_NETWORK_DISCLOSURE: &str = "The selected provider may contact an external AI service and incur provider charges. Credentials remain owned by the provider CLI; Runtime Manager does not inspect or store them.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FusionProviderStatus {
    pub provider: String,
    pub display_name: String,
    pub executable_name: String,
    /// A PATH executable was found. This is not an authentication check.
    pub available: bool,
    /// A manifest-verified provider-specific native adapter was found.
    pub adapter_available: bool,
    pub usable: bool,
    pub selected: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FusionProviderReport {
    pub adapter_resource: String,
    pub selected_provider: Option<String>,
    pub providers: Vec<FusionProviderStatus>,
    pub network_disclosure: String,
}

#[derive(Debug, Clone)]
struct AdapterDiscovery {
    adapter: Option<std::path::PathBuf>,
    manifest: Option<FusionAgentAdapterManifestV1>,
    manifest_error: bool,
}

pub fn provider_report(paths: &StorePaths) -> RuntimeManagerResult<FusionProviderReport> {
    let search_path = std::env::var_os("PATH");
    provider_report_in_path(paths, search_path.as_deref())
}

/// Read-only discovery with an explicit PATH value. Runtime Manager's normal
/// entry point uses the process PATH; the injected form keeps fixture tests
/// isolated and never mutates the test process environment.
pub fn provider_report_in_path(
    paths: &StorePaths,
    search_path: Option<&OsStr>,
) -> RuntimeManagerResult<FusionProviderReport> {
    let selected = configured_fusion_provider(paths)?;
    if let Some(provider) = selected.as_deref() {
        uta_fusion_agent_adapter::Provider::parse(provider).map_err(|_| {
            RuntimeManagerError::new(
                "resource_corrupt",
                "persisted Fusion provider selection is unsupported",
            )
        })?;
    }
    let providers = uta_fusion_agent_adapter::Provider::ALL
        .into_iter()
        .map(|provider| {
            let provider_available = search_path
                .and_then(|path| {
                    uta_fusion_agent_adapter::discover_provider_in_path(provider, path)
                })
                .is_some();
            let discovery = discover_adapter(paths, provider);
            let adapter_available = discovery.manifest.is_some();
            let mut reasons = Vec::new();
            if !provider_available {
                reasons.push("provider_cli_missing".to_string());
            }
            if discovery.adapter.is_none() {
                reasons.push("adapter_missing".to_string());
            } else if discovery.manifest_error {
                reasons.push("adapter_protocol_mismatch".to_string());
            }
            FusionProviderStatus {
                provider: provider.id().to_string(),
                display_name: provider.display_name().to_string(),
                executable_name: provider.executable_name().to_string(),
                available: provider_available,
                adapter_available,
                usable: provider_available && adapter_available,
                selected: selected.as_deref() == Some(provider.id()),
                reasons,
                adapter_identity: discovery
                    .manifest
                    .as_ref()
                    .map(|manifest| manifest.adapter_id.clone()),
                adapter_version: discovery
                    .manifest
                    .as_ref()
                    .map(|manifest| manifest.adapter_version.clone()),
                protocol_version: discovery
                    .manifest
                    .as_ref()
                    .map(|manifest| manifest.fusion_protocol_version),
            }
        })
        .collect();
    Ok(FusionProviderReport {
        adapter_resource: format!("tool:{FUSION_AGENT_ADAPTER_ID}"),
        selected_provider: selected,
        providers,
        network_disclosure: FUSION_PROVIDER_NETWORK_DISCLOSURE.to_string(),
    })
}

pub fn select_provider(
    paths: &StorePaths,
    provider_id: &str,
) -> RuntimeManagerResult<FusionProviderReport> {
    let search_path = std::env::var_os("PATH");
    select_provider_in_path(paths, provider_id, search_path.as_deref())
}

pub fn select_provider_in_path(
    paths: &StorePaths,
    provider_id: &str,
    search_path: Option<&OsStr>,
) -> RuntimeManagerResult<FusionProviderReport> {
    let provider = uta_fusion_agent_adapter::Provider::parse(provider_id).map_err(|_| {
        RuntimeManagerError::new(
            "invalid_resource",
            format!("unsupported Fusion provider: {provider_id}"),
        )
    })?;
    let report = provider_report_in_path(paths, search_path)?;
    let status = report
        .providers
        .iter()
        .find(|status| status.provider == provider.id())
        .expect("all providers are represented");
    if !status.usable {
        return Err(RuntimeManagerError::new(
            "tool_unusable",
            format!(
                "Fusion provider {} is unavailable or its native adapter is not manifest-verified",
                provider.display_name()
            ),
        )
        .with_resource(format!("tool:{FUSION_AGENT_ADAPTER_ID}")));
    }
    configure_fusion_provider(paths, provider.id())?;
    provider_report_in_path(paths, search_path)
}

pub fn clear_provider(paths: &StorePaths) -> RuntimeManagerResult<FusionProviderReport> {
    clear_fusion_provider(paths)?;
    provider_report(paths)
}

/// Resolve the selected provider-specific adapter. An explicit legacy
/// `configure-tool` path remains authoritative only when no provider selection
/// exists; selecting a provider always chooses its own discovered adapter.
pub(crate) fn selected_adapter_path(
    paths: &StorePaths,
) -> RuntimeManagerResult<Option<std::path::PathBuf>> {
    if let Some(provider) = configured_fusion_provider(paths)? {
        let provider = uta_fusion_agent_adapter::Provider::parse(&provider).map_err(|_| {
            RuntimeManagerError::new(
                "resource_corrupt",
                "persisted Fusion provider selection is unsupported",
            )
        })?;
        return Ok(paths.fusion_adapter_fallback_path(provider.id()));
    }
    Ok(None)
}

pub(crate) fn selected_provider_available(paths: &StorePaths) -> RuntimeManagerResult<bool> {
    let Some(provider) = configured_fusion_provider(paths)? else {
        return Ok(true);
    };
    let provider = uta_fusion_agent_adapter::Provider::parse(&provider).map_err(|_| {
        RuntimeManagerError::new(
            "resource_corrupt",
            "persisted Fusion provider selection is unsupported",
        )
    })?;
    Ok(uta_fusion_agent_adapter::discover_provider(provider).is_some())
}

fn discover_adapter(
    paths: &StorePaths,
    provider: uta_fusion_agent_adapter::Provider,
) -> AdapterDiscovery {
    let adapter = paths.fusion_adapter_fallback_path(provider.id());
    let Some(adapter_path) = adapter.as_deref() else {
        return AdapterDiscovery {
            adapter: None,
            manifest: None,
            manifest_error: false,
        };
    };
    match fusion_adapter_manifest(adapter_path) {
        Ok(manifest) => AdapterDiscovery {
            adapter,
            manifest: Some(manifest),
            manifest_error: false,
        },
        Err(_) => AdapterDiscovery {
            adapter,
            manifest: None,
            manifest_error: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    fn executable(path: &Path, body: &[u8]) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    fn manifest(path: &Path) {
        std::fs::write(
            fusion_adapter_manifest_path(path),
            serde_json::json!({
                "contract": crate::external_tool::FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT,
                "version": 1,
                "adapter_id": FUSION_AGENT_ADAPTER_ID,
                "adapter_version": "test",
                "fusion_protocol_version": crate::external_tool::FUSION_AGENT_PROTOCOL_VERSION
            })
            .to_string(),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn fusion_adapter_manifest_path(path: &Path) -> PathBuf {
        crate::external_tool::fusion_adapter_manifest_path(path)
    }

    #[test]
    fn report_has_three_provider_rows_without_running_provider_clis() {
        let report = provider_report(&StorePaths::new(
            std::env::temp_dir().join(format!("uta-fusion-provider-report-{}", std::process::id())),
        ))
        .unwrap();
        assert_eq!(report.providers.len(), 3);
        assert_eq!(report.providers[0].provider, "pi");
        assert_eq!(report.providers[1].provider, "codex");
        assert_eq!(report.providers[2].provider, "claude");
        assert!(report.network_disclosure.contains("external AI"));
    }

    #[cfg(unix)]
    #[test]
    fn selecting_provider_requires_both_cli_and_manifest_adapter() {
        let root = std::env::temp_dir().join(format!(
            "uta-fusion-provider-select-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let adapter = root.join("uta-fusion-agent-pi");
        executable(&adapter, b"#!/bin/sh\nexit 0\n");
        manifest(&adapter);
        let provider_cli = root.join("pi");
        executable(&provider_cli, b"#!/bin/sh\nexit 0\n");
        let paths =
            StorePaths::new(root.join("store")).with_fusion_adapter_fallback("pi", adapter.clone());
        let report = select_provider_in_path(&paths, "pi", Some(root.as_os_str())).unwrap();
        assert_eq!(report.selected_provider.as_deref(), Some("pi"));
        assert!(
            report
                .providers
                .iter()
                .any(|status| { status.provider == "pi" && status.selected && status.usable })
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
