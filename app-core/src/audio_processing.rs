//! Persistent audio-processing settings and the immutable run snapshot.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audio_model::{
    AUDIO_CATALOG_SCHEMA_VERSION, AUDIO_CATALOG_VERSION, AudioModelCatalogSummary,
    AudioModelStatus, AudioParameterMap, AudioParameterValue, DEFAULT_LEGACY_KARAOKE_MODEL_ID,
};

#[allow(dead_code)]
const PLACEHOLDER_HASHES: &[&str] = &["REPLACE_WITH_VERIFIED_FULL_SHA256", "TODO", "UNKNOWN", ""];

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub struct AudioProcessingSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal_model_id: Option<String>,
    #[serde(default)]
    pub vocal_cleanup_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accompaniment_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub karaoke_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multistem_model_id: Option<String>,
    #[serde(default)]
    pub common_overrides: AudioParameterMap,
    #[serde(default)]
    pub per_model_overrides: BTreeMap<String, AudioParameterMap>,
    #[serde(default = "default_torch_backend")]
    pub torch_backend: String,
    #[serde(default = "default_onnx_backend")]
    pub onnx_backend: String,
    #[serde(default = "default_precision")]
    pub precision_policy: String,
    #[serde(default = "default_memory_policy")]
    pub memory_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_profile: Option<String>,
}

fn default_torch_backend() -> String {
    "torch_cpu".to_string()
}

fn default_onnx_backend() -> String {
    "onnx_cpu".to_string()
}

fn default_precision() -> String {
    "fp32".to_string()
}

fn default_memory_policy() -> String {
    "normal".to_string()
}

impl AudioProcessingSettings {
    pub fn from_legacy_separator(separator: &str) -> Self {
        let (legacy_profile, karaoke, vocal, accompaniment, multistem) = match separator {
            "demucs" => (
                Some("legacy_htdemucs".to_string()),
                None,
                None,
                None,
                Some("htdemucs_6s".to_string()),
            ),
            "openvino_demucs" => (
                Some("legacy_openvino_demucs".to_string()),
                None,
                None,
                None,
                None,
            ),
            _ => (
                Some("legacy_karaoke_roformer".to_string()),
                None,
                Some(DEFAULT_LEGACY_KARAOKE_MODEL_ID.to_string()),
                None,
                None,
            ),
        };
        Self {
            vocal_model_id: vocal,
            vocal_cleanup_chain: Vec::new(),
            accompaniment_model_id: accompaniment,
            karaoke_model_id: karaoke,
            multistem_model_id: multistem,
            common_overrides: AudioParameterMap::new(),
            per_model_overrides: BTreeMap::new(),
            torch_backend: default_torch_backend(),
            onnx_backend: if separator == "openvino_demucs" {
                "openvino_gpu".to_string()
            } else {
                default_onnx_backend()
            },
            precision_policy: default_precision(),
            memory_policy: default_memory_policy(),
            legacy_profile,
        }
    }

    pub fn derived_legacy_separator(&self) -> &'static str {
        match self.legacy_profile.as_deref() {
            Some("legacy_htdemucs") => "demucs",
            Some("legacy_openvino_demucs") => "openvino_demucs",
            Some("legacy_karaoke_roformer") | None
                if self.multistem_model_id.as_deref() == Some("htdemucs_6s")
                    && self.vocal_model_id.is_none()
                    && self.karaoke_model_id.is_none() =>
            {
                "demucs"
            }
            Some("legacy_karaoke_roformer") | None => "karaoke",
            _ => "karaoke",
        }
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
    pub torch_backend: String,
    pub onnx_backend: String,
    pub precision_policy: String,
    #[serde(default = "default_fallback_policy")]
    pub fallback_policy: String,
}

fn default_fallback_policy() -> String {
    "whole_model_cpu".to_string()
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

impl AudioProcessingPlanSnapshot {
    pub fn from_settings(settings: &AudioProcessingSettings) -> Self {
        let runtime = AudioRuntimeRequest {
            torch_backend: settings.torch_backend.clone(),
            onnx_backend: settings.onnx_backend.clone(),
            precision_policy: settings.precision_policy.clone(),
            fallback_policy: default_fallback_policy(),
        };
        let demucs_only = is_demucs_chart_path(settings);
        let mut plan = if demucs_only {
            demucs_snapshot(settings, runtime)
        } else if let Some(vocal) = settings.vocal_model_id.as_deref() {
            chart_snapshot(vocal, settings, runtime)
        } else {
            chart_snapshot(DEFAULT_LEGACY_KARAOKE_MODEL_ID, settings, runtime)
        };
        if !demucs_only {
            if let Some(model_id) = settings.karaoke_model_id.as_deref() {
                append_karaoke_side_path(&mut plan, model_id, settings);
            }
            if settings.multistem_model_id.is_some() {
                append_multistem_side_path(&mut plan, settings);
            }
        }
        plan
    }
}

pub fn is_demucs_chart_path(settings: &AudioProcessingSettings) -> bool {
    settings.legacy_profile.as_deref() == Some("legacy_htdemucs")
        || (settings.multistem_model_id.as_deref() == Some("htdemucs_6s")
            && settings.vocal_model_id.is_none())
}

fn param(map: &AudioParameterMap, extra: &AudioParameterMap) -> AudioParameterMap {
    let mut merged = map.clone();
    merged.extend(extra.clone());
    merged
}

fn chart_snapshot(
    vocal: &str,
    settings: &AudioProcessingSettings,
    runtime: AudioRuntimeRequest,
) -> AudioProcessingPlanSnapshot {
    let mut vocal_roles = vec!["extracted_vocal".to_string()];
    if settings.accompaniment_model_id.is_none() {
        vocal_roles.push("residual_instrumental".to_string());
    }
    let mut steps = vec![AudioProcessingStep {
        step_id: "extract_vocals".to_string(),
        model_id: vocal.to_string(),
        input: AudioInputReference::SourceMedia,
        selected_output_roles: vocal_roles,
        effective_parameters: param(
            &settings.common_overrides,
            settings
                .per_model_overrides
                .get(vocal)
                .unwrap_or(&AudioParameterMap::new()),
        ),
    }];
    let mut current_step = "extract_vocals".to_string();
    let mut current_role = "extracted_vocal".to_string();
    for model_id in &settings.vocal_cleanup_chain {
        let (step_id, role) = if model_id.contains("denoise") {
            ("denoise_vocals", "clean_audio")
        } else {
            ("dereverb_vocals", "dry_audio")
        };
        steps.push(AudioProcessingStep {
            step_id: step_id.to_string(),
            model_id: model_id.clone(),
            input: AudioInputReference::StepOutput {
                step_id: current_step.clone(),
                role: current_role.clone(),
            },
            selected_output_roles: vec![role.to_string()],
            effective_parameters: param(
                &settings.common_overrides,
                settings
                    .per_model_overrides
                    .get(model_id)
                    .unwrap_or(&AudioParameterMap::new()),
            ),
        });
        current_step = step_id.to_string();
        current_role = role.to_string();
    }
    if let Some(accompaniment) = settings.accompaniment_model_id.as_deref() {
        steps.push(AudioProcessingStep {
            step_id: "extract_accompaniment".to_string(),
            model_id: accompaniment.to_string(),
            input: AudioInputReference::SourceMedia,
            selected_output_roles: vec!["instrumental".to_string()],
            effective_parameters: param(
                &settings.common_overrides,
                settings
                    .per_model_overrides
                    .get(accompaniment)
                    .unwrap_or(&AudioParameterMap::new()),
            ),
        });
    }
    let mut bindings = vec![
        AudioOutputBinding {
            artifact_role: "analysis_vocal".to_string(),
            step_id: current_step.clone(),
            role: current_role.clone(),
            sum: None,
        },
        AudioOutputBinding {
            artifact_role: "vocals".to_string(),
            step_id: current_step,
            role: current_role,
            sum: None,
        },
    ];
    if settings.accompaniment_model_id.is_some() {
        bindings.push(AudioOutputBinding {
            artifact_role: "instrumental".to_string(),
            step_id: "extract_accompaniment".to_string(),
            role: "instrumental".to_string(),
            sum: None,
        });
    } else {
        bindings.push(AudioOutputBinding {
            artifact_role: "instrumental".to_string(),
            step_id: "extract_vocals".to_string(),
            role: "residual_instrumental".to_string(),
            sum: None,
        });
    }
    AudioProcessingPlanSnapshot {
        schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
        steps,
        output_bindings: bindings,
        requested_runtime: runtime,
        profile_id: Some("chart_analysis_hq".to_string()),
    }
}

fn karaoke_side_step(model_id: &str, settings: &AudioProcessingSettings) -> AudioProcessingStep {
    AudioProcessingStep {
        step_id: "extract_karaoke".to_string(),
        model_id: model_id.to_string(),
        input: AudioInputReference::SourceMedia,
        selected_output_roles: vec![
            "karaoke_instrumental".to_string(),
            "extracted_vocal".to_string(),
        ],
        effective_parameters: param(
            &settings.common_overrides,
            settings
                .per_model_overrides
                .get(model_id)
                .unwrap_or(&AudioParameterMap::new()),
        ),
    }
}

fn append_karaoke_side_path(
    plan: &mut AudioProcessingPlanSnapshot,
    model_id: &str,
    settings: &AudioProcessingSettings,
) {
    if plan
        .steps
        .iter()
        .any(|step| step.step_id == "extract_karaoke")
    {
        return;
    }
    plan.steps.push(karaoke_side_step(model_id, settings));
    plan.output_bindings.push(AudioOutputBinding {
        artifact_role: "karaoke_instrumental".to_string(),
        step_id: "extract_karaoke".to_string(),
        role: "karaoke_instrumental".to_string(),
        sum: None,
    });
}

fn append_multistem_side_path(
    plan: &mut AudioProcessingPlanSnapshot,
    settings: &AudioProcessingSettings,
) {
    if plan.steps.iter().any(|step| step.step_id == "separate_6s") {
        return;
    }
    let side = demucs_snapshot(settings, plan.requested_runtime.clone());
    plan.steps.extend(side.steps);
    for binding in side.output_bindings {
        if matches!(
            binding.artifact_role.as_str(),
            "vocals" | "instrumental" | "analysis_vocal"
        ) {
            continue;
        }
        plan.output_bindings.push(binding);
    }
}

fn demucs_snapshot(
    settings: &AudioProcessingSettings,
    runtime: AudioRuntimeRequest,
) -> AudioProcessingPlanSnapshot {
    let model_id = settings
        .multistem_model_id
        .clone()
        .unwrap_or_else(|| "htdemucs_6s".to_string());
    AudioProcessingPlanSnapshot {
        schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
        steps: vec![AudioProcessingStep {
            step_id: "separate_6s".to_string(),
            model_id,
            input: AudioInputReference::SourceMedia,
            selected_output_roles: vec![
                "vocals".to_string(),
                "drums".to_string(),
                "bass".to_string(),
                "guitar".to_string(),
                "piano".to_string(),
                "other".to_string(),
            ],
            effective_parameters: settings.common_overrides.clone(),
        }],
        output_bindings: vec![
            AudioOutputBinding {
                artifact_role: "vocals".to_string(),
                step_id: "separate_6s".to_string(),
                role: "vocals".to_string(),
                sum: None,
            },
            AudioOutputBinding {
                artifact_role: "instrumental".to_string(),
                step_id: "separate_6s".to_string(),
                role: "instrumental".to_string(),
                sum: Some(vec![
                    "drums".to_string(),
                    "bass".to_string(),
                    "guitar".to_string(),
                    "piano".to_string(),
                    "other".to_string(),
                ]),
            },
        ],
        requested_runtime: runtime,
        profile_id: Some("multistem_6s".to_string()),
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
    let mut resolved = BTreeMap::new();
    let defaults = [
        (
            "common.normalizationThreshold",
            AudioParameterValue::Number(0.9),
        ),
        (
            "runtime.precisionPolicy",
            AudioParameterValue::Text(settings.precision_policy.clone()),
        ),
        (
            "runtime.memoryPolicy",
            AudioParameterValue::Text(settings.memory_policy.clone()),
        ),
    ];
    for (key, value) in defaults {
        resolved.insert(
            key.to_string(),
            ResolvedAudioParameter {
                value,
                source: "model_default".to_string(),
            },
        );
    }
    for (key, value) in &settings.common_overrides {
        resolved.insert(
            key.clone(),
            ResolvedAudioParameter {
                value: value.clone(),
                source: "global_settings".to_string(),
            },
        );
    }
    if let Some(song) = song_overrides {
        for (key, value) in song {
            resolved.insert(
                key.clone(),
                ResolvedAudioParameter {
                    value: value.clone(),
                    source: "song_profile".to_string(),
                },
            );
        }
    }
    if let Some(run) = run_overrides {
        for (key, value) in run {
            resolved.insert(
                key.clone(),
                ResolvedAudioParameter {
                    value: value.clone(),
                    source: "run_override".to_string(),
                },
            );
        }
    }
    resolved
}

#[allow(dead_code)]
pub fn is_placeholder_hash(value: &str) -> bool {
    PLACEHOLDER_HASHES.contains(&value) || value.eq_ignore_ascii_case("todo")
}

#[allow(dead_code)]
pub fn sha256_hex_ok(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !is_placeholder_hash(value)
}

pub fn validate_audio_processing_profile(
    settings: &AudioProcessingSettings,
) -> Result<AudioProcessingPlanSnapshot, String> {
    if let Some(model_id) = settings.vocal_model_id.as_deref() {
        validate_model_id(model_id)?;
    }
    if let Some(model_id) = settings.accompaniment_model_id.as_deref() {
        validate_model_id(model_id)?;
    }
    if let Some(model_id) = settings.karaoke_model_id.as_deref() {
        validate_model_id(model_id)?;
    }
    if let Some(model_id) = settings.multistem_model_id.as_deref() {
        validate_model_id(model_id)?;
    }
    for model_id in &settings.vocal_cleanup_chain {
        validate_model_id(model_id)?;
    }
    for key in settings.common_overrides.keys() {
        if key == "overlap" || key == "segment_size" {
            return Err("bare overlap/segment_size parameters are not allowed".to_string());
        }
    }
    Ok(AudioProcessingPlanSnapshot::from_settings(settings))
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    if model_id.contains('/') || model_id.contains(".ckpt") || model_id.contains(".onnx") {
        return Err("audio processing settings must store catalog model IDs".to_string());
    }
    Ok(())
}

const CATALOG_MODELS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "bs_roformer_vocals_ep317",
        "BS-RoFormer Vocals EP317",
        "Vocal extraction",
        "mdxc_bs_roformer",
        "separate_vocals",
        "mdxc_torch",
    ),
    (
        "melband_roformer_inst_v2",
        "MelBand-RoFormer Inst V2",
        "High-quality accompaniment",
        "mdxc_melband_roformer",
        "separate_instrumental",
        "mdxc_torch",
    ),
    (
        "htdemucs_6s",
        "HTDemucs 6-stem",
        "Six-stem separation",
        "demucs",
        "separate_multistem",
        "demucs_torch",
    ),
    (
        "melband_roformer_denoise_aufr33",
        "MelBand-RoFormer Denoise",
        "Vocal denoise",
        "mdxc_melband_roformer",
        "denoise",
        "mdxc_torch",
    ),
    (
        "melband_roformer_dereverb_anvuew",
        "MelBand-RoFormer Dereverb",
        "Vocal dereverb",
        "mdxc_melband_roformer",
        "dereverb",
        "mdxc_torch",
    ),
    (
        "uvr_mdxnet_karaoke_2",
        "UVR MDX-NET Karaoke 2",
        "Karaoke accompaniment",
        "mdx_onnx",
        "separate_karaoke",
        "mdx_onnx",
    ),
    (
        "melband_roformer_karaoke_aufr33_viperx",
        "MelBand-RoFormer Karaoke (aufr33 + viperx)",
        "Default analysis karaoke (lead vocal isolation)",
        "mdxc_melband_roformer",
        "separate_vocals",
        "mdxc_torch",
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
    entry: &(&str, &str, &str, &str, &str, &str),
) -> AudioModelStatus {
    let (model_id, display_name, purpose, architecture, operation, runner) = *entry;
    let directory = crate::audio_model::audio_model_dir(models_dir, model_id);
    let manifest = directory.join("install-manifest.json");
    let state = if manifest.is_file() {
        "installed"
    } else {
        "missing"
    };
    AudioModelStatus {
        model_id: model_id.to_string(),
        display_name: display_name.to_string(),
        purpose: purpose.to_string(),
        architecture: architecture.to_string(),
        operation: operation.to_string(),
        runner: runner.to_string(),
        supported_backends: match runner {
            "mdx_onnx" => vec![
                "openvino_gpu".to_string(),
                "openvino_cpu".to_string(),
                "onnx_cpu".to_string(),
            ],
            _ => vec![
                "torch_cuda".to_string(),
                "torch_xpu".to_string(),
                "torch_cpu".to_string(),
            ],
        },
        license: crate::audio_model::AudioModelLicense {
            status: "review_recorded".to_string(),
            source_attribution: "UVR public model catalog".to_string(),
            source_page: None,
        },
        estimated_bytes: None,
        state: state.to_string(),
        files: Vec::new(),
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
    }
}

pub fn list_audio_models_from_python(
    models_dir: impl AsRef<Path>,
) -> Result<AudioModelCatalogSummary, String> {
    list_audio_models_from_dir(models_dir)
}

pub fn install_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    validate_model_id(model_id)?;
    run_audio_model_setup("install", model_id)?;
    get_audio_model_status(model_id)
}

pub fn reinstall_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let _ = remove_audio_model(model_id);
    install_audio_model(model_id)
}

pub fn remove_audio_model(model_id: &str) -> Result<(), String> {
    validate_model_id(model_id)?;
    let directory = crate::audio_model::audio_model_dir(crate::cache::models_dir(), model_id);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)
            .map_err(|error| format!("could not remove {}: {error}", directory.display()))?;
    }
    Ok(())
}

fn run_audio_model_setup(action: &str, model_id: &str) -> Result<(), String> {
    let python = crate::vendor::python_path();
    if !python.is_file() {
        return Err(
            "analysis runtime is not installed; use Settings > Models & runtime".to_string(),
        );
    }
    let script = crate::vendor::analyzer_dir().join("model_setup.py");
    let output = std::process::Command::new(python)
        .arg(script)
        .arg("--models-dir")
        .arg(crate::cache::models_dir())
        .arg("--backend")
        .arg("cpu")
        .arg("--engine")
        .arg("whisper")
        .arg("--whisper-model")
        .arg("tiny")
        .arg("--separator")
        .arg("karaoke")
        .arg("--align-backend")
        .arg("whisperx")
        .arg("--target")
        .arg("audio_model")
        .arg("--audio-model-id")
        .arg(model_id)
        .arg("--audio-model-action")
        .arg(action)
        .output()
        .map_err(|error| format!("could not start audio model setup: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_karaoke_round_trips() {
        let settings = AudioProcessingSettings::from_legacy_separator("karaoke");
        assert_eq!(settings.derived_legacy_separator(), "karaoke");
        assert_eq!(
            settings.legacy_profile.as_deref(),
            Some("legacy_karaoke_roformer")
        );
        assert_eq!(
            settings.vocal_model_id.as_deref(),
            Some(DEFAULT_LEGACY_KARAOKE_MODEL_ID)
        );
        let snapshot = AudioProcessingPlanSnapshot::from_settings(&settings);
        assert_eq!(snapshot.steps.len(), 1);
        assert_eq!(snapshot.steps[0].model_id, DEFAULT_LEGACY_KARAOKE_MODEL_ID);
        assert!(
            snapshot
                .output_bindings
                .iter()
                .any(|binding| binding.artifact_role == "instrumental")
        );
    }

    #[test]
    fn snapshot_is_frozen_from_settings() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("karaoke");
        settings.vocal_model_id = Some("bs_roformer_vocals_ep317".to_string());
        settings.accompaniment_model_id = Some("melband_roformer_inst_v2".to_string());
        settings.vocal_cleanup_chain = vec![
            "melband_roformer_denoise_aufr33".to_string(),
            "melband_roformer_dereverb_anvuew".to_string(),
        ];
        let first = AudioProcessingPlanSnapshot::from_settings(&settings);
        settings.vocal_cleanup_chain.clear();
        let second = AudioProcessingPlanSnapshot::from_settings(&settings);
        assert_eq!(first.steps.len(), 4);
        assert_eq!(second.steps.len(), 2);
        assert_ne!(
            first
                .steps
                .iter()
                .map(|s| s.model_id.as_str())
                .collect::<Vec<_>>(),
            second
                .steps
                .iter()
                .map(|s| s.model_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn karaoke_is_a_side_path_and_does_not_replace_the_chart_chain() {
        let mut settings = AudioProcessingSettings::from_legacy_separator("karaoke");
        settings.vocal_model_id = Some("bs_roformer_vocals_ep317".to_string());
        settings.accompaniment_model_id = Some("melband_roformer_inst_v2".to_string());
        settings.vocal_cleanup_chain = vec![
            "melband_roformer_denoise_aufr33".to_string(),
            "melband_roformer_dereverb_anvuew".to_string(),
        ];
        settings.karaoke_model_id = Some("melband_roformer_karaoke_aufr33_viperx".to_string());
        let snapshot = AudioProcessingPlanSnapshot::from_settings(&settings);
        let step_ids: Vec<&str> = snapshot
            .steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect();
        assert_eq!(
            step_ids,
            vec![
                "extract_vocals",
                "denoise_vocals",
                "dereverb_vocals",
                "extract_accompaniment",
                "extract_karaoke",
            ]
        );
        assert!(
            snapshot
                .output_bindings
                .iter()
                .any(|binding| binding.artifact_role == "analysis_vocal"
                    && binding.step_id == "dereverb_vocals")
        );
        assert!(
            snapshot
                .output_bindings
                .iter()
                .any(|binding| binding.artifact_role == "karaoke_instrumental")
        );
        assert!(!snapshot.output_bindings.iter().any(
            |binding| binding.artifact_role == "vocals" && binding.step_id == "extract_karaoke"
        ));
    }

    #[test]
    fn sha256_helpers_reject_placeholders() {
        assert!(sha256_hex_ok(
            "bf32e15105a09c0f7dddd2b67346146334d6f3ecb399ed7638eba2ab07cbf5f4"
        ));
        assert!(is_placeholder_hash("TODO"));
        assert!(!sha256_hex_ok("TODO"));
    }

    #[test]
    fn rejects_checkpoint_filenames() {
        let settings = AudioProcessingSettings {
            vocal_model_id: Some("model_bs_roformer_ep_317_sdr_12.9755.ckpt".to_string()),
            ..AudioProcessingSettings::default()
        };
        assert!(validate_audio_processing_profile(&settings).is_err());
    }

    #[test]
    fn parameter_sources_follow_global_song_run() {
        let mut settings = AudioProcessingSettings::default();
        settings.common_overrides.insert(
            "common.normalizationThreshold".to_string(),
            AudioParameterValue::Number(0.8),
        );
        let mut song = AudioParameterMap::new();
        song.insert(
            "mdxc.overlapCount".to_string(),
            AudioParameterValue::Integer(4),
        );
        let mut run = AudioParameterMap::new();
        run.insert(
            "mdxc.overlapCount".to_string(),
            AudioParameterValue::Integer(6),
        );
        let resolved = preview_effective_audio_params(&settings, Some(&song), Some(&run));
        assert_eq!(
            resolved["common.normalizationThreshold"].source,
            "global_settings"
        );
        assert_eq!(resolved["mdxc.overlapCount"].source, "run_override");
    }
}
