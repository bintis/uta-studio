use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::catalog::{
    AcquisitionSpec, BackendCapability, LicenseInfo, ModelCatalogEntry, NativeBackend,
    ResourceCatalog, SourceIdentity,
};
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::external_tool::{
    FUSION_AGENT_ADAPTER_ID, clear_tool_path, configure_tool_path, configured_fusion_provider,
    executable_file, fusion_adapter_manifest,
};
use crate::fusion_provider::{
    FusionProviderReport, clear_provider, provider_report, select_provider, selected_adapter_path,
    selected_provider_available,
};
use crate::lease::ResourceLease;
use crate::manifest::{
    is_generation_id, read_install_manifest, verify_generation, verify_generation_metadata,
};
use crate::platform::{executable_for_runtime, worker_supports_model};
use crate::resource::{ResourceKind, ResourceRef};
use crate::state::{
    InstallState, ReadinessReason, ResourceOrigin, ResourceStatus, RuntimePolicy, ValidationState,
};
use crate::store::{CurrentPointer, StorePaths};

const LEGACY_RMVPE_IR_RELATIVE_DIR: &str = "pitch/rmvpe/openvino-ir-2026.3.0-bucketed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedModel {
    pub model_id: String,
    pub generation: String,
    pub model_path: PathBuf,
    pub runtime_id: String,
    pub runtime_generation: String,
    pub runtime_content_digest: String,
    pub runtime_executable: PathBuf,
    pub backend: NativeBackend,
    pub validation_state: ValidationState,
    pub model_content_digest: String,
    pub model_recipe_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
    #[serde(skip)]
    pub lease: ResourceLease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTool {
    pub resource: ResourceRef,
    pub executable: PathBuf,
    pub identity: String,
    pub version: String,
    pub protocol_version: u32,
    pub origin: ResourceOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceMetadata {
    pub display_name: String,
    pub purpose: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub acquisition: Vec<AcquisitionSpec>,
    #[serde(default)]
    pub dependencies: Vec<ResourceRef>,
    #[serde(default)]
    pub backends: Vec<BackendCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<LicenseInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_download_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_installed_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDetails {
    pub resource: ResourceRef,
    pub metadata: ResourceMetadata,
    pub status: ResourceStatus,
}

#[derive(Debug, Clone)]
pub struct RuntimeManager {
    catalog: ResourceCatalog,
    paths: StorePaths,
}

impl RuntimeManager {
    pub fn new(catalog: ResourceCatalog, paths: StorePaths) -> Self {
        Self { catalog, paths }
    }

    pub fn with_default_catalog(paths: StorePaths) -> RuntimeManagerResult<Self> {
        Ok(Self::new(ResourceCatalog::default_catalog()?, paths))
    }

    pub fn catalog(&self) -> &ResourceCatalog {
        &self.catalog
    }

    pub fn paths(&self) -> &StorePaths {
        &self.paths
    }

    pub fn list(&self, policy: RuntimePolicy) -> RuntimeManagerResult<Vec<ResourceStatus>> {
        self.catalog
            .resource_refs()
            .into_iter()
            .map(|resource| self.status(&resource, policy))
            .collect()
    }

    pub fn status_requirements(
        &self,
        requirements: &crate::requirements::RequirementSet,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<Vec<ResourceStatus>> {
        requirements.validate()?;
        requirements
            .resources
            .iter()
            .map(|requirement| self.status(&requirement.resource, policy))
            .collect()
    }

    pub fn status(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourceStatus> {
        self.status_with_backend(resource, policy, None)
    }

    /// Resolve status for an explicitly requested model backend. This is a
    /// selection, not fallback: an unavailable or unvalidated requested route
    /// remains unusable instead of silently choosing another device.
    pub fn status_with_backend(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
        requested_backend: Option<NativeBackend>,
    ) -> RuntimeManagerResult<ResourceStatus> {
        match resource.kind {
            ResourceKind::Model => self.model_status(resource, policy, requested_backend),
            ResourceKind::Runtime => self.runtime_status(resource, policy),
            ResourceKind::Tool => self.tool_status(resource),
            ResourceKind::Bundle => self.bundle_status(resource, policy),
        }
    }

    /// Return status backed by an exhaustive structural generation check.
    pub(crate) fn verified_status(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourceStatus> {
        let mut status = self.status(resource, policy)?;
        if status.origin != ResourceOrigin::Managed {
            return Ok(status);
        }
        let Some(generation) = status.generation.as_deref() else {
            return Ok(status);
        };
        let Some(directory) = self.generation_path(resource, generation) else {
            return Ok(status);
        };
        let verification = verify_generation(&directory, generation, resource);
        status.install_state = verification.state;
        status.integrity_verified = verification.state == InstallState::Installed;
        if verification.state != InstallState::Installed {
            status.runnable = false;
            status.usable = false;
            status.reasons.retain(|reason| {
                !matches!(
                    reason,
                    ReadinessReason::Incomplete | ReadinessReason::Corrupt
                )
            });
            status.reasons.push(match verification.state {
                InstallState::Incomplete => ReadinessReason::Incomplete,
                _ => ReadinessReason::Corrupt,
            });
        }
        Ok(status)
    }

    pub fn show(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourceDetails> {
        self.show_with_backend(resource, policy, None)
    }

    pub fn show_with_backend(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
        requested_backend: Option<NativeBackend>,
    ) -> RuntimeManagerResult<ResourceDetails> {
        let metadata = match resource.kind {
            ResourceKind::Model => {
                let model = self
                    .catalog
                    .model(&resource.id)
                    .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
                ResourceMetadata {
                    display_name: model.display_name.clone(),
                    purpose: model.purpose.clone(),
                    capabilities: model.capabilities.clone(),
                    acquisition: model.acquisition.clone(),
                    dependencies: model.dependencies.clone(),
                    backends: model.backends.clone(),
                    source: Some(model.source.clone()),
                    license: Some(model.license.clone()),
                    estimated_download_bytes: model.estimated_download_bytes,
                    estimated_installed_bytes: model.estimated_installed_bytes,
                    recipe_digest: Some(model.recipe_digest.clone()),
                    runtime_recipe_digest: model.runtime_recipe_digest.clone(),
                }
            }
            ResourceKind::Runtime => {
                let runtime = self
                    .catalog
                    .runtime(&resource.id)
                    .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
                ResourceMetadata {
                    display_name: runtime.display_name.clone(),
                    purpose: runtime.purpose.clone(),
                    capabilities: Vec::new(),
                    acquisition: runtime.acquisition.clone(),
                    dependencies: Vec::new(),
                    backends: runtime.backends.clone(),
                    source: None,
                    license: None,
                    estimated_download_bytes: None,
                    estimated_installed_bytes: None,
                    recipe_digest: runtime.recipe_digest.clone(),
                    runtime_recipe_digest: runtime.recipe_digest.clone(),
                }
            }
            ResourceKind::Tool => {
                let tool = self
                    .catalog
                    .tools
                    .get(&resource.id)
                    .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
                ResourceMetadata {
                    display_name: tool.display_name.clone(),
                    purpose: tool.purpose.clone(),
                    capabilities: Vec::new(),
                    acquisition: tool.acquisition.clone(),
                    dependencies: Vec::new(),
                    backends: Vec::new(),
                    source: None,
                    license: None,
                    estimated_download_bytes: None,
                    estimated_installed_bytes: None,
                    recipe_digest: None,
                    runtime_recipe_digest: None,
                }
            }
            ResourceKind::Bundle => {
                let bundle = self
                    .catalog
                    .bundles
                    .get(&resource.id)
                    .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
                ResourceMetadata {
                    display_name: bundle.display_name.clone(),
                    purpose: bundle.purpose.clone(),
                    capabilities: Vec::new(),
                    acquisition: Vec::new(),
                    dependencies: bundle.dependencies.clone(),
                    backends: Vec::new(),
                    source: None,
                    license: None,
                    estimated_download_bytes: None,
                    estimated_installed_bytes: None,
                    recipe_digest: None,
                    runtime_recipe_digest: None,
                }
            }
        };
        Ok(ResourceDetails {
            resource: resource.clone(),
            metadata,
            status: self.status_with_backend(resource, policy, requested_backend)?,
        })
    }

    pub fn verify(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<crate::verify::VerifyReport> {
        crate::verify::verify_report(self, resources, policy)
    }

    pub fn doctor(&self) -> crate::doctor::DoctorReport {
        crate::doctor::run_doctor(self)
    }

    pub fn configure_external_tool(
        &self,
        resource: &ResourceRef,
        executable: &std::path::Path,
    ) -> RuntimeManagerResult<ResourceStatus> {
        if resource.kind != ResourceKind::Tool || !self.catalog.contains(resource) {
            return Err(RuntimeManagerError::unknown_resource(resource));
        }
        let canonical_executable = std::fs::canonicalize(executable).map_err(|error| {
            RuntimeManagerError::new(
                "tool_unusable",
                format!("could not canonicalize {}: {error}", executable.display()),
            )
            .with_resource(resource.clone())
        })?;
        if resource.id == FUSION_AGENT_ADAPTER_ID {
            fusion_adapter_manifest(&canonical_executable)
                .map_err(|error| error.with_resource(resource.clone()))?;
        } else if !executable_file(&canonical_executable) {
            return Err(RuntimeManagerError::new(
                "tool_unusable",
                format!(
                    "tool path is not executable: {}",
                    canonical_executable.display()
                ),
            )
            .with_resource(resource.clone()));
        }
        configure_tool_path(&self.paths, &resource.id, &canonical_executable)?;
        self.status(resource, RuntimePolicy::Production)
    }

    pub fn clear_external_tool(
        &self,
        resource: &ResourceRef,
    ) -> RuntimeManagerResult<ResourceStatus> {
        if resource.kind != ResourceKind::Tool || !self.catalog.contains(resource) {
            return Err(RuntimeManagerError::unknown_resource(resource));
        }
        clear_tool_path(&self.paths, &resource.id)?;
        self.status(resource, RuntimePolicy::Production)
    }

    /// Report provider CLIs and their manifest-verified native adapters. This
    /// is read-only: provider CLIs are never launched and authentication is
    /// never inferred.
    pub fn fusion_provider_report(&self) -> RuntimeManagerResult<FusionProviderReport> {
        provider_report(&self.paths)
    }

    /// Persist a provider identity owned by Runtime Manager. The selected
    /// provider determines which sibling adapter resolves to the canonical
    /// `tool:fusion_agent_adapter` resource.
    pub fn select_fusion_provider(
        &self,
        provider: &str,
    ) -> RuntimeManagerResult<FusionProviderReport> {
        select_provider(&self.paths, provider)
    }

    pub fn clear_fusion_provider(&self) -> RuntimeManagerResult<FusionProviderReport> {
        clear_provider(&self.paths)
    }

    pub fn resolve_tool(
        &self,
        tool_id: &str,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResolvedTool> {
        let resource = ResourceRef::tool(tool_id)?;
        let status = self.status(&resource, policy)?;
        if !status.usable {
            return Err(error_for_unusable(&status));
        }
        let executable = if tool_id == FUSION_AGENT_ADAPTER_ID {
            if configured_fusion_provider(&self.paths)?.is_some() {
                selected_adapter_path(&self.paths)?
            } else {
                self.paths.tool_candidate_path_result(tool_id)?
            }
        } else {
            self.paths.tool_candidate_path_result(tool_id)?
        }
        .filter(|path| executable_file(path))
        .ok_or_else(|| RuntimeManagerError::runtime_missing(&resource))?;
        if tool_id == FUSION_AGENT_ADAPTER_ID {
            let manifest = fusion_adapter_manifest(&executable)?;
            return Ok(ResolvedTool {
                resource,
                executable,
                identity: manifest.adapter_id,
                version: manifest.adapter_version,
                protocol_version: manifest.fusion_protocol_version,
                origin: status.origin,
            });
        }
        Ok(ResolvedTool {
            resource,
            executable,
            identity: tool_id.to_string(),
            version: "external".to_string(),
            protocol_version: 0,
            origin: status.origin,
        })
    }

    pub fn resolve_model(
        &self,
        model_id: &str,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResolvedModel> {
        self.resolve_model_with_backend(model_id, policy, None)
    }

    pub fn resolve_model_with_backend(
        &self,
        model_id: &str,
        policy: RuntimePolicy,
        requested_backend: Option<NativeBackend>,
    ) -> RuntimeManagerResult<ResolvedModel> {
        let resource = ResourceRef::model(model_id)?;
        let mut status = self.status_with_backend(&resource, policy, requested_backend)?;
        if status.origin == ResourceOrigin::Managed {
            let Some(generation) = status.generation.as_deref() else {
                return Err(RuntimeManagerError::resource_corrupt(&resource));
            };
            let directory = self
                .generation_path(&resource, generation)
                .ok_or_else(|| RuntimeManagerError::resource_corrupt(&resource))?;
            let verification = verify_generation(&directory, generation, &resource);
            status.install_state = verification.state;
            status.integrity_verified = verification.state == InstallState::Installed;
            if verification.state != InstallState::Installed {
                return Err(match verification.state {
                    InstallState::Incomplete => RuntimeManagerError::resource_missing(&resource),
                    _ => RuntimeManagerError::resource_corrupt(&resource),
                });
            }
        }
        if !status.usable {
            return Err(error_for_unusable(&status));
        }
        let model = self
            .catalog
            .model(model_id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(&resource))?;
        let runtime_ref = status
            .runtime_resource
            .clone()
            .ok_or_else(|| RuntimeManagerError::runtime_missing(&resource))?;
        let runtime = self
            .catalog
            .runtime(&runtime_ref.id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(&runtime_ref))?;
        let executable = executable_for_runtime(runtime, &self.paths)
            .ok_or_else(|| RuntimeManagerError::runtime_missing(&resource))?;
        let selected_backend = status
            .selected_backend
            .ok_or_else(|| RuntimeManagerError::no_validated_backend(&resource))?;
        let external_ggml = selected_backend == NativeBackend::Vulkan
            && uses_legacy_ggml_layout(model_id)
            && status.origin == ResourceOrigin::Legacy
            && self.paths.ggml_model_path(model_id).is_some();
        let (model_root, model_path, generation, model_content_digest, model_recipe_digest) =
            if external_ggml {
                let path = self
                    .paths
                    .ggml_model_path(model_id)
                    .ok_or_else(|| RuntimeManagerError::resource_missing(&resource))?;
                let content_identity = ggml_model_identity(model_id)
                    .ok_or_else(|| RuntimeManagerError::no_validated_backend(&resource))?
                    .0;
                (
                    None,
                    path,
                    content_identity.to_string(),
                    content_identity.to_string(),
                    model.recipe_digest.clone(),
                )
            } else {
                let root = self
                    .model_generation_path(model_id, status.generation.as_deref())
                    .ok_or_else(|| RuntimeManagerError::resource_missing(&resource))?;
                let path = if selected_backend == NativeBackend::Vulkan {
                    model
                        .source
                        .converted_artifact
                        .as_ref()
                        .map(|artifact| root.join(&artifact.manifest_filename))
                        .filter(|path| path.is_file())
                        .or_else(|| {
                            model
                                .source
                                .filename
                                .as_deref()
                                .map(|filename| root.join(filename))
                                .filter(|path| path.is_file())
                        })
                        .unwrap_or_else(|| root.clone())
                } else if model.source.converted_artifact.is_some() {
                    root.clone()
                } else {
                    model
                        .source
                        .filename
                        .as_deref()
                        .map(|filename| root.join(filename))
                        .filter(|path| path.is_file())
                        .unwrap_or_else(|| root.clone())
                };
                let generation = status.generation.unwrap_or_else(|| "legacy".to_string());
                let recipe = if is_generation_id(&generation) {
                    read_install_manifest(&root)
                        .and_then(|manifest| manifest.model_recipe_digest)
                        .ok_or_else(|| RuntimeManagerError::resource_corrupt(&resource))?
                } else {
                    model.recipe_digest.clone()
                };
                (Some(root), path, generation.clone(), generation, recipe)
            };
        let runtime_status = self.verified_status(&runtime_ref, policy)?;
        let managed_runtime_generation = runtime_status.generation;
        let runtime_generation = managed_runtime_generation
            .clone()
            .unwrap_or_else(|| "environment".to_string());
        let runtime_content_digest =
            managed_runtime_generation.unwrap_or_else(|| "environment".to_string());
        let model_lease_anchor = model_root
            .as_ref()
            .filter(|_| is_generation_id(&generation))
            .map(|root| root.join("install-manifest.json"));
        let runtime_lease_anchor = is_generation_id(&runtime_generation)
            .then(|| {
                self.generation_path(&runtime_ref, &runtime_generation)
                    .map(|path| path.join("install-manifest.json"))
            })
            .flatten();
        let lease = ResourceLease::acquire_with_files([
            (
                generation_lease_key(&resource, &generation),
                model_lease_anchor,
            ),
            (
                generation_lease_key(&runtime_ref, &runtime_generation),
                runtime_lease_anchor,
            ),
        ])?;
        Ok(ResolvedModel {
            model_id: model_id.to_string(),
            generation: generation.clone(),
            model_path,
            runtime_id: runtime_ref.id,
            runtime_generation,
            runtime_content_digest,
            runtime_executable: executable,
            backend: selected_backend,
            validation_state: status.validation_state,
            model_content_digest,
            model_recipe_digest,
            runtime_recipe_digest: runtime.recipe_digest.clone(),
            lease,
        })
    }

    pub fn lease_resolved_models(&self, models: &[ResolvedModel]) -> crate::lease::ResourceLease {
        crate::lease::ResourceLease::merged(models.iter().map(|model| model.lease.clone()))
    }

    pub fn paths_summary(&self) -> crate::store::PathsSummary {
        self.paths.paths_summary()
    }

    fn model_status(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
        requested_backend: Option<NativeBackend>,
    ) -> RuntimeManagerResult<ResourceStatus> {
        let model = self
            .catalog
            .model(&resource.id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
        let selected = select_backend(model, policy, requested_backend);
        let validation_state = selected
            .as_ref()
            .map(|capability| capability.validation)
            .unwrap_or_else(|| strongest_validation(&model.backends));
        let runtime_resource = selected
            .as_ref()
            .map(|capability| capability.backend)
            .or(model.pinned_backend)
            .and_then(|backend| self.runtime_for_backend(model, backend));
        let managed_install = self.model_install_state(&resource.id);
        let managed_current = managed_install.state == InstallState::Installed
            && managed_install
                .generation
                .as_deref()
                .is_some_and(|generation| {
                    self.managed_model_identity(model, generation) == ManagedModelIdentity::Current
                });
        let external_ggml = selected.as_ref().is_some_and(|capability| {
            capability.backend == NativeBackend::Vulkan
                && uses_legacy_ggml_layout(resource.id.as_str())
                && self.paths.ggml_model_path(&resource.id).is_some()
                && !managed_current
        });
        let install = if external_ggml {
            self.ggml_model_install_state(&resource.id)
        } else {
            managed_install
        };
        let managed_identity = if install.state == InstallState::Installed {
            install
                .generation
                .as_deref()
                .map(|generation| self.managed_model_identity(model, generation))
                .unwrap_or(ManagedModelIdentity::Corrupt)
        } else {
            ManagedModelIdentity::Current
        };
        let install_state = match managed_identity {
            ManagedModelIdentity::Current => install.state,
            ManagedModelIdentity::RecipeMismatch => InstallState::Legacy,
            ManagedModelIdentity::Corrupt => InstallState::Corrupt,
        };
        let mut reasons = Vec::new();
        if selected.is_none() {
            reasons.push(ReadinessReason::BackendUnvalidated);
        }
        match install_state {
            InstallState::Absent => reasons.push(ReadinessReason::Absent),
            InstallState::Incomplete => reasons.push(ReadinessReason::Incomplete),
            InstallState::Corrupt => reasons.push(ReadinessReason::Corrupt),
            InstallState::Installed | InstallState::Legacy => {}
        }

        let dependency_status = runtime_resource
            .as_ref()
            .map(|runtime| self.runtime_status(runtime, policy))
            .transpose()?;
        let dependency_runtime_ready = dependency_status
            .as_ref()
            .is_none_or(|status| status.usable);
        let dependency_runtime_runnable = dependency_status
            .as_ref()
            .is_none_or(|status| status.runnable);
        let executable_ready = dependency_status
            .as_ref()
            .is_none_or(|status| status.executable_ready);
        if !dependency_runtime_runnable {
            reasons.push(ReadinessReason::RuntimeMissing);
        }
        let worker_supported = runtime_resource
            .as_ref()
            .and_then(|runtime| self.catalog.runtime(&runtime.id))
            .is_some_and(|runtime| worker_supports_model(runtime, &resource.id));
        if !worker_supported {
            reasons.push(ReadinessReason::WorkerCapabilityMissing);
        }
        if install_state == InstallState::Legacy {
            reasons.push(ReadinessReason::Legacy);
        }
        let integrity_verified =
            install.integrity_verified && managed_identity == ManagedModelIdentity::Current;
        let backend_route_permitted = policy == RuntimePolicy::Experimental
            || model
                .backends
                .iter()
                .any(|capability| capability.validation != ValidationState::Unsupported);
        let testing_policy = policy == RuntimePolicy::Experimental;
        let locally_present = install_state.locally_present();
        // Exact external GGUF files are intentionally unmanaged user data.
        // Fast status checks only their immutable expected size; execution
        // resolution hashes all bytes before creating the Vulkan context.
        let integrity_permitted = integrity_verified
            || (install_state == InstallState::Legacy && (testing_policy || external_ggml));
        let runnable = locally_present
            && integrity_permitted
            && backend_route_permitted
            && dependency_runtime_runnable
            && executable_ready
            && worker_supported;
        let dependencies_ready = if testing_policy {
            dependency_runtime_runnable
        } else {
            dependency_runtime_ready
        };
        let usable = runnable && selected.is_some() && dependencies_ready;
        Ok(ResourceStatus {
            resource: resource.clone(),
            install_state,
            origin: if install.state == InstallState::Installed {
                ResourceOrigin::Managed
            } else {
                origin_for_install_state(install_state)
            },
            integrity_verified,
            runnable,
            validation_state,
            dependencies_ready,
            executable_ready,
            usable,
            reasons,
            selected_backend: selected.map(|capability| capability.backend),
            runtime_resource,
            generation: install.generation,
            tool_identity: None,
            tool_version: None,
            tool_protocol_version: None,
        })
    }

    fn runtime_status(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourceStatus> {
        let runtime = self
            .catalog
            .runtime(&resource.id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
        let selected = select_capability(&runtime.backends, None, policy);
        let validation_state = selected
            .as_ref()
            .map(|capability| capability.validation)
            .unwrap_or_else(|| strongest_validation(&runtime.backends));
        let install = self.generic_install_state(resource);
        let executable_ready = executable_for_runtime(runtime, &self.paths).is_some();
        let mut reasons = Vec::new();
        if selected.is_none() {
            reasons.push(ReadinessReason::BackendUnvalidated);
        }
        match install.state {
            InstallState::Absent if !executable_ready => reasons.push(ReadinessReason::Absent),
            InstallState::Incomplete => reasons.push(ReadinessReason::Incomplete),
            InstallState::Corrupt => reasons.push(ReadinessReason::Corrupt),
            InstallState::Absent | InstallState::Installed | InstallState::Legacy => {}
        }
        if !executable_ready {
            reasons.push(ReadinessReason::ExecutableMissing);
        }
        let effective_install_state = if install.state == InstallState::Absent && executable_ready {
            InstallState::Legacy
        } else {
            install.state
        };
        let backend_route_permitted = policy == RuntimePolicy::Experimental
            || runtime
                .backends
                .iter()
                .any(|capability| capability.validation != ValidationState::Unsupported);
        let runnable = executable_ready
            && effective_install_state.locally_present()
            && backend_route_permitted;
        Ok(ResourceStatus {
            resource: resource.clone(),
            install_state: effective_install_state,
            origin: if install.state == InstallState::Absent && executable_ready {
                ResourceOrigin::EnvironmentOverride
            } else {
                origin_for_install_state(effective_install_state)
            },
            integrity_verified: install.integrity_verified,
            runnable,
            validation_state,
            dependencies_ready: true,
            executable_ready,
            usable: selected.is_some() && runnable,
            reasons,
            selected_backend: selected.map(|capability| capability.backend),
            runtime_resource: None,
            generation: install.generation,
            tool_identity: None,
            tool_version: None,
            tool_protocol_version: None,
        })
    }

    fn tool_status(&self, resource: &ResourceRef) -> RuntimeManagerResult<ResourceStatus> {
        if !self.catalog.tools.contains_key(&resource.id) {
            return Err(RuntimeManagerError::unknown_resource(resource));
        }
        if resource.id == FUSION_AGENT_ADAPTER_ID {
            return self.fusion_adapter_status(resource);
        }
        let install = self.generic_install_state(resource);
        let executable_ready = self.paths.tool_executable(&resource.id).is_some();
        let effective_install_state = if install.state == InstallState::Absent && executable_ready {
            InstallState::Legacy
        } else {
            install.state
        };
        let locally_present = effective_install_state.locally_present();
        Ok(ResourceStatus {
            resource: resource.clone(),
            install_state: effective_install_state,
            origin: if install.state == InstallState::Absent && executable_ready {
                ResourceOrigin::EnvironmentOverride
            } else {
                origin_for_install_state(effective_install_state)
            },
            integrity_verified: install.integrity_verified,
            runnable: locally_present && executable_ready,
            validation_state: ValidationState::ProductionPinned,
            dependencies_ready: true,
            executable_ready,
            usable: locally_present && executable_ready,
            reasons: match effective_install_state {
                InstallState::Corrupt => vec![ReadinessReason::Corrupt],
                InstallState::Incomplete => vec![ReadinessReason::Incomplete],
                InstallState::Absent => vec![ReadinessReason::Absent],
                InstallState::Installed | InstallState::Legacy if !executable_ready => {
                    vec![ReadinessReason::ExecutableMissing]
                }
                InstallState::Installed | InstallState::Legacy => Vec::new(),
            },
            selected_backend: None,
            runtime_resource: None,
            generation: install.generation,
            tool_identity: None,
            tool_version: None,
            tool_protocol_version: None,
        })
    }

    fn fusion_adapter_status(
        &self,
        resource: &ResourceRef,
    ) -> RuntimeManagerResult<ResourceStatus> {
        let selected_provider = configured_fusion_provider(&self.paths)?;
        let candidate = if selected_provider.is_some() {
            selected_adapter_path(&self.paths)?
        } else {
            self.paths.tool_candidate_path_result(&resource.id)?
        };
        let origin = if selected_provider.is_some() {
            ResourceOrigin::ExternalConfiguration
        } else if self.paths.tool_override_path(&resource.id).is_some() {
            ResourceOrigin::EnvironmentOverride
        } else if self
            .paths
            .configured_tool_path_result(&resource.id)?
            .is_some()
        {
            ResourceOrigin::ExternalConfiguration
        } else if self.paths.tool_fallback_path(&resource.id).is_some() {
            ResourceOrigin::Derived
        } else {
            ResourceOrigin::Missing
        };
        let executable_ready = candidate.as_deref().is_some_and(executable_file);
        let manifest_result = candidate
            .as_deref()
            .filter(|_| executable_ready)
            .map(fusion_adapter_manifest)
            .transpose();
        let (manifest, protocol_ready) = match manifest_result {
            Ok(manifest) => (manifest, executable_ready),
            Err(_) => (None, false),
        };
        let provider_ready = selected_provider_available(&self.paths)?;
        let mut reasons = Vec::new();
        if candidate.is_none() {
            reasons.push(ReadinessReason::Absent);
        } else if !executable_ready {
            reasons.push(ReadinessReason::ExecutableMissing);
        } else if !protocol_ready {
            reasons.push(ReadinessReason::ProtocolMismatch);
        }
        if selected_provider.is_some() && !provider_ready {
            reasons.push(ReadinessReason::ExecutableMissing);
        }
        let usable = executable_ready && protocol_ready && provider_ready;
        Ok(ResourceStatus {
            resource: resource.clone(),
            install_state: if candidate.is_none() {
                InstallState::Absent
            } else if usable {
                InstallState::Installed
            } else {
                InstallState::Incomplete
            },
            origin,
            integrity_verified: false,
            runnable: usable,
            validation_state: ValidationState::ProductionPinned,
            dependencies_ready: true,
            executable_ready,
            usable,
            reasons,
            selected_backend: None,
            runtime_resource: None,
            generation: None,
            tool_identity: manifest
                .as_ref()
                .map(|manifest| manifest.adapter_id.clone()),
            tool_version: manifest
                .as_ref()
                .map(|manifest| manifest.adapter_version.clone()),
            tool_protocol_version: manifest
                .as_ref()
                .map(|manifest| manifest.fusion_protocol_version),
        })
    }

    fn bundle_status(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourceStatus> {
        let bundle = self
            .catalog
            .bundles
            .get(&resource.id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
        let mut dependencies_ready = true;
        let mut all_installed = true;
        let mut integrity_verified = true;
        let mut runnable = true;
        let mut reasons = Vec::new();
        for dependency in &bundle.dependencies {
            let status = self.status(dependency, policy)?;
            all_installed &= status.install_state.locally_present();
            integrity_verified &= status.integrity_verified;
            runnable &= status.runnable;
            if !status.usable {
                dependencies_ready = false;
                reasons.push(ReadinessReason::DependencyMissing);
            }
        }
        Ok(ResourceStatus {
            resource: resource.clone(),
            install_state: if all_installed {
                InstallState::Installed
            } else {
                InstallState::Absent
            },
            origin: ResourceOrigin::Derived,
            integrity_verified,
            runnable,
            validation_state: ValidationState::ProductionPinned,
            dependencies_ready,
            executable_ready: runnable,
            usable: dependencies_ready,
            reasons,
            selected_backend: None,
            runtime_resource: None,
            generation: None,
            tool_identity: None,
            tool_version: None,
            tool_protocol_version: None,
        })
    }

    fn managed_model_identity(
        &self,
        model: &ModelCatalogEntry,
        generation: &str,
    ) -> ManagedModelIdentity {
        let resource = model.resource();
        let Some(directory) = self.generation_path(&resource, generation) else {
            return ManagedModelIdentity::Corrupt;
        };
        let Some(manifest) = read_install_manifest(&directory) else {
            return ManagedModelIdentity::Corrupt;
        };
        let installed_artifact_matches =
            if let Some(converted) = model.source.converted_artifact.as_ref() {
                manifest.files.iter().any(|file| {
                    file.path.as_path() == std::path::Path::new(&converted.manifest_filename)
                })
            } else if let Some(filename) = model.source.filename.as_deref() {
                manifest
                    .files
                    .iter()
                    .any(|file| file.path.as_path() == std::path::Path::new(filename))
            } else {
                !manifest.files.is_empty()
            };
        if !installed_artifact_matches {
            return ManagedModelIdentity::Corrupt;
        }
        if manifest.catalog_version != self.catalog.catalog_version {
            return ManagedModelIdentity::RecipeMismatch;
        }
        ManagedModelIdentity::Current
    }

    fn runtime_for_backend(
        &self,
        model: &ModelCatalogEntry,
        backend: NativeBackend,
    ) -> Option<ResourceRef> {
        let runtime_backend = match backend {
            NativeBackend::CpuReference => NativeBackend::OpenVino,
            other => other,
        };
        model.dependencies.iter().find_map(|dependency| {
            let runtime = self.catalog.runtime(&dependency.id)?;
            (dependency.kind == ResourceKind::Runtime
                && worker_supports_model(runtime, model.id.as_str())
                && runtime
                    .backends
                    .iter()
                    .any(|capability| capability.backend == runtime_backend))
            .then(|| dependency.clone())
        })
    }

    fn ggml_model_install_state(&self, model_id: &str) -> InstallProbe {
        let Some(path) = self.paths.ggml_model_path(model_id) else {
            return InstallProbe::absent();
        };
        let expected_size = ggml_model_identity(model_id).map(|(_, size)| size);
        if path.metadata().ok().map(|metadata| metadata.len()) != expected_size {
            return InstallProbe {
                state: InstallState::Corrupt,
                generation: None,
                integrity_verified: false,
            };
        }
        InstallProbe {
            state: InstallState::Legacy,
            generation: Some("legacy".to_string()),
            integrity_verified: false,
        }
    }

    fn model_install_state(&self, model_id: &str) -> InstallProbe {
        let resource = ResourceRef::model(model_id).expect("catalog ids are valid");
        if let Some(probe) = self.probe_current_pointer(&resource) {
            return probe;
        }
        if self.legacy_model_present(model_id) {
            return InstallProbe {
                state: InstallState::Legacy,
                generation: Some("legacy".to_string()),
                integrity_verified: false,
            };
        }
        InstallProbe::absent()
    }

    fn legacy_model_present(&self, model_id: &str) -> bool {
        let Some(root) = self.paths.legacy_models_root.as_ref() else {
            return false;
        };
        match model_id {
            "melband_roformer_inst_v2"
            | "melband_roformer_harmony"
            | "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew" => root
                .join("audio-processing")
                .join(model_id)
                .join("install-manifest.json")
                .is_file(),
            "firered_asr2_aed" => root
                .join("firered-asr2-aed/openvino-ir-2026.3.0-smoke/manifest.json")
                .is_file(),
            "qwen3_asr_1_7b" => root.join("qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf").is_file(),
            "qwen3_forced_aligner_0_6b" => root.join("qwen-align/install-manifest.json").is_file(),
            "rmvpe" => legacy_rmvpe_present(root),
            "fcpe" => root
                .join("pitch/fcpe/openvino-ir-2026.3.0-smoke/manifest.json")
                .is_file(),
            "game" => root.join("boundary/game/install-manifest.json").is_file(),
            "basic_pitch" => root
                .join("boundary/basic-pitch/openvino-ir-2026.3.0-smoke/manifest.json")
                .is_file(),
            "stars" => root.join("technique/stars/install-manifest.json").is_file(),
            _ => false,
        }
    }

    fn generic_install_state(&self, resource: &ResourceRef) -> InstallProbe {
        self.probe_current_pointer(resource)
            .unwrap_or_else(InstallProbe::absent)
    }

    fn probe_current_pointer(&self, resource: &ResourceRef) -> Option<InstallProbe> {
        let path = self.paths.current_pointer_path(resource)?;
        if !path.exists() {
            return None;
        }
        if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
            return Some(InstallProbe {
                state: InstallState::Corrupt,
                generation: None,
                integrity_verified: false,
            });
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Some(InstallProbe {
                    state: InstallState::Corrupt,
                    generation: None,
                    integrity_verified: false,
                });
            }
        };
        let pointer: CurrentPointer = match serde_json::from_slice(&bytes) {
            Ok(pointer) => pointer,
            Err(_) => {
                return Some(InstallProbe {
                    state: InstallState::Corrupt,
                    generation: None,
                    integrity_verified: false,
                });
            }
        };
        if pointer.generation.is_empty() {
            return Some(InstallProbe {
                state: InstallState::Incomplete,
                generation: None,
                integrity_verified: false,
            });
        }
        if !is_generation_id(&pointer.generation) {
            return Some(InstallProbe {
                state: InstallState::Corrupt,
                generation: None,
                integrity_verified: false,
            });
        }
        let generation_dir = self.generation_path(resource, &pointer.generation)?;
        let verification =
            verify_generation_metadata(&generation_dir, &pointer.generation, resource);
        Some(InstallProbe {
            state: verification.state,
            generation: verification.content_digest,
            integrity_verified: verification.state == InstallState::Installed,
        })
    }

    fn generation_path(&self, resource: &ResourceRef, generation: &str) -> Option<PathBuf> {
        if !is_generation_id(generation) {
            return None;
        }
        let root = self.paths.store_root.as_ref()?;
        let kind = match resource.kind {
            ResourceKind::Model => "models",
            ResourceKind::Runtime => "runtimes",
            ResourceKind::Tool => "tools",
            ResourceKind::Bundle => return None,
        };
        Some(
            root.join(kind)
                .join(&resource.id)
                .join("generations")
                .join(generation),
        )
    }

    fn model_generation_path(&self, model_id: &str, generation: Option<&str>) -> Option<PathBuf> {
        if let Some(generation) = generation.filter(|generation| *generation != "legacy") {
            return self.generation_path(
                &ResourceRef::model(model_id).expect("catalog ids are valid"),
                generation,
            );
        }
        legacy_model_path(self.paths.legacy_models_root.as_deref()?, model_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedModelIdentity {
    Current,
    RecipeMismatch,
    Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallProbe {
    state: InstallState,
    generation: Option<String>,
    integrity_verified: bool,
}

impl InstallProbe {
    fn absent() -> Self {
        Self {
            state: InstallState::Absent,
            generation: None,
            integrity_verified: false,
        }
    }
}

pub(crate) fn generation_lease_key(resource: &ResourceRef, generation: &str) -> String {
    format!("{resource}:{generation}")
}

fn origin_for_install_state(state: InstallState) -> ResourceOrigin {
    match state {
        InstallState::Absent => ResourceOrigin::Missing,
        InstallState::Installed | InstallState::Incomplete | InstallState::Corrupt => {
            ResourceOrigin::Managed
        }
        InstallState::Legacy => ResourceOrigin::Legacy,
    }
}

fn select_backend(
    model: &ModelCatalogEntry,
    policy: RuntimePolicy,
    requested: Option<NativeBackend>,
) -> Option<&BackendCapability> {
    if let Some(requested) = requested {
        return model
            .backends
            .iter()
            .find(|capability| capability.backend == requested)
            .filter(|capability| {
                policy.allows(capability.validation)
                    && (requested != NativeBackend::CpuReference
                        || policy == RuntimePolicy::Experimental)
            });
    }
    select_capability(&model.backends, model.pinned_backend, policy)
}

fn select_capability(
    capabilities: &[BackendCapability],
    pinned: Option<NativeBackend>,
    policy: RuntimePolicy,
) -> Option<&BackendCapability> {
    let supported = |capability: &&BackendCapability| {
        policy.allows(capability.validation)
            && (capability.backend != NativeBackend::CpuReference
                || policy != RuntimePolicy::Production)
    };
    if let Some(pinned) = pinned {
        return capabilities
            .iter()
            .find(|capability| capability.backend == pinned)
            .filter(supported);
    }
    for backend in [
        NativeBackend::OpenVino,
        NativeBackend::Vulkan,
        NativeBackend::NativeDsp,
        NativeBackend::CpuReference,
    ] {
        if let Some(capability) = capabilities
            .iter()
            .find(|capability| capability.backend == backend)
            .filter(supported)
        {
            return Some(capability);
        }
    }
    None
}

fn strongest_validation(capabilities: &[BackendCapability]) -> ValidationState {
    capabilities
        .iter()
        .map(|capability| capability.validation)
        .min_by_key(|validation| validation_rank(*validation))
        .unwrap_or(ValidationState::Unsupported)
}

fn validation_rank(validation: ValidationState) -> u8 {
    match validation {
        ValidationState::ProductionPinned => 0,
        ValidationState::BenchmarkCandidate => 1,
        ValidationState::Experimental => 2,
        ValidationState::Unsupported => 3,
    }
}

fn error_for_unusable(status: &ResourceStatus) -> RuntimeManagerError {
    if status.reasons.contains(&ReadinessReason::ProtocolMismatch) {
        return RuntimeManagerError::new(
            "tool_protocol_mismatch",
            format!(
                "external tool protocol is not verified: {}",
                status.resource
            ),
        )
        .with_resource(status.resource.clone());
    }
    if status.reasons.contains(&ReadinessReason::Corrupt) {
        return RuntimeManagerError::resource_corrupt(&status.resource);
    }
    if status.reasons.contains(&ReadinessReason::Absent)
        || status.reasons.contains(&ReadinessReason::Incomplete)
    {
        return RuntimeManagerError::resource_missing(&status.resource);
    }
    if status
        .reasons
        .contains(&ReadinessReason::BackendUnvalidated)
    {
        return RuntimeManagerError::no_validated_backend(&status.resource);
    }
    if status.reasons.contains(&ReadinessReason::RuntimeMissing)
        || status.reasons.contains(&ReadinessReason::ExecutableMissing)
    {
        return RuntimeManagerError::runtime_missing(&status.resource);
    }
    if status
        .reasons
        .contains(&ReadinessReason::WorkerCapabilityMissing)
    {
        return RuntimeManagerError::worker_capability_missing(&status.resource);
    }
    RuntimeManagerError::resource_missing(&status.resource)
}

fn uses_legacy_ggml_layout(model_id: &str) -> bool {
    matches!(
        model_id,
        "melband_roformer_inst_v2"
            | "melband_roformer_harmony"
            | "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew"
            | "bs_polarformer_public_instrumental"
            | "rmvpe"
            | "game"
    )
}

fn ggml_model_identity(model_id: &str) -> Option<(&'static str, u64)> {
    match model_id {
        "melband_roformer_denoise_aufr33" => Some((
            "eb03fce4c5a450f88718e8a529b8adcd653618a5d32cb55275fa212a80fef33a",
            457_008_736,
        )),
        "melband_roformer_dereverb_anvuew" => Some((
            "f850fb2460099df356676ce37ba48875e3c75726d7a848b42d75ff6015955ac7",
            457_008_736,
        )),
        "melband_roformer_inst_v2" => Some((
            "e2b39b979e2413af172bad88a6b0a324a54d47fbca6622083f7f3817b9046897",
            787_918_656,
        )),
        "melband_roformer_harmony" => Some((
            "d463c06a1bf5d3889a2a6be58cc469f0a996155eafb91845ff5e8c139a3d64be",
            457_008_736,
        )),
        "bs_polarformer_public_instrumental" => Some((
            "f5e40ac0dc7487a0c2ccb247e5b948cd6f2c7aaf46a2994023606e1e800ed2c1",
            204_237_408,
        )),
        "rmvpe" => Some((
            crate::runtime_lock::RMVPE_GGUF_SHA256,
            crate::runtime_lock::RMVPE_GGUF_SIZE_BYTES,
        )),
        "game" => Some((
            "a69c52a01f452c8092f1479630074592d4c3f0ef7404bb65ddd73fab01a0606e",
            199_584_064,
        )),
        _ => None,
    }
}

fn legacy_model_path(root: &std::path::Path, model_id: &str) -> Option<PathBuf> {
    let relative = match model_id {
        "melband_roformer_inst_v2"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => PathBuf::from("audio-processing").join(model_id),
        "firered_asr2_aed" => PathBuf::from("firered-asr2-aed/openvino-ir-2026.3.0-smoke"),
        "qwen3_asr_1_7b" => PathBuf::from("qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf"),
        "qwen3_forced_aligner_0_6b" => {
            PathBuf::from("qwen-align/qwen3-forced-aligner-predict-woo-f16.gguf")
        }
        "rmvpe" => PathBuf::from(LEGACY_RMVPE_IR_RELATIVE_DIR),
        "fcpe" => PathBuf::from("pitch/fcpe/openvino-ir-2026.3.0-smoke"),
        "game" => PathBuf::from("boundary/game"),
        "basic_pitch" => PathBuf::from("boundary/basic-pitch/openvino-ir-2026.3.0-smoke"),
        "stars" => PathBuf::from("technique/stars"),
        _ => return None,
    };
    Some(root.join(relative))
}

fn legacy_rmvpe_present(root: &std::path::Path) -> bool {
    root.join(LEGACY_RMVPE_IR_RELATIVE_DIR)
        .join("manifest.json")
        .is_file()
}

#[cfg(test)]
mod tests;
