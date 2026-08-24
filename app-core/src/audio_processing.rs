//! Native audio-processing settings and immutable execution snapshots.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audio_model::{
    AUDIO_CATALOG_SCHEMA_VERSION, AUDIO_CATALOG_VERSION, AudioModelCatalogSummary,
    AudioModelStatus, AudioParameterMap, AudioParameterValue, DEFAULT_BGM_MODEL_ID,
    DEFAULT_VOCAL_MODEL_ID,
};
use crate::backend_cli::{
    InstallStateWireV1, NativeBackendWireV1, RuntimeCliClient, RuntimePolicyWireV1,
    RuntimeResourceDetailsWireV1, RuntimeResourceRefWireV1, ValidationStateWireV1,
};

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AudioProcessingSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal_model_id: Option<String>,
    #[serde(default)]
    pub vocal_cleanup_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accompaniment_model_id: Option<String>,
    #[serde(default)]
    pub accompaniment_cleanup_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub karaoke_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multistem_model_id: Option<String>,
    #[serde(default)]
    pub common_overrides: AudioParameterMap,
    #[serde(default)]
    pub per_model_overrides: BTreeMap<String, AudioParameterMap>,
    #[serde(default = "default_runtime_policy")]
    pub runtime_policy: String,
    #[serde(default = "default_precision")]
    pub precision_policy: String,
    #[serde(default = "default_memory_policy")]
    pub memory_policy: String,
    #[serde(
        default,
        alias = "legacy_profile",
        skip_serializing_if = "Option::is_none"
    )]
    pub migrated_profile: Option<String>,
}

fn default_runtime_policy() -> String {
    "validated_auto".to_string()
}

fn default_precision() -> String {
    "model_pinned".to_string()
}

fn default_memory_policy() -> String {
    "conservative".to_string()
}

impl Default for AudioProcessingSettings {
    fn default() -> Self {
        Self {
            vocal_model_id: None,
            vocal_cleanup_chain: Vec::new(),
            accompaniment_model_id: None,
            accompaniment_cleanup_chain: Vec::new(),
            karaoke_model_id: None,
            multistem_model_id: None,
            common_overrides: AudioParameterMap::new(),
            per_model_overrides: BTreeMap::new(),
            runtime_policy: default_runtime_policy(),
            precision_policy: default_precision(),
            memory_policy: default_memory_policy(),
            migrated_profile: None,
        }
    }
}

pub fn cleanup_model_enabled(model_id: &str) -> bool {
    !matches!(model_id.trim(), "" | "none" | "off")
}

impl AudioProcessingSettings {
    pub fn from_legacy_separator(_separator: &str) -> Self {
        Self {
            vocal_model_id: Some(DEFAULT_VOCAL_MODEL_ID.to_string()),
            accompaniment_model_id: Some(DEFAULT_BGM_MODEL_ID.to_string()),
            runtime_policy: default_runtime_policy(),
            precision_policy: default_precision(),
            memory_policy: default_memory_policy(),
            migrated_profile: Some("pre_native_audio_profile".to_string()),
            ..Self::default()
        }
    }

    pub fn derived_legacy_separator(&self) -> &'static str {
        "native_workflow"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioInputReference {
    SourceMedia,
    StepOutput { step_id: String, role: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AudioProcessingStep {
    pub step_id: String,
    pub model_id: String,
    pub input: AudioInputReference,
    pub selected_output_roles: Vec<String>,
    pub effective_parameters: AudioParameterMap,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AudioOutputBinding {
    pub artifact_role: String,
    pub step_id: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sum: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AudioRuntimeRequest {
    pub routing_policy: String,
    pub precision_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AudioProcessingPlanSnapshot {
    pub schema_version: u32,
    pub catalog_version: String,
    pub steps: Vec<AudioProcessingStep>,
    pub output_bindings: Vec<AudioOutputBinding>,
    pub requested_runtime: AudioRuntimeRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

fn effective_parameters(settings: &AudioProcessingSettings, model_id: &str) -> AudioParameterMap {
    let mut parameters = settings.common_overrides.clone();
    if let Some(model) = settings.per_model_overrides.get(model_id) {
        parameters.extend(model.clone());
    }
    parameters
}

fn append_chain(
    steps: &mut Vec<AudioProcessingStep>,
    settings: &AudioProcessingSettings,
    chain: &[String],
    lane: &str,
    mut input_step: String,
    mut input_role: String,
) -> (String, String) {
    for (index, model_id) in chain
        .iter()
        .filter(|model_id| cleanup_model_enabled(model_id))
        .enumerate()
    {
        let operation = if model_id.contains("denoise") {
            "denoise"
        } else {
            "dereverb"
        };
        let step_id = format!("{lane}_{operation}_{}", index + 1);
        steps.push(AudioProcessingStep {
            step_id: step_id.clone(),
            model_id: model_id.clone(),
            input: AudioInputReference::StepOutput {
                step_id: input_step,
                role: input_role,
            },
            selected_output_roles: vec!["audio".to_string()],
            effective_parameters: effective_parameters(settings, model_id),
        });
        input_step = step_id;
        input_role = "audio".to_string();
    }
    (input_step, input_role)
}

impl AudioProcessingPlanSnapshot {
    pub fn from_settings(settings: &AudioProcessingSettings) -> Self {
        let vocal_model = settings
            .vocal_model_id
            .as_deref()
            .unwrap_or(DEFAULT_VOCAL_MODEL_ID);
        let bgm_model = settings
            .accompaniment_model_id
            .as_deref()
            .unwrap_or(DEFAULT_BGM_MODEL_ID);
        let mut steps = vec![
            AudioProcessingStep {
                step_id: "extract_vocals".to_string(),
                model_id: vocal_model.to_string(),
                input: AudioInputReference::SourceMedia,
                selected_output_roles: vec!["vocal".to_string()],
                effective_parameters: effective_parameters(settings, vocal_model),
            },
            AudioProcessingStep {
                step_id: "extract_instrumental".to_string(),
                model_id: bgm_model.to_string(),
                input: AudioInputReference::SourceMedia,
                selected_output_roles: vec!["instrumental".to_string()],
                effective_parameters: effective_parameters(settings, bgm_model),
            },
        ];
        let vocal = append_chain(
            &mut steps,
            settings,
            &settings.vocal_cleanup_chain,
            "vocal",
            "extract_vocals".to_string(),
            "vocal".to_string(),
        );
        let bgm = append_chain(
            &mut steps,
            settings,
            &settings.accompaniment_cleanup_chain,
            "bgm",
            "extract_instrumental".to_string(),
            "instrumental".to_string(),
        );
        if let Some(model_id) = settings.karaoke_model_id.as_deref() {
            steps.push(AudioProcessingStep {
                step_id: "harmony_split".to_string(),
                model_id: model_id.to_string(),
                input: AudioInputReference::StepOutput {
                    step_id: vocal.0.clone(),
                    role: vocal.1.clone(),
                },
                selected_output_roles: vec!["lead_vocal".to_string(), "vocal_residual".to_string()],
                effective_parameters: effective_parameters(settings, model_id),
            });
        }
        if let Some(model_id) = settings.multistem_model_id.as_deref() {
            steps.push(AudioProcessingStep {
                step_id: "optional_multistem".to_string(),
                model_id: model_id.to_string(),
                input: AudioInputReference::SourceMedia,
                selected_output_roles: vec![
                    "drums".to_string(),
                    "bass".to_string(),
                    "guitar".to_string(),
                    "piano".to_string(),
                    "other".to_string(),
                ],
                effective_parameters: effective_parameters(settings, model_id),
            });
        }
        Self {
            schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
            catalog_version: AUDIO_CATALOG_VERSION.to_string(),
            steps,
            output_bindings: vec![
                AudioOutputBinding {
                    artifact_role: "analysis_vocal".to_string(),
                    step_id: vocal.0.clone(),
                    role: vocal.1.clone(),
                    sum: None,
                },
                AudioOutputBinding {
                    artifact_role: "vocals".to_string(),
                    step_id: vocal.0,
                    role: vocal.1,
                    sum: None,
                },
                AudioOutputBinding {
                    artifact_role: "instrumental".to_string(),
                    step_id: bgm.0,
                    role: bgm.1,
                    sum: None,
                },
            ],
            requested_runtime: AudioRuntimeRequest {
                routing_policy: settings.runtime_policy.clone(),
                precision_policy: settings.precision_policy.clone(),
            },
            profile_id: Some("native-workflow-migration-v1".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedAudioParameter {
    pub value: AudioParameterValue,
    pub source: String,
}

pub fn preview_effective_audio_params(
    settings: &AudioProcessingSettings,
    song_overrides: Option<&AudioParameterMap>,
    run_overrides: Option<&AudioParameterMap>,
) -> BTreeMap<String, ResolvedAudioParameter> {
    let mut resolved = BTreeMap::from([
        (
            "runtime.routingPolicy".to_string(),
            ResolvedAudioParameter {
                value: AudioParameterValue::Text(settings.runtime_policy.clone()),
                source: "global_settings".to_string(),
            },
        ),
        (
            "runtime.precisionPolicy".to_string(),
            ResolvedAudioParameter {
                value: AudioParameterValue::Text(settings.precision_policy.clone()),
                source: "model_recipe".to_string(),
            },
        ),
    ]);
    for (key, value) in &settings.common_overrides {
        resolved.insert(
            key.clone(),
            ResolvedAudioParameter {
                value: value.clone(),
                source: "global_settings".to_string(),
            },
        );
    }
    for (source, values) in [
        ("song_profile", song_overrides),
        ("run_override", run_overrides),
    ] {
        if let Some(values) = values {
            for (key, value) in values {
                resolved.insert(
                    key.clone(),
                    ResolvedAudioParameter {
                        value: value.clone(),
                        source: source.to_string(),
                    },
                );
            }
        }
    }
    resolved
}

pub fn validate_audio_processing_profile(
    settings: &AudioProcessingSettings,
) -> Result<AudioProcessingPlanSnapshot, String> {
    for model_id in settings
        .vocal_model_id
        .iter()
        .chain(settings.accompaniment_model_id.iter())
        .chain(settings.karaoke_model_id.iter())
        .chain(settings.multistem_model_id.iter())
        .chain(settings.vocal_cleanup_chain.iter())
        .chain(settings.accompaniment_cleanup_chain.iter())
        .filter(|model_id| cleanup_model_enabled(model_id))
    {
        if model_id.contains('/') || model_id.contains(".onnx") || model_id.contains(".gguf") {
            return Err("audio settings must store catalog model IDs, not paths".to_string());
        }
    }
    if !matches!(
        settings.runtime_policy.as_str(),
        "validated_auto" | "testing_auto" | "experimental" | "auto"
    ) {
        return Err("audio processing requires automatic local runtime routing".to_string());
    }
    Ok(AudioProcessingPlanSnapshot::from_settings(settings))
}

fn studio_audio_operation(model_id: &str) -> Option<&'static str> {
    match model_id {
        "bs_roformer_vocals_ep317" => Some("separate_vocals"),
        "melband_roformer_inst_v2" => Some("separate_instrumental"),
        "melband_roformer_harmony" => Some("separate_harmony"),
        "melband_roformer_denoise_aufr33" => Some("denoise"),
        "melband_roformer_dereverb_anvuew" => Some("dereverb"),
        _ => None,
    }
}

const AUDIO_MODEL_CATALOG_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct AudioModelCatalogCache {
    value: Option<AudioModelCatalogSummary>,
    refreshed_at: Option<Instant>,
}

static AUDIO_MODEL_CATALOG_CACHE: OnceLock<Mutex<AudioModelCatalogCache>> = OnceLock::new();

fn audio_model_catalog_cache() -> &'static Mutex<AudioModelCatalogCache> {
    AUDIO_MODEL_CATALOG_CACHE.get_or_init(|| Mutex::new(AudioModelCatalogCache::default()))
}

pub(crate) fn invalidate_audio_model_catalog_cache() {
    if let Ok(mut cache) = audio_model_catalog_cache().lock() {
        cache.value = None;
        cache.refreshed_at = None;
    }
}

pub fn list_audio_models() -> Result<AudioModelCatalogSummary, String> {
    if let Ok(cache) = audio_model_catalog_cache().lock()
        && cache
            .refreshed_at
            .is_some_and(|refreshed| refreshed.elapsed() < AUDIO_MODEL_CATALOG_CACHE_TTL)
        && let Some(value) = cache.value.as_ref()
    {
        return Ok(value.clone());
    }

    let value =
        list_audio_models_with_client(&audio_runtime_client(crate::cache::models_dir(), true)?)?;
    if let Ok(mut cache) = audio_model_catalog_cache().lock() {
        cache.value = Some(value.clone());
        cache.refreshed_at = Some(Instant::now());
    }
    Ok(value)
}

#[cfg(test)]
fn list_audio_models_from_dir(
    models_dir: impl AsRef<Path>,
) -> Result<AudioModelCatalogSummary, String> {
    list_audio_models_with_client(&audio_runtime_client(models_dir.as_ref(), false)?)
}

fn list_audio_models_with_client(
    client: &RuntimeCliClient,
) -> Result<AudioModelCatalogSummary, String> {
    let models = client
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|status| {
            let model_id = status.resource.0.strip_prefix("model:")?.to_string();
            let operation = studio_audio_operation(&model_id)?;
            Some((status.resource, model_id, operation))
        })
        .map(|(resource, model_id, operation)| {
            let details = client.show(&resource).map_err(|error| error.to_string())?;
            audio_model_status_from_details(details, &model_id, operation)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AudioModelCatalogSummary {
        schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
        models,
    })
}

pub fn get_audio_model_status(model_id: &str) -> Result<AudioModelStatus, String> {
    get_audio_model_status_with_client(
        &audio_runtime_client(crate::cache::models_dir(), true)?,
        model_id,
    )
}

#[cfg(test)]
fn get_audio_model_status_from_dir(
    models_dir: impl AsRef<Path>,
    model_id: &str,
) -> Result<AudioModelStatus, String> {
    get_audio_model_status_with_client(&audio_runtime_client(models_dir.as_ref(), false)?, model_id)
}

pub(crate) fn audio_model_is_usable(model_id: &str) -> Result<bool, String> {
    studio_audio_operation(model_id)
        .ok_or_else(|| format!("unknown audio model id: {model_id}"))?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    let statuses = audio_runtime_client(crate::cache::models_dir(), true)?
        .status(&[resource])
        .map_err(|error| error.to_string())?;
    Ok(statuses.first().is_some_and(|status| status.usable))
}

fn get_audio_model_status_with_client(
    client: &RuntimeCliClient,
    model_id: &str,
) -> Result<AudioModelStatus, String> {
    let operation = studio_audio_operation(model_id)
        .ok_or_else(|| format!("unknown audio model id: {model_id}"))?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    let details = client.show(&resource).map_err(|error| error.to_string())?;
    audio_model_status_from_details(details, model_id, operation)
}

fn audio_runtime_client(
    models_dir: impl AsRef<Path>,
    include_environment: bool,
) -> Result<RuntimeCliClient, String> {
    let client = RuntimeCliClient::discover()
        .map_err(|error| error.to_string())?
        .with_legacy_models(models_dir.as_ref());
    Ok(if include_environment {
        client.with_policy(RuntimePolicyWireV1::Experimental)
    } else {
        client.with_store(models_dir.as_ref().join("runtime-store"))
    })
}

fn audio_model_status_from_details(
    details: RuntimeResourceDetailsWireV1,
    model_id: &str,
    operation: &str,
) -> Result<AudioModelStatus, String> {
    Ok(AudioModelStatus {
        model_id: model_id.to_string(),
        display_name: details.metadata.display_name,
        purpose: details.metadata.purpose,
        architecture: "roformer".to_string(),
        operation: operation.to_string(),
        runner: "native_roformer".to_string(),
        supported_backends: details
            .metadata
            .backends
            .iter()
            .filter(|backend| backend.validation != ValidationStateWireV1::Unsupported)
            .map(|backend| native_backend_label(backend.backend).to_string())
            .collect(),
        license: details
            .metadata
            .license
            .map(|license| crate::audio_model::AudioModelLicense {
                status: license.status,
                source_attribution: license.source_attribution,
                source_page: license.source_page,
            })
            .unwrap_or_else(|| crate::audio_model::AudioModelLicense {
                status: "review_required".to_string(),
                source_attribution: "Pinned model/runtime catalog".to_string(),
                source_page: None,
            }),
        estimated_bytes: details.metadata.estimated_installed_bytes,
        state: install_state_label(details.status.install_state).to_string(),
        files: Vec::new(),
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
    })
}

fn native_backend_label(backend: NativeBackendWireV1) -> &'static str {
    match backend {
        NativeBackendWireV1::OpenVino => "openvino",
        NativeBackendWireV1::Vulkan => "vulkan",
        NativeBackendWireV1::NativeDsp => "native_dsp",
        NativeBackendWireV1::CpuReference => "cpu_reference",
    }
}

fn install_state_label(state: InstallStateWireV1) -> &'static str {
    match state {
        InstallStateWireV1::Absent => "missing",
        InstallStateWireV1::Installed | InstallStateWireV1::Legacy => "installed",
        InstallStateWireV1::Incomplete => "incomplete",
        InstallStateWireV1::Corrupt => "integrity_failed",
    }
}

pub fn install_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let client = audio_runtime_client(crate::cache::models_dir(), true)?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    client
        .install(&[resource], &[])
        .map_err(|error| error.to_string())?;
    let status = get_audio_model_status_with_client(&client, model_id)?;
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

pub fn reinstall_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let client = audio_runtime_client(crate::cache::models_dir(), true)?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    client
        .reinstall(&[resource], &[])
        .map_err(|error| error.to_string())?;
    let status = get_audio_model_status_with_client(&client, model_id)?;
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

pub fn remove_audio_model(model_id: &str) -> Result<(), String> {
    let client = audio_runtime_client(crate::cache::models_dir(), true)?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    let result = client
        .remove(std::slice::from_ref(&resource))
        .map_err(|error| error.to_string())?;
    if result.changed.is_empty() {
        let status = client
            .status(&[resource])
            .map_err(|error| error.to_string())?;
        if status
            .first()
            .is_some_and(|status| status.install_state == InstallStateWireV1::Legacy)
        {
            return Err("legacy model data is not manager-owned; import or adopt it explicitly before removal".to_string());
        }
    }
    crate::invalidate_analysis_runtime_status_cache();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_order_and_duplicate_instances_survive_the_snapshot() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("old");
        settings.vocal_cleanup_chain = vec![
            "melband_roformer_denoise_aufr33".to_string(),
            "melband_roformer_dereverb_anvuew".to_string(),
            "melband_roformer_denoise_aufr33".to_string(),
        ];
        let snapshot = validate_audio_processing_profile(&settings).unwrap();
        let ids = snapshot
            .steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.windows(2).all(|pair| pair[0] != pair[1]));
        assert_eq!(
            ids[2..5],
            ["vocal_denoise_1", "vocal_dereverb_2", "vocal_denoise_3"]
        );
    }

    #[test]
    fn harmony_route_never_relabels_residual_as_backing_or_harmony() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("old");
        settings.karaoke_model_id = Some("melband_roformer_harmony".to_string());
        let snapshot = AudioProcessingPlanSnapshot::from_settings(&settings);
        let split = snapshot
            .steps
            .iter()
            .find(|step| step.step_id == "harmony_split")
            .unwrap();
        assert_eq!(
            split.selected_output_roles,
            ["lead_vocal", "vocal_residual"]
        );
        assert!(!split.selected_output_roles.iter().any(|role| {
            matches!(
                role.as_str(),
                "back_vocal" | "backing_vocal" | "harmony_vocal"
            )
        }));
    }

    #[test]
    fn runtime_policy_fails_closed() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("old");
        settings.runtime_policy = "cpu".to_string();
        assert!(validate_audio_processing_profile(&settings).is_err());
    }

    #[test]
    fn studio_audio_catalog_is_adapted_from_runtime_manager() {
        let models = list_audio_models_from_dir(std::env::temp_dir().join(format!(
            "uta-studio-audio-catalog-{}-absent",
            std::process::id()
        )))
        .unwrap()
        .models;
        assert_eq!(models.len(), 5);
        let vocals = models
            .iter()
            .find(|model| model.model_id == "bs_roformer_vocals_ep317")
            .unwrap();
        assert_eq!(vocals.display_name, "BS-RoFormer Vocals EP317");
        assert_eq!(vocals.operation, "separate_vocals");
        assert_eq!(vocals.supported_backends, vec!["vulkan"]);
        assert!(matches!(vocals.state.as_str(), "missing" | "installed"));
        let fetched = get_audio_model_status_from_dir(
            std::env::temp_dir().join(format!(
                "uta-studio-audio-catalog-{}-absent",
                std::process::id()
            )),
            "bs_roformer_vocals_ep317",
        )
        .unwrap();
        assert_eq!(&fetched, vocals);
    }
}
