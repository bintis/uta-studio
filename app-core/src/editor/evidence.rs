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

#[derive(Deserialize)]
struct TechniqueEvidenceWireV1 {
    contract: String,
    version: u32,
    model_id: String,
    taxonomy: Vec<String>,
    calibration: String,
    intervals: Vec<TechniqueIntervalWireV1>,
}

#[derive(Deserialize)]
struct TechniqueIntervalWireV1 {
    range: TechniqueRangeWireV1,
    raw_logits: Vec<f32>,
    source_local_scores: Vec<f32>,
}

#[derive(Deserialize)]
struct TechniqueRangeWireV1 {
    start: u64,
    end: u64,
}

pub fn technique_evidence_track(
    bytes: &[u8],
    source: ArtifactRef,
) -> Result<EvidenceTrack, String> {
    let evidence: TechniqueEvidenceWireV1 =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if evidence.contract != "uta.analysis-engine.technique-evidence"
        || evidence.version != 1
        || evidence.model_id != "stars"
        || evidence.calibration != "source_local_sigmoid_uncalibrated"
        || evidence.taxonomy
            != [
                "bubble",
                "breathe",
                "pharyngeal",
                "vibrato",
                "glissando",
                "mixed",
                "falsetto",
                "weak",
                "strong",
            ]
        || evidence.intervals.is_empty()
    {
        return Err("STARS technique evidence identity is invalid".to_string());
    }
    let mut points = Vec::new();
    for interval in evidence.intervals {
        if interval.range.end <= interval.range.start
            || interval.raw_logits.len() != evidence.taxonomy.len()
            || interval.source_local_scores.len() != evidence.taxonomy.len()
        {
            return Err("STARS technique interval is invalid".to_string());
        }
        let time = (interval.range.start + (interval.range.end - interval.range.start) / 2) as f64
            / 1_000_000.0;
        for ((class, raw_logit), score) in evidence
            .taxonomy
            .iter()
            .zip(interval.raw_logits)
            .zip(interval.source_local_scores)
        {
            if !raw_logit.is_finite() || !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err("STARS technique score is invalid".to_string());
            }
            points.push(EvidencePoint {
                time,
                value: score,
                pitch: None,
                label: Some(format!(
                    "{class} · source-local score {score:.3} · raw logit {raw_logit:.3} · uncalibrated"
                )),
            });
        }
    }
    Ok(EvidenceTrack {
        id: "stars.technique".to_string(),
        label: "STARS technique · source-local scores (uncalibrated)".to_string(),
        kind: EvidenceKind::StarsTechnique,
        source,
        points,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn technique_projection_is_read_only_and_calls_scores_uncalibrated() {
        let source = ArtifactRef {
            file_hash: "song".to_string(),
            kind: crate::analysis_graph::ArtifactKind::TechniqueEvidence,
            revision_id: "technique-revision".to_string(),
        };
        let bytes = serde_json::to_vec(&serde_json::json!({
            "contract":"uta.analysis-engine.technique-evidence",
            "version":1,
            "model_id":"stars",
            "taxonomy":["bubble","breathe","pharyngeal","vibrato","glissando","mixed","falsetto","weak","strong"],
            "calibration":"source_local_sigmoid_uncalibrated",
            "intervals":[{
                "range":{"start":1000000,"end":1200000},
                "phoneme_id":1,
                "raw_logits":[0.0,0.0,0.0,1.0,0.0,0.0,0.0,0.0,0.0],
                "source_local_scores":[0.5,0.5,0.5,0.7310586,0.5,0.5,0.5,0.5,0.5]
            }],
            "style_scope":"segment_global",
            "styles":[],
            "provenance":{}
        })).unwrap();
        let track = technique_evidence_track(&bytes, source).unwrap();
        assert_eq!(track.kind, EvidenceKind::StarsTechnique);
        assert_eq!(track.points.len(), 9);
        assert!(track.label.contains("uncalibrated"));
        assert!(
            track.points[3]
                .label
                .as_deref()
                .unwrap()
                .contains("raw logit")
        );
    }
}
