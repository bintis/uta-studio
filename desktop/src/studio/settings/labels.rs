use super::*;
use crate::studio::*;

pub(crate) fn compute_backend_label(value: &str) -> &'static str {
    match value {
        "cuda" => "NVIDIA CUDA",
        "intel" => "Intel Arc",
        _ => "CPU",
    }
}

pub(crate) fn audio_settings(config: &AppConfig) -> app_core::AudioProcessingSettings {
    config.audio_processing.clone().unwrap_or_else(|| {
        app_core::AudioProcessingSettings::from_legacy_separator(config.separator())
    })
}

pub(crate) fn audio_settings_mut(config: &mut AppConfig) -> &mut app_core::AudioProcessingSettings {
    if config.audio_processing.is_none() {
        config.audio_processing = Some(app_core::AudioProcessingSettings::from_legacy_separator(
            config.separator(),
        ));
    }
    config
        .audio_processing
        .as_mut()
        .expect("audio_processing initialized")
}

const DENOISE_MODEL_ID: &str = "melband_roformer_denoise_aufr33";
const DEREVERB_MODEL_ID: &str = "melband_roformer_dereverb_anvuew";

fn cleanup_slot_value(chain: &[String], slot: usize) -> &'static str {
    match chain.get(slot).map(String::as_str) {
        Some(model_id) if model_id.contains("denoise") => DENOISE_MODEL_ID,
        Some(model_id) if model_id.contains("dereverb") => DEREVERB_MODEL_ID,
        _ => "none",
    }
}

pub(crate) fn vocal_postprocess_value(config: &AppConfig, slot: usize) -> &'static str {
    cleanup_slot_value(&audio_settings(config).vocal_cleanup_chain, slot)
}

pub(crate) fn bgm_postprocess_value(config: &AppConfig, slot: usize) -> &'static str {
    cleanup_slot_value(&audio_settings(config).accompaniment_cleanup_chain, slot)
}

pub(crate) fn rewrite_cleanup_slot(chain: &[String], slot: usize, value: &str) -> Vec<String> {
    let mut slots = [
        chain
            .first()
            .filter(|value| app_core::cleanup_model_enabled(value))
            .cloned(),
        chain
            .get(1)
            .filter(|value| app_core::cleanup_model_enabled(value))
            .cloned(),
    ];
    let selected = match value {
        "none" => None,
        DEREVERB_MODEL_ID => Some(DEREVERB_MODEL_ID.to_string()),
        _ => Some(DENOISE_MODEL_ID.to_string()),
    };
    if let Some(model_id) = selected.as_deref() {
        for (index, current) in slots.iter_mut().enumerate() {
            if index != slot && current.as_deref() == Some(model_id) {
                *current = None;
            }
        }
    }
    slots[slot.min(1)] = selected;
    slots
        .into_iter()
        .map(|value| value.unwrap_or_else(|| "none".to_string()))
        .collect()
}

pub(crate) fn vocal_separation_model_id(config: &AppConfig) -> &str {
    match config.separator() {
        "karaoke" => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.vocal_model_id.as_deref())
            .unwrap_or(app_core::DEFAULT_LEGACY_KARAOKE_MODEL_ID),
        "demucs" => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.multistem_model_id.as_deref())
            .unwrap_or("htdemucs_6s"),
        "openvino_demucs" => "openvino_demucs",
        _ => app_core::DEFAULT_LEGACY_KARAOKE_MODEL_ID,
    }
}

pub(crate) fn vocal_separation_label(config: &AppConfig) -> &'static str {
    settings_select_label(
        SettingsSelectKind::Separator,
        vocal_separation_model_id(config),
    )
}

pub(crate) fn asr_engine_label(value: &str) -> &'static str {
    if value == "parakeet" {
        "Parakeet v3 (Experimental)"
    } else {
        "Whisper"
    }
}

pub(crate) fn align_backend_label(value: &str) -> &'static str {
    match value {
        "ctc" => "CTC Forced Alignment",
        "qwen" => "Qwen Forced Alignment",
        "mms_karaoke" => "MMS Karaoke (Japanese)",
        _ => "WhisperX",
    }
}

pub(crate) fn pitch_model_label(value: &str) -> &'static str {
    match value {
        "rmvpe" => "RMVPE",
        _ => "RMVPE",
    }
}

pub(crate) fn settings_select_value(kind: SettingsSelectKind, config: &AppConfig) -> &str {
    match kind {
        SettingsSelectKind::UiLanguage => config.ui_language(),
        SettingsSelectKind::ComputeBackend => config.compute_backend.as_deref().unwrap_or("cpu"),
        SettingsSelectKind::Separator => vocal_separation_model_id(config),
        SettingsSelectKind::SeparatorPreset => separator_preset(config),
        SettingsSelectKind::AsrEngine => config.asr_engine(),
        SettingsSelectKind::WhisperModel => config.whisper_model(),
        SettingsSelectKind::AlignBackend => config.align_backend(),
        SettingsSelectKind::PitchModel => config.pitch_model(),
        SettingsSelectKind::AudioVocalModel => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.vocal_model_id.as_deref())
            .unwrap_or(app_core::DEFAULT_LEGACY_KARAOKE_MODEL_ID),
        SettingsSelectKind::AudioMultistemModel => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.multistem_model_id.as_deref())
            .unwrap_or("htdemucs_6s"),
        SettingsSelectKind::AudioAccompanimentModel => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.accompaniment_model_id.as_deref())
            .unwrap_or(app_core::DEFAULT_BGM_MODEL_ID),
        SettingsSelectKind::AudioKaraokeModel => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.karaoke_model_id.as_deref())
            .unwrap_or("none"),
        SettingsSelectKind::AudioVocalPostprocess1 => vocal_postprocess_value(config, 0),
        SettingsSelectKind::AudioVocalPostprocess2 => vocal_postprocess_value(config, 1),
        SettingsSelectKind::AudioBgmPostprocess1 => bgm_postprocess_value(config, 0),
        SettingsSelectKind::AudioBgmPostprocess2 => bgm_postprocess_value(config, 1),
        SettingsSelectKind::AudioTorchBackend => config
            .audio_processing
            .as_ref()
            .map(|settings| settings.torch_backend.as_str())
            .unwrap_or("torch_cpu"),
        SettingsSelectKind::AudioOnnxBackend => config
            .audio_processing
            .as_ref()
            .map(|settings| settings.onnx_backend.as_str())
            .unwrap_or("onnx_cpu"),
        SettingsSelectKind::AudioPrecisionPolicy => config
            .audio_processing
            .as_ref()
            .map(|settings| settings.precision_policy.as_str())
            .unwrap_or("fp32"),
    }
}

pub(crate) fn settings_select_label(kind: SettingsSelectKind, value: &str) -> &'static str {
    match kind {
        SettingsSelectKind::UiLanguage => match value {
            "en" => "English",
            "zh-CN" => "简体中文",
            "ja" => "日本語",
            _ => "System default",
        },
        SettingsSelectKind::ComputeBackend => compute_backend_label(value),
        SettingsSelectKind::Separator => match value {
            "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317",
            "htdemucs_6s" | "demucs" => "HTDemucs 6-stem",
            "openvino_demucs" => "OpenVINO Demucs v4 (Intel GPU)",
            _ => "Default karaoke (aufr33 + viperx)",
        },
        SettingsSelectKind::SeparatorPreset => match value {
            "memory" => "Memory saver",
            "quality" => "Quality",
            "custom" => "Custom",
            _ => "Balanced",
        },
        SettingsSelectKind::AsrEngine => asr_engine_label(value),
        SettingsSelectKind::WhisperModel => match value {
            "large-v3" => "Large v3",
            "large-v3-turbo" => "Large v3 Turbo",
            "medium" => "Medium",
            "small" => "Small",
            "base" => "Base",
            "tiny" => "Tiny",
            _ => "Large v3",
        },
        SettingsSelectKind::AlignBackend => align_backend_label(value),
        SettingsSelectKind::PitchModel => pitch_model_label(value),
        SettingsSelectKind::AudioVocalModel => match value {
            "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317",
            "melband_roformer_karaoke_aufr33_viperx" => "Default karaoke (aufr33 + viperx)",
            _ => "Default karaoke (aufr33 + viperx)",
        },
        SettingsSelectKind::AudioMultistemModel => match value {
            "htdemucs_6s" => "HTDemucs 6-stem",
            _ => "HTDemucs 6-stem",
        },
        SettingsSelectKind::AudioAccompanimentModel => match value {
            "melband_roformer_inst_v2" => "MelBand-RoFormer Inst V2",
            _ => "MelBand-RoFormer Inst V2",
        },
        SettingsSelectKind::AudioKaraokeModel => match value {
            "uvr_mdxnet_karaoke_2" => "UVR MDX-NET Karaoke 2",
            _ => "Off",
        },
        SettingsSelectKind::AudioVocalPostprocess1
        | SettingsSelectKind::AudioVocalPostprocess2
        | SettingsSelectKind::AudioBgmPostprocess1
        | SettingsSelectKind::AudioBgmPostprocess2 => match value {
            "melband_roformer_denoise_aufr33" => "Denoise",
            "melband_roformer_dereverb_anvuew" => "Dereverb",
            _ => "Off",
        },
        SettingsSelectKind::AudioTorchBackend => match value {
            "torch_cuda" => "PyTorch CUDA",
            "torch_xpu" => "PyTorch XPU",
            _ => "PyTorch CPU",
        },
        SettingsSelectKind::AudioOnnxBackend => match value {
            "openvino_gpu" => "OpenVINO GPU",
            "openvino_cpu" => "OpenVINO CPU",
            "onnx_cuda" => "ONNX CUDA",
            _ => "ONNX CPU",
        },
        SettingsSelectKind::AudioPrecisionPolicy => match value {
            "fp16" => "FP16",
            "bf16" => "BF16",
            "auto" => "Auto",
            _ => "FP32",
        },
    }
}

pub(crate) fn settings_select_options(
    kind: SettingsSelectKind,
    intel_backend: bool,
) -> &'static [(&'static str, &'static str)] {
    match kind {
        SettingsSelectKind::UiLanguage => &[
            ("system", "System default"),
            ("en", "English"),
            ("zh-CN", "简体中文"),
            ("ja", "日本語"),
        ],
        SettingsSelectKind::ComputeBackend => &[
            ("cpu", "CPU"),
            ("cuda", "NVIDIA CUDA"),
            ("intel", "Intel Arc"),
        ],
        SettingsSelectKind::Separator if intel_backend => &[
            (
                "melband_roformer_karaoke_aufr33_viperx",
                "Default karaoke (aufr33 + viperx)",
            ),
            ("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317"),
            ("htdemucs_6s", "HTDemucs 6-stem"),
            ("openvino_demucs", "OpenVINO Demucs v4"),
        ],
        SettingsSelectKind::Separator => &[
            (
                "melband_roformer_karaoke_aufr33_viperx",
                "Default karaoke (aufr33 + viperx)",
            ),
            ("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317"),
            ("htdemucs_6s", "HTDemucs 6-stem"),
        ],
        SettingsSelectKind::SeparatorPreset => &[
            ("balanced", "Balanced · recommended"),
            ("memory", "Memory saver · lower peak usage"),
            ("quality", "Quality · slower, more context"),
        ],
        SettingsSelectKind::AsrEngine => &[
            ("whisper", "Whisper"),
            ("parakeet", "Parakeet v3 (Experimental)"),
        ],
        SettingsSelectKind::WhisperModel => &[
            ("large-v3", "Large v3"),
            ("large-v3-turbo", "Large v3 Turbo"),
            ("medium", "Medium"),
            ("small", "Small"),
            ("base", "Base"),
            ("tiny", "Tiny"),
        ],
        SettingsSelectKind::AlignBackend => &[
            ("whisperx", "WhisperX"),
            ("ctc", "CTC Forced Alignment"),
            ("qwen", "Qwen Forced Alignment"),
            ("mms_karaoke", "MMS Karaoke (Japanese)"),
        ],
        SettingsSelectKind::PitchModel => &[("rmvpe", "RMVPE")],
        SettingsSelectKind::AudioVocalModel => &[
            (
                "melband_roformer_karaoke_aufr33_viperx",
                "Default karaoke (aufr33 + viperx)",
            ),
            ("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317"),
        ],
        SettingsSelectKind::AudioMultistemModel => &[("htdemucs_6s", "HTDemucs 6-stem")],
        SettingsSelectKind::AudioAccompanimentModel => {
            &[("melband_roformer_inst_v2", "MelBand-RoFormer Inst V2")]
        }
        SettingsSelectKind::AudioKaraokeModel => &[
            ("none", "Off"),
            ("uvr_mdxnet_karaoke_2", "UVR MDX-NET Karaoke 2"),
        ],
        SettingsSelectKind::AudioVocalPostprocess1
        | SettingsSelectKind::AudioVocalPostprocess2
        | SettingsSelectKind::AudioBgmPostprocess1
        | SettingsSelectKind::AudioBgmPostprocess2 => &[
            ("none", "Off"),
            ("melband_roformer_denoise_aufr33", "Denoise"),
            ("melband_roformer_dereverb_anvuew", "Dereverb"),
        ],
        SettingsSelectKind::AudioTorchBackend => &[
            ("torch_cpu", "PyTorch CPU"),
            ("torch_xpu", "PyTorch XPU"),
            ("torch_cuda", "PyTorch CUDA"),
        ],
        SettingsSelectKind::AudioOnnxBackend => &[
            ("onnx_cpu", "ONNX CPU"),
            ("openvino_gpu", "OpenVINO GPU"),
            ("openvino_cpu", "OpenVINO CPU"),
            ("onnx_cuda", "ONNX CUDA"),
        ],
        SettingsSelectKind::AudioPrecisionPolicy => &[
            ("fp32", "FP32"),
            ("fp16", "FP16"),
            ("bf16", "BF16"),
            ("auto", "Auto"),
        ],
    }
}

pub(crate) fn separator_preset(config: &AppConfig) -> &'static str {
    match config.separator() {
        "karaoke"
            if config.separator_segment_size.is_none()
                && config.separator_overlap() == 8
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "balanced"
        }
        "karaoke"
            if config.separator_segment_size == Some(128)
                && config.separator_overlap() == 4
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "memory"
        }
        "karaoke"
            if config.separator_segment_size == Some(512)
                && config.separator_overlap() == 16
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 95 =>
        {
            "quality"
        }
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 25 => "balanced",
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 15 => "memory",
        "demucs" if config.demucs_shifts() == 2 && config.demucs_overlap_pct() == 50 => "quality",
        "openvino_demucs" => "balanced",
        _ => "custom",
    }
}

pub(crate) fn apply_separator_preset(config: &mut AppConfig, preset: &str) {
    match (config.separator(), preset) {
        ("karaoke", "balanced") => {
            config.separator_segment_size = None;
            config.separator_overlap = None;
            config.separator_batch_size = None;
            config.separator_normalization_pct = None;
        }
        ("karaoke", "memory") => {
            config.separator_segment_size = Some(128);
            config.separator_overlap = Some(4);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(90);
        }
        ("karaoke", "quality") => {
            config.separator_segment_size = Some(512);
            config.separator_overlap = Some(16);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(95);
        }
        ("demucs", "balanced") => {
            config.demucs_shifts = None;
            config.demucs_overlap_pct = None;
        }
        ("demucs", "memory") => {
            config.demucs_shifts = Some(1);
            config.demucs_overlap_pct = Some(15);
        }
        ("demucs", "quality") => {
            config.demucs_shifts = Some(2);
            config.demucs_overlap_pct = Some(50);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{DEREVERB_MODEL_ID, rewrite_cleanup_slot};

    #[test]
    fn cleanup_slots_preserve_an_off_first_slot() {
        let chain = rewrite_cleanup_slot(&[], 1, DEREVERB_MODEL_ID);
        assert_eq!(chain, vec!["none", DEREVERB_MODEL_ID]);
        assert_eq!(
            rewrite_cleanup_slot(&chain, 1, "none"),
            vec!["none", "none"]
        );
    }
}
