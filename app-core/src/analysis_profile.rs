//! Snapshot of the model/algorithm/device knobs that shape an Analysis Run.
//!
//! Phase 1 scope only: a serializable snapshot type so `AnalysisPlan`/
//! `AnalysisRequest` have something concrete to embed and history rows have
//! something to persist (docs/analysis-dag-redesign.md §12). Wiring this to
//! the real global/song/run parameter inheritance chain (phase plan §8.4) is
//! later-phase work; this does not read or replace any existing settings
//! storage yet.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisProfileSnapshot {
    pub separator: String,
    pub alignment_backend: String,
    pub asr_engine: String,
    pub requested_device: String,
    #[serde(default)]
    pub language_override: Option<String>,
}

impl Default for AnalysisProfileSnapshot {
    fn default() -> Self {
        Self {
            separator: "native_workflow".to_string(),
            alignment_backend: "qwen3_forced_aligner".to_string(),
            asr_engine: "transcript_fusion".to_string(),
            requested_device: "auto".to_string(),
            language_override: None,
        }
    }
}

impl AnalysisProfileSnapshot {
    /// Tier 1 (Global Defaults) of the three-tier chain, built from the real
    /// `AppConfig` -- as opposed to `::default()`'s hardcoded stand-ins,
    /// which don't reflect what the user actually has configured globally.
    /// Used as the preview/execution fallback whenever a song has no saved
    /// profile (tier 2) and no run override (tier 3) for a given field.
    pub fn from_app_config(config: &crate::config::AppConfig, file_hash: &str) -> Self {
        Self {
            separator: config.separator().to_string(),
            alignment_backend: config.align_backend().to_string(),
            asr_engine: config.asr_engine().to_string(),
            requested_device: "auto".to_string(),
            language_override: config.language_override(file_hash).map(str::to_string),
        }
    }

    fn field(&self, field: ProfileField) -> &str {
        match field {
            ProfileField::Separator => &self.separator,
            ProfileField::AsrEngine => &self.asr_engine,
            ProfileField::AlignmentBackend => &self.alignment_backend,
        }
    }
}

/// One of the three profile-controlled pipeline knobs (phase plan §7.4's
/// `selected_stage_parameter` mapping: `stems.separate` -> Separator,
/// `lyrics.transcribe` -> AsrEngine, `lyrics.align` -> AlignmentBackend).
/// Every other node has no profile-controlled parameter at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    Separator,
    AsrEngine,
    AlignmentBackend,
}

/// Which tier of the three-tier chain (phase plan §8.4: Global Defaults ->
/// Song Profile Overrides -> One-run Overrides) actually won for a given
/// field -- surfaced by the inspector's PARAMETER SOURCE fact
/// (`desktop/src/studio/analysis.rs`) so preview and reality use the same
/// resolution logic and can never disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    RunOverride,
    SongProfile,
    GlobalDefault,
}

pub struct ProfileFieldResolution {
    pub value: String,
    pub source: ProfileSource,
}

/// The one real resolution rule behind both `process_song` (real execution)
/// and the Node Inspector's PARAMETER SOURCE fact (preview): a Run override
/// (set only for the one run it was configured for, see
/// `configure_analysis_node_for_run`) wins over a saved Song Profile, which
/// wins over the real global `AppConfig` default.
pub fn resolve_profile_field(
    field: ProfileField,
    global: &AnalysisProfileSnapshot,
    song: Option<&AnalysisProfileSnapshot>,
    run_override: Option<&str>,
) -> ProfileFieldResolution {
    if let Some(value) = run_override {
        return ProfileFieldResolution {
            value: value.to_string(),
            source: ProfileSource::RunOverride,
        };
    }
    if let Some(song) = song {
        return ProfileFieldResolution {
            value: song.field(field).to_string(),
            source: ProfileSource::SongProfile,
        };
    }
    ProfileFieldResolution {
        value: global.field(field).to_string(),
        source: ProfileSource::GlobalDefault,
    }
}

/// Middle tier of the three-tier inheritance chain (phase plan §8.4):
/// Global Defaults -> Song Profile Overrides -> One-run Overrides. Persists
/// a per-song override; absence means "inherit the global defaults."
pub fn set_song_analysis_profile(
    file_hash: &str,
    snapshot: &AnalysisProfileSnapshot,
) -> Result<(), String> {
    let json = serde_json::to_string(snapshot).map_err(|e| e.to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    crate::library_db::song_analysis_profile_set(file_hash, &json, now_ms)
        .map_err(|e| e.to_string())
}

pub fn get_song_analysis_profile(file_hash: &str) -> Option<AnalysisProfileSnapshot> {
    let json = crate::library_db::song_analysis_profile_get(file_hash).ok()??;
    serde_json::from_str(&json).ok()
}

/// Removes the song-level override so the song falls back to global
/// defaults again. Never touches global settings or any other song.
pub fn reset_song_analysis_profile(file_hash: &str) -> Result<(), String> {
    crate::library_db::song_analysis_profile_delete(file_hash).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_round_trips_through_json() {
        let snapshot = AnalysisProfileSnapshot {
            separator: "roformer".to_string(),
            alignment_backend: "qwen3_forced_aligner".to_string(),
            asr_engine: "qwen3_asr_1_7b".to_string(),
            requested_device: "auto".to_string(),
            language_override: Some("ja".to_string()),
        };
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let restored: AnalysisProfileSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snapshot, restored);
    }

    /// See `library_db::reconnect_for_test` -- shared across every test
    /// module in the crate that needs real SQL (including
    /// `analysis_artifact`'s tests), so isolation holds crate-wide, not
    /// just within this module.
    fn isolated_test_db(label: &str) -> std::sync::MutexGuard<'static, ()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "uta-studio-profile-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        crate::library_db::reconnect_for_test(&dir)
    }

    #[test]
    fn song_profile_persists_and_is_retrievable() {
        let _guard = isolated_test_db("persist");
        let snapshot = AnalysisProfileSnapshot {
            separator: "roformer".to_string(),
            ..AnalysisProfileSnapshot::default()
        };
        set_song_analysis_profile("songA", &snapshot).unwrap();
        let loaded = get_song_analysis_profile("songA").unwrap();
        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn song_profile_only_affects_the_named_song() {
        let _guard = isolated_test_db("scoped");
        let snapshot = AnalysisProfileSnapshot {
            separator: "roformer".to_string(),
            ..AnalysisProfileSnapshot::default()
        };
        set_song_analysis_profile("songA", &snapshot).unwrap();
        assert!(get_song_analysis_profile("songB").is_none());
    }

    fn config_with(
        separator: &str,
        asr_engine: &str,
        align_backend: &str,
    ) -> crate::config::AppConfig {
        crate::config::AppConfig {
            separator: Some(separator.to_string()),
            asr_engine: Some(asr_engine.to_string()),
            align_backend: Some(align_backend.to_string()),
            ..crate::config::AppConfig::default()
        }
    }

    #[test]
    fn from_app_config_normalizes_old_values_to_native_defaults() {
        let config = config_with("old", "old", "old");
        let snapshot = AnalysisProfileSnapshot::from_app_config(&config, "songA");
        assert_eq!(snapshot.separator, "native_workflow");
        assert_eq!(snapshot.asr_engine, "transcript_fusion");
        assert_eq!(snapshot.alignment_backend, "qwen3_forced_aligner");
    }

    #[test]
    fn from_app_config_picks_up_the_per_song_language_override() {
        let mut config = config_with(
            "native_workflow",
            "transcript_fusion",
            "qwen3_forced_aligner",
        );
        config.set_language_override("songA".to_string(), "ja".to_string());
        let snapshot = AnalysisProfileSnapshot::from_app_config(&config, "songA");
        assert_eq!(snapshot.language_override.as_deref(), Some("ja"));
        let other_song = AnalysisProfileSnapshot::from_app_config(&config, "songB");
        assert_eq!(other_song.language_override, None);
    }

    #[test]
    fn resolve_profile_field_prefers_run_override_over_song_over_global() {
        let global = AnalysisProfileSnapshot {
            separator: "native_workflow".to_string(),
            ..AnalysisProfileSnapshot::default()
        };
        let song = AnalysisProfileSnapshot {
            separator: "roformer".to_string(),
            ..AnalysisProfileSnapshot::default()
        };

        let global_only = resolve_profile_field(ProfileField::Separator, &global, None, None);
        assert_eq!(global_only.value, "native_workflow");
        assert_eq!(global_only.source, ProfileSource::GlobalDefault);

        let with_song = resolve_profile_field(ProfileField::Separator, &global, Some(&song), None);
        assert_eq!(with_song.value, "roformer");
        assert_eq!(with_song.source, ProfileSource::SongProfile);

        let with_run_override = resolve_profile_field(
            ProfileField::Separator,
            &global,
            Some(&song),
            Some("original_mix"),
        );
        assert_eq!(with_run_override.value, "original_mix");
        assert_eq!(with_run_override.source, ProfileSource::RunOverride);
    }

    #[test]
    fn resolve_profile_field_reads_the_field_matching_the_variant() {
        let global = AnalysisProfileSnapshot {
            separator: "native_workflow".to_string(),
            asr_engine: "transcript_fusion".to_string(),
            alignment_backend: "qwen3_forced_aligner".to_string(),
            ..AnalysisProfileSnapshot::default()
        };
        assert_eq!(
            resolve_profile_field(ProfileField::AsrEngine, &global, None, None).value,
            "transcript_fusion"
        );
        assert_eq!(
            resolve_profile_field(ProfileField::AlignmentBackend, &global, None, None).value,
            "qwen3_forced_aligner"
        );
    }

    #[test]
    fn reset_removes_the_override_and_falls_back_to_none() {
        let _guard = isolated_test_db("reset");
        let snapshot = AnalysisProfileSnapshot::default();
        set_song_analysis_profile("songA", &snapshot).unwrap();
        assert!(get_song_analysis_profile("songA").is_some());
        reset_song_analysis_profile("songA").unwrap();
        assert!(get_song_analysis_profile("songA").is_none());
    }
}
