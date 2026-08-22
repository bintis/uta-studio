use serde::{Deserialize, Serialize};

use crate::artifact_workbench::ArtifactRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionType {
    Pitch,
    Boundary,
    LyricText,
    LyricBoundary,
    TrackRole,
    Technique,
    Voicing,
    DeleteFalseNote,
    AddMissedNote,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HumanCorrection {
    pub song_id: String,
    pub workflow_revision: String,
    pub candidate_revision: String,
    pub authored_revision: String,
    pub start: f64,
    pub end: f64,
    pub correction_type: CorrectionType,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    #[serde(default)]
    pub evidence_snapshot: Vec<ArtifactRef>,
}

impl HumanCorrection {
    pub fn deterministic_id(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(blake3::hash(&bytes).to_hex()[..32].to_string())
    }
}
