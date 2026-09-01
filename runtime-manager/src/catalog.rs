use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resource::{ModelId, ResourceKind, ResourceRef};
use crate::runtime_lock::{
    BASIC_PITCH_IR_MANIFEST_SHA256, BASIC_PITCH_SOURCE_SHA256, FCPE_IR_MANIFEST_SHA256,
    FCPE_SOURCE_SHA256, FIRERED_IR_MANIFEST_SHA256, GAME_IR_MANIFEST_SHA256,
    OPENVINO_WORKER_RECIPE_SHA256, RMVPE_CONVERSION_RECIPE_SHA256, RMVPE_IR_MANIFEST_SHA256,
    RMVPE_SOURCE_SHA256, ROFORMER_DENOISE_CONVERSION_RECIPE_SHA256,
    ROFORMER_DENOISE_IR_MANIFEST_SHA256, ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256,
    ROFORMER_DEREVERB_IR_MANIFEST_SHA256, ROFORMER_HARMONY_CONVERSION_RECIPE_SHA256,
    ROFORMER_HARMONY_IR_MANIFEST_SHA256, ROFORMER_INST_V2_CONVERSION_RECIPE_SHA256,
    ROFORMER_INST_V2_IR_MANIFEST_SHA256, ROSVOT_CONVERSION_RECIPE_SHA256,
    ROSVOT_IR_MANIFEST_SHA256, STARS_CONVERSION_RECIPE_SHA256, STARS_IR_MANIFEST_SHA256,
    native_runtime_lock, runtime_recipe_digest,
};
use crate::state::ValidationState;

mod candidates;

pub const RUNTIME_CATALOG_VERSION: &str = "runtime-manager-p0-a-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBackend {
    OpenVino,
    Vulkan,
    NativeDsp,
    CpuReference,
}

impl FromStr for NativeBackend {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        match value {
            "openvino" => Ok(Self::OpenVino),
            "vulkan" | "ggml_vulkan" => Ok(Self::Vulkan),
            "native_dsp" => Ok(Self::NativeDsp),
            "cpu_reference" | "openvino_cpu" => Ok(Self::CpuReference),
            other => Err(RuntimeManagerError::new(
                "invalid_backend",
                format!("unknown native backend: {other}"),
            )),
        }
    }
}

/// Device-class preference, orthogonal to `NativeBackend`. Captured through
/// the process boundary; Runtime Manager does not yet enumerate multiple
/// physical devices, so this does not change device selection on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeDeviceClass {
    Cpu,
    Gpu,
    IntegratedGpu,
}

impl FromStr for NativeDeviceClass {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        match value {
            "cpu" => Ok(Self::Cpu),
            "gpu" => Ok(Self::Gpu),
            "integrated_gpu" => Ok(Self::IntegratedGpu),
            other => Err(RuntimeManagerError::new(
                "invalid_device_class",
                format!("unknown native device class: {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapability {
    pub backend: NativeBackend,
    pub validation: ValidationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMethod {
    Bundled,
    ManagedDownload,
    LocalImport,
    SourceConvert,
    ExternalTool,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquisitionSpec {
    pub method: AcquisitionMethod,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlgorithmIdentity {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub license_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceArtifactIdentity {
    pub filename: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertedArtifactIdentity {
    pub format: String,
    pub manifest_filename: String,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conversion_recipe_sha256: String,
    pub runtime_id: String,
    pub runtime_version: String,
    pub runtime_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<AlgorithmIdentity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<SourceArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub converted_artifact: Option<ConvertedArtifactIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub status: String,
    pub source_attribution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub id: ModelId,
    pub display_name: String,
    pub purpose: String,
    pub capabilities: Vec<String>,
    pub source: SourceIdentity,
    pub license: LicenseInfo,
    pub acquisition: Vec<AcquisitionSpec>,
    pub dependencies: Vec<ResourceRef>,
    pub backends: Vec<BackendCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_backend: Option<NativeBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_download_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_installed_bytes: Option<u64>,
    pub recipe_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

impl ModelCatalogEntry {
    pub fn resource(&self) -> ResourceRef {
        ResourceRef {
            kind: ResourceKind::Model,
            id: self.id.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub purpose: String,
    pub backends: Vec<BackendCapability>,
    pub acquisition: Vec<AcquisitionSpec>,
    pub executable_component_id: String,
    /// Static P0 capability declaration, owned with the shipped worker recipe.
    /// A worker hello/capabilities handshake may replace this in a later phase.
    #[serde(default)]
    pub supported_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_digest: Option<String>,
}

impl RuntimeCatalogEntry {
    pub fn resource(&self) -> RuntimeManagerResult<ResourceRef> {
        ResourceRef::runtime(self.id.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub purpose: String,
    pub acquisition: Vec<AcquisitionSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub purpose: String,
    pub dependencies: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeModelRuntime {
    pub model_id: String,
    pub component_id: String,
    pub backends: Vec<BackendCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_backend: Option<NativeBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCatalog {
    pub schema_version: u32,
    pub catalog_version: String,
    pub models: BTreeMap<String, ModelCatalogEntry>,
    pub runtimes: BTreeMap<String, RuntimeCatalogEntry>,
    pub tools: BTreeMap<String, ToolCatalogEntry>,
    pub bundles: BTreeMap<String, BundleCatalogEntry>,
}

impl ResourceCatalog {
    pub fn default_catalog() -> RuntimeManagerResult<Self> {
        let mut catalog = Self {
            schema_version: 1,
            catalog_version: RUNTIME_CATALOG_VERSION.to_string(),
            models: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            tools: BTreeMap::new(),
            bundles: BTreeMap::new(),
        };
        catalog.add_default_runtimes()?;
        catalog.add_default_models()?;
        catalog.add_openvino_cpu_reference_routes();
        catalog.add_ggml_roformer_routes()?;
        catalog.promote_all_effective_model_routes_to_production();
        catalog.add_default_tools_and_bundles()?;
        Ok(catalog)
    }

    pub fn model(&self, id: &str) -> Option<&ModelCatalogEntry> {
        self.models.get(id)
    }

    pub fn runtime(&self, id: &str) -> Option<&RuntimeCatalogEntry> {
        self.runtimes.get(id)
    }

    pub fn contains(&self, resource: &ResourceRef) -> bool {
        match resource.kind {
            ResourceKind::Model => self.models.contains_key(&resource.id),
            ResourceKind::Runtime => self.runtimes.contains_key(&resource.id),
            ResourceKind::Tool => self.tools.contains_key(&resource.id),
            ResourceKind::Bundle => self.bundles.contains_key(&resource.id),
        }
    }

    pub fn resource_refs(&self) -> Vec<ResourceRef> {
        self.models
            .keys()
            .map(|id| ResourceRef::model(id.clone()).expect("catalog ids are valid"))
            .chain(
                self.runtimes
                    .keys()
                    .map(|id| ResourceRef::runtime(id.clone()).expect("catalog ids are valid")),
            )
            .chain(
                self.tools
                    .keys()
                    .map(|id| ResourceRef::tool(id.clone()).expect("catalog ids are valid")),
            )
            .chain(
                self.bundles
                    .keys()
                    .map(|id| ResourceRef::bundle(id.clone()).expect("catalog ids are valid")),
            )
            .collect()
    }

    pub fn native_runtime_registry(&self) -> Vec<NativeModelRuntime> {
        self.models
            .values()
            .filter_map(|model| {
                let runtime = model
                    .dependencies
                    .iter()
                    .find(|dependency| dependency.kind == ResourceKind::Runtime)?;
                Some(NativeModelRuntime {
                    model_id: model.id.as_str().to_string(),
                    component_id: runtime.id.clone(),
                    backends: model.backends.clone(),
                    pinned_backend: model.pinned_backend,
                    runtime_recipe_digest: model.runtime_recipe_digest.clone(),
                })
            })
            .collect()
    }

    fn add_openvino_cpu_reference_routes(&mut self) {
        for model in self.models.values_mut().filter(|model| {
            model
                .backends
                .iter()
                .any(|capability| capability.backend == NativeBackend::OpenVino)
        }) {
            if !model
                .backends
                .iter()
                .any(|capability| capability.backend == NativeBackend::CpuReference)
            {
                model.backends.push(BackendCapability {
                    backend: NativeBackend::CpuReference,
                    validation: ValidationState::Experimental,
                    evidence_id: Some("validation:openvino-ir-explicit-cpu-reference".to_string()),
                });
            }
        }
    }

    fn add_ggml_roformer_routes(&mut self) -> RuntimeManagerResult<()> {
        const ROFORMER_MODELS: [&str; 4] = [
            "melband_roformer_inst_v2",
            "melband_roformer_harmony",
            "melband_roformer_denoise_aufr33",
            "melband_roformer_dereverb_anvuew",
        ];
        for model_id in ROFORMER_MODELS {
            let model = self
                .models
                .get_mut(model_id)
                .ok_or_else(|| RuntimeManagerError::invalid_catalog("missing RoFormer model"))?;
            model.backends.clear();
            model.backends.push(BackendCapability {
                backend: NativeBackend::Vulkan,
                validation: ValidationState::BenchmarkCandidate,
                evidence_id: Some(
                    "validation:ggml-roformer-fullsong-serial-2026-08-24".to_string(),
                ),
            });
            model.pinned_backend = Some(NativeBackend::Vulkan);
            model.dependencies.clear();
            model
                .dependencies
                .push(ResourceRef::runtime("ggml_vulkan_v1")?);
            model.runtime_recipe_digest = Some(
                "4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a".to_string(),
            );
            let (artifact, installed_bytes) = ggml_roformer_artifact(model_id);
            model.source.converted_artifact = Some(artifact);
            model.estimated_installed_bytes = Some(installed_bytes);
            model.acquisition = vec![acquisition(
                AcquisitionMethod::LocalImport,
                "explicit import of the exact catalog-pinned GGUF",
            )];
        }
        Ok(())
    }

    /// Applies the repository owner's explicit release policy: every model's
    /// effective native route is admitted under Production policy. CPU reference
    /// routes remain diagnostic-only and therefore stay Experimental.
    ///
    /// This changes policy admission only. Existing evidence identifiers,
    /// provenance, license metadata, structural validation, and fail-closed
    /// runtime checks remain intact and continue to be surfaced to the UI.
    fn promote_all_effective_model_routes_to_production(&mut self) {
        for runtime in self.runtimes.values_mut() {
            for capability in &mut runtime.backends {
                if capability.backend != NativeBackend::CpuReference {
                    capability.validation = ValidationState::ProductionPinned;
                }
            }
        }
        for model in self.models.values_mut() {
            for capability in &mut model.backends {
                if capability.backend != NativeBackend::CpuReference {
                    capability.validation = ValidationState::ProductionPinned;
                }
            }
        }
    }

    fn add_default_runtimes(&mut self) -> RuntimeManagerResult<()> {
        use NativeBackend::*;
        use ValidationState::*;
        self.insert_runtime(RuntimeCatalogEntry {
            id: "openvino_2026_3".to_string(),
            display_name: "OpenVINO 2026.3 Worker".to_string(),
            purpose: "Pinned OpenVINO CPU/GPU inference worker".to_string(),
            backends: vec![BackendCapability {
                backend: OpenVino,
                validation: ProductionPinned,
                evidence_id: Some("validation:openvino-worker-pinned".to_string()),
            }],
            acquisition: vec![acquisition(
                AcquisitionMethod::Bundled,
                "packaged native worker",
            )],
            executable_component_id: "openvino_2026_3".to_string(),
            supported_models: [
                "firered_asr2_aed",
                "rmvpe",
                "fcpe",
                "game",
                "basic_pitch",
                "stars",
                "rosvot",
                "bs_polarformer_public_instrumental",
                "jbm555_cectc_80",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })?;
        self.insert_runtime(RuntimeCatalogEntry {
            id: "ggml_vulkan_v1".to_string(),
            display_name: "GGML Vulkan Worker".to_string(),
            purpose: "Manifest-pinned Production GGUF Vulkan inference for RoFormer models"
                .to_string(),
            backends: vec![BackendCapability {
                backend: Vulkan,
                validation: BenchmarkCandidate,
                evidence_id: Some(
                    "validation:ggml-roformer-fullsong-serial-2026-08-24".to_string(),
                ),
            }],
            acquisition: vec![acquisition(
                AcquisitionMethod::Bundled,
                "packaged native worker and pinned local runtime",
            )],
            executable_component_id: "ggml_vulkan_v1".to_string(),
            supported_models: [
                "bs_roformer_leap_xe90_vocals",
                "melband_roformer_inst_v2",
                "melband_roformer_harmony",
                "melband_roformer_denoise_aufr33",
                "melband_roformer_dereverb_anvuew",
                "bs_polarformer_public_instrumental",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            recipe_digest: Some(
                "4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a".to_string(),
            ),
        })?;
        self.insert_runtime(RuntimeCatalogEntry {
            id: "qwen_asr_runtime".to_string(),
            display_name: "Qwen ASR Vulkan Runtime".to_string(),
            purpose: "Pinned Vulkan GGML runtime for Qwen3 ASR".to_string(),
            backends: vec![BackendCapability {
                backend: Vulkan,
                validation: BenchmarkCandidate,
                evidence_id: Some("validation:qwen-runtime-validation".to_string()),
            }],
            acquisition: vec![acquisition(
                AcquisitionMethod::Bundled,
                "packaged native worker",
            )],
            executable_component_id: "qwen_asr_runtime".to_string(),
            supported_models: vec!["qwen3_asr_1_7b".to_string()],
            recipe_digest: Some(
                runtime_recipe_digest("qwen3_asr_1_7b")
                    .map_err(RuntimeManagerError::invalid_catalog)?,
            ),
        })?;
        self.insert_runtime(RuntimeCatalogEntry {
            id: "qwen_align_runtime".to_string(),
            display_name: "Qwen Forced Aligner Vulkan Runtime".to_string(),
            purpose: "Pinned Vulkan GGML runtime for Qwen3 forced alignment".to_string(),
            backends: vec![BackendCapability {
                backend: Vulkan,
                validation: BenchmarkCandidate,
                evidence_id: Some(
                    "validation:qwen-runtime-validation#aligner-static-closure".to_string(),
                ),
            }],
            acquisition: vec![acquisition(
                AcquisitionMethod::Bundled,
                "packaged native worker",
            )],
            executable_component_id: "qwen_align_runtime".to_string(),
            supported_models: vec!["qwen3_forced_aligner_0_6b".to_string()],
            recipe_digest: Some(
                runtime_recipe_digest("qwen3_forced_aligner_0_6b")
                    .map_err(RuntimeManagerError::invalid_catalog)?,
            ),
        })?;
        Ok(())
    }

    fn add_default_models(&mut self) -> RuntimeManagerResult<()> {
        use NativeBackend::*;
        use ValidationState::*;
        for model in candidates::task_23_models()? {
            self.insert_model(model)?;
        }
        let roformer = [
            (
                "melband_roformer_inst_v2",
                "MelBand-RoFormer Inst V2",
                "audio.extract_instrumental",
            ),
            (
                "melband_roformer_harmony",
                "MelBand-RoFormer Lead Isolation",
                "audio.lead_isolate",
            ),
            (
                "melband_roformer_denoise_aufr33",
                "MelBand-RoFormer Denoise",
                "audio.denoise",
            ),
            (
                "melband_roformer_dereverb_anvuew",
                "MelBand-RoFormer Dereverb",
                "audio.dereverb",
            ),
        ];
        for (id, display_name, capability) in roformer {
            let (mut source, license, estimated_download_bytes) = roformer_source(id);
            if id == "melband_roformer_inst_v2" {
                source.converted_artifact = Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_explicit_cpu_gpu_islands".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: ROFORMER_INST_V2_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: ROFORMER_INST_V2_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                });
                self.insert_model(ModelCatalogEntry {
                    id: ModelId::new(id)?,
                    display_name: display_name.to_string(),
                    purpose: "Exact-context 44.1 kHz stereo instrumental extraction".to_string(),
                    capabilities: vec![capability.to_string()],
                    source,
                    license,
                    acquisition: vec![acquisition(
                        AcquisitionMethod::LocalImport,
                        "explicit import of the accepted 33-island Inst V2 OpenVINO generation",
                    )],
                    dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
                    backends: vec![BackendCapability {
                        backend: OpenVino,
                        validation: BenchmarkCandidate,
                        evidence_id: Some(
                            "validation:inst-v2-exact-context-split-openvino".to_string(),
                        ),
                    }],
                    pinned_backend: Some(OpenVino),
                    estimated_download_bytes,
                    estimated_installed_bytes: Some(1_583_142_000),
                    recipe_digest: catalog_recipe_digest(id),
                    runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
                })?;
                continue;
            }
            if id == "melband_roformer_harmony" {
                source.converted_artifact = Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_explicit_cpu_gpu_islands_dual_residual".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: ROFORMER_HARMONY_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: ROFORMER_HARMONY_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                });
                self.insert_model(ModelCatalogEntry {
                    id: ModelId::new(id)?,
                    display_name: display_name.to_string(),
                    purpose: "Exact 44.1 kHz all-vocals lead isolation yielding LeadVocal and VocalResidual".to_string(),
                    capabilities: vec![capability.to_string()],
                    source,
                    license,
                    acquisition: vec![acquisition(
                        AcquisitionMethod::LocalImport,
                        "explicit import of the accepted Karaoke OpenVINO neural island and dual-output residual contract",
                    )],
                    dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
                    backends: vec![BackendCapability {
                        backend: OpenVino,
                        validation: BenchmarkCandidate,
                        evidence_id: Some(
                            "validation:harmony-karaoke-dual-residual-openvino".to_string(),
                        ),
                    }],
                    pinned_backend: Some(OpenVino),
                    estimated_download_bytes,
                    estimated_installed_bytes: Some(914_688_155),
                    recipe_digest: catalog_recipe_digest(id),
                    runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
                })?;
                continue;
            }
            if id == "melband_roformer_denoise_aufr33" {
                source.converted_artifact = Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_melband_neural_island".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: ROFORMER_DENOISE_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: ROFORMER_DENOISE_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                });
                self.insert_model(ModelCatalogEntry {
                    id: ModelId::new(id)?,
                    display_name: display_name.to_string(),
                    purpose: "Exact 44.1 kHz stereo dry-stem cleanup".to_string(),
                    capabilities: vec![capability.to_string()],
                    source,
                    license,
                    acquisition: vec![acquisition(
                        AcquisitionMethod::LocalImport,
                        "explicit import of the accepted R03 OpenVINO neural island and exact Denoise config",
                    )],
                    dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
                    backends: vec![BackendCapability {
                        backend: OpenVino,
                        validation: BenchmarkCandidate,
                        evidence_id: Some(
                            "validation:r03b-roformer-denoise-native-integration".to_string(),
                        ),
                    }],
                    pinned_backend: Some(OpenVino),
                    estimated_download_bytes,
                    estimated_installed_bytes: Some(914_692_150),
                    recipe_digest: catalog_recipe_digest(id),
                    runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
                })?;
                continue;
            }
            if id == "melband_roformer_dereverb_anvuew" {
                source.converted_artifact = Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_melband_neural_island".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: ROFORMER_DEREVERB_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256
                        .to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                });
                self.insert_model(ModelCatalogEntry {
                    id: ModelId::new(id)?,
                    display_name: display_name.to_string(),
                    purpose: "Exact 44.1 kHz stereo noreverb-stem cleanup (checkpoint may remove additional content)".to_string(),
                    capabilities: vec![capability.to_string()],
                    source,
                    license,
                    acquisition: vec![acquisition(
                        AcquisitionMethod::LocalImport,
                        "explicit import of the accepted R04 OpenVINO neural island and exact Dereverb config",
                    )],
                    dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
                    backends: vec![BackendCapability {
                        backend: OpenVino,
                        validation: BenchmarkCandidate,
                        evidence_id: Some(
                            "validation:roformer-dereverb-native-integration".to_string(),
                        ),
                    }],
                    pinned_backend: Some(OpenVino),
                    estimated_download_bytes,
                    estimated_installed_bytes: Some(914_694_000),
                    recipe_digest: catalog_recipe_digest(id),
                    runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
                })?;
                continue;
            }
        }

        self.insert_optional_openvino_expert(
            "firered_asr2_aed",
            "FireRed ASR2 AED",
            "Optional deterministic windowed transcript challenger over fixed IR buckets",
            "speech.transcribe.challenger",
            SourceIdentity {
                repository: Some(
                    "https://huggingface.co/42ailab/FireRedASR2-AED-ONNX".to_string(),
                ),
                revision: Some(
                    "13f950858934f7b6a0d3ce52bae65af0dc022258".to_string(),
                ),
                filename: None,
                sha256: None,
                source_format: Some("split_int8_onnx".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/FireRedTeam/FireRedASR2S".to_string(),
                    revision: Some(
                        "4e7d9aaf4482a47cec1724807026b9b151926eb5".to_string(),
                    ),
                    license_id: "Apache-2.0".to_string(),
                }),
                artifacts: vec![
                    source_artifact(
                        "encoder.int8.onnx",
                        "0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9",
                    ),
                    source_artifact(
                        "decoder.int8.onnx",
                        "aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833",
                    ),
                    source_artifact(
                        "ctc.int8.onnx",
                        "8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4",
                    ),
                    source_artifact(
                        "cmvn.ark",
                        "6efba6105429d1630c05d818d956bfe4edfad37a04b3b27bb5a029b9adb37945",
                    ),
                    source_artifact(
                        "tokens.txt",
                        "1bc613de2112d257e61a349c3e72d1b1a9cf19c33d3ca954197ad2171e5ea07b",
                    ),
                ],
                converted_artifact: Some(openvino_artifact(
                    "openvino_ir_v11_smoke_buckets",
                    FIRERED_IR_MANIFEST_SHA256,
                )),
            },
            LicenseInfo {
                status: "apache-2.0".to_string(),
                source_attribution: "FireRedTeam/FireRedASR2S canonical project; selected executable graphs are the community 42ailab/ManySpeech ONNX conversion, not an official FireRedTeam binary".to_string(),
                source_page: Some("https://huggingface.co/42ailab/FireRedASR2-AED-ONNX/tree/13f950858934f7b6a0d3ce52bae65af0dc022258".to_string()),
            },
            ValidationState::ProductionPinned,
            "validation:firered-openvino-worker-windowed-v1",
        )?;
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new("rmvpe")?,
            display_name: "RMVPE".to_string(),
            purpose: "Primary continuous F0 tracking".to_string(),
            capabilities: vec!["pitch.track".to_string()],
            source: SourceIdentity {
                repository: Some("https://huggingface.co/lj1995/VoiceConversionWebUI".to_string()),
                revision: Some("e6d0c1a17da07c33557852f9dfa2bd44cc75737d".to_string()),
                filename: Some("rmvpe.onnx".to_string()),
                sha256: Some(RMVPE_SOURCE_SHA256.to_string()),
                source_format: Some("onnx".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/Dream-High/RMVPE".to_string(),
                    revision: None,
                    license_id: "Apache-2.0".to_string(),
                }),
                artifacts: Vec::new(),
                converted_artifact: Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_bucketed".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: RMVPE_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: RMVPE_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                }),
            },
            license: LicenseInfo {
                status: "mit".to_string(),
                source_attribution:
                    "lj1995/VoiceConversionWebUI rmvpe.onnx distribution; Dream-High/RMVPE algorithm lineage is separately Apache-2.0"
                        .to_string(),
                source_page: Some(
                    "https://huggingface.co/lj1995/VoiceConversionWebUI/blob/e6d0c1a17da07c33557852f9dfa2bd44cc75737d/rmvpe.onnx"
                        .to_string(),
                ),
            },
            acquisition: vec![acquisition(
                AcquisitionMethod::LocalImport,
                "explicit import of pinned bucketed RMVPE OpenVINO IR converted from the exact ONNX source",
            )],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: vec![BackendCapability {
                backend: OpenVino,
                validation: ProductionPinned,
                evidence_id: Some("validation:rmvpe-openvino-worker".to_string()),
            }],
            pinned_backend: Some(OpenVino),
            estimated_download_bytes: None,
            estimated_installed_bytes: Some(396_644_647),
            recipe_digest: catalog_recipe_digest("rmvpe"),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })?;
        self.insert_optional_openvino_expert(
            "fcpe",
            "FCPE",
            "Optional deterministic windowed secondary continuous-F0 disagreement expert",
            "pitch.secondary",
            SourceIdentity {
                repository: Some("https://huggingface.co/gzivdo/fcpe-onnx".to_string()),
                revision: Some(
                    "5800a2b1944967f55bb0bfeb9718cb749f809310".to_string(),
                ),
                filename: Some("fcpe.onnx".to_string()),
                sha256: Some(FCPE_SOURCE_SHA256.to_string()),
                source_format: Some("onnx".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/CNChTu/FCPE".to_string(),
                    revision: Some(
                        "6a149c1afb1c7e7821b71869dfb31ad50c95b516".to_string(),
                    ),
                    license_id: "MIT".to_string(),
                }),
                artifacts: vec![source_artifact("fcpe.onnx", FCPE_SOURCE_SHA256)],
                converted_artifact: Some(openvino_artifact(
                    "openvino_ir_v11",
                    FCPE_IR_MANIFEST_SHA256,
                )),
            },
            LicenseInfo {
                status: "mit".to_string(),
                source_attribution: "CNChTu/FCPE canonical project; selected fcpe.onnx is the explicitly unofficial gzivdo community export".to_string(),
                source_page: Some("https://huggingface.co/gzivdo/fcpe-onnx/tree/5800a2b1944967f55bb0bfeb9718cb749f809310".to_string()),
            },
            ValidationState::BenchmarkCandidate,
            "validation:fcpe-windowed-schema3-secondary-f0",
        )?;
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new("game")?,
            display_name: "GAME".to_string(),
            purpose: "Primary singing note and boundary expert".to_string(),
            capabilities: vec!["notes.game".to_string()],
            source: SourceIdentity {
                repository: Some("https://github.com/openvpi/GAME.git".to_string()),
                revision: Some("475a8ee781fe8cca980b3b12fbe6c80c768a813a".to_string()),
                filename: Some("manifest.json".to_string()),
                sha256: Some(GAME_IR_MANIFEST_SHA256.to_string()),
                source_format: Some("openvino_ir_v11_static_chunked_estimator_buckets".to_string()),
                ..SourceIdentity::default()
            },
            license: LicenseInfo {
                status: "cc-by-nc-sa-4.0-explicit-acceptance".to_string(),
                source_attribution: "openvpi GAME 1.0.3 medium model".to_string(),
                source_page: Some(
                    "https://github.com/openvpi/GAME/releases/tag/v1.0.3".to_string(),
                ),
            },
            acquisition: vec![AcquisitionSpec {
                method: AcquisitionMethod::LocalImport,
                label:
                    "pinned GAME OpenVINO IR directory produced by the audited conversion recipe"
                        .to_string(),
                license_id: Some("cc-by-nc-sa-4.0".to_string()),
            }],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: vec![BackendCapability {
                backend: OpenVino,
                validation: ProductionPinned,
                evidence_id: Some(
                    "validation:game-stitching-repaired-fullsong-2026-08-24".to_string(),
                ),
            }],
            pinned_backend: Some(OpenVino),
            estimated_download_bytes: None,
            estimated_installed_bytes: Some(209_892_667),
            recipe_digest: catalog_recipe_digest("game"),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })?;
        self.insert_optional_openvino_expert(
            "basic_pitch",
            "Basic Pitch",
            "Optional reference-overlap windowed onset/contour activation challenger",
            "notes.basic_pitch",
            SourceIdentity {
                repository: Some(
                    "https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models".to_string(),
                ),
                revision: Some(
                    "327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763".to_string(),
                ),
                filename: Some("nmp.onnx".to_string()),
                sha256: Some(BASIC_PITCH_SOURCE_SHA256.to_string()),
                source_format: Some("onnx".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/spotify/basic-pitch".to_string(),
                    revision: Some(
                        "fa5997af0a8210982619003269994a1be25eddf3".to_string(),
                    ),
                    license_id: "Apache-2.0".to_string(),
                }),
                artifacts: vec![source_artifact("nmp.onnx", BASIC_PITCH_SOURCE_SHA256)],
                converted_artifact: Some(openvino_artifact(
                    "openvino_ir_v11",
                    BASIC_PITCH_IR_MANIFEST_SHA256,
                )),
            },
            LicenseInfo {
                status: "apache-2.0".to_string(),
                source_attribution: "Spotify Basic Pitch canonical project; selected nmp.onnx is the AEmotionStudio mirror of Spotify ONNX bytes".to_string(),
                source_page: Some("https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models/tree/327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763".to_string()),
            },
            ValidationState::BenchmarkCandidate,
            "validation:basic-pitch-reference-overlap-schema3",
        )?;
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new("stars")?,
            display_name: "STARS Chinese P1".to_string(),
            purpose: "Optional lyric-conditioned note, technique, and style evidence".to_string(),
            capabilities: vec!["notes.stars".to_string(), "technique.analyze".to_string()],
            source: SourceIdentity {
                repository: Some("https://huggingface.co/verstar/STARS".to_string()),
                revision: Some("744a7ad02e1d788452293cd903ea6a933f7862c4".to_string()),
                filename: Some("model_ckpt_steps_200000.ckpt".to_string()),
                sha256: Some("9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c".to_string()),
                source_format: Some("pytorch_checkpoint".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/gwx314/STARS".to_string(),
                    revision: Some("f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167".to_string()),
                    license_id: "MIT".to_string(),
                }),
                artifacts: vec![
                    source_artifact(
                        "model_ckpt_steps_200000.ckpt",
                        "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c",
                    ),
                    source_artifact(
                        "stars_chinese.yaml",
                        "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91",
                    ),
                ],
                converted_artifact: Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_conditioned_segmented_p1".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: STARS_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: STARS_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                }),
            },
            license: LicenseInfo {
                status: "checkpoint-license-unresolved".to_string(),
                source_attribution: "gwx314/STARS MIT source; verstar/STARS Chinese checkpoint rights are tracked separately".to_string(),
                source_page: Some("https://huggingface.co/verstar/STARS/tree/744a7ad02e1d788452293cd903ea6a933f7862c4".to_string()),
            },
            acquisition: vec![acquisition(
                AcquisitionMethod::LocalImport,
                "explicit import of the pinned conditioned STARS P1 OpenVINO generation",
            )],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: vec![BackendCapability {
                backend: OpenVino,
                validation: BenchmarkCandidate,
                evidence_id: Some("validation:stars-p0-split-gpu-parity".to_string()),
            }],
            pinned_backend: Some(OpenVino),
            estimated_download_bytes: None,
            estimated_installed_bytes: Some(528_000_000),
            recipe_digest: catalog_recipe_digest("stars"),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })?;
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new("rosvot")?,
            display_name: "ROSVOT P0".to_string(),
            purpose: "Optional TimedTranscript-conditioned singing note evidence".to_string(),
            capabilities: vec!["notes.rosvot".to_string()],
            source: SourceIdentity {
                repository: Some("https://github.com/RickyL-2000/ROSVOT".to_string()),
                revision: Some("3c8332bf43adae35f6e4d64971862f2f6139b310".to_string()),
                filename: Some("rosvot".to_string()),
                sha256: Some("7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb".to_string()),
                source_format: Some("pytorch_checkpoint".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/RickyL-2000/ROSVOT".to_string(),
                    revision: Some("3c8332bf43adae35f6e4d64971862f2f6139b310".to_string()),
                    license_id: "MIT".to_string(),
                }),
                artifacts: vec![
                    source_artifact(
                        "rosvot",
                        "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb",
                    ),
                    source_artifact(
                        "config.yaml",
                        "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2",
                    ),
                    source_artifact(
                        "source-manifest.json",
                        "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8",
                    ),
                ],
                converted_artifact: Some(ConvertedArtifactIdentity {
                    format: "openvino_ir_v11_conditioned_segmented".to_string(),
                    manifest_filename: "manifest.json".to_string(),
                    manifest_sha256: ROSVOT_IR_MANIFEST_SHA256.to_string(),
                    conversion_recipe_sha256: ROSVOT_CONVERSION_RECIPE_SHA256.to_string(),
                    runtime_id: "openvino_2026_3".to_string(),
                    runtime_version: "2026.3.0".to_string(),
                    runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
                }),
            },
            license: LicenseInfo {
                status: "checkpoint-license-unresolved".to_string(),
                source_attribution: "RickyL-2000/ROSVOT MIT source; selected checkpoint rights are tracked separately".to_string(),
                source_page: Some("https://github.com/RickyL-2000/ROSVOT/tree/3c8332bf43adae35f6e4d64971862f2f6139b310".to_string()),
            },
            acquisition: vec![acquisition(
                AcquisitionMethod::LocalImport,
                "explicit import of the pinned TimedTranscript-conditioned ROSVOT OpenVINO generation",
            )],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: vec![BackendCapability {
                backend: OpenVino,
                validation: BenchmarkCandidate,
                evidence_id: Some("validation:rosvot-p0-split-gpu-parity".to_string()),
            }],
            pinned_backend: Some(OpenVino),
            estimated_download_bytes: None,
            estimated_installed_bytes: Some(410_000_000),
            recipe_digest: catalog_recipe_digest("rosvot"),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })?;

        let runtime_lock = native_runtime_lock().map_err(RuntimeManagerError::invalid_catalog)?;
        let asr = &runtime_lock.components.qwen3_asr_1_7b;
        self.insert_qwen_model(
            "qwen3_asr_1_7b",
            "Qwen3-ASR-1.7B",
            "Baseline transcript expert",
            "speech.transcribe",
            "qwen_asr_runtime",
            SourceIdentity {
                repository: Some(asr.gguf_repository.clone()),
                revision: Some(asr.gguf_repository_revision.clone()),
                filename: Some(asr.gguf_file.clone()),
                sha256: Some(asr.gguf_sha256.clone()),
                source_format: Some("gguf".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: asr.source_model_repository.clone(),
                    revision: Some(asr.source_model_revision.clone()),
                    license_id: "apache-2.0".to_string(),
                }),
                ..SourceIdentity::default()
            },
            AcquisitionMethod::ManagedDownload,
            runtime_recipe_digest("qwen3_asr_1_7b")
                .map_err(RuntimeManagerError::invalid_catalog)?,
        )?;
        let align = &runtime_lock.components.qwen3_forced_aligner_0_6b;
        self.insert_qwen_model(
            "qwen3_forced_aligner_0_6b",
            "Qwen3 Forced Aligner 0.6B",
            "Baseline forced alignment expert",
            "speech.align",
            "qwen_align_runtime",
            SourceIdentity {
                repository: Some(align.model_repository.clone()),
                revision: Some(align.model_revision.clone()),
                filename: Some(align.source_file.clone()),
                sha256: Some(align.source_sha256.clone()),
                source_format: Some(align.source_format.clone()),
                algorithm: Some(AlgorithmIdentity {
                    repository: align.model_repository.clone(),
                    revision: Some(align.model_revision.clone()),
                    license_id: align.source_license.clone(),
                }),
                artifacts: vec![source_artifact(&align.source_file, &align.source_sha256)],
                converted_artifact: Some(ConvertedArtifactIdentity {
                    format: align.gguf_format.clone(),
                    manifest_filename: align.gguf_file.clone(),
                    manifest_sha256: align.gguf_sha256.clone(),
                    conversion_recipe_sha256: align.conversion_recipe_digest.clone(),
                    runtime_id: "qwen_align_runtime".to_string(),
                    runtime_version: align.runtime_commit.clone(),
                    runtime_commit: align.runtime_commit.clone(),
                }),
            },
            AcquisitionMethod::LocalImport,
            runtime_recipe_digest("qwen3_forced_aligner_0_6b")
                .map_err(RuntimeManagerError::invalid_catalog)?,
        )?;
        Ok(())
    }

    fn add_default_tools_and_bundles(&mut self) -> RuntimeManagerResult<()> {
        self.tools.insert(
            "ffmpeg".to_string(),
            ToolCatalogEntry {
                id: "ffmpeg".to_string(),
                display_name: "FFmpeg".to_string(),
                purpose: "Audio decode/encode utility where explicitly supported".to_string(),
                acquisition: vec![
                    acquisition(AcquisitionMethod::Bundled, "packaged ffmpeg"),
                    acquisition(
                        AcquisitionMethod::ExternalTool,
                        "explicit system ffmpeg path",
                    ),
                ],
            },
        );
        self.tools.insert(
            crate::external_tool::FUSION_AGENT_ADAPTER_ID.to_string(),
            ToolCatalogEntry {
                id: crate::external_tool::FUSION_AGENT_ADAPTER_ID.to_string(),
                display_name: "Fusion Agent Adapter".to_string(),
                purpose: "Verified external adapter for bounded AI candidate-path selection"
                    .to_string(),
                acquisition: vec![acquisition(
                    AcquisitionMethod::ExternalTool,
                    "explicit verified Uta Fusion Agent Adapter executable",
                )],
            },
        );
        self.bundles.insert(
            "roformer".to_string(),
            BundleCatalogEntry {
                id: "roformer".to_string(),
                display_name: "RoFormer family".to_string(),
                purpose: "Convenience dependency set; not an inference identity".to_string(),
                dependencies: [
                    "bs_roformer_leap_xe90_vocals",
                    "melband_roformer_inst_v2",
                    "melband_roformer_harmony",
                    "melband_roformer_denoise_aufr33",
                    "melband_roformer_dereverb_anvuew",
                ]
                .into_iter()
                .map(ResourceRef::model)
                .collect::<Result<Vec<_>, _>>()?,
            },
        );
        self.bundles.insert(
            "engine-fast".to_string(),
            BundleCatalogEntry {
                id: "engine-fast".to_string(),
                display_name: "Engine Fast baseline resources".to_string(),
                purpose: "Convenience set for the first Analysis Engine path".to_string(),
                dependencies: [
                    "bs_roformer_leap_xe90_vocals",
                    "melband_roformer_harmony",
                    "qwen3_asr_1_7b",
                    "qwen3_forced_aligner_0_6b",
                    "rmvpe",
                    "game",
                ]
                .into_iter()
                .map(ResourceRef::model)
                .collect::<Result<Vec<_>, _>>()?,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_optional_openvino_expert(
        &mut self,
        id: &str,
        display_name: &str,
        purpose: &str,
        capability: &str,
        source: SourceIdentity,
        license: LicenseInfo,
        validation: ValidationState,
        evidence_id: &str,
    ) -> RuntimeManagerResult<()> {
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new(id)?,
            display_name: display_name.to_string(),
            purpose: purpose.to_string(),
            capabilities: vec![capability.to_string()],
            source,
            license,
            acquisition: vec![acquisition(
                AcquisitionMethod::LocalImport,
                "explicit import of the exact verified fixed-window OpenVINO IR directory",
            )],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: vec![BackendCapability {
                backend: NativeBackend::OpenVino,
                validation,
                evidence_id: Some(evidence_id.to_string()),
            }],
            pinned_backend: Some(NativeBackend::OpenVino),
            estimated_download_bytes: None,
            estimated_installed_bytes: None,
            recipe_digest: catalog_recipe_digest(id),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_qwen_model(
        &mut self,
        id: &str,
        display_name: &str,
        purpose: &str,
        capability: &str,
        runtime_id: &str,
        source: SourceIdentity,
        acquisition_method: AcquisitionMethod,
        recipe_digest: String,
    ) -> RuntimeManagerResult<()> {
        self.insert_model(ModelCatalogEntry {
            id: ModelId::new(id)?,
            display_name: display_name.to_string(),
            purpose: purpose.to_string(),
            capabilities: vec![capability.to_string()],
            license: qwen_model_license(&source),
            source,
            acquisition: vec![acquisition(
                acquisition_method,
                match acquisition_method {
                    AcquisitionMethod::ManagedDownload => "pinned Qwen recipe",
                    AcquisitionMethod::LocalImport => "pinned local import recipe",
                    _ => "acquisition recipe has not been audited",
                },
            )],
            dependencies: vec![ResourceRef::runtime(runtime_id)?],
            backends: vec![BackendCapability {
                backend: NativeBackend::Vulkan,
                validation: ValidationState::BenchmarkCandidate,
                evidence_id: Some(if id == "qwen3_forced_aligner_0_6b" {
                    "validation:qwen-runtime-validation#aligner-static-closure".to_string()
                } else {
                    "validation:qwen-runtime-validation".to_string()
                }),
            }],
            pinned_backend: Some(NativeBackend::Vulkan),
            estimated_download_bytes: (id == "qwen3_asr_1_7b").then_some(1_319_830_496),
            estimated_installed_bytes: (id == "qwen3_asr_1_7b").then_some(1_319_830_496),
            recipe_digest: recipe_digest.clone(),
            runtime_recipe_digest: Some(recipe_digest),
        })
    }

    fn insert_model(&mut self, entry: ModelCatalogEntry) -> RuntimeManagerResult<()> {
        let id = entry.id.as_str().to_string();
        if self.models.insert(id.clone(), entry).is_some() {
            return Err(RuntimeManagerError::invalid_catalog(format!(
                "duplicate model id: {id}"
            )));
        }
        Ok(())
    }

    fn insert_runtime(&mut self, entry: RuntimeCatalogEntry) -> RuntimeManagerResult<()> {
        if self.runtimes.insert(entry.id.clone(), entry).is_some() {
            return Err(RuntimeManagerError::invalid_catalog("duplicate runtime id"));
        }
        Ok(())
    }
}

fn catalog_recipe_digest(resource_id: &str) -> String {
    let recipe_version = match resource_id {
        "rmvpe" => "runtime-manager-rmvpe-identity-v2",
        "melband_roformer_denoise_aufr33" => {
            "runtime-manager-roformer-denoise-openvino-identity-v1"
        }
        "melband_roformer_dereverb_anvuew" => {
            "runtime-manager-roformer-dereverb-openvino-identity-v1"
        }
        "qwen3_forced_aligner_0_6b" => "runtime-manager-qwen-aligner-identity-v2",
        "firered_asr2_aed" | "fcpe" | "basic_pitch" => {
            "runtime-manager-optional-openvino-identity-v2"
        }
        _ => RUNTIME_CATALOG_VERSION,
    };
    format!(
        "{:x}",
        Sha256::digest(format!("{recipe_version}:{resource_id}").as_bytes())
    )
}

fn source_artifact(filename: &str, sha256: &str) -> SourceArtifactIdentity {
    SourceArtifactIdentity {
        filename: filename.to_string(),
        sha256: sha256.to_string(),
    }
}

fn openvino_artifact(format: &str, manifest_sha256: &str) -> ConvertedArtifactIdentity {
    ConvertedArtifactIdentity {
        format: format.to_string(),
        manifest_filename: "manifest.json".to_string(),
        manifest_sha256: manifest_sha256.to_string(),
        // The historical fixed-window manifests did not record a reproducible
        // conversion recipe. Keep that absence distinct from artifact identity.
        conversion_recipe_sha256: String::new(),
        runtime_id: "openvino_2026_3".to_string(),
        runtime_version: "2026.3.0".to_string(),
        runtime_commit: "8a17657b995fd3b4a52f8484acfcf2bb61214623".to_string(),
    }
}

fn ggml_roformer_artifact(model_id: &str) -> (ConvertedArtifactIdentity, u64) {
    let (sha256, size_bytes) = match model_id {
        "melband_roformer_denoise_aufr33" => (
            "eb03fce4c5a450f88718e8a529b8adcd653618a5d32cb55275fa212a80fef33a",
            457_008_736,
        ),
        "melband_roformer_dereverb_anvuew" => (
            "f850fb2460099df356676ce37ba48875e3c75726d7a848b42d75ff6015955ac7",
            457_008_736,
        ),
        "melband_roformer_inst_v2" => (
            "e2b39b979e2413af172bad88a6b0a324a54d47fbca6622083f7f3817b9046897",
            787_918_656,
        ),
        "melband_roformer_harmony" => (
            "d463c06a1bf5d3889a2a6be58cc469f0a996155eafb91845ff5e8c139a3d64be",
            457_008_736,
        ),
        _ => unreachable!("shipped GGML RoFormer id"),
    };
    (
        ConvertedArtifactIdentity {
            format: "gguf_f16".to_string(),
            manifest_filename: "model-fp16.gguf".to_string(),
            manifest_sha256: sha256.to_string(),
            conversion_recipe_sha256: String::new(),
            runtime_id: "ggml_vulkan_v1".to_string(),
            runtime_version: "1".to_string(),
            runtime_commit: "8c63e70982c95ceb862e3a1073a2c1beef75d60a".to_string(),
        },
        size_bytes,
    )
}

fn acquisition(method: AcquisitionMethod, label: &str) -> AcquisitionSpec {
    AcquisitionSpec {
        method,
        label: label.to_string(),
        license_id: None,
    }
}

fn roformer_source(id: &str) -> (SourceIdentity, LicenseInfo, Option<u64>) {
    let (repository, revision, filename, sha256, bytes, attribution, source_page) = match id {
        "melband_roformer_inst_v2" => (
            "pcunwa/Mel-Band-Roformer-Inst",
            "f86cd9e99d63eb9499b00fca424bc4ed8a8aeaba",
            "melband_roformer_inst_v2.ckpt",
            "bd19766620f7d6f58fdf7aaada7e89907fe41bc64490ce3faa9a6dab15d6e1f2",
            1_574_477_088,
            "Unwa MelBand RoFormer Inst V2",
            "https://huggingface.co/pcunwa/Mel-Band-Roformer-Inst",
        ),
        "melband_roformer_harmony" => (
            "https://github.com/TRvlvr/model_repo",
            "all_public_uvr_models",
            "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt",
            "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813",
            913_096_801,
            "aufr33 + viperx MelBand RoFormer Karaoke / UVR public catalog",
            "https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models",
        ),
        "melband_roformer_denoise_aufr33" => (
            "poiqazwsx/melband-roformer-denoise",
            "4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9",
            "denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt",
            "7c1c39191edc34e942ca7f2346ce6b6c0e1208a5f76349ffce6f696bd12910de",
            913_097_300,
            "aufr33 MelBand RoFormer denoise",
            "https://huggingface.co/poiqazwsx/melband-roformer-denoise",
        ),
        "melband_roformer_dereverb_anvuew" => (
            "anvuew/dereverb_mel_band_roformer",
            "cef05ad2b5b3145ea5c149d3ad5d1f8439b34d06",
            "dereverb_mel_band_roformer_anvuew_sdr_19.1729.ckpt",
            "9262877b87e9ebb0fb808a456b0a411fa677f5df31c8383c1254af531c078970",
            913_107_578,
            "anvuew MelBand RoFormer dereverb",
            "https://huggingface.co/anvuew/dereverb_mel_band_roformer",
        ),
        _ => unreachable!("shipped RoFormer id"),
    };
    (
        SourceIdentity {
            repository: Some(repository.to_string()),
            revision: Some(revision.to_string()),
            filename: Some(filename.to_string()),
            sha256: Some(sha256.to_string()),
            source_format: Some("ckpt".to_string()),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "review_recorded_user_download".to_string(),
            source_attribution: attribution.to_string(),
            source_page: Some(source_page.to_string()),
        },
        Some(bytes),
    )
}

fn qwen_model_license(source: &SourceIdentity) -> LicenseInfo {
    LicenseInfo {
        status: "apache-2.0".to_string(),
        source_attribution: if source.algorithm.is_some() {
            "Qwen canonical model weights and converted GGUF artifact".to_string()
        } else {
            "Qwen model weights".to_string()
        },
        source_page: source
            .algorithm
            .as_ref()
            .map(|identity| &identity.repository)
            .or(source.repository.as_ref())
            .map(|repository| format!("https://huggingface.co/{repository}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rmvpe_identity_revision_does_not_change_other_catalog_recipes() {
        let unrelated = "unrelated-resource";
        assert_eq!(
            catalog_recipe_digest(unrelated),
            format!(
                "{:x}",
                Sha256::digest(format!("{RUNTIME_CATALOG_VERSION}:{unrelated}").as_bytes())
            )
        );
        assert_eq!(
            catalog_recipe_digest("rmvpe"),
            format!(
                "{:x}",
                Sha256::digest(b"runtime-manager-rmvpe-identity-v2:rmvpe")
            )
        );
    }

    #[test]
    fn catalog_contains_initial_required_resources() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        for model in [
            "bs_roformer_leap_xe90_vocals",
            "bs_polarformer_public_instrumental",
            "jbm555_cectc_80",
            "melband_roformer_inst_v2",
            "melband_roformer_harmony",
            "melband_roformer_denoise_aufr33",
            "melband_roformer_dereverb_anvuew",
            "firered_asr2_aed",
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
            "rmvpe",
            "fcpe",
            "game",
            "basic_pitch",
            "stars",
            "rosvot",
        ] {
            assert!(catalog.models.contains_key(model), "{model}");
        }
        for runtime in ["openvino_2026_3", "qwen_asr_runtime", "qwen_align_runtime"] {
            assert!(catalog.runtimes.contains_key(runtime), "{runtime}");
        }
        assert!(catalog.tools.contains_key("ffmpeg"));
        assert!(catalog.tools.contains_key("fusion_agent_adapter"));
        assert!(catalog.bundles.contains_key("roformer"));
    }

    #[test]
    fn roformer_sources_are_independent_and_exactly_pinned() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let expected = [
            (
                "melband_roformer_inst_v2",
                "bd19766620f7d6f58fdf7aaada7e89907fe41bc64490ce3faa9a6dab15d6e1f2",
            ),
            (
                "melband_roformer_harmony",
                "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813",
            ),
            (
                "melband_roformer_denoise_aufr33",
                "7c1c39191edc34e942ca7f2346ce6b6c0e1208a5f76349ffce6f696bd12910de",
            ),
            (
                "melband_roformer_dereverb_anvuew",
                "9262877b87e9ebb0fb808a456b0a411fa677f5df31c8383c1254af531c078970",
            ),
        ];
        for (id, sha256) in expected {
            let model = catalog.model(id).unwrap();
            assert_eq!(model.source.sha256.as_deref(), Some(sha256));
            assert_eq!(model.pinned_backend, Some(NativeBackend::Vulkan));
            assert_eq!(
                model.dependencies,
                [ResourceRef::runtime("ggml_vulkan_v1").unwrap()]
            );
            assert_eq!(model.backends.len(), 1);
            assert!(model.backends.iter().all(|backend| {
                backend.backend == NativeBackend::Vulkan
                    && backend.validation == ValidationState::ProductionPinned
            }));
            assert_eq!(
                model.runtime_recipe_digest.as_deref(),
                Some("4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a")
            );
        }
        let leap = catalog.model("bs_roformer_leap_xe90_vocals").unwrap();
        assert_eq!(leap.source.sha256, None);
        assert_eq!(leap.pinned_backend, Some(NativeBackend::Vulkan));
        assert_eq!(
            catalog
                .model("melband_roformer_inst_v2")
                .unwrap()
                .source
                .revision
                .as_deref(),
            Some("f86cd9e99d63eb9499b00fca424bc4ed8a8aeaba")
        );
        assert_eq!(
            catalog
                .model("melband_roformer_dereverb_anvuew")
                .unwrap()
                .source
                .revision
                .as_deref(),
            Some("cef05ad2b5b3145ea5c149d3ad5d1f8439b34d06")
        );
        let bundle = catalog.bundles.get("roformer").unwrap();
        assert_eq!(bundle.dependencies.len(), expected.len() + 1);
        assert!(expected.iter().all(|(id, _)| {
            bundle
                .dependencies
                .contains(&ResourceRef::model(*id).unwrap())
        }));
    }

    #[test]
    fn acquisition_and_worker_capabilities_are_truthful() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let roformer = catalog.model("bs_roformer_leap_xe90_vocals").unwrap();
        assert_eq!(roformer.source.sha256, None);
        assert!(
            roformer
                .acquisition
                .iter()
                .all(|spec| spec.method == AcquisitionMethod::ManagedDownload)
        );
        assert_eq!(roformer.pinned_backend, Some(NativeBackend::Vulkan));
        assert_eq!(
            roformer.source.filename.as_deref(),
            Some("bs_leap_xe_voc-F32.gguf")
        );
        let align = catalog.model("qwen3_forced_aligner_0_6b").unwrap();
        assert!(
            align
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::LocalImport)
        );
        assert_eq!(
            align.source.sha256.as_deref(),
            Some("00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86")
        );
        assert_eq!(align.source.filename.as_deref(), Some("model.safetensors"));
        let converted = align.source.converted_artifact.as_ref().unwrap();
        assert_eq!(
            converted.manifest_sha256,
            "c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b"
        );
        assert_eq!(
            converted.conversion_recipe_sha256,
            "ffd8a575238c81823509e2a7bf645bf9bb5d38db2903bc3306648afd619b42d6"
        );
        assert!(align.backends.iter().all(|backend| {
            backend.validation == ValidationState::ProductionPinned
                && backend.evidence_id.as_deref()
                    == Some("validation:qwen-runtime-validation#aligner-static-closure")
        }));
        let asr = catalog.model("qwen3_asr_1_7b").unwrap();
        assert_eq!(
            asr.source.revision.as_deref(),
            Some("92282af1610a2db19d66f2bef1e260f5deca782d")
        );
        assert_eq!(
            asr.source.sha256.as_deref(),
            Some("b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e")
        );
        assert_eq!(
            asr.source
                .algorithm
                .as_ref()
                .and_then(|identity| identity.revision.as_deref()),
            Some("7278e1e70fe206f11671096ffdd38061171dd6e5")
        );
        let rmvpe = catalog.model("rmvpe").unwrap();
        assert_eq!(
            rmvpe.source.repository.as_deref(),
            Some("https://huggingface.co/lj1995/VoiceConversionWebUI")
        );
        assert_eq!(
            rmvpe.source.revision.as_deref(),
            Some("e6d0c1a17da07c33557852f9dfa2bd44cc75737d")
        );
        assert_eq!(rmvpe.source.filename.as_deref(), Some("rmvpe.onnx"));
        assert_eq!(rmvpe.source.sha256.as_deref(), Some(RMVPE_SOURCE_SHA256));
        assert_eq!(rmvpe.source.source_format.as_deref(), Some("onnx"));
        let converted = rmvpe.source.converted_artifact.as_ref().unwrap();
        assert_eq!(converted.manifest_filename, "manifest.json");
        assert_eq!(converted.manifest_sha256, RMVPE_IR_MANIFEST_SHA256);
        assert_eq!(
            converted.conversion_recipe_sha256,
            RMVPE_CONVERSION_RECIPE_SHA256
        );
        assert_eq!(converted.runtime_id, "openvino_2026_3");
        assert_eq!(converted.runtime_version, "2026.3.0");
        assert_eq!(
            converted.runtime_commit,
            "8a17657b995fd3b4a52f8484acfcf2bb61214623"
        );
        assert_ne!(rmvpe.source.sha256, Some(converted.manifest_sha256.clone()));
        assert_eq!(rmvpe.license.status, "mit");
        assert_eq!(
            rmvpe.source.algorithm.as_ref().unwrap().license_id,
            "Apache-2.0"
        );
        assert!(
            rmvpe
                .acquisition
                .iter()
                .any(|spec| spec.method == AcquisitionMethod::LocalImport)
        );
        let game = catalog.model("game").unwrap();
        assert_eq!(game.source.sha256.as_deref(), Some(GAME_IR_MANIFEST_SHA256));
        assert!(game.acquisition.iter().any(|spec| {
            spec.method == AcquisitionMethod::LocalImport
                && spec.license_id.as_deref() == Some("cc-by-nc-sa-4.0")
        }));
        for (id, capability, manifest, recipe) in [
            (
                "stars",
                "notes.stars",
                STARS_IR_MANIFEST_SHA256,
                STARS_CONVERSION_RECIPE_SHA256,
            ),
            (
                "rosvot",
                "notes.rosvot",
                ROSVOT_IR_MANIFEST_SHA256,
                ROSVOT_CONVERSION_RECIPE_SHA256,
            ),
        ] {
            let model = catalog.model(id).unwrap();
            if id == "stars" {
                assert_eq!(model.capabilities, [capability, "technique.analyze"]);
            } else {
                assert_eq!(model.capabilities, [capability]);
            }
            assert!(
                model
                    .acquisition
                    .iter()
                    .all(|spec| spec.method == AcquisitionMethod::LocalImport)
            );
            assert_eq!(model.pinned_backend, Some(NativeBackend::OpenVino));
            assert!(model.backends.iter().any(|backend| {
                backend.backend == NativeBackend::OpenVino
                    && backend.validation == ValidationState::ProductionPinned
            }));
            assert!(model.backends.iter().any(|backend| {
                backend.backend == NativeBackend::CpuReference
                    && backend.validation == ValidationState::Experimental
            }));
            let converted = model.source.converted_artifact.as_ref().unwrap();
            assert_eq!(converted.manifest_sha256, manifest);
            assert_eq!(converted.conversion_recipe_sha256, recipe);
            assert_eq!(
                converted.format,
                if id == "stars" {
                    "openvino_ir_v11_conditioned_segmented_p1"
                } else {
                    "openvino_ir_v11_conditioned_segmented"
                }
            );
        }
        let stars = catalog.model("stars").unwrap();
        assert_eq!(
            stars.source.sha256.as_deref(),
            Some("9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c")
        );
        assert!(stars.source.artifacts.iter().all(|artifact| artifact.sha256
            != "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2"));
        let openvino = catalog.runtime("openvino_2026_3").unwrap();
        assert!(
            openvino
                .supported_models
                .iter()
                .any(|model| model == "game")
        );
        assert!(
            openvino
                .supported_models
                .iter()
                .any(|model| model == "rmvpe")
        );
        for model in ["stars", "rosvot"] {
            assert!(openvino.supported_models.iter().any(|value| value == model));
        }
    }

    #[test]
    fn every_roformer_is_ggml_only_and_openvino_cannot_resolve_it() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let openvino = catalog.runtime("openvino_2026_3").unwrap();
        for model_id in [
            "bs_roformer_leap_xe90_vocals",
            "melband_roformer_inst_v2",
            "melband_roformer_harmony",
            "melband_roformer_denoise_aufr33",
            "melband_roformer_dereverb_anvuew",
        ] {
            let model = catalog.model(model_id).unwrap();
            assert_eq!(model.pinned_backend, Some(NativeBackend::Vulkan));
            assert_eq!(
                model.dependencies,
                [ResourceRef::runtime("ggml_vulkan_v1").unwrap()]
            );
            assert_eq!(model.backends.len(), 1);
            assert!(model.backends.iter().all(|backend| {
                backend.backend == NativeBackend::Vulkan
                    && backend.validation == ValidationState::ProductionPinned
            }));
            if model_id != "bs_roformer_leap_xe90_vocals" {
                let converted = model.source.converted_artifact.as_ref().unwrap();
                assert_eq!(converted.format, "gguf_f16");
                assert_eq!(converted.runtime_id, "ggml_vulkan_v1");
            }
            assert!(
                !model
                    .capabilities
                    .iter()
                    .any(|capability| capability == "audio.lead_partition")
            );
            assert!(!openvino.supported_models.contains(&model_id.to_string()));
        }
    }

    #[test]
    fn optional_openvino_experts_preserve_source_converted_and_policy_identity() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let expected = [
            (
                "firered_asr2_aed",
                "speech.transcribe.challenger",
                "https://huggingface.co/42ailab/FireRedASR2-AED-ONNX",
                "13f950858934f7b6a0d3ce52bae65af0dc022258",
                None,
                FIRERED_IR_MANIFEST_SHA256,
                "https://github.com/FireRedTeam/FireRedASR2S",
                "Apache-2.0",
                ValidationState::ProductionPinned,
            ),
            (
                "fcpe",
                "pitch.secondary",
                "https://huggingface.co/gzivdo/fcpe-onnx",
                "5800a2b1944967f55bb0bfeb9718cb749f809310",
                Some(FCPE_SOURCE_SHA256),
                FCPE_IR_MANIFEST_SHA256,
                "https://github.com/CNChTu/FCPE",
                "MIT",
                ValidationState::ProductionPinned,
            ),
            (
                "basic_pitch",
                "notes.basic_pitch",
                "https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models",
                "327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763",
                Some(BASIC_PITCH_SOURCE_SHA256),
                BASIC_PITCH_IR_MANIFEST_SHA256,
                "https://github.com/spotify/basic-pitch",
                "Apache-2.0",
                ValidationState::ProductionPinned,
            ),
        ];
        for (
            id,
            capability,
            repository,
            revision,
            source_sha256,
            manifest_sha256,
            algorithm_repository,
            algorithm_license,
            expected_validation,
        ) in expected
        {
            let model = catalog.model(id).unwrap();
            assert_eq!(model.capabilities, [capability]);
            assert_eq!(model.source.repository.as_deref(), Some(repository));
            assert_eq!(model.source.revision.as_deref(), Some(revision));
            assert_eq!(model.source.sha256.as_deref(), source_sha256);
            assert!(!model.source.artifacts.is_empty());
            let algorithm = model.source.algorithm.as_ref().unwrap();
            assert_eq!(algorithm.repository, algorithm_repository);
            assert_eq!(algorithm.license_id, algorithm_license);
            let converted = model.source.converted_artifact.as_ref().unwrap();
            assert_eq!(converted.manifest_sha256, manifest_sha256);
            assert!(converted.conversion_recipe_sha256.is_empty());
            assert_ne!(model.source.sha256.as_deref(), Some(manifest_sha256));
            assert!(
                model
                    .acquisition
                    .iter()
                    .any(|spec| { spec.method == AcquisitionMethod::LocalImport })
            );
            assert_eq!(model.pinned_backend, Some(NativeBackend::OpenVino));
            assert!(
                model
                    .backends
                    .iter()
                    .all(|backend| backend.evidence_id.is_some())
            );
            assert!(model.backends.iter().any(|backend| {
                backend.backend == NativeBackend::OpenVino
                    && backend.validation == expected_validation
            }));
            assert!(model.backends.iter().any(|backend| {
                backend.backend == NativeBackend::CpuReference
                    && backend.validation == ValidationState::Experimental
            }));
        }
        assert_eq!(
            catalog
                .model("firered_asr2_aed")
                .unwrap()
                .source
                .artifacts
                .len(),
            5
        );
    }

    #[test]
    fn every_openvino_ir_model_has_explicit_cpu_only_diagnostics() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let mut ir_models = 0;
        for model in catalog.models.values().filter(|model| {
            model
                .backends
                .iter()
                .any(|backend| backend.backend == NativeBackend::OpenVino)
        }) {
            ir_models += 1;
            assert!(model.backends.iter().any(|backend| {
                backend.backend == NativeBackend::CpuReference
                    && backend.validation == ValidationState::Experimental
            }));
        }
        assert_eq!(ir_models, 9);
        for qwen in ["qwen3_asr_1_7b", "qwen3_forced_aligner_0_6b"] {
            assert!(
                catalog
                    .model(qwen)
                    .unwrap()
                    .backends
                    .iter()
                    .all(|backend| { backend.backend != NativeBackend::CpuReference })
            );
        }
    }

    #[test]
    fn every_effective_model_and_runtime_route_is_production_pinned() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let effective_models = catalog
            .models
            .values()
            .filter(|model| !model.backends.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(effective_models.len(), 16);
        for model in effective_models {
            let pinned = model
                .pinned_backend
                .expect("every effective model must declare its backend");
            assert!(
                model.backends.iter().any(|capability| {
                    capability.backend == pinned
                        && capability.validation == ValidationState::ProductionPinned
                }),
                "{}",
                model.id.as_str()
            );
            assert!(
                model.backends.iter().all(|capability| {
                    capability.backend != NativeBackend::CpuReference
                        || capability.validation == ValidationState::Experimental
                }),
                "{}",
                model.id.as_str()
            );
        }
        for runtime in catalog.runtimes.values() {
            assert!(
                runtime.backends.iter().all(|capability| {
                    capability.backend == NativeBackend::CpuReference
                        || capability.validation == ValidationState::ProductionPinned
                }),
                "{}",
                runtime.id
            );
        }
    }
}
