use serde::{Deserialize, Serialize};

use crate::artifact_workbench::ArtifactRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    FusedF0,
    RmvpeF0,
    FcpeF0,
    GameBoundary,
    BasicPitchOnset,
    QwenWordBoundary,
    FireRedWordBoundary,
    StarsTechnique,
    FusionConfidence,
    Disagreement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidencePoint {
    pub time: f64,
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceTrack {
    pub id: String,
    pub label: String,
    pub kind: EvidenceKind,
    pub source: ArtifactRef,
    #[serde(default)]
    pub points: Vec<EvidencePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    Info,
    Warning,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewReason {
    LowConfidence,
    PitchDisagreement,
    BoundaryDisagreement,
    OctaveRisk,
    LyricBoundaryLowConfidence,
    WordNoteMismatch,
    VoicingConflict,
    LeadHarmonyLeak,
    TechniqueAmbiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRegion {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub severity: ReviewSeverity,
    pub reasons: Vec<ReviewReason>,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_refs: Vec<ArtifactRef>,
    #[serde(default)]
    pub reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorAudioArtifact {
    pub revision: ArtifactRef,
    pub role: crate::workflow::AudioRole,
    pub label: String,
    pub producer: crate::workflow::WorkflowNodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorSourceContext {
    pub opened_chart: ArtifactRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_bundle: Option<ArtifactRef>,
    #[serde(default)]
    pub audio_artifacts: Vec<EditorAudioArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newer_candidate: Option<ArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SingingEvidenceBundle {
    #[serde(default = "default_timeline_step")]
    pub timeline_step_ms: u32,
    #[serde(default)]
    pub tracks: Vec<EvidenceTrack>,
    #[serde(default)]
    pub review_regions: Vec<ReviewRegion>,
}

const fn default_timeline_step() -> u32 {
    crate::singing::CANONICAL_TIMELINE_STEP_MS
}

impl SingingEvidenceBundle {
    /// Projects the canonical fusion artifact into the editor's read-only
    /// evidence contract. The canonical track remains the source of truth;
    /// accepting a suggestion still goes through the ordinary editor action
    /// and undo history.
    pub fn from_canonical_track(
        track: &crate::singing::CanonicalSingingTrack,
        source: ArtifactRef,
    ) -> Self {
        let fused_f0 = EvidenceTrack {
            id: "fusion.f0".to_string(),
            label: "Fused F0".to_string(),
            kind: EvidenceKind::FusedF0,
            source: source.clone(),
            points: track
                .f0_curve
                .iter()
                .map(|point| EvidencePoint {
                    time: point.time,
                    value: point.confidence,
                    pitch: Some(point.hz),
                    label: None,
                })
                .collect(),
        };
        let review_regions = crate::singing::build_review_regions(track)
            .into_iter()
            .map(|region| ReviewRegion {
                id: region.id,
                start: region.range.start,
                end: region.range.end,
                severity: if region.confidence < 0.4 {
                    ReviewSeverity::High
                } else if region.confidence < 0.65 {
                    ReviewSeverity::Warning
                } else {
                    ReviewSeverity::Info
                },
                reasons: region.reasons.into_iter().map(map_review_reason).collect(),
                confidence: region.confidence,
                evidence_refs: vec![source.clone()],
                reviewed: region.reviewed,
            })
            .collect();
        Self {
            timeline_step_ms: crate::singing::CANONICAL_TIMELINE_STEP_MS,
            tracks: vec![fused_f0],
            review_regions,
        }
    }
}

fn map_review_reason(reason: crate::singing::SingingReviewReason) -> ReviewReason {
    use crate::singing::SingingReviewReason as Singing;
    match reason {
        Singing::LowConfidence => ReviewReason::LowConfidence,
        Singing::PitchDisagreement => ReviewReason::PitchDisagreement,
        Singing::BoundaryDisagreement => ReviewReason::BoundaryDisagreement,
        Singing::OctaveRisk => ReviewReason::OctaveRisk,
        Singing::WordNoteMismatch => ReviewReason::WordNoteMismatch,
        Singing::VoicingConflict => ReviewReason::VoicingConflict,
        Singing::LeadHarmonyLeak => ReviewReason::LeadHarmonyLeak,
        Singing::TechniqueAmbiguous => ReviewReason::TechniqueAmbiguous,
    }
}
