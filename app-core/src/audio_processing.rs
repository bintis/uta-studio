//! Native audio-processing settings and immutable execution snapshots.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audio_model::{
    AUDIO_CATALOG_SCHEMA_VERSION, AUDIO_CATALOG_VERSION, AudioModelCatalogSummary,
    AudioModelStatus, AudioParameterMap, AudioParameterValue, DEFAULT_BGM_MODEL_ID,
    DEFAULT_VOCAL_MODEL_ID,
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
                selected_output_roles: vec!["lead_vocal".to_string(), "back_vocal".to_string()],
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
    if settings.runtime_policy != "validated_auto" {
        return Err("production audio processing uses validated automatic routing".to_string());
    }
    Ok(AudioProcessingPlanSnapshot::from_settings(settings))
}

const CATALOG_MODELS: &[(&str, &str, &str, &str)] = &[
    (
        "bs_roformer_vocals_ep317",
        "BS-RoFormer Vocals EP317",
        "Vocal extraction",
        "separate_vocals",
    ),
    (
        "melband_roformer_inst_v2",
        "MelBand-RoFormer Inst V2",
        "BGM extraction",
        "separate_instrumental",
    ),
    (
        "melband_roformer_harmony",
        "MelBand-RoFormer Lead / Back",
        "Lead and harmony separation",
        "separate_harmony",
    ),
    (
        "melband_roformer_denoise_aufr33",
        "MelBand-RoFormer Denoise",
        "Vocal or BGM denoise",
        "denoise",
    ),
    (
        "melband_roformer_dereverb_anvuew",
        "MelBand-RoFormer Dereverb",
        "Vocal or BGM dereverb",
        "dereverb",
    ),
];

pub fn list_audio_models() -> Result<AudioModelCatalogSummary, String> {
    list_audio_models_from_dir(crate::cache::models_dir())
}

pub fn list_audio_models_from_dir(
    models_dir: impl AsRef<Path>,
) -> Result<AudioModelCatalogSummary, String> {
    Ok(AudioModelCatalogSummary {
        schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
        models: CATALOG_MODELS
            .iter()
            .map(|entry| audio_model_status_from_disk(models_dir.as_ref(), entry))
            .collect(),
    })
}

pub fn get_audio_model_status(model_id: &str) -> Result<AudioModelStatus, String> {
    get_audio_model_status_from_dir(crate::cache::models_dir(), model_id)
}

pub fn get_audio_model_status_from_dir(
    models_dir: impl AsRef<Path>,
    model_id: &str,
) -> Result<AudioModelStatus, String> {
    CATALOG_MODELS
        .iter()
        .find(|entry| entry.0 == model_id)
        .map(|entry| audio_model_status_from_disk(models_dir.as_ref(), entry))
        .ok_or_else(|| format!("unknown audio model id: {model_id}"))
}

fn audio_model_status_from_disk(
    models_dir: &Path,
    entry: &(&str, &str, &str, &str),
) -> AudioModelStatus {
    let (model_id, display_name, purpose, operation) = *entry;
    let manifest =
        crate::audio_model::audio_model_dir(models_dir, model_id).join("install-manifest.json");
    AudioModelStatus {
        model_id: model_id.to_string(),
        display_name: display_name.to_string(),
        purpose: purpose.to_string(),
        architecture: "roformer".to_string(),
        operation: operation.to_string(),
        runner: "native_roformer".to_string(),
        supported_backends: vec!["vulkan".to_string()],
        license: crate::audio_model::AudioModelLicense {
            status: "review_required".to_string(),
            source_attribution: "Pinned model manifest".to_string(),
            source_page: None,
        },
        estimated_bytes: None,
        state: if manifest.is_file() {
            "installed"
        } else {
            "missing"
        }
        .to_string(),
        files: Vec::new(),
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
    }
}

pub fn install_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let _ = get_audio_model_status(model_id)?;
    crate::vendor::step_download_model(crate::vendor::ModelDownloadTarget::RoFormer, |_| {})?;
    get_audio_model_status(model_id)
}

pub fn reinstall_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    install_audio_model(model_id)
}

pub fn remove_audio_model(model_id: &str) -> Result<(), String> {
    let _ = get_audio_model_status(model_id)?;
    let directory = crate::audio_model::audio_model_dir(crate::cache::models_dir(), model_id);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)
            .map_err(|error| format!("could not remove {}: {error}", directory.display()))?;
    }
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
    fn runtime_policy_fails_closed() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("old");
        settings.runtime_policy = "cpu".to_string();
        assert!(validate_audio_processing_profile(&settings).is_err());
    }
}
