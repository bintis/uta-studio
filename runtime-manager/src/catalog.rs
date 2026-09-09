use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resource::{ModelId, ResourceKind, ResourceRef};
use crate::runtime_lock::{FCPE_GGUF_SHA256, FCPE_GGUF_SIZE_BYTES, GGML_RUNTIME_RECIPE_SHA256};
use crate::state::ValidationState;

pub const RUNTIME_CATALOG_VERSION: &str = "ggml";
const GGML_COMMIT: &str = "8c63e70982c95ceb862e3a1073a2c1beef75d60a";

/// Model execution is uniformly owned by GGML. Hardware selection is carried
/// separately by [`NativeDeviceClass`]; it is not a second model runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBackend {
    Ggml,
}

impl FromStr for NativeBackend {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        match value {
            "ggml" | "ggml_vulkan" | "vulkan" => Ok(Self::Ggml),
            other => Err(RuntimeManagerError::new(
                "invalid_backend",
                format!("unknown execution backend: {other}"),
            )),
        }
    }
}

/// Device-class preference for a GGML execution.
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
pub struct ModelArtifactSpec {
    pub name: String,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub id: ModelId,
    pub display_name: String,
    pub purpose: String,
    pub capabilities: Vec<String>,
    pub source: SourceIdentity,
    pub license: LicenseInfo,
    /// Complete, named runtime file set. `model` is the graph/weight artifact;
    /// sidecars are explicit peers rather than paths guessed by workers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_artifacts: Vec<ModelArtifactSpec>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_version: Option<String>,
    pub models: BTreeMap<String, ModelCatalogEntry>,
    pub runtimes: BTreeMap<String, RuntimeCatalogEntry>,
    pub tools: BTreeMap<String, ToolCatalogEntry>,
    pub bundles: BTreeMap<String, BundleCatalogEntry>,
}

impl ResourceCatalog {
    pub fn default_catalog() -> RuntimeManagerResult<Self> {
        let mut catalog = Self {
            schema_version: None,
            catalog_version: None,
            models: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            tools: BTreeMap::new(),
            bundles: BTreeMap::new(),
        };
        catalog.add_runtimes()?;
        catalog.add_models()?;
        catalog.add_tools_and_bundles()?;
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

    fn add_runtimes(&mut self) -> RuntimeManagerResult<()> {
        self.insert_runtime(ggml_runtime(
            "ggml_vulkan",
            "GGML model runtime",
            "uta-ggml-worker",
            &[
                "bs_roformer_leap_xe90_vocals",
                "bs_roformer_leap_xe90_instrumental",
                "bs_polarformer_public_instrumental",
                "melband_roformer_harmony",
                "melband_roformer_denoise_aufr33",
                "melband_roformer_dereverb_anvuew",
                "rmvpe",
                "fcpe",
                "basic_pitch",
                "game_1_0_3_small",
                "game_1_0_3_medium",
                "game_1_0_3_large",
                "jbm555_cectc_80",
                "stars",
                "rosvot",
                "firered_asr2_aed",
                "qwen3_asr_1_7b",
                "qwen3_forced_aligner_0_6b",
            ],
            GGML_RUNTIME_RECIPE_SHA256,
        ))?;
        Ok(())
    }

    fn add_models(&mut self) -> RuntimeManagerResult<()> {
        self.insert_model(leap_model()?)?;
        self.insert_model(leap_instrumental_model()?)?;
        self.insert_model(polarformer_model()?)?;
        for (id, name, purpose, capability, source) in [
            (
                "melband_roformer_harmony",
                "MelBand-RoFormer Lead Isolation",
                "Lead-vocal extraction with vocal residual",
                "audio.lead_isolate",
                roformer_source("melband_roformer_harmony"),
            ),
            (
                "melband_roformer_denoise_aufr33",
                "MelBand-RoFormer Denoise",
                "44.1 kHz stereo vocal denoise",
                "audio.denoise",
                roformer_source("melband_roformer_denoise_aufr33"),
            ),
            (
                "melband_roformer_dereverb_anvuew",
                "MelBand-RoFormer Dereverb",
                "44.1 kHz stereo vocal dereverb",
                "audio.dereverb",
                roformer_source("melband_roformer_dereverb_anvuew"),
            ),
        ] {
            self.insert_model(ggml_model(
                id,
                name,
                purpose,
                &[capability],
                source.0,
                source.1,
                "ggml_vulkan",
                source.2,
                Some(457_008_736),
                GGML_RUNTIME_RECIPE_SHA256,
            )?)?;
        }
        self.insert_model(rmvpe_model()?)?;
        self.insert_model(fcpe_model()?)?;
        self.insert_model(basic_pitch_model()?)?;
        for variant in ["small", "medium", "large"] {
            self.insert_model(game_model(variant)?)?;
        }
        self.insert_model(jbm555_model()?)?;
        self.insert_model(stars_model()?)?;
        self.insert_model(rosvot_model()?)?;
        self.insert_model(firered_model()?)?;
        self.insert_model(qwen_asr_model()?)?;
        self.insert_model(qwen_aligner_model()?)?;
        Ok(())
    }

    fn add_tools_and_bundles(&mut self) -> RuntimeManagerResult<()> {
        self.tools.insert(
            "ffmpeg".to_string(),
            ToolCatalogEntry {
                id: "ffmpeg".to_string(),
                display_name: "FFmpeg".to_string(),
                purpose: "Audio decode and encode utility".to_string(),
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
                purpose: "External adapter for bounded AI candidate-path selection".to_string(),
                acquisition: vec![acquisition(
                    AcquisitionMethod::ExternalTool,
                    "explicit Uta Fusion Agent Adapter executable",
                )],
            },
        );
        let baseline = [
            "bs_roformer_leap_xe90_vocals",
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
            "rmvpe",
            "game_1_0_3_medium",
        ]
        .into_iter()
        .map(ResourceRef::model)
        .collect::<Result<Vec<_>, _>>()?;
        self.bundles.insert(
            "engine-fast".to_string(),
            BundleCatalogEntry {
                id: "engine-fast".to_string(),
                display_name: "GGML analysis baseline".to_string(),
                purpose: "Currently implemented GGML analysis resources".to_string(),
                dependencies: baseline,
            },
        );
        self.bundles.insert(
            "roformer".to_string(),
            BundleCatalogEntry {
                id: "roformer".to_string(),
                display_name: "RoFormer family".to_string(),
                purpose: "GGML RoFormer separation resources".to_string(),
                dependencies: [
                    "bs_roformer_leap_xe90_vocals",
                    "bs_roformer_leap_xe90_instrumental",
                    "bs_polarformer_public_instrumental",
                    "melband_roformer_harmony",
                    "melband_roformer_denoise_aufr33",
                    "melband_roformer_dereverb_anvuew",
                ]
                .into_iter()
                .map(ResourceRef::model)
                .collect::<Result<Vec<_>, _>>()?,
            },
        );
        Ok(())
    }

    fn insert_model(&mut self, entry: ModelCatalogEntry) -> RuntimeManagerResult<()> {
        let id = entry.id.as_str().to_string();
        if self.models.insert(id.clone(), entry).is_some() {
            return Err(RuntimeManagerError::invalid_catalog(format!(
                "duplicate model id {id}"
            )));
        }
        Ok(())
    }

    fn insert_runtime(&mut self, entry: RuntimeCatalogEntry) -> RuntimeManagerResult<()> {
        let id = entry.id.clone();
        if self.runtimes.insert(id.clone(), entry).is_some() {
            return Err(RuntimeManagerError::invalid_catalog(format!(
                "duplicate runtime id {id}"
            )));
        }
        Ok(())
    }
}

fn backend() -> BackendCapability {
    BackendCapability {
        backend: NativeBackend::Ggml,
        validation: ValidationState::ProductionPinned,
        evidence_id: Some("validation:ggml-runtime".to_string()),
    }
}

fn ggml_runtime(
    id: &str,
    name: &str,
    component: &str,
    models: &[&str],
    recipe: &str,
) -> RuntimeCatalogEntry {
    RuntimeCatalogEntry {
        id: id.to_string(),
        display_name: name.to_string(),
        purpose: "Rust-hosted, local GGML execution".to_string(),
        backends: vec![backend()],
        acquisition: vec![acquisition(
            AcquisitionMethod::Bundled,
            "packaged Rust GGML worker",
        )],
        executable_component_id: component.to_string(),
        supported_models: models.iter().map(|model| (*model).to_string()).collect(),
        recipe_digest: Some(recipe.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn ggml_model(
    id: &str,
    name: &str,
    purpose: &str,
    capabilities: &[&str],
    source: SourceIdentity,
    license: LicenseInfo,
    runtime: &str,
    download_bytes: Option<u64>,
    installed_bytes: Option<u64>,
    runtime_recipe: &str,
) -> RuntimeManagerResult<ModelCatalogEntry> {
    let primary_filename = source
        .converted_artifact
        .as_ref()
        .map(|artifact| artifact.manifest_filename.clone())
        .or_else(|| source.filename.clone())
        .ok_or_else(|| {
            RuntimeManagerError::invalid_catalog(format!(
                "GGML model {id} has no runtime artifact filename"
            ))
        })?;
    Ok(ModelCatalogEntry {
        id: ModelId::new(id)?,
        display_name: name.to_string(),
        purpose: purpose.to_string(),
        capabilities: capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        source,
        license,
        runtime_artifacts: vec![ModelArtifactSpec {
            name: "model".to_string(),
            filename: primary_filename,
        }],
        acquisition: vec![acquisition(
            if matches!(
                id,
                "bs_roformer_leap_xe90_vocals" | "bs_roformer_leap_xe90_instrumental"
            ) {
                AcquisitionMethod::ManagedDownload
            } else {
                AcquisitionMethod::LocalImport
            },
            "GGUF model for the pinned GGML runtime",
        )],
        dependencies: vec![ResourceRef::runtime(runtime)?],
        backends: vec![backend()],
        pinned_backend: Some(NativeBackend::Ggml),
        estimated_download_bytes: download_bytes,
        estimated_installed_bytes: installed_bytes,
        recipe_digest: catalog_recipe_digest(id),
        runtime_recipe_digest: Some(runtime_recipe.to_string()),
    })
}

fn leap_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "bs_roformer_leap_xe90_vocals",
        "BS-RoFormer Leap XE90",
        "Single-pass GuideVocals and Instrumental-residual extraction",
        &["audio.extract_vocals", "audio.extract_instrumental"],
        SourceIdentity {
            repository: Some("scragnog/HOT-Step-CPP-SuperSep".to_string()),
            revision: Some("440487b8300dcd61453cc52ec244a38150b03456".to_string()),
            filename: Some("bs_leap_xe_voc-F32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://huggingface.co/pcunwa/BS-Roformer-Leap".to_string(),
                revision: Some("4e47d6662ae82eaa8b4ac4329fe66099a843b48e".to_string()),
                license_id: "source-attribution".to_string(),
            }),
            // The author's own checkpoint at that revision, named exactly, so
            // the GGUF this entry installs can be traced to a public file
            // rather than only to the repository that repackaged it.
            //
            // Verified on 2026-09-10 rather than assumed. The installed GGUF
            // and this checkpoint hold the same 66,845,708 weights: identical
            // element count, identical sum 15098.179294, identical sum of
            // magnitudes 2604152.1378, and identical minimum -3.006065 and
            // maximum 3.201639, the last three being independent of tensor
            // naming and of the dimension order a GGUF conversion applies.
            // The copy in noblebarkrr/mvsepless_resources as
            // bs_roformer/bs_leap_xe_voc_unwa.ckpt is byte-identical to this
            // one, same size and same digest.
            artifacts: vec![SourceArtifactIdentity {
                filename: "Xe/bs_leap_xe_voc.ckpt".to_string(),
                sha256: "b739c1d2d87a81cd3dd3844ed9ad0bd678708c7a0a761a03a1aaff9af79a096d"
                    .to_string(),
            }],
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "informational".to_string(),
            source_attribution: "pcunwa BS-RoFormer Leap; public GGUF by scragnog".to_string(),
            source_page: Some("https://huggingface.co/scragnog/HOT-Step-CPP-SuperSep".to_string()),
        },
        "ggml_vulkan",
        Some(267_433_600),
        Some(267_433_600),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn leap_instrumental_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "bs_roformer_leap_xe90_instrumental",
        "BS-RoFormer Leap XE90 Instrumental",
        "Single-pass direct Instrumental and vocal-residual extraction",
        &["audio.extract_vocals", "audio.extract_instrumental"],
        SourceIdentity {
            repository: Some("scragnog/HOT-Step-CPP-SuperSep".to_string()),
            revision: Some("440487b8300dcd61453cc52ec244a38150b03456".to_string()),
            filename: Some("bs_leap_xe_inst-F32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://huggingface.co/pcunwa/BS-Roformer-Leap".to_string(),
                revision: Some("4e47d6662ae82eaa8b4ac4329fe66099a843b48e".to_string()),
                license_id: "source-attribution".to_string(),
            }),
            artifacts: vec![SourceArtifactIdentity {
                filename: "Xe/bs_leap_xe_inst.ckpt".to_string(),
                sha256: "33ee9415f491d257fa7a79f6e92a80d647bc71ed622bfef9a137b6e4250307d3"
                    .to_string(),
            }],
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "informational".to_string(),
            source_attribution: "pcunwa BS-RoFormer Leap; public GGUF by scragnog".to_string(),
            source_page: Some("https://huggingface.co/scragnog/HOT-Step-CPP-SuperSep".to_string()),
        },
        "ggml_vulkan",
        Some(267_433_600),
        Some(267_433_600),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn polarformer_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "bs_polarformer_public_instrumental",
        "BS-PolarFormer Public",
        "Experimental single-pass GuideVocals and Instrumental-residual extraction",
        &["audio.extract_vocals", "audio.extract_instrumental"],
        SourceIdentity {
            repository: Some("bgkb/bs_polarformer".to_string()),
            revision: Some("9158719ee2173edd480a735764627526506fe4af".to_string()),
            filename: Some("model-fp16.gguf".to_string()),
            source_format: Some("gguf-f16".to_string()),
            converted_artifact: Some(ConvertedArtifactIdentity {
                format: "gguf_f16".to_string(),
                manifest_filename: "model-fp16.gguf".to_string(),
                manifest_sha256: "f5e40ac0dc7487a0c2ccb247e5b948cd6f2c7aaf46a2994023606e1e800ed2c1"
                    .to_string(),
                conversion_recipe_sha256: String::new(),
                runtime_id: "ggml_vulkan".to_string(),
                runtime_version: "1".to_string(),
                runtime_commit: GGML_COMMIT.to_string(),
            }),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "mit".to_string(),
            source_attribution: "bgkb public BS-PolarFormer model".to_string(),
            source_page: Some("https://huggingface.co/bgkb/bs_polarformer".to_string()),
        },
        "ggml_vulkan",
        None,
        Some(204_237_408),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn roformer_source(id: &str) -> (SourceIdentity, LicenseInfo, Option<u64>) {
    let (repository, revision, filename, source_sha, bytes, attribution, page, gguf_sha) = match id
    {
        "melband_roformer_harmony" => (
            "https://github.com/TRvlvr/model_repo",
            "all_public_uvr_models",
            "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt",
            "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813",
            913_096_801,
            "aufr33 + viperx MelBand RoFormer Karaoke",
            "https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models",
            "d463c06a1bf5d3889a2a6be58cc469f0a996155eafb91845ff5e8c139a3d64be",
        ),
        "melband_roformer_denoise_aufr33" => (
            "poiqazwsx/melband-roformer-denoise",
            "4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9",
            "denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt",
            "7c1c39191edc34e942ca7f2346ce6b6c0e1208a5f76349ffce6f696bd12910de",
            913_097_300,
            "aufr33 MelBand RoFormer denoise",
            "https://huggingface.co/poiqazwsx/melband-roformer-denoise",
            "eb03fce4c5a450f88718e8a529b8adcd653618a5d32cb55275fa212a80fef33a",
        ),
        "melband_roformer_dereverb_anvuew" => (
            "anvuew/dereverb_mel_band_roformer",
            "cef05ad2b5b3145ea5c149d3ad5d1f8439b34d06",
            "dereverb_mel_band_roformer_anvuew_sdr_19.1729.ckpt",
            "9262877b87e9ebb0fb808a456b0a411fa677f5df31c8383c1254af531c078970",
            913_107_578,
            "anvuew MelBand RoFormer dereverb",
            "https://huggingface.co/anvuew/dereverb_mel_band_roformer",
            "f850fb2460099df356676ce37ba48875e3c75726d7a848b42d75ff6015955ac7",
        ),
        _ => unreachable!("known RoFormer id"),
    };
    (
        SourceIdentity {
            repository: Some(repository.to_string()),
            revision: Some(revision.to_string()),
            filename: Some(filename.to_string()),
            sha256: Some(source_sha.to_string()),
            source_format: Some("ckpt".to_string()),
            converted_artifact: Some(ConvertedArtifactIdentity {
                format: "gguf_f16".to_string(),
                manifest_filename: "model-fp16.gguf".to_string(),
                manifest_sha256: gguf_sha.to_string(),
                conversion_recipe_sha256: String::new(),
                runtime_id: "ggml_vulkan".to_string(),
                runtime_version: "1".to_string(),
                runtime_commit: GGML_COMMIT.to_string(),
            }),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "informational".to_string(),
            source_attribution: attribution.to_string(),
            source_page: Some(page.to_string()),
        },
        Some(bytes),
    )
}

fn rmvpe_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "rmvpe",
        "RMVPE",
        "Continuous F0 tracking",
        &["pitch.track"],
        SourceIdentity {
            filename: Some("rmvpe-f32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "informational".to_string(),
            source_attribution: "RMVPE model".to_string(),
            source_page: None,
        },
        "ggml_vulkan",
        None,
        Some(361_625_344),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn fcpe_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "fcpe",
        "FCPE",
        "Secondary continuous-F0 disagreement expert",
        &["pitch.secondary", "pitch.secondary.fcpe"],
        SourceIdentity {
            repository: Some("https://huggingface.co/gzivdo/fcpe-onnx".to_string()),
            revision: Some("5800a2b1944967f55bb0bfeb9718cb749f809310".to_string()),
            filename: Some("fcpe.onnx".to_string()),
            sha256: Some(
                "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0"
                    .to_string(),
            ),
            source_format: Some("onnx".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/CNChTu/FCPE".to_string(),
                revision: Some("6a149c1afb1c7e7821b71869dfb31ad50c95b516".to_string()),
                license_id: "MIT".to_string(),
            }),
            artifacts: vec![SourceArtifactIdentity {
                filename: "fcpe.onnx".to_string(),
                sha256:
                    "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0"
                        .to_string(),
            }],
            converted_artifact: Some(ConvertedArtifactIdentity {
                format: "gguf_f32".to_string(),
                manifest_filename: "fcpe-f32.gguf".to_string(),
                manifest_sha256: FCPE_GGUF_SHA256.to_string(),
                conversion_recipe_sha256:
                    "bbb1173ef2aadda4240a6132b5b254be692a48235cfd46bfae146c3a9df0b8fc"
                        .to_string(),
                runtime_id: "ggml_vulkan".to_string(),
                runtime_version: "1".to_string(),
                runtime_commit: GGML_COMMIT.to_string(),
            }),
        },
        LicenseInfo {
            status: "mit".to_string(),
            source_attribution: "CNChTu/FCPE canonical project; gzivdo community ONNX export"
                .to_string(),
            source_page: Some(
                "https://huggingface.co/gzivdo/fcpe-onnx/tree/5800a2b1944967f55bb0bfeb9718cb749f809310"
                    .to_string(),
            ),
        },
        "ggml_vulkan",
        None,
        Some(FCPE_GGUF_SIZE_BYTES),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn basic_pitch_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "basic_pitch",
        "Basic Pitch",
        "Optional onset and activation evidence",
        &["notes.basic_pitch"],
        SourceIdentity {
            repository: Some(
                "https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models".to_string(),
            ),
            revision: Some("327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763".to_string()),
            filename: Some("basic-pitch-f32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/spotify/basic-pitch".to_string(),
                revision: Some("fa5997af0a8210982619003269994a1be25eddf3".to_string()),
                license_id: "Apache-2.0".to_string(),
            }),
            artifacts: vec![SourceArtifactIdentity {
                filename: "nmp.onnx".to_string(),
                sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec"
                    .to_string(),
            }],
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "apache-2.0".to_string(),
            source_attribution:
                "Spotify Basic Pitch; selected source bytes are from the AEmotionStudio mirror"
                    .to_string(),
            source_page: Some(
                "https://huggingface.co/AEmotionStudio/basic-pitch-onnx-models/tree/327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763"
                    .to_string(),
            ),
        },
        "ggml_vulkan",
        None,
        Some(144_512),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn game_model(variant: &str) -> RuntimeManagerResult<ModelCatalogEntry> {
    let (id, name, filename, bytes) = match variant {
        "small" => (
            "game_1_0_3_small",
            "GAME 1.0.3 Small",
            "game-small-f32.gguf",
            50_734_944,
        ),
        "medium" => (
            "game_1_0_3_medium",
            "GAME 1.0.3 Medium",
            "game-medium-f32.gguf",
            199_584_064,
        ),
        "large" => (
            "game_1_0_3_large",
            "GAME 1.0.3 Large",
            "game-large-f32.gguf",
            396_034_784,
        ),
        _ => return Err(RuntimeManagerError::invalid_catalog("unknown GAME variant")),
    };
    ggml_model(
        id,
        name,
        "Primary singing note and boundary evidence",
        &["notes.game"],
        SourceIdentity {
            repository: Some("https://github.com/openvpi/GAME".to_string()),
            revision: Some("475a8ee781fe8cca980b3b12fbe6c80c768a813a".to_string()),
            filename: Some(filename.to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/openvpi/GAME".to_string(),
                revision: Some("475a8ee781fe8cca980b3b12fbe6c80c768a813a".to_string()),
                license_id: "CC-BY-NC-SA-4.0".to_string(),
            }),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "cc-by-nc-sa-4.0".to_string(),
            source_attribution: format!("openvpi GAME 1.0.3 {variant} model"),
            source_page: Some("https://github.com/openvpi/GAME/releases/tag/v1.0.3".to_string()),
        },
        "ggml_vulkan",
        None,
        Some(bytes),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn jbm555_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "jbm555_cectc_80",
        "JBM555 CE-CTC 80",
        "Japanese mix-and-vocal conditioned note evidence",
        &["notes.jbm555"],
        SourceIdentity {
            repository: Some("https://github.com/york135/CECTC_baseline_APSIPA25".to_string()),
            revision: Some("d1352eda1ea69d94cf7b1b06bf0b003d874b389a".to_string()),
            filename: Some("jbm555-cectc80-f32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "informational".to_string(),
            source_attribution: "york135 CE-CTC baseline for APSIPA 2025".to_string(),
            source_page: Some("https://github.com/york135/CECTC_baseline_APSIPA25".to_string()),
        },
        "ggml_vulkan",
        None,
        Some(3_981_024),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn stars_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    let mut model = ggml_model(
        "stars",
        "STARS Chinese P1",
        "Timed-transcript-conditioned note, technique, and style evidence",
        &["notes.stars", "technique.analyze"],
        SourceIdentity {
            repository: Some("https://huggingface.co/verstar/STARS".to_string()),
            revision: Some("744a7ad02e1d788452293cd903ea6a933f7862c4".to_string()),
            filename: Some("stars-f32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/gwx314/STARS".to_string(),
                revision: Some("f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167".to_string()),
                license_id: "MIT".to_string(),
            }),
            artifacts: vec![SourceArtifactIdentity {
                filename: "model_ckpt_steps_200000.ckpt".to_string(),
                sha256: "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c"
                    .to_string(),
            }],
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "checkpoint-license-unresolved".to_string(),
            source_attribution:
                "gwx314/STARS MIT source; verstar/STARS checkpoint provenance is recorded separately"
                    .to_string(),
            source_page: Some(
                "https://huggingface.co/verstar/STARS/tree/744a7ad02e1d788452293cd903ea6a933f7862c4"
                    .to_string(),
            ),
        },
        "ggml_vulkan",
        None,
        Some(201_133_440),
        GGML_RUNTIME_RECIPE_SHA256,
    )?;
    model.dependencies.insert(0, ResourceRef::model("rmvpe")?);
    Ok(model)
}

fn rosvot_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    let mut model = ggml_model(
        "rosvot",
        "ROSVOT P0",
        "Timed-transcript-conditioned singing note evidence",
        &["notes.rosvot"],
        SourceIdentity {
            repository: Some("https://github.com/RickyL-2000/ROSVOT".to_string()),
            revision: Some("3c8332bf43adae35f6e4d64971862f2f6139b310".to_string()),
            filename: Some("rosvot-f32.gguf".to_string()),
            source_format: Some("gguf-f32".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/RickyL-2000/ROSVOT".to_string(),
                revision: Some("3c8332bf43adae35f6e4d64971862f2f6139b310".to_string()),
                license_id: "MIT".to_string(),
            }),
            artifacts: vec![SourceArtifactIdentity {
                filename: "rosvot".to_string(),
                sha256: "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb"
                    .to_string(),
            }],
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "checkpoint-license-unresolved".to_string(),
            source_attribution:
                "RickyL-2000/ROSVOT MIT source; selected checkpoint provenance is recorded separately"
                    .to_string(),
            source_page: Some(
                "https://github.com/RickyL-2000/ROSVOT/tree/3c8332bf43adae35f6e4d64971862f2f6139b310"
                    .to_string(),
            ),
        },
        "ggml_vulkan",
        None,
        Some(48_196_032),
        GGML_RUNTIME_RECIPE_SHA256,
    )?;
    model.dependencies.insert(0, ResourceRef::model("rmvpe")?);
    Ok(model)
}

fn firered_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    let mut model = ggml_model(
        "firered_asr2_aed",
        "FireRedASR2-AED",
        "Optional Mandarin, dialect, English, code-switching, and singing transcript challenger",
        &["speech.transcribe.challenger"],
        SourceIdentity {
            repository: Some("https://huggingface.co/FireRedTeam/FireRedASR2-AED".to_string()),
            revision: Some("2304afed56eacfee6256dee5937ed22ffa0b64ec".to_string()),
            filename: Some("model.pth.tar".to_string()),
            source_format: Some("pytorch-checkpoint".to_string()),
            algorithm: Some(AlgorithmIdentity {
                repository: "https://github.com/FireRedTeam/FireRedASR2S".to_string(),
                revision: Some("4e7d9aaf4482a47cec1724807026b9b151926eb5".to_string()),
                license_id: "Apache-2.0".to_string(),
            }),
            artifacts: vec![
                SourceArtifactIdentity {
                    filename: "cmvn.ark".to_string(),
                    sha256: "6efba6105429d1630c05d818d956bfe4edfad37a04b3b27bb5a029b9adb37945"
                        .to_string(),
                },
                SourceArtifactIdentity {
                    filename: "dict.txt".to_string(),
                    sha256: "1bc613de2112d257e61a349c3e72d1b1a9cf19c33d3ca954197ad2171e5ea07b"
                        .to_string(),
                },
            ],
            converted_artifact: Some(ConvertedArtifactIdentity {
                format: "gguf_f32".to_string(),
                manifest_filename: "firered-f32.gguf".to_string(),
                // The container GGML can open. The historical converter wrote
                // PyTorch dimension order, which `cargo xtask gguf firered`
                // rewrites; pinning the pre-migration digest here meant the
                // catalog pinned a file the runtime refuses to load.
                manifest_sha256:
                    "7724d4f01ac8c208670be968cef236b73f4276eddd0de8f85441b56bb6e9d132"
                        .to_string(),
                conversion_recipe_sha256: String::new(),
                runtime_id: "ggml_vulkan".to_string(),
                runtime_version: "1".to_string(),
                runtime_commit: GGML_COMMIT.to_string(),
            }),
            ..SourceIdentity::default()
        },
        LicenseInfo {
            status: "apache-2.0".to_string(),
            source_attribution: "FireRedTeam FireRedASR2-AED canonical checkpoint".to_string(),
            source_page: Some(
                "https://huggingface.co/FireRedTeam/FireRedASR2-AED/tree/2304afed56eacfee6256dee5937ed22ffa0b64ec"
                    .to_string(),
            ),
        },
        "ggml_vulkan",
        None,
        Some(4_686_998_595),
        GGML_RUNTIME_RECIPE_SHA256,
    )?;
    model.runtime_artifacts = vec![
        ModelArtifactSpec {
            name: "model".to_string(),
            filename: "firered-f32.gguf".to_string(),
        },
        ModelArtifactSpec {
            name: "cmvn".to_string(),
            filename: "cmvn.ark".to_string(),
        },
        ModelArtifactSpec {
            name: "tokens".to_string(),
            filename: "dict.txt".to_string(),
        },
    ];
    Ok(model)
}

fn qwen_asr_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "qwen3_asr_1_7b",
        "Qwen3-ASR 1.7B",
        "Primary singing transcription",
        &["speech.transcribe"],
        SourceIdentity {
            repository: Some("https://huggingface.co/Qwen/Qwen3-ASR-1.7B".to_string()),
            revision: Some("7278e1e70fe206f11671096ffdd38061171dd6e5".to_string()),
            filename: Some("Qwen3-ASR-1.7B-F16.gguf".to_string()),
            source_format: Some("gguf-f16".to_string()),
            ..SourceIdentity::default()
        },
        qwen_license("Qwen/Qwen3-ASR-1.7B"),
        "ggml_vulkan",
        None,
        Some(4_083_087_904),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn qwen_aligner_model() -> RuntimeManagerResult<ModelCatalogEntry> {
    ggml_model(
        "qwen3_forced_aligner_0_6b",
        "Qwen3 Forced Aligner 0.6B",
        "Word-level forced alignment",
        &["speech.align"],
        SourceIdentity {
            repository: Some("https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B-hf".to_string()),
            revision: Some("c07281df297b9905d24a508279258cccf987a064".to_string()),
            filename: Some("Qwen3-ForcedAligner-0.6B-F16.gguf".to_string()),
            source_format: Some("gguf-f16".to_string()),
            ..SourceIdentity::default()
        },
        qwen_license("Qwen/Qwen3-ForcedAligner-0.6B-hf"),
        "ggml_vulkan",
        None,
        Some(1_842_216_416),
        GGML_RUNTIME_RECIPE_SHA256,
    )
}

fn qwen_license(repository: &str) -> LicenseInfo {
    LicenseInfo {
        status: "apache-2.0".to_string(),
        source_attribution: "Qwen canonical model weights converted locally to GGUF".to_string(),
        source_page: Some(format!("https://huggingface.co/{repository}")),
    }
}

fn acquisition(method: AcquisitionMethod, label: &str) -> AcquisitionSpec {
    AcquisitionSpec {
        method,
        label: label.to_string(),
        license_id: None,
    }
}

fn catalog_recipe_digest(id: &str) -> String {
    format!("{:x}", Sha256::digest(format!("catalog:{id}").as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_traces_its_weights_to_the_author_s_public_checkpoint() {
        let leap = leap_model().unwrap();
        let algorithm = leap.source.algorithm.as_ref().unwrap();
        assert_eq!(
            algorithm.repository,
            "https://huggingface.co/pcunwa/BS-Roformer-Leap"
        );
        assert_eq!(
            algorithm.revision.as_deref(),
            Some("4e47d6662ae82eaa8b4ac4329fe66099a843b48e")
        );
        let checkpoint = leap
            .source
            .artifacts
            .iter()
            .find(|artifact| artifact.filename == "Xe/bs_leap_xe_voc.ckpt")
            .expect("the upstream checkpoint is named");
        assert_eq!(
            checkpoint.sha256,
            "b739c1d2d87a81cd3dd3844ed9ad0bd678708c7a0a761a03a1aaff9af79a096d"
        );
        // Naming the checkpoint must not disturb what resolution installs.
        assert_eq!(
            leap.source.filename.as_deref(),
            Some("bs_leap_xe_voc-F32.gguf")
        );
    }

    #[test]
    fn catalog_contains_every_implemented_ggml_model() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        assert_eq!(catalog.models.len(), 18);
        assert_eq!(catalog.runtimes.len(), 1);
        for model in catalog.models.values() {
            assert_eq!(model.backends, vec![backend()]);
            assert_eq!(model.pinned_backend, Some(NativeBackend::Ggml));
            assert!(
                model
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == &ResourceRef::runtime("ggml_vulkan").unwrap())
            );
        }
        assert_eq!(
            catalog
                .runtime("ggml_vulkan")
                .unwrap()
                .supported_models
                .len(),
            catalog.models.len()
        );
        for id in [
            "basic_pitch",
            "game_1_0_3_small",
            "game_1_0_3_medium",
            "game_1_0_3_large",
            "jbm555_cectc_80",
            "stars",
            "rosvot",
            "firered_asr2_aed",
            "qwen3_asr_1_7b",
            "qwen3_forced_aligner_0_6b",
        ] {
            assert!(catalog.model(id).is_some(), "{id}");
        }
    }

    #[test]
    fn firered_declares_its_complete_named_artifact_set() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let model = catalog.model("firered_asr2_aed").unwrap();
        assert_eq!(
            model
                .runtime_artifacts
                .iter()
                .map(|artifact| (artifact.name.as_str(), artifact.filename.as_str()))
                .collect::<Vec<_>>(),
            [
                ("model", "firered-f32.gguf"),
                ("cmvn", "cmvn.ark"),
                ("tokens", "dict.txt"),
            ]
        );
    }

    #[test]
    fn conditioned_models_declare_rmvpe_dependency() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        for id in ["stars", "rosvot"] {
            let model = catalog.model(id).unwrap();
            assert!(
                model
                    .dependencies
                    .contains(&ResourceRef::model("rmvpe").unwrap())
            );
        }
    }

    #[test]
    fn leap_strategies_declare_both_outputs_on_one_model() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        for model_id in [
            "bs_roformer_leap_xe90_vocals",
            "bs_roformer_leap_xe90_instrumental",
        ] {
            let leap = catalog.model(model_id).unwrap();
            assert_eq!(
                leap.capabilities,
                ["audio.extract_vocals", "audio.extract_instrumental"]
            );
        }
        let instrumental = catalog.model("bs_roformer_leap_xe90_instrumental").unwrap();
        assert_eq!(
            instrumental.source.filename.as_deref(),
            Some("bs_leap_xe_inst-F32.gguf")
        );
        assert!(
            instrumental
                .source
                .artifacts
                .iter()
                .any(|artifact| artifact.filename == "Xe/bs_leap_xe_inst.ckpt")
        );
    }
}
