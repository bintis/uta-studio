use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Sparse Studio-owned per-song analysis intent. Backend/model/runtime
/// selection never lives in this record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct SongAnalysisProfile {
    pub analysis_experience: crate::analysis_experience::AnalysisExperienceOverride,
}

pub fn set_song_analysis_profile(
    file_hash: &str,
    profile: &SongAnalysisProfile,
) -> Result<(), String> {
    let json = serde_json::to_string(profile).map_err(|error| error.to_string())?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    crate::library_db::song_analysis_profile_set(file_hash, &json, now_ms)
        .map_err(|error| error.to_string())
}

pub fn get_song_analysis_profile(file_hash: &str) -> Option<SongAnalysisProfile> {
    let json = crate::library_db::song_analysis_profile_get(file_hash).ok()??;
    serde_json::from_str(&json).ok()
}

pub fn reset_song_analysis_profile(file_hash: &str) -> Result<(), String> {
    crate::library_db::song_analysis_profile_delete(file_hash).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_concrete_fields_are_ignored_while_product_intent_is_preserved() {
        let profile: SongAnalysisProfile = serde_json::from_value(serde_json::json!({
            "separator": "old-separator",
            "asr_engine": "old-asr",
            "requested_device": "old-device",
            "analysis_experience": {"quality_profile": "maximum"}
        }))
        .unwrap();
        assert_eq!(
            profile.analysis_experience.quality_profile,
            Some(crate::AnalysisQualityProfile::Maximum)
        );
        let serialized = serde_json::to_value(profile).unwrap();
        assert!(serialized.get("separator").is_none());
        assert!(serialized.get("asr_engine").is_none());
        assert!(serialized.get("requested_device").is_none());
    }
}
