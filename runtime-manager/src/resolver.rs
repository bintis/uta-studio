use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::catalog::{
    AcquisitionSpec, BackendCapability, LicenseInfo, ModelCatalogEntry, NativeBackend,
    ResourceCatalog, SourceIdentity,
};
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::lease::ResourceLease;
use crate::manifest::{
    is_generation_id, read_install_manifest, verify_generation, verify_generation_metadata,
};
use crate::platform::{executable_for_runtime, worker_supports_model};
use crate::resource::{ResourceKind, ResourceRef};
use crate::runtime_lock::RMVPE_IR_RELATIVE_DIR;
use crate::state::{
    InstallState, ReadinessReason, ResourceOrigin, ResourceStatus, RuntimePolicy, ValidationState,
};
use crate::store::{CurrentPointer, StorePaths};

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
            && !matches!(model_id, "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b");
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
        let external_ggml = selected.as_ref().is_some_and(|capability| {
            capability.backend == NativeBackend::Vulkan
                && !matches!(
                    resource.id.as_str(),
                    "qwen3_asr_1_7b" | "qwen3_forced_aligner_0_6b"
                )
        });
        let install = if external_ggml {
            self.ggml_model_install_state(&resource.id)
        } else {
            self.model_install_state(&resource.id)
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
        })
    }

    fn tool_status(&self, resource: &ResourceRef) -> RuntimeManagerResult<ResourceStatus> {
        if !self.catalog.tools.contains_key(&resource.id) {
            return Err(RuntimeManagerError::unknown_resource(resource));
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
            "bs_roformer_vocals_ep317"
            | "melband_roformer_inst_v2"
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

fn ggml_model_identity(model_id: &str) -> Option<(&'static str, u64)> {
    match model_id {
        "bs_roformer_vocals_ep317" => Some((
            "8dc288b386a2bb1b554258b0852479bafca71bf37a2d831b92e890fb9dc4b5de",
            320_092_800,
        )),
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
        _ => None,
    }
}

fn legacy_model_path(root: &std::path::Path, model_id: &str) -> Option<PathBuf> {
    let relative = match model_id {
        "bs_roformer_vocals_ep317"
        | "melband_roformer_inst_v2"
        | "melband_roformer_harmony"
        | "melband_roformer_denoise_aufr33"
        | "melband_roformer_dereverb_anvuew" => PathBuf::from("audio-processing").join(model_id),
        "firered_asr2_aed" => PathBuf::from("firered-asr2-aed/openvino-ir-2026.3.0-smoke"),
        "qwen3_asr_1_7b" => PathBuf::from("qwen-asr/Qwen3-ASR-1.7B-Q4_K_M.gguf"),
        "qwen3_forced_aligner_0_6b" => {
            PathBuf::from("qwen-align/qwen3-forced-aligner-predict-woo-f16.gguf")
        }
        "rmvpe" => PathBuf::from(RMVPE_IR_RELATIVE_DIR),
        "fcpe" => PathBuf::from("pitch/fcpe/openvino-ir-2026.3.0-smoke"),
        "game" => PathBuf::from("boundary/game"),
        "basic_pitch" => PathBuf::from("boundary/basic-pitch/openvino-ir-2026.3.0-smoke"),
        "stars" => PathBuf::from("technique/stars"),
        _ => return None,
    };
    Some(root.join(relative))
}

fn legacy_rmvpe_present(root: &std::path::Path) -> bool {
    root.join(RMVPE_IR_RELATIVE_DIR)
        .join("manifest.json")
        .is_file()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn game_is_production_usable_after_repaired_full_song_rerun() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("game");
        fixture.write_model_current_with_catalog("game", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("game").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::OpenVino));
    }

    #[test]
    fn game_is_benchmark_usable_when_verified_model_and_worker_are_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("game");
        fixture.write_model_current_with_catalog("game", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("game").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::OpenVino));
    }

    #[test]
    fn rmvpe_is_production_usable_when_installed_and_runtime_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::OpenVino));
    }

    #[test]
    fn cpu_reference_route_requires_explicit_experimental_selection() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let resource = ResourceRef::model("rmvpe").unwrap();
        let status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Experimental,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::CpuReference));
        let resolved = manager
            .resolve_model_with_backend(
                "rmvpe",
                RuntimePolicy::Experimental,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::CpuReference);
        let production = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Production,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert!(!production.usable);
        assert_eq!(production.selected_backend, None);
    }

    #[test]
    fn pinned_ggml_route_selects_its_worker_without_hash_rejection() {
        let fixture = Fixture::new();
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let ggml_root = fixture.root.join("ggml-models");
        let model_dir = ggml_root.join("bs_roformer_vocals_ep317");
        fs::create_dir_all(&model_dir).unwrap();
        let model = fs::File::create(model_dir.join("model-fp16.gguf")).unwrap();
        model.set_len(320_092_800).unwrap();
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_ggml_models_root(ggml_root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let resource = ResourceRef::model("bs_roformer_vocals_ep317").unwrap();
        let status = manager.status(&resource, RuntimePolicy::Benchmark).unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::Vulkan));
        assert_eq!(
            status.runtime_resource,
            Some(ResourceRef::runtime("ggml_vulkan_v1").unwrap())
        );
        let resolved = manager
            .resolve_model("bs_roformer_vocals_ep317", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::Vulkan);
    }

    #[test]
    fn managed_qwen_vulkan_resolution_selects_the_converted_gguf_file() {
        let fixture = Fixture::new();
        let mut catalog = ResourceCatalog::default_catalog().unwrap();
        let payload_digest = format!("{:x}", Sha256::digest(b"managed fixture"));
        let model = catalog.models.get_mut("qwen3_forced_aligner_0_6b").unwrap();
        model.source.filename = Some("model.bin".to_string());
        model.source.sha256 = Some(payload_digest.clone());
        model.source.artifacts.clear();
        let converted = model.source.converted_artifact.as_mut().unwrap();
        converted.manifest_filename = "model.bin".to_string();
        converted.manifest_sha256 = payload_digest;
        converted.conversion_recipe_sha256.clear();
        fixture.write_model_current_with_catalog("qwen3_forced_aligner_0_6b", &catalog);
        let worker = fixture.write_executable("qwen-align-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("qwen_align_runtime", worker),
        );
        let resolved = manager
            .resolve_model("qwen3_forced_aligner_0_6b", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::Vulkan);
        assert_eq!(resolved.model_path.file_name().unwrap(), "model.bin");
        assert!(resolved.model_path.is_file());
    }

    #[test]
    fn managed_pinned_source_cannot_claim_identity_for_different_payload() {
        let fixture = Fixture::new();
        fixture.write_model_current("qwen3_asr_1_7b", "fake-qwen");
        let worker = fixture.write_executable("qwen-worker");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("qwen_asr_runtime", worker),
        )
        .unwrap();
        let status = manager
            .status(
                &ResourceRef::model("qwen3_asr_1_7b").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert!(!status.usable);
        assert_eq!(status.install_state, InstallState::Corrupt);
        assert_eq!(status.origin, ResourceOrigin::Managed);
        assert!(status.reasons.contains(&ReadinessReason::Corrupt));
    }

    #[test]
    fn production_rejects_candidate_even_when_installed_and_runtime_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("fcpe");
        fixture.write_model_current_with_catalog("fcpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("fcpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert!(status.integrity_verified);
        assert!(status.runnable);
        assert!(!status.usable);
        let error = manager
            .resolve_model("fcpe", RuntimePolicy::Production)
            .unwrap_err();
        assert_eq!(error.code, "no_validated_backend");
    }

    #[test]
    fn benchmark_can_resolve_candidate_when_all_local_pieces_are_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("fcpe");
        fixture.write_model_current_with_catalog("fcpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker.clone()),
        );
        let resolved = manager
            .resolve_model("fcpe", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.model_id, "fcpe");
        assert_eq!(resolved.backend, NativeBackend::OpenVino);
        assert_eq!(resolved.runtime_content_digest, "environment");
        assert_eq!(resolved.runtime_executable, worker);
    }

    #[test]
    fn installed_model_is_not_usable_when_runtime_is_missing() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default().with_store_root(&fixture.root),
        );
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Installed);
        assert!(!status.usable);
        assert!(status.reasons.contains(&ReadinessReason::RuntimeMissing));
    }

    #[test]
    fn show_combines_catalog_metadata_with_local_status() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let details = manager
            .show(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(details.metadata.display_name, "RMVPE");
        assert!(details.status.usable);
        assert_eq!(details.metadata.capabilities, vec!["pitch.track"]);
    }

    #[test]
    fn metadata_and_verified_status_use_structural_checks_for_same_size_payloads() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let resource = ResourceRef::model("rmvpe").unwrap();

        let payload = fixture.current_model_generation("rmvpe").join("model.bin");
        fs::remove_file(&payload).unwrap();
        assert_eq!(
            manager
                .status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Incomplete
        );

        fs::write(&payload, b"managed fixturE").unwrap();
        assert_eq!(
            manager
                .status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Installed
        );
        assert_eq!(
            manager
                .verified_status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Installed
        );
    }

    #[test]
    fn managed_generation_rejects_undeclared_files() {
        let fixture = Fixture::new();
        fixture.write_model_current("rmvpe", "rmvpe-gen");
        fs::write(
            fixture
                .current_model_generation("rmvpe")
                .join("unknown.bin"),
            b"unmanaged",
        )
        .unwrap();
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Corrupt);
    }

    #[test]
    fn legacy_openvino_roformer_manifest_cannot_restore_forbidden_route() {
        let fixture = Fixture::new();
        let model_dir = fixture
            .root
            .join("audio-processing/bs_roformer_vocals_ep317");
        fs::create_dir_all(&model_dir).unwrap();
        fs::write(model_dir.join("install-manifest.json"), b"{}").unwrap();
        let worker = fixture.write_executable("roformer-worker");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default()
                .with_legacy_models_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        )
        .unwrap();
        let resource = ResourceRef::model("bs_roformer_vocals_ep317").unwrap();
        let status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Benchmark,
                Some(NativeBackend::OpenVino),
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Legacy);
        assert!(!status.usable);
        assert!(status.reasons.contains(&ReadinessReason::Legacy));

        let testing_status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Experimental,
                Some(NativeBackend::OpenVino),
            )
            .unwrap();
        assert!(!testing_status.runnable, "{testing_status:#?}");
        assert!(!testing_status.usable);
        assert!(
            testing_status
                .reasons
                .contains(&ReadinessReason::BackendUnvalidated)
        );
        assert!(testing_status.reasons.contains(&ReadinessReason::Legacy));
    }

    #[test]
    fn read_operations_do_not_create_a_configured_store() {
        let fixture = Fixture::new();
        let absent_store = fixture.root.join("not-created");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&absent_store),
        )
        .unwrap();
        let rmvpe = ResourceRef::model("rmvpe").unwrap();
        let _ = manager.list(RuntimePolicy::Production).unwrap();
        let _ = manager.show(&rmvpe, RuntimePolicy::Production).unwrap();
        let _ = manager.status(&rmvpe, RuntimePolicy::Production).unwrap();
        let _ = manager.paths_summary();
        let _ = manager.verify(&[rmvpe], RuntimePolicy::Production).unwrap();
        let _ = manager.doctor();
        assert!(!absent_store.exists());
    }

    #[test]
    fn verify_reports_corrupt_current_pointer_without_mutating_it() {
        let fixture = Fixture::new();
        fixture.write_raw_model_current("rmvpe", b"not json");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let before = fixture.read_model_current("rmvpe");
        let report = manager
            .verify(
                &[ResourceRef::model("rmvpe").unwrap()],
                RuntimePolicy::Production,
            )
            .unwrap();
        let after = fixture.read_model_current("rmvpe");
        assert_eq!(before, after);
        assert_eq!(report.corrupt, vec![ResourceRef::model("rmvpe").unwrap()]);
    }

    #[test]
    fn doctor_is_read_only_and_reports_runtime_lock() {
        let fixture = Fixture::new();
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let before = fs::read_dir(&fixture.root).unwrap().count();
        let report = manager.doctor();
        let after = fs::read_dir(&fixture.root).unwrap().count();
        assert_eq!(before, after);
        assert!(report.checks.iter().any(|check| check.id == "runtime_lock"));
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "uta-runtime-manager-test-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn catalog_with_fixture_model(model_id: &str) -> ResourceCatalog {
            let mut catalog = ResourceCatalog::default_catalog().unwrap();
            let model = catalog.models.get_mut(model_id).unwrap();
            model.source.filename = Some("model.bin".to_string());
            model.source.sha256 = Some(format!("{:x}", Sha256::digest(b"managed fixture")));
            model.source.converted_artifact = None;
            catalog
        }

        fn write_model_current(&self, model_id: &str, _generation_label: &str) {
            let catalog = ResourceCatalog::default_catalog().unwrap();
            self.write_model_current_with_catalog(model_id, &catalog);
        }

        fn write_model_current_with_catalog(&self, model_id: &str, catalog: &ResourceCatalog) {
            let resource = ResourceRef::model(model_id).unwrap();
            let generation_root = self.root.join("models").join(model_id).join("generations");
            let payload = b"managed fixture";
            let payload_digest = format!("{:x}", Sha256::digest(payload));
            let model = catalog.model(model_id).unwrap();
            let manifest = crate::manifest::InstallManifest {
                schema: crate::manifest::INSTALL_MANIFEST_SCHEMA.to_string(),
                schema_version: crate::manifest::INSTALL_MANIFEST_SCHEMA_VERSION,
                resource,
                catalog_version: crate::catalog::RUNTIME_CATALOG_VERSION.to_string(),
                source: Some(model.source.clone()),
                source_sha256: model.source.sha256.clone(),
                model_recipe_digest: Some(model.recipe_digest.clone()),
                conversion_recipe_digest: None,
                runtime_recipe_digest: model.runtime_recipe_digest.clone(),
                files: vec![crate::manifest::InstalledFile {
                    path: PathBuf::from("model.bin"),
                    sha256: payload_digest,
                    size: payload.len() as u64,
                }],
                created_timestamp: "fixture".to_string(),
            };
            let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
            let generation = crate::manifest::generation_id(&manifest_bytes);
            let generation_dir = generation_root.join(&generation);
            fs::create_dir_all(&generation_dir).unwrap();
            fs::write(generation_dir.join("model.bin"), payload).unwrap();
            fs::write(generation_dir.join("install-manifest.json"), manifest_bytes).unwrap();
            self.write_raw_model_current(
                model_id,
                format!(r#"{{"generation":"{generation}"}}"#).as_bytes(),
            );
        }

        fn write_raw_model_current(&self, model_id: &str, bytes: &[u8]) {
            let dir = self.root.join("models").join(model_id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("current.json"), bytes).unwrap();
        }

        fn read_model_current(&self, model_id: &str) -> Vec<u8> {
            fs::read(self.root.join("models").join(model_id).join("current.json")).unwrap()
        }

        fn current_model_generation(&self, model_id: &str) -> PathBuf {
            let pointer: CurrentPointer =
                serde_json::from_slice(&self.read_model_current(model_id)).unwrap();
            self.root
                .join("models")
                .join(model_id)
                .join("generations")
                .join(pointer.generation)
        }

        fn write_executable(&self, name: &str) -> PathBuf {
            #[cfg(windows)]
            let path = self.root.join(format!("{name}.exe"));
            #[cfg(not(windows))]
            let path = self.root.join(name);
            fs::write(&path, b"fixture").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
