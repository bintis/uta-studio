use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::acquire::{AcquisitionTransport, HttpAcquisitionTransport};
use crate::catalog::{
    AcquisitionMethod, AcquisitionSpec, LicenseInfo, ModelCatalogEntry, ResourceCatalog,
};
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::lease::ResourceLease;
use crate::manifest::{
    INSTALL_MANIFEST_SCHEMA, INSTALL_MANIFEST_SCHEMA_VERSION, InstallManifest, InstalledFile,
    generation_id, is_generation_id, safe_relative_path, verify_generation,
};
use crate::resolver::{RuntimeManager, generation_lease_key};
use crate::resource::{ResourceKind, ResourceRef};
use crate::state::{InstallState, ReadinessReason, ResourceOrigin, RuntimePolicy, ValidationState};
use crate::store::{CurrentPointer, StorePaths};

mod advanced_notes;
mod game;
mod optional_experts;
mod roformer_denoise;
mod roformer_dereverb;
mod roformer_harmony;
mod roformer_inst_v2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourcePlan {
    pub requested: Vec<ResourceRef>,
    pub to_add: Vec<PlannedResource>,
    pub satisfied: Vec<ResourceRef>,
    pub unavailable: Vec<ResourceRef>,
    pub network_required: bool,
    pub estimated_download_bytes: Option<u64>,
    pub estimated_installed_bytes: Option<u64>,
    pub existing_resources_to_change: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedResource {
    pub resource: ResourceRef,
    pub dependencies: Vec<ResourceRef>,
    pub acquisition: Vec<AcquisitionSpec>,
    pub validation_state: ValidationState,
    pub conversion_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<LicenseInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub changed: Vec<ResourceRef>,
    pub unchanged: Vec<ResourceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationOptions {
    pub confirmed: bool,
}

impl RuntimeManager {
    pub fn plan(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<ResourcePlan> {
        let mut expanded = Vec::new();
        let mut visiting = BTreeSet::new();
        for resource in resources {
            expand_resource(self.catalog(), resource, &mut visiting, &mut expanded)?;
        }
        let mut seen = BTreeSet::new();
        expanded.retain(|resource| seen.insert(resource.clone()));

        let mut plan = ResourcePlan {
            requested: resources.to_vec(),
            to_add: Vec::new(),
            satisfied: Vec::new(),
            unavailable: Vec::new(),
            network_required: false,
            estimated_download_bytes: Some(0),
            estimated_installed_bytes: Some(0),
            existing_resources_to_change: Vec::new(),
        };
        for resource in expanded {
            let status = self.status(&resource, policy)?;
            if status.install_state.locally_present()
                && !status.reasons.contains(&ReadinessReason::Legacy)
                && (resource.kind == ResourceKind::Model || status.executable_ready)
            {
                plan.satisfied.push(resource);
                continue;
            }
            if matches!(
                status.install_state,
                InstallState::Corrupt | InstallState::Incomplete
            ) || (status.install_state == InstallState::Legacy
                && status.origin == ResourceOrigin::Managed)
            {
                plan.existing_resources_to_change.push(resource.clone());
            }
            let fields = catalog_plan_fields(self.catalog(), &resource)?;
            let acquirable = fields
                .acquisition
                .iter()
                .any(|spec| spec.method != AcquisitionMethod::Unavailable);
            if !acquirable {
                plan.unavailable.push(resource.clone());
            }
            let resource_needs_network = fields
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::ManagedDownload);
            plan.network_required |= resource_needs_network;
            if resource_needs_network {
                plan.estimated_download_bytes = sum_estimate(
                    plan.estimated_download_bytes,
                    fields.estimated_download_bytes,
                );
            }
            plan.estimated_installed_bytes = sum_estimate(
                plan.estimated_installed_bytes,
                fields.estimated_installed_bytes,
            );
            plan.to_add.push(PlannedResource {
                resource,
                dependencies: fields.dependencies,
                conversion_required: fields
                    .acquisition
                    .iter()
                    .any(|spec| spec.method == AcquisitionMethod::SourceConvert),
                acquisition: fields.acquisition,
                validation_state: fields.validation_state,
                license: fields.license,
            });
        }
        Ok(plan)
    }

    pub fn install(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        self.install_with_transport(resources, policy, options, &HttpAcquisitionTransport)
    }

    pub fn install_with_transport(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
        options: &MutationOptions,
        transport: &dyn AcquisitionTransport,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        let plan = self.plan(resources, policy)?;
        if let Some(corrupt) = plan.to_add.iter().find(|item| {
            self.status(&item.resource, policy)
                .is_ok_and(|status| status.install_state == InstallState::Corrupt)
        }) {
            return Err(RuntimeManagerError::resource_corrupt(&corrupt.resource));
        }
        if let Some(resource) = plan.unavailable.first() {
            return Err(not_acquirable(
                resource,
                "the shipped catalog has no audited acquisition for this resource",
            ));
        }
        let mut changed = Vec::new();
        for item in &plan.to_add {
            acquire_planned_resource(self, item, options, transport)?;
            changed.push(item.resource.clone());
        }
        Ok(MutationResult {
            changed,
            unchanged: plan.satisfied,
        })
    }

    pub fn setup_requirements(
        &self,
        requirements: &crate::requirements::RequirementSet,
        policy: RuntimePolicy,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        requirements.validate()?;
        for requirement in &requirements.resources {
            if !self.catalog().contains(&requirement.resource) {
                return Err(RuntimeManagerError::unknown_resource(&requirement.resource));
            }
        }
        let resources = requirements
            .resources
            .iter()
            .filter(|requirement| requirement.required)
            .map(|requirement| requirement.resource.clone())
            .collect::<Vec<_>>();
        self.install(&resources, policy, options)
    }

    pub fn import_resource(
        &self,
        resource: &ResourceRef,
        source: &Path,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        if resource.kind != ResourceKind::Model {
            return Err(not_acquirable(
                resource,
                "only audited model imports are supported",
            ));
        }
        let model = self
            .catalog()
            .model(&resource.id)
            .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
        if !model
            .acquisition
            .iter()
            .any(|spec| spec.method == AcquisitionMethod::LocalImport)
            && !(model
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::ManagedDownload)
                && model
                    .source
                    .source_format
                    .as_deref()
                    .is_some_and(|format| format.starts_with("gguf")))
        {
            return Err(not_acquirable(resource, "no audited import recipe exists"));
        }
        let converted_file = model
            .source
            .converted_artifact
            .as_ref()
            .filter(|artifact| artifact.format.starts_with("gguf"));
        if source.is_dir() && converted_file.is_none() {
            let imported = match resource.id.as_str() {
                "game" => Some(game::import_game_ir_directory(
                    self, resource, model, source,
                )?),
                "melband_roformer_inst_v2" => {
                    Some(roformer_inst_v2::import_roformer_inst_v2_ir_directory(
                        self, resource, model, source,
                    )?)
                }
                "melband_roformer_harmony" => {
                    Some(roformer_harmony::import_roformer_harmony_ir_directory(
                        self, resource, model, source,
                    )?)
                }
                "melband_roformer_denoise_aufr33" => {
                    Some(roformer_denoise::import_roformer_denoise_ir_directory(
                        self, resource, model, source,
                    )?)
                }
                "melband_roformer_dereverb_anvuew" => {
                    Some(roformer_dereverb::import_roformer_dereverb_ir_directory(
                        self, resource, model, source,
                    )?)
                }
                "stars" | "rosvot" => Some(advanced_notes::import_advanced_note_ir_directory(
                    self, resource, model, source,
                )?),
                "firered_asr2_aed" | "fcpe" | "basic_pitch" => {
                    Some(optional_experts::import_optional_expert_ir_directory(
                        self, resource, model, source,
                    )?)
                }
                _ => None,
            };
            if imported.is_some() {
                return Ok(MutationResult {
                    changed: vec![resource.clone()],
                    unchanged: Vec::new(),
                });
            }
        }
        let filename = converted_file
            .map(|artifact| artifact.manifest_filename.as_str())
            .or(model.source.filename.as_deref())
            .ok_or_else(|| {
                not_acquirable(
                    resource,
                    "the catalog does not declare an installed filename",
                )
            })?;
        if filename.contains('/') || filename.contains('\\') || filename.contains(':') {
            return Err(RuntimeManagerError::new(
                "invalid_catalog",
                "catalog installed filename is not a safe leaf name",
            )
            .with_resource(resource));
        }
        let source_file = if source.is_dir() {
            source.join(filename)
        } else {
            source.to_path_buf()
        };
        let _generation = publish_single_file(
            self.paths(),
            resource,
            &self.catalog().catalog_version,
            &source_file,
            Path::new(filename),
            PublishIdentity {
                source: Some(model.source.clone()),
                source_sha256: converted_file
                    .map(|artifact| artifact.manifest_sha256.clone())
                    .or_else(|| model.source.sha256.clone()),
                model_recipe_digest: Some(model.recipe_digest.clone()),
                conversion_recipe_digest: converted_file
                    .map(|artifact| artifact.conversion_recipe_sha256.clone())
                    .filter(|digest| !digest.is_empty()),
                runtime_recipe_digest: model.runtime_recipe_digest.clone(),
            },
        )?;
        Ok(MutationResult {
            changed: vec![resource.clone()],
            unchanged: Vec::new(),
        })
    }

    pub fn repair(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        self.repair_with_transport(resources, policy, options, &HttpAcquisitionTransport)
    }

    pub fn repair_with_transport(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
        options: &MutationOptions,
        transport: &dyn AcquisitionTransport,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        for resource in resources {
            let status = self.status(resource, policy)?;
            if !matches!(
                status.install_state,
                InstallState::Corrupt | InstallState::Incomplete
            ) {
                unchanged.push(resource.clone());
                continue;
            }
            let item = planned_resource_for(self.catalog(), resource)?;
            if !item
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::ManagedDownload)
            {
                return Err(RuntimeManagerError::new(
                    "repair_requires_source",
                    format!("repair for {resource} requires its pinned import source"),
                )
                .with_resource(resource));
            }
            acquire_planned_resource(self, &item, options, transport)?;
            changed.push(resource.clone());
        }
        Ok(MutationResult { changed, unchanged })
    }

    pub fn reinstall(
        &self,
        resources: &[ResourceRef],
        policy: RuntimePolicy,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        self.reinstall_with_transport(resources, policy, options, &HttpAcquisitionTransport)
    }

    pub fn reinstall_with_transport(
        &self,
        resources: &[ResourceRef],
        _policy: RuntimePolicy,
        options: &MutationOptions,
        transport: &dyn AcquisitionTransport,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        let mut changed = Vec::new();
        for resource in resources {
            let item = planned_resource_for(self.catalog(), resource)?;
            if !item.acquisition.iter().any(|spec| {
                matches!(
                    spec.method,
                    AcquisitionMethod::ManagedDownload | AcquisitionMethod::SourceConvert
                )
            }) {
                return Err(not_acquirable(
                    resource,
                    "reinstall requires the pinned import source for this resource",
                ));
            }
            acquire_planned_resource(self, &item, options, transport)?;
            changed.push(resource.clone());
        }
        Ok(MutationResult {
            changed,
            unchanged: Vec::new(),
        })
    }

    pub fn rollback(
        &self,
        resource: &ResourceRef,
        generation: &str,
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        if !is_generation_id(generation) {
            return Err(RuntimeManagerError::new(
                "integrity_mismatch",
                "rollback generation id is invalid",
            )
            .with_resource(resource));
        }
        let resource_root = managed_resource_root(self.paths(), resource).ok_or_else(|| {
            RuntimeManagerError::new("publish_failed", "runtime store is not configured")
        })?;
        let _lock = MutationLock::acquire(self.paths())?;
        let generation_dir = resource_root.join("generations").join(generation);
        if verify_generation(&generation_dir, generation, resource).state != InstallState::Installed
        {
            return Err(RuntimeManagerError::new(
                "integrity_mismatch",
                "rollback generation is not verified",
            )
            .with_resource(resource));
        }
        switch_current(&resource_root, generation)?;
        Ok(MutationResult {
            changed: vec![resource.clone()],
            unchanged: Vec::new(),
        })
    }

    pub fn remove(
        &self,
        resources: &[ResourceRef],
        options: &MutationOptions,
    ) -> RuntimeManagerResult<MutationResult> {
        require_confirmation(options)?;
        let _lock = MutationLock::acquire(self.paths())?;
        let mut removals = Vec::new();
        let mut unchanged = Vec::new();
        for resource in resources {
            if !self.catalog().contains(resource) {
                return Err(RuntimeManagerError::unknown_resource(resource));
            }
            let resource_root = managed_resource_root(self.paths(), resource).ok_or_else(|| {
                RuntimeManagerError::new("publish_failed", "runtime store is not configured")
            })?;
            if !resource_root.exists() {
                unchanged.push(resource.clone());
                continue;
            }
            validate_owned_resource_tree(&resource_root, resource)?;
            for entry in std::fs::read_dir(resource_root.join("generations")).map_err(publish_io)? {
                let entry = entry.map_err(publish_io)?;
                let generation = entry.file_name().to_string_lossy().into_owned();
                if ResourceLease::is_active(&generation_lease_key(resource, &generation))
                    || ResourceLease::path_is_locked(&entry.path().join("install-manifest.json"))
                {
                    return Err(RuntimeManagerError::new(
                        "resource_in_use",
                        format!("{resource} generation {generation} is leased"),
                    )
                    .with_resource(resource));
                }
            }
            removals.push((resource.clone(), resource_root));
        }
        let mut changed = Vec::new();
        for (resource, resource_root) in removals {
            std::fs::remove_dir_all(&resource_root).map_err(publish_io)?;
            changed.push(resource);
        }
        Ok(MutationResult { changed, unchanged })
    }
}

fn planned_resource_for(
    catalog: &ResourceCatalog,
    resource: &ResourceRef,
) -> RuntimeManagerResult<PlannedResource> {
    let fields = catalog_plan_fields(catalog, resource)?;
    Ok(PlannedResource {
        resource: resource.clone(),
        dependencies: fields.dependencies,
        conversion_required: fields
            .acquisition
            .iter()
            .any(|spec| spec.method == AcquisitionMethod::SourceConvert),
        acquisition: fields.acquisition,
        validation_state: fields.validation_state,
        license: fields.license,
    })
}

fn acquire_planned_resource(
    manager: &RuntimeManager,
    item: &PlannedResource,
    _options: &MutationOptions,
    transport: &dyn AcquisitionTransport,
) -> RuntimeManagerResult<()> {
    match item.resource.kind {
        ResourceKind::Model => {
            let model = manager
                .catalog()
                .model(&item.resource.id)
                .ok_or_else(|| RuntimeManagerError::unknown_resource(&item.resource))?;
            if item
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::ManagedDownload)
            {
                acquire_managed_model(manager, model, transport)
            } else if item
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::SourceConvert)
            {
                Err(RuntimeManagerError::new(
                    "conversion_failed",
                    "the catalog-pinned conversion recipe is not available in this build",
                )
                .with_resource(&item.resource))
            } else {
                Err(not_acquirable(
                    &item.resource,
                    "this resource requires an explicit pinned local import",
                ))
            }
        }
        ResourceKind::Runtime => Err(not_acquirable(
            &item.resource,
            "bundled runtimes are supplied by the application package",
        )),
        ResourceKind::Tool => Err(not_acquirable(
            &item.resource,
            "external tools must be installed or configured explicitly",
        )),
        ResourceKind::Bundle => Err(RuntimeManagerError::invalid_catalog(
            "bundles must be expanded before acquisition",
        )),
    }
}

fn acquire_managed_model(
    manager: &RuntimeManager,
    model: &ModelCatalogEntry,
    transport: &dyn AcquisitionTransport,
) -> RuntimeManagerResult<()> {
    let resource = model.resource();
    let filename =
        model.source.filename.as_deref().ok_or_else(|| {
            not_acquirable(&resource, "the catalog has no pinned artifact filename")
        })?;
    validate_leaf_filename(filename).map_err(|error| error.with_resource(&resource))?;
    let url = managed_download_url(model)?;
    let root = manager.paths().store_root.as_ref().ok_or_else(|| {
        RuntimeManagerError::new("publish_failed", "runtime store is not configured")
    })?;
    prepare_store_root(root)?;
    let required_space = model
        .estimated_download_bytes
        .zip(model.estimated_installed_bytes)
        .and_then(|(download, installed)| download.checked_add(installed))
        .or(model.estimated_download_bytes);
    if required_space.is_some_and(|required| {
        fs2::available_space(root).is_ok_and(|available| available < required)
    }) {
        return Err(RuntimeManagerError::new(
            "insufficient_space",
            format!("insufficient disk space to acquire {resource}"),
        )
        .with_resource(&resource));
    }
    let downloads = root.join("downloads");
    ensure_managed_directory(&downloads)?;
    let temporary = downloads.join(format!(
        ".{}-{}.download",
        resource.id,
        unique_operation_id()
    ));
    let guard = DownloadGuard(temporary.clone());
    transport.download(&url, &temporary, model.estimated_download_bytes)?;
    publish_single_file(
        manager.paths(),
        &resource,
        &manager.catalog().catalog_version,
        &temporary,
        Path::new(filename),
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: model.source.sha256.clone(),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: None,
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )?;
    drop(guard);
    Ok(())
}

fn managed_download_url(model: &ModelCatalogEntry) -> RuntimeManagerResult<String> {
    let resource = model.resource();
    let repository =
        model.source.repository.as_deref().ok_or_else(|| {
            not_acquirable(&resource, "the catalog has no pinned source repository")
        })?;
    if repository.is_empty()
        || repository.starts_with('/')
        || repository.contains("..")
        || repository.contains(['?', '#', '\\'])
    {
        return Err(
            RuntimeManagerError::invalid_catalog("managed repository identity is unsafe")
                .with_resource(&resource),
        );
    }
    let revision = model.source.revision.as_deref().unwrap_or("main");
    if revision.is_empty()
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(
            RuntimeManagerError::invalid_catalog("managed repository revision is unsafe")
                .with_resource(&resource),
        );
    }
    let filename = model
        .source
        .filename
        .as_deref()
        .ok_or_else(|| not_acquirable(&resource, "the catalog has no artifact filename"))?;
    validate_leaf_filename(filename).map_err(|error| error.with_resource(&resource))?;
    Ok(format!(
        "https://huggingface.co/{repository}/resolve/{revision}/{filename}"
    ))
}

fn validate_leaf_filename(filename: &str) -> RuntimeManagerResult<()> {
    if filename.is_empty()
        || filename == "."
        || filename == ".."
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains(':')
    {
        Err(RuntimeManagerError::invalid_catalog(
            "catalog artifact filename is not a safe leaf name",
        ))
    } else {
        Ok(())
    }
}

fn expand_resource(
    catalog: &ResourceCatalog,
    resource: &ResourceRef,
    visiting: &mut BTreeSet<ResourceRef>,
    output: &mut Vec<ResourceRef>,
) -> RuntimeManagerResult<()> {
    if !catalog.contains(resource) {
        return Err(RuntimeManagerError::unknown_resource(resource));
    }
    if !visiting.insert(resource.clone()) {
        return Err(RuntimeManagerError::new(
            "invalid_catalog",
            format!("resource dependency cycle at {resource}"),
        ));
    }
    let dependencies = match resource.kind {
        ResourceKind::Model => catalog
            .model(&resource.id)
            .map(|entry| entry.dependencies.clone())
            .unwrap_or_default(),
        ResourceKind::Bundle => catalog
            .bundles
            .get(&resource.id)
            .map(|entry| entry.dependencies.clone())
            .unwrap_or_default(),
        ResourceKind::Runtime | ResourceKind::Tool => Vec::new(),
    };
    for dependency in dependencies {
        expand_resource(catalog, &dependency, visiting, output)?;
    }
    visiting.remove(resource);
    if resource.kind != ResourceKind::Bundle {
        output.push(resource.clone());
    }
    Ok(())
}

struct CatalogPlanFields {
    dependencies: Vec<ResourceRef>,
    acquisition: Vec<AcquisitionSpec>,
    validation_state: ValidationState,
    estimated_download_bytes: Option<u64>,
    estimated_installed_bytes: Option<u64>,
    license: Option<LicenseInfo>,
}

fn catalog_plan_fields(
    catalog: &ResourceCatalog,
    resource: &ResourceRef,
) -> RuntimeManagerResult<CatalogPlanFields> {
    match resource.kind {
        ResourceKind::Model => {
            let entry = catalog
                .model(&resource.id)
                .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
            let validation = entry
                .backends
                .iter()
                .map(|backend| backend.validation)
                .min_by_key(|state| match state {
                    ValidationState::ProductionPinned => 0,
                    ValidationState::BenchmarkCandidate => 1,
                    ValidationState::Experimental => 2,
                    ValidationState::Unsupported => 3,
                })
                .unwrap_or(ValidationState::Unsupported);
            Ok(CatalogPlanFields {
                dependencies: entry.dependencies.clone(),
                acquisition: entry.acquisition.clone(),
                validation_state: validation,
                estimated_download_bytes: entry.estimated_download_bytes,
                estimated_installed_bytes: entry.estimated_installed_bytes,
                license: Some(entry.license.clone()),
            })
        }
        ResourceKind::Runtime => {
            let entry = catalog
                .runtime(&resource.id)
                .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
            let validation = entry
                .backends
                .first()
                .map(|backend| backend.validation)
                .unwrap_or(ValidationState::Unsupported);
            Ok(CatalogPlanFields {
                dependencies: Vec::new(),
                acquisition: entry.acquisition.clone(),
                validation_state: validation,
                estimated_download_bytes: None,
                estimated_installed_bytes: None,
                license: None,
            })
        }
        ResourceKind::Tool => {
            let entry = catalog
                .tools
                .get(&resource.id)
                .ok_or_else(|| RuntimeManagerError::unknown_resource(resource))?;
            Ok(CatalogPlanFields {
                dependencies: Vec::new(),
                acquisition: entry.acquisition.clone(),
                validation_state: ValidationState::ProductionPinned,
                estimated_download_bytes: None,
                estimated_installed_bytes: None,
                license: None,
            })
        }
        ResourceKind::Bundle => Err(RuntimeManagerError::new(
            "invalid_catalog",
            "bundles must be expanded before planning",
        )),
    }
}

fn sum_estimate(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => current.checked_add(next),
        _ => None,
    }
}

fn require_confirmation(options: &MutationOptions) -> RuntimeManagerResult<()> {
    if options.confirmed {
        Ok(())
    } else {
        Err(RuntimeManagerError::new(
            "confirmation_required",
            "explicit confirmation is required for this mutation",
        ))
    }
}

fn not_acquirable(resource: &ResourceRef, message: &str) -> RuntimeManagerError {
    RuntimeManagerError::new("resource_not_acquirable", message).with_resource(resource)
}

struct PublishIdentity {
    source: Option<crate::catalog::SourceIdentity>,
    source_sha256: Option<String>,
    model_recipe_digest: Option<String>,
    conversion_recipe_digest: Option<String>,
    runtime_recipe_digest: Option<String>,
}

fn publish_single_file(
    paths: &StorePaths,
    resource: &ResourceRef,
    catalog_version: &str,
    source: &Path,
    relative: &Path,
    identity: PublishIdentity,
) -> RuntimeManagerResult<String> {
    publish_file_set(
        paths,
        resource,
        catalog_version,
        &[(source.to_path_buf(), relative.to_path_buf())],
        identity,
    )
}

fn publish_file_set(
    paths: &StorePaths,
    resource: &ResourceRef,
    catalog_version: &str,
    sources: &[(PathBuf, PathBuf)],
    identity: PublishIdentity,
) -> RuntimeManagerResult<String> {
    if sources.is_empty() {
        return Err(RuntimeManagerError::new(
            "publish_failed",
            "cannot publish an empty file set",
        ));
    }
    let _lock = MutationLock::acquire(paths)?;
    let root = paths.store_root.as_ref().ok_or_else(|| {
        RuntimeManagerError::new("publish_failed", "runtime store is not configured")
    })?;
    let staging_root = root.join("staging");
    ensure_managed_directory(&staging_root)?;
    let operation = unique_operation_id();
    let staging = staging_root.join(&operation);
    std::fs::create_dir(&staging).map_err(publish_io)?;
    let guard = StagingGuard(staging.clone());

    let mut installed_files = Vec::with_capacity(sources.len());
    let mut relative_paths = BTreeSet::new();
    for (source, relative) in sources {
        if !safe_relative_path(relative) {
            return Err(RuntimeManagerError::invalid_catalog(
                "published file path is not a safe relative path",
            ));
        }
        if !relative_paths.insert(relative.clone()) {
            return Err(RuntimeManagerError::new(
                "publish_failed",
                "published file paths must be unique",
            ));
        }
        let staged_file = staging.join(relative);
        if let Some(parent) = staged_file.parent() {
            std::fs::create_dir_all(parent).map_err(publish_io)?;
        }
        copy_file(source, &staged_file)?;
        let metadata = std::fs::metadata(&staged_file).map_err(publish_io)?;
        installed_files.push(InstalledFile {
            path: relative.clone(),
            sha256: sha256_file(&staged_file).map_err(publish_io)?,
            size: metadata.len(),
        });
    }
    installed_files.sort_by(|left, right| left.path.cmp(&right.path));
    let manifest = InstallManifest {
        schema: INSTALL_MANIFEST_SCHEMA.to_string(),
        schema_version: INSTALL_MANIFEST_SCHEMA_VERSION,
        resource: resource.clone(),
        catalog_version: catalog_version.to_string(),
        source: identity.source,
        source_sha256: identity.source_sha256,
        model_recipe_digest: identity.model_recipe_digest,
        conversion_recipe_digest: identity.conversion_recipe_digest,
        runtime_recipe_digest: identity.runtime_recipe_digest,
        files: installed_files,
        created_timestamp: created_timestamp(),
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|error| {
        RuntimeManagerError::new(
            "publish_failed",
            format!("manifest serialization failed: {error}"),
        )
    })?;
    let generation = generation_id(&manifest_bytes);
    write_new_file_sync(&staging.join("install-manifest.json"), &manifest_bytes)?;

    let resource_root = managed_resource_root(paths, resource).ok_or_else(|| {
        RuntimeManagerError::new("publish_failed", "resource kind cannot be published")
    })?;
    let kind_root = resource_root.parent().ok_or_else(|| {
        RuntimeManagerError::new("publish_failed", "managed resource root has no parent")
    })?;
    ensure_managed_directory(kind_root)?;
    ensure_managed_directory(&resource_root)?;
    validate_publish_resource_root(&resource_root, resource)?;
    let generations = resource_root.join("generations");
    ensure_managed_directory(&generations)?;
    let destination = generations.join(&generation);
    let destination_created = if destination.exists() {
        let verified = verify_generation(&destination, &generation, resource);
        if verified.state != InstallState::Installed {
            return Err(RuntimeManagerError::new(
                "publish_failed",
                "an existing generation with the same identity is invalid",
            )
            .with_resource(resource));
        }
        false
    } else {
        std::fs::rename(&staging, &destination).map_err(publish_io)?;
        sync_directory(&generations)?;
        true
    };
    let verified = verify_generation(&destination, &generation, resource);
    if verified.state != InstallState::Installed {
        if destination_created {
            let _ = std::fs::remove_dir_all(&destination);
        }
        return Err(RuntimeManagerError::new(
            "publish_failed",
            "published generation failed structural validation",
        )
        .with_resource(resource));
    }
    if let Err(error) = switch_current(&resource_root, &generation) {
        if destination_created {
            let _ = std::fs::remove_dir_all(&destination);
        }
        return Err(error);
    }
    drop(guard);
    Ok(generation)
}

fn managed_resource_root(paths: &StorePaths, resource: &ResourceRef) -> Option<PathBuf> {
    let root = paths.store_root.as_ref()?;
    let kind = match resource.kind {
        ResourceKind::Model => "models",
        ResourceKind::Runtime => "runtimes",
        ResourceKind::Tool => "tools",
        ResourceKind::Bundle => return None,
    };
    Some(root.join(kind).join(&resource.id))
}

fn switch_current(resource_root: &Path, generation: &str) -> RuntimeManagerResult<()> {
    let bytes = serde_json::to_vec(&CurrentPointer {
        generation: generation.to_string(),
    })
    .map_err(|error| RuntimeManagerError::new("publish_failed", error.to_string()))?;
    let temporary = resource_root.join(format!(".current-{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(publish_io)?;
    file.write_all(&bytes).map_err(publish_io)?;
    file.sync_all().map_err(publish_io)?;
    let current = resource_root.join("current.json");
    let result = atomic_replace(&temporary, &current).and_then(|()| sync_directory(resource_root));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> RuntimeManagerResult<()> {
    std::fs::rename(source, destination).map_err(publish_io)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> RuntimeManagerResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(publish_io(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn validate_publish_resource_root(root: &Path, resource: &ResourceRef) -> RuntimeManagerResult<()> {
    for entry in std::fs::read_dir(root).map_err(publish_io)? {
        let entry = entry.map_err(publish_io)?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(publish_io)?;
        let owned = match entry.file_name().to_string_lossy().as_ref() {
            "current.json" => metadata.file_type().is_file(),
            "generations" => metadata.file_type().is_dir(),
            _ => false,
        };
        if !owned {
            return Err(RuntimeManagerError::new(
                "unmanaged_files_present",
                format!("unmanaged content is present under {}", root.display()),
            )
            .with_resource(resource));
        }
    }
    Ok(())
}

fn validate_owned_resource_tree(root: &Path, resource: &ResourceRef) -> RuntimeManagerResult<()> {
    let mut has_current = false;
    for entry in std::fs::read_dir(root).map_err(publish_io)? {
        let entry = entry.map_err(publish_io)?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(publish_io)?;
        match entry.file_name().to_string_lossy().as_ref() {
            "current.json" if metadata.file_type().is_file() => has_current = true,
            "generations" if metadata.file_type().is_dir() => {}
            _ => {
                return Err(RuntimeManagerError::new(
                    "unmanaged_files_present",
                    format!("unmanaged content is present under {}", root.display()),
                )
                .with_resource(resource));
            }
        }
    }
    if !has_current {
        return Err(RuntimeManagerError::new(
            "unmanaged_files_present",
            "managed resource has no current pointer",
        )
        .with_resource(resource));
    }
    let generations = root.join("generations");
    if !generations.is_dir() {
        return Err(RuntimeManagerError::new(
            "unmanaged_files_present",
            "managed resource has no generations directory",
        )
        .with_resource(resource));
    }
    for entry in std::fs::read_dir(generations).map_err(publish_io)? {
        let entry = entry.map_err(publish_io)?;
        let generation = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_dir()
            || !is_generation_id(&generation)
            || verify_generation(&entry.path(), &generation, resource).state
                != InstallState::Installed
        {
            return Err(RuntimeManagerError::new(
                "unmanaged_files_present",
                "resource contains an invalid or unmanaged generation",
            )
            .with_resource(resource));
        }
    }
    Ok(())
}

fn prepare_store_root(root: &Path) -> RuntimeManagerResult<()> {
    if !root.exists() {
        std::fs::create_dir_all(root).map_err(publish_io)?;
    }
    let metadata = std::fs::symlink_metadata(root).map_err(publish_io)?;
    if !metadata.file_type().is_dir() {
        return Err(RuntimeManagerError::new(
            "publish_failed",
            "runtime store root is not a real directory",
        ));
    }
    Ok(())
}

fn ensure_managed_directory(path: &Path) -> RuntimeManagerResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(RuntimeManagerError::new(
            "unmanaged_files_present",
            format!(
                "managed store path is not a real directory: {}",
                path.display()
            ),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(publish_io)
        }
        Err(error) => Err(publish_io(error)),
    }
}

struct MutationLock {
    _file: std::fs::File,
}

impl MutationLock {
    fn acquire(paths: &StorePaths) -> RuntimeManagerResult<Self> {
        let root = paths.store_root.as_ref().ok_or_else(|| {
            RuntimeManagerError::new("publish_failed", "runtime store is not configured")
        })?;
        prepare_store_root(root)?;
        let locks = root.join("locks");
        ensure_managed_directory(&locks)?;
        let path = locks.join("mutation.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(publish_io)?;
        FileExt::try_lock_exclusive(&file).map_err(|_| {
            RuntimeManagerError::new(
                "resource_in_use",
                "another Runtime Manager mutation is in progress",
            )
        })?;
        Ok(Self { _file: file })
    }
}

struct DownloadGuard(PathBuf);

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct StagingGuard(PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_new_file_sync(path: &Path, bytes: &[u8]) -> RuntimeManagerResult<()> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(publish_io)?;
    file.write_all(bytes).map_err(publish_io)?;
    file.sync_all().map_err(publish_io)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> RuntimeManagerResult<()> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(publish_io)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> RuntimeManagerResult<()> {
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> RuntimeManagerResult<()> {
    let mut input = std::fs::File::open(source).map_err(publish_io)?;
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(publish_io)?;
    std::io::copy(&mut input, &mut output).map_err(publish_io)?;
    output.sync_all().map_err(publish_io)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut input = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn unique_operation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("op-{}-{nanos}", std::process::id())
}

fn created_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

fn publish_io(error: std::io::Error) -> RuntimeManagerError {
    RuntimeManagerError::new("publish_failed", error.to_string())
}

#[cfg(test)]
mod tests;
