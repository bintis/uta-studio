use super::*;
use crate::studio::*;

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
    config
        .audio_processing
        .as_ref()
        .and_then(|settings| settings.vocal_model_id.as_deref())
        .unwrap_or(app_core::DEFAULT_VOCAL_MODEL_ID)
}

pub(crate) fn vocal_separation_label(config: &AppConfig) -> &'static str {
    settings_select_label(
        SettingsSelectKind::Separator,
        vocal_separation_model_id(config),
    )
}

pub(crate) fn asr_engine_label(_value: &str) -> &'static str {
    "FireRed + Qwen transcript fusion"
}

pub(crate) fn align_backend_label(_value: &str) -> &'static str {
    "Qwen3 Forced Aligner"
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
        SettingsSelectKind::Separator => vocal_separation_model_id(config),
        SettingsSelectKind::AsrEngine => config.asr_engine(),
        SettingsSelectKind::WhisperModel => config.whisper_model(),
        SettingsSelectKind::AlignBackend => config.align_backend(),
        SettingsSelectKind::PitchModel => config.pitch_model(),
        SettingsSelectKind::AnalysisTarget => config.analysis_default_target().as_str(),
        SettingsSelectKind::AudioVocalModel => config
            .audio_processing
            .as_ref()
            .and_then(|settings| settings.vocal_model_id.as_deref())
            .unwrap_or(app_core::DEFAULT_VOCAL_MODEL_ID),
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
        SettingsSelectKind::Separator => match value {
            "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317",
            _ => "BS-RoFormer Vocals EP317",
        },
        SettingsSelectKind::AsrEngine => asr_engine_label(value),
        SettingsSelectKind::WhisperModel => "Qwen3-ASR-1.7B",
        SettingsSelectKind::AlignBackend => align_backend_label(value),
        SettingsSelectKind::PitchModel => pitch_model_label(value),
        SettingsSelectKind::AnalysisTarget => match value {
            "transcript" => "Transcript",
            "alignment" => "Alignment",
            "pitch_evidence" => "Pitch evidence",
            "instrumental" => "Instrumental",
            _ => "Full candidate chart",
        },
        SettingsSelectKind::AudioVocalModel => match value {
            "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317",
            _ => "BS-RoFormer Vocals EP317",
        },
        SettingsSelectKind::AudioAccompanimentModel => match value {
            "melband_roformer_inst_v2" => "MelBand-RoFormer Inst V2",
            _ => "MelBand-RoFormer Inst V2",
        },
        SettingsSelectKind::AudioKaraokeModel => match value {
            "melband_roformer_harmony" => "MelBand-RoFormer Lead Isolation",
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
        SettingsSelectKind::Separator if intel_backend => {
            &[("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317")]
        }
        SettingsSelectKind::Separator => {
            &[("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317")]
        }
        SettingsSelectKind::AsrEngine => {
            &[("transcript_fusion", "FireRed + Qwen transcript fusion")]
        }
        SettingsSelectKind::WhisperModel => &[("qwen3_asr_1_7b", "Qwen3-ASR-1.7B")],
        SettingsSelectKind::AlignBackend => &[("qwen3_forced_aligner", "Qwen3 Forced Aligner")],
        SettingsSelectKind::PitchModel => &[("rmvpe", "RMVPE")],
        SettingsSelectKind::AnalysisTarget => &[
            ("full_candidate", "Full candidate chart"),
            ("transcript", "Transcript"),
            ("alignment", "Alignment"),
            ("pitch_evidence", "Pitch evidence"),
            ("instrumental", "Instrumental"),
        ],
        SettingsSelectKind::AudioVocalModel => {
            &[("bs_roformer_vocals_ep317", "BS-RoFormer Vocals EP317")]
        }
        SettingsSelectKind::AudioAccompanimentModel => {
            &[("melband_roformer_inst_v2", "MelBand-RoFormer Inst V2")]
        }
        SettingsSelectKind::AudioKaraokeModel => &[
            ("none", "Off"),
            (
                "melband_roformer_harmony",
                "MelBand-RoFormer Lead Isolation",
            ),
        ],
        SettingsSelectKind::AudioVocalPostprocess1
        | SettingsSelectKind::AudioVocalPostprocess2
        | SettingsSelectKind::AudioBgmPostprocess1
        | SettingsSelectKind::AudioBgmPostprocess2 => &[
            ("none", "Off"),
            ("melband_roformer_denoise_aufr33", "Denoise"),
            ("melband_roformer_dereverb_anvuew", "Dereverb"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEREVERB_MODEL_ID, SettingsSelectKind, rewrite_cleanup_slot, settings_select_options,
    };

    #[test]
    fn cleanup_slots_preserve_an_off_first_slot() {
        let chain = rewrite_cleanup_slot(&[], 1, DEREVERB_MODEL_ID);
        assert_eq!(chain, vec!["none", DEREVERB_MODEL_ID]);
        assert_eq!(
            rewrite_cleanup_slot(&chain, 1, "none"),
            vec!["none", "none"]
        );
    }

    #[test]
    fn production_single_provider_kinds_remain_identifiable_as_fixed_rows() {
        for kind in [
            SettingsSelectKind::AudioVocalModel,
            SettingsSelectKind::AudioAccompanimentModel,
            SettingsSelectKind::WhisperModel,
            SettingsSelectKind::AlignBackend,
            SettingsSelectKind::PitchModel,
        ] {
            assert_eq!(settings_select_options(kind, false).len(), 1);
        }
    }
}
