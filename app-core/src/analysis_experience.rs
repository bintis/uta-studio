//! Studio-owned product preferences for analysis behavior.
//!
//! These settings describe user intent. They deliberately do not contain
//! checkpoint paths, worker executables, backend overrides, or provider IDs:
//! Engine v1 cannot truthfully honor per-capability provider preferences yet.

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

/// Current serialized Studio analysis-settings schema.
pub const ANALYSIS_EXPERIENCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AnalysisQualityProfile {
    Fast,
    #[default]
    Balanced,
    Maximum,
}

impl<'de> Deserialize<'de> for AnalysisQualityProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "fast" => Self::Fast,
            "maximum" => Self::Maximum,
            _ => Self::Balanced,
        })
    }
}

impl AnalysisQualityProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::Maximum => "maximum",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AnalysisDefaultTarget {
    #[default]
    FullCandidate,
    Transcript,
    Alignment,
    PitchEvidence,
    Instrumental,
}

impl<'de> Deserialize<'de> for AnalysisDefaultTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "transcript" => Self::Transcript,
            "alignment" => Self::Alignment,
            "pitch_evidence" => Self::PitchEvidence,
            "instrumental" => Self::Instrumental,
            _ => Self::FullCandidate,
        })
    }
}

impl AnalysisDefaultTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullCandidate => "full_candidate",
            Self::Transcript => "transcript",
            Self::Alignment => "alignment",
            Self::PitchEvidence => "pitch_evidence",
            Self::Instrumental => "instrumental",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutomaticStrategy {
    #[default]
    Automatic,
}

impl<'de> Deserialize<'de> for AutomaticStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let _stored_value = String::deserialize(deserializer)?;
        Ok(Self::Automatic)
    }
}

/// Strategy-only preferences. The actual provider is resolved by Engine and
/// Runtime Manager and is shown separately by the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AnalysisAudioPreferences {
    pub vocal_strategy: AutomaticStrategy,
    pub instrumental_strategy: AutomaticStrategy,
}

impl Default for AnalysisAudioPreferences {
    fn default() -> Self {
        Self {
            vocal_strategy: AutomaticStrategy::Automatic,
            instrumental_strategy: AutomaticStrategy::Automatic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AnalysisLyricsPreferences {
    pub transcription_strategy: AutomaticStrategy,
    pub alignment_strategy: AutomaticStrategy,
}

impl Default for AnalysisLyricsPreferences {
    fn default() -> Self {
        Self {
            transcription_strategy: AutomaticStrategy::Automatic,
            alignment_strategy: AutomaticStrategy::Automatic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AnalysisSingingPreferences {
    pub continuous_pitch_strategy: AutomaticStrategy,
    pub note_boundary_strategy: AutomaticStrategy,
}

impl Default for AnalysisSingingPreferences {
    fn default() -> Self {
        Self {
            continuous_pitch_strategy: AutomaticStrategy::Automatic,
            note_boundary_strategy: AutomaticStrategy::Automatic,
        }
    }
}

fn schema_version() -> u32 {
    ANALYSIS_EXPERIENCE_SCHEMA_VERSION
}

fn enabled() -> bool {
    true
}

/// Versioned global defaults for product-level analysis intent.
///
/// Legacy concrete `AppConfig` fields remain readable during migration and
/// continue to own compatible advanced/cleanup controls for now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AnalysisExperienceSettings {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub quality_profile: AnalysisQualityProfile,
    pub default_target: AnalysisDefaultTarget,
    #[serde(default = "enabled")]
    pub preserve_continuous_pitch: bool,
    /// Enables the Engine-owned symbolic Candidate timing stage. This does not
    /// affect continuous pitch evidence or the Editor's authored-note command.
    #[serde(default)]
    pub enable_quantization: bool,
    pub audio: AnalysisAudioPreferences,
    pub lyrics: AnalysisLyricsPreferences,
    pub singing: AnalysisSingingPreferences,
}

impl Default for AnalysisExperienceSettings {
    fn default() -> Self {
        Self {
            schema_version: ANALYSIS_EXPERIENCE_SCHEMA_VERSION,
            quality_profile: AnalysisQualityProfile::Balanced,
            default_target: AnalysisDefaultTarget::FullCandidate,
            preserve_continuous_pitch: true,
            enable_quantization: false,
            audio: AnalysisAudioPreferences::default(),
            lyrics: AnalysisLyricsPreferences::default(),
            singing: AnalysisSingingPreferences::default(),
        }
    }
}

impl AnalysisExperienceSettings {
    pub fn normalize(&mut self) {
        self.schema_version = ANALYSIS_EXPERIENCE_SCHEMA_VERSION;
    }
}

/// Sparse Song Profile or temporary Run Override values. Every field resolves
/// independently as Run > Song > Global.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AnalysisExperienceOverride {
    pub quality_profile: Option<AnalysisQualityProfile>,
    pub default_target: Option<AnalysisDefaultTarget>,
    pub preserve_continuous_pitch: Option<bool>,
    pub enable_quantization: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AnalysisSettingSource {
    Global,
    Song,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EffectiveAnalysisSetting<T> {
    pub value: T,
    pub source: AnalysisSettingSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EffectiveAnalysisExperience {
    pub quality_profile: EffectiveAnalysisSetting<AnalysisQualityProfile>,
    pub default_target: EffectiveAnalysisSetting<AnalysisDefaultTarget>,
    pub preserve_continuous_pitch: EffectiveAnalysisSetting<bool>,
    pub enable_quantization: EffectiveAnalysisSetting<bool>,
}

fn resolve<T: Copy>(global: T, song: Option<T>, run: Option<T>) -> EffectiveAnalysisSetting<T> {
    if let Some(value) = run {
        EffectiveAnalysisSetting {
            value,
            source: AnalysisSettingSource::Run,
        }
    } else if let Some(value) = song {
        EffectiveAnalysisSetting {
            value,
            source: AnalysisSettingSource::Song,
        }
    } else {
        EffectiveAnalysisSetting {
            value: global,
            source: AnalysisSettingSource::Global,
        }
    }
}

pub fn resolve_analysis_experience(
    global: &AnalysisExperienceSettings,
    song: Option<&AnalysisExperienceOverride>,
    run: Option<&AnalysisExperienceOverride>,
) -> EffectiveAnalysisExperience {
    EffectiveAnalysisExperience {
        quality_profile: resolve(
            global.quality_profile,
            song.and_then(|value| value.quality_profile),
            run.and_then(|value| value.quality_profile),
        ),
        default_target: resolve(
            global.default_target,
            song.and_then(|value| value.default_target),
            run.and_then(|value| value.default_target),
        ),
        preserve_continuous_pitch: resolve(
            global.preserve_continuous_pitch,
            song.and_then(|value| value.preserve_continuous_pitch),
            run.and_then(|value| value.preserve_continuous_pitch),
        ),
        enable_quantization: resolve(
            global.enable_quantization,
            song.and_then(|value| value.enable_quantization),
            run.and_then(|value| value.enable_quantization),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_is_the_product_default() {
        let defaults = AnalysisExperienceSettings::default();
        assert_eq!(defaults.quality_profile, AnalysisQualityProfile::Balanced);
        assert!(defaults.preserve_continuous_pitch);
        assert!(!defaults.enable_quantization);
    }

    #[test]
    fn profiles_and_settings_round_trip() {
        for quality in [
            AnalysisQualityProfile::Fast,
            AnalysisQualityProfile::Balanced,
            AnalysisQualityProfile::Maximum,
        ] {
            let settings = AnalysisExperienceSettings {
                quality_profile: quality,
                default_target: AnalysisDefaultTarget::PitchEvidence,
                preserve_continuous_pitch: false,
                enable_quantization: false,
                ..AnalysisExperienceSettings::default()
            };
            let json = serde_json::to_string(&settings).unwrap();
            assert_eq!(
                serde_json::from_str::<AnalysisExperienceSettings>(&json).unwrap(),
                settings
            );
        }
    }

    #[test]
    fn invalid_persisted_product_values_normalize_safely() {
        let settings: AnalysisExperienceSettings = serde_json::from_value(serde_json::json!({
            "schema_version": 999,
            "quality_profile": "future_profile",
            "default_target": "future_target"
        }))
        .unwrap();
        assert_eq!(settings.quality_profile, AnalysisQualityProfile::Balanced);
        assert_eq!(
            settings.default_target,
            AnalysisDefaultTarget::FullCandidate
        );
        assert!(settings.preserve_continuous_pitch);
        assert!(!settings.enable_quantization);
    }

    #[test]
    fn normalization_preserves_real_engine_quantization_intent() {
        let mut settings = AnalysisExperienceSettings {
            enable_quantization: true,
            ..AnalysisExperienceSettings::default()
        };
        settings.normalize();
        assert!(settings.enable_quantization);
    }

    #[test]
    fn every_migrated_field_resolves_run_over_song_over_global() {
        let global = AnalysisExperienceSettings::default();
        let song = AnalysisExperienceOverride {
            quality_profile: Some(AnalysisQualityProfile::Fast),
            default_target: Some(AnalysisDefaultTarget::Transcript),
            preserve_continuous_pitch: Some(false),
            enable_quantization: Some(false),
        };
        let run = AnalysisExperienceOverride {
            quality_profile: Some(AnalysisQualityProfile::Maximum),
            default_target: Some(AnalysisDefaultTarget::Instrumental),
            preserve_continuous_pitch: Some(true),
            enable_quantization: Some(true),
        };
        let effective = resolve_analysis_experience(&global, Some(&song), Some(&run));
        assert_eq!(
            effective.quality_profile.value,
            AnalysisQualityProfile::Maximum
        );
        assert_eq!(
            effective.default_target.value,
            AnalysisDefaultTarget::Instrumental
        );
        assert!(effective.preserve_continuous_pitch.value);
        assert!(effective.enable_quantization.value);
        assert_eq!(effective.quality_profile.source, AnalysisSettingSource::Run);

        let effective = resolve_analysis_experience(&global, Some(&song), None);
        assert_eq!(
            effective.quality_profile.value,
            AnalysisQualityProfile::Fast
        );
        assert_eq!(
            effective.quality_profile.source,
            AnalysisSettingSource::Song
        );

        let effective = resolve_analysis_experience(&global, None, None);
        assert_eq!(
            effective.quality_profile.value,
            AnalysisQualityProfile::Balanced
        );
        assert_eq!(
            effective.quality_profile.source,
            AnalysisSettingSource::Global
        );
    }
}
