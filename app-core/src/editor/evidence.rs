use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};

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
    /// Track-specific measured value. F0 tracks use Hz; uncalibrated
    /// technique tracks use their explicitly labeled source-local score.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_chart: Option<ArtifactRef>,
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
    10
}

#[derive(Deserialize)]
struct SingingAnalysisWireV1 {
    contract: String,
    version: u32,
    format_version: String,
    timebase: u32,
    candidate_evidence: Vec<SingingCandidateWireV1>,
    #[serde(default)]
    candidate_hard_boundaries: SingingHardBoundarySetWireV1,
    review_regions: Vec<SingingReviewRegionWireV1>,
    provenance: SingingProvenanceWireV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingCandidateWireV1 {
    id: String,
    range: SingingRangeWireV1,
    target_midi: u8,
    boundary_source: String,
    boundary_kind: String,
    #[serde(default)]
    boundary_role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_fractional_midi: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_decision_parameter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    presence_decision_parameter: Option<f32>,
    #[serde(default)]
    boundary_hard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_support: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_calibrated_confidence: Option<f32>,
    target_pitch_source: String,
    center_pitch_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rmvpe_center_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rmvpe_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rmvpe_cents_difference: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rmvpe_voiced_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rmvpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fcpe_center_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fcpe_observed_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fcpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fcpe_cents_from_rmvpe: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fcpe_supports_rmvpe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acoustic: Option<SingingAcousticCandidateWireV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    basic_pitch: Option<SingingBasicPitchCandidateWireV1>,
    #[serde(default)]
    boundary_alternatives: Vec<SingingBoundaryAlternativeWireV1>,
    #[serde(default)]
    boundary_constraints: Vec<SingingBoundaryConstraintWireV1>,
    #[serde(default)]
    technique_evidence: Vec<SingingTechniqueCandidateWireV1>,
    #[serde(default)]
    techniques: SingingTechniqueScoresWireV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    word_id: Option<String>,
    #[serde(default)]
    alternatives: Vec<SingingPitchAlternativeWireV1>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingRangeWireV1 {
    start: u64,
    end: u64,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingHardBoundarySetWireV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    boundaries: Vec<SingingHardBoundaryWireV1>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingHardBoundaryWireV1 {
    source: String,
    level: String,
    range: SingingRangeWireV1,
}

#[cfg(test)]
#[derive(Serialize)]
struct SingingCandidatePoolDigestWireV2<'a> {
    schema_version: u32,
    candidates: &'a [SingingCandidateWireV1],
    hard_boundaries: &'a SingingHardBoundarySetWireV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingAcousticCandidateWireV1 {
    frame_count: usize,
    mean_rms: f32,
    mean_periodicity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fundamental_center_hz: Option<f32>,
    mean_snr_db: f32,
    #[serde(default)]
    mean_vibrato_activation: f32,
    #[serde(default)]
    mean_glide_activation: f32,
    #[serde(default)]
    mean_ornament_activation: f32,
    #[serde(default)]
    mean_breath_activation: f32,
    #[serde(default)]
    max_voicing_transition_activation: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    onset_flux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preceding_flux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    onset_supported: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingBasicPitchCandidateWireV1 {
    onset_activation: f32,
    #[serde(default)]
    note_activation: f32,
    #[serde(default)]
    contour_activation: f32,
    #[serde(default)]
    contour_class: usize,
    onset_supported: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingBoundaryAlternativeWireV1 {
    source_expert: String,
    range: SingingRangeWireV1,
    #[serde(default)]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fractional_midi: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_local_score: Option<f32>,
    #[serde(default)]
    hard: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingBoundaryConstraintWireV1 {
    source_expert: String,
    kind: String,
    time: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_local_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    calibrated_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    calibration_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correlation_group: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingTechniqueCandidateWireV1 {
    source_expert: String,
    calibration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vibrato_activation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glissando_activation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    falsetto_activation: Option<f32>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingTechniqueScoresWireV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vibrato: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glissando: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    falsetto: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ornament: Option<f32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SingingPitchAlternativeWireV1 {
    source_expert: String,
    center_hz: f32,
    cents_from_target: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
}

fn valid_unit_interval(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_boundary_kind(kind: &str) -> bool {
    matches!(
        kind,
        "game"
            | "f0_derived"
            | "advanced_note"
            | "basic_pitch_onset"
            | "acoustic_onset"
            | "alignment"
            | "f0_transition"
            | "f0_consolidation"
            | "phrase_constraint"
            | "constraint"
    )
}

fn valid_constraint_kind(kind: &str) -> bool {
    matches!(
        kind,
        "phrase_start"
            | "word_start"
            | "word_end"
            | "voicing_transition"
            | "pitch_discontinuity"
            | "basic_pitch_onset"
            | "acoustic_articulation"
    )
}

impl SingingCandidateWireV1 {
    fn validate(&self) -> Result<(), String> {
        let invalid_optional_unit =
            |value: Option<f32>| value.is_some_and(|v| !valid_unit_interval(v));
        let invalid_optional_finite = |value: Option<f32>| value.is_some_and(|v| !v.is_finite());
        let invalid_optional_non_negative =
            |value: Option<f32>| value.is_some_and(|v| !v.is_finite() || v < 0.0);
        if self.id.trim().is_empty()
            || self.range.start >= self.range.end
            || self.target_midi > 127
            || self.boundary_source.trim().is_empty()
            || !valid_boundary_kind(&self.boundary_kind)
            || !matches!(self.boundary_role.as_str(), "primary" | "challenger")
            || (self.boundary_kind == "game" && self.boundary_fractional_midi.is_none())
            || (self.boundary_role == "challenger"
                && matches!(self.boundary_kind.as_str(), "game" | "f0_derived"))
            || self
                .boundary_fractional_midi
                .is_some_and(|value| !value.is_finite() || !(0.0..128.0).contains(&value))
            || invalid_optional_unit(self.boundary_decision_parameter)
            || invalid_optional_unit(self.presence_decision_parameter)
            || invalid_optional_unit(self.boundary_support)
            || invalid_optional_unit(self.boundary_calibrated_confidence)
            || self.target_pitch_source.trim().is_empty()
            || !valid_hz(self.center_pitch_hz)
            || self.rmvpe_center_hz.is_some_and(|value| !valid_hz(value))
            || invalid_optional_unit(self.rmvpe_confidence)
            || invalid_optional_finite(self.rmvpe_cents_difference)
            || invalid_optional_unit(self.rmvpe_voiced_ratio)
            || invalid_optional_non_negative(self.rmvpe_pitch_mad_cents)
            || self.fcpe_center_hz.is_some_and(|value| !valid_hz(value))
            || invalid_optional_unit(self.fcpe_observed_ratio)
            || invalid_optional_non_negative(self.fcpe_pitch_mad_cents)
            || invalid_optional_finite(self.fcpe_cents_from_rmvpe)
            || self.fcpe_supports_rmvpe.is_some() != self.fcpe_cents_from_rmvpe.is_some()
            || [
                self.techniques.vibrato,
                self.techniques.glissando,
                self.techniques.falsetto,
                self.techniques.ornament,
            ]
            .into_iter()
            .flatten()
            .any(|value| !valid_unit_interval(value))
        {
            return Err(format!("invalid singing-analysis candidate {}", self.id));
        }

        if self.boundary_alternatives.iter().any(|alternative| {
            alternative.source_expert.trim().is_empty()
                || alternative.range.start >= alternative.range.end
                || !valid_boundary_kind(&alternative.kind)
                || alternative
                    .fractional_midi
                    .is_some_and(|value| !value.is_finite() || !(0.0..128.0).contains(&value))
                || invalid_optional_unit(alternative.source_local_score)
        }) || self.boundary_constraints.iter().any(|constraint| {
            constraint.source_expert.trim().is_empty()
                || !valid_constraint_kind(&constraint.kind)
                || invalid_optional_unit(constraint.source_local_strength)
                || invalid_optional_unit(constraint.calibrated_confidence)
                || (constraint.calibrated_confidence.is_some()
                    && constraint
                        .calibration_version
                        .as_deref()
                        .is_none_or(|version| version.trim().is_empty()))
                || constraint
                    .depends_on
                    .iter()
                    .any(|dependency| dependency.trim().is_empty())
        }) || self.technique_evidence.iter().any(|evidence| {
            evidence.source_expert.trim().is_empty()
                || evidence.calibration.trim().is_empty()
                || [
                    evidence.vibrato_activation,
                    evidence.glissando_activation,
                    evidence.falsetto_activation,
                ]
                .into_iter()
                .flatten()
                .any(|value| !valid_unit_interval(value))
        }) || self.alternatives.iter().any(|alternative| {
            alternative.source_expert.trim().is_empty()
                || !valid_hz(alternative.center_hz)
                || !alternative.cents_from_target.is_finite()
                || invalid_optional_unit(alternative.confidence)
        }) {
            return Err(format!(
                "singing-analysis candidate {} has invalid nested evidence",
                self.id
            ));
        }

        if let Some(acoustic) = &self.acoustic
            && (acoustic.frame_count == 0
                || !acoustic.mean_rms.is_finite()
                || acoustic.mean_rms < 0.0
                || !valid_unit_interval(acoustic.mean_periodicity)
                || acoustic
                    .fundamental_center_hz
                    .is_some_and(|hz| !hz.is_finite() || hz <= 0.0)
                || !acoustic.mean_snr_db.is_finite()
                || [
                    acoustic.mean_vibrato_activation,
                    acoustic.mean_glide_activation,
                    acoustic.mean_ornament_activation,
                    acoustic.mean_breath_activation,
                    acoustic.max_voicing_transition_activation,
                ]
                .into_iter()
                .any(|value| !valid_unit_interval(value))
                || invalid_optional_non_negative(acoustic.onset_flux)
                || invalid_optional_non_negative(acoustic.preceding_flux))
        {
            return Err(format!(
                "singing-analysis candidate {} has invalid acoustic evidence",
                self.id
            ));
        }
        if let Some(basic_pitch) = &self.basic_pitch
            && (![
                basic_pitch.onset_activation,
                basic_pitch.note_activation,
                basic_pitch.contour_activation,
            ]
            .into_iter()
            .all(valid_unit_interval)
                || basic_pitch.contour_class >= 264)
        {
            return Err(format!(
                "singing-analysis candidate {} has invalid Basic Pitch evidence",
                self.id
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct SingingReviewRegionWireV1 {
    id: String,
    range: SingingRangeWireV1,
    #[serde(default)]
    confidence: Option<f32>,
    reasons: Vec<SingingReviewReasonWireV1>,
    #[serde(default)]
    reviewed: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SingingReviewReasonWireV1 {
    UnknownConfidence,
    LowConfidence,
    PitchDisagreement,
    LowPitchCoverage,
    PitchInstability,
    BoundaryDisagreement,
    OctaveRisk,
    WordNoteMismatch,
    VoicingConflict,
    LeadHarmonyLeak,
    VocalTopologyUnknown,
    ForegroundOverlap,
    SupportVocalActivity,
    TechniqueAmbiguous,
    F0SegmentationFallback,
    TranscriptLowConfidence,
    TranscriptReferenceMismatch,
    TranscriptLanguageMismatch,
    TranscriptCoverageMismatch,
}

#[derive(Deserialize)]
struct SingingProvenanceWireV1 {
    #[serde(rename = "execution_fingerprint")]
    _execution_fingerprint: String,
    fusion_algorithm: String,
    fusion_decision: SingingDecisionWireV1,
}

#[derive(Deserialize)]
#[serde(tag = "decision_mode", rename_all = "snake_case")]
enum SingingDecisionWireV1 {
    Algorithm {
        selector: String,
        selector_version: String,
        #[serde(rename = "candidate_set_digest")]
        _candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        reuse_policy: String,
    },
    AiJudgment {
        adapter_resource: String,
        adapter_protocol: String,
        adapter_protocol_version: u32,
        adapter_identity: String,
        adapter_version: String,
        #[serde(rename = "candidate_set_digest")]
        _candidate_set_digest: String,
        selected_candidate_ids: Vec<String>,
        #[serde(rename = "response_digest")]
        _response_digest: String,
        reuse_policy: String,
    },
}

impl SingingDecisionWireV1 {
    fn selected_candidate_ids(&self) -> &[String] {
        match self {
            Self::Algorithm {
                selected_candidate_ids,
                ..
            }
            | Self::AiJudgment {
                selected_candidate_ids,
                ..
            } => selected_candidate_ids,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Algorithm {
                selector,
                selector_version,
                _candidate_set_digest: _,
                reuse_policy,
                ..
            } if selector == "hsmm_viterbi"
                && selector_version == "hsmm-v15"
                && reuse_policy == "deterministic" =>
            {
                Ok(())
            }
            Self::AiJudgment {
                adapter_resource,
                adapter_protocol,
                adapter_protocol_version,
                adapter_identity,
                adapter_version,
                _candidate_set_digest: _,
                _response_digest: _,
                reuse_policy,
                ..
            } if adapter_resource == "tool:fusion_agent_adapter"
                && adapter_protocol == "uta.fusion_agent_request/uta.fusion_agent_response"
                && *adapter_protocol_version == 4
                && !adapter_identity.trim().is_empty()
                && !adapter_version.trim().is_empty()
                && reuse_policy == "preserved_revision_only" =>
            {
                Ok(())
            }
            _ => Err("invalid SingingAnalysis fusion decision provenance".to_string()),
        }
    }
}

fn reject_unknown_object_fields(
    value: &serde_json::Value,
    path: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path} must be an object"))?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!("unsupported SingingAnalysis field {path}.{field}"));
    }
    Ok(())
}

fn validate_singing_analysis_wire_shape(value: &serde_json::Value) -> Result<(), String> {
    const TOP: &[&str] = &[
        "contract",
        "version",
        "format_version",
        "timebase",
        "chart_references",
        "track",
        "candidate_evidence",
        "candidate_hard_boundaries",
        "review_regions",
        "provenance",
    ];
    const CANDIDATE: &[&str] = &[
        "id",
        "range",
        "target_midi",
        "boundary_source",
        "boundary_kind",
        "boundary_role",
        "boundary_fractional_midi",
        "boundary_decision_parameter",
        "presence_decision_parameter",
        "boundary_hard",
        "boundary_support",
        "boundary_calibrated_confidence",
        "target_pitch_source",
        "center_pitch_hz",
        "rmvpe_center_hz",
        "rmvpe_confidence",
        "rmvpe_cents_difference",
        "rmvpe_voiced_ratio",
        "rmvpe_pitch_mad_cents",
        "fcpe_center_hz",
        "fcpe_observed_ratio",
        "fcpe_pitch_mad_cents",
        "fcpe_cents_from_rmvpe",
        "fcpe_supports_rmvpe",
        "acoustic",
        "basic_pitch",
        "boundary_alternatives",
        "boundary_constraints",
        "technique_evidence",
        "techniques",
        "word_id",
        "alternatives",
    ];
    const REVIEW: &[&str] = &[
        "id",
        "range",
        "confidence",
        "reasons",
        "evidence_experts",
        "reviewed",
    ];
    const PROVENANCE: &[&str] = &[
        "execution_fingerprint",
        "fusion_algorithm",
        "fusion_decision",
        "candidate_graph_algorithm",
    ];
    const ALGORITHM: &[&str] = &[
        "decision_mode",
        "selector",
        "selector_version",
        "candidate_set_digest",
        "selected_candidate_ids",
        "reuse_policy",
    ];
    const AI: &[&str] = &[
        "decision_mode",
        "adapter_resource",
        "adapter_protocol",
        "adapter_protocol_version",
        "adapter_identity",
        "adapter_version",
        "candidate_set_digest",
        "selected_candidate_ids",
        "response_digest",
        "reuse_policy",
    ];

    reject_unknown_object_fields(value, "analysis", TOP)?;
    let object = value.as_object().unwrap();
    const MAX_CANDIDATE_STATES: usize = 100_000;
    let candidate_evidence = object
        .get("candidate_evidence")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "analysis.candidate_evidence must be an array".to_string())?;
    if candidate_evidence.len() > MAX_CANDIDATE_STATES {
        return Err(format!(
            "analysis.candidate_evidence exceeds the {MAX_CANDIDATE_STATES} state limit"
        ));
    }
    const MAX_CANDIDATE_RELATIONS: usize = 10_000_000;
    let mut candidate_relations = 0usize;
    for candidate in candidate_evidence {
        reject_unknown_object_fields(candidate, "analysis.candidate_evidence[]", CANDIDATE)?;
        reject_unknown_object_fields(
            candidate
                .get("range")
                .ok_or_else(|| "candidate range is missing".to_string())?,
            "analysis.candidate_evidence[].range",
            &["start", "end"],
        )?;
        for field in [
            "boundary_alternatives",
            "boundary_constraints",
            "technique_evidence",
            "alternatives",
        ] {
            let count = candidate
                .get(field)
                .map(|value| {
                    value.as_array().map(Vec::len).ok_or_else(|| {
                        format!("analysis.candidate_evidence[].{field} must be an array")
                    })
                })
                .transpose()?
                .unwrap_or(0);
            candidate_relations = candidate_relations
                .checked_add(count)
                .ok_or_else(|| "SingingAnalysis candidate relation count overflows".to_string())?;
            if candidate_relations > MAX_CANDIDATE_RELATIONS {
                return Err(format!(
                    "analysis.candidate_evidence exceeds the {MAX_CANDIDATE_RELATIONS} nested-relation limit"
                ));
            }
        }
    }
    if let Some(boundary_set) = object.get("candidate_hard_boundaries") {
        reject_unknown_object_fields(
            boundary_set,
            "analysis.candidate_hard_boundaries",
            &["boundaries"],
        )?;
        let boundaries = boundary_set
            .get("boundaries")
            .map(|boundaries| {
                boundaries.as_array().ok_or_else(|| {
                    "analysis.candidate_hard_boundaries.boundaries must be an array".to_string()
                })
            })
            .transpose()?
            .map(Vec::as_slice)
            .unwrap_or_default();
        candidate_relations = candidate_relations
            .checked_add(boundaries.len())
            .ok_or_else(|| "SingingAnalysis candidate relation count overflows".to_string())?;
        if candidate_relations > MAX_CANDIDATE_RELATIONS {
            return Err(format!(
                "analysis candidate evidence exceeds the {MAX_CANDIDATE_RELATIONS} nested-relation limit"
            ));
        }
        for boundary in boundaries {
            reject_unknown_object_fields(
                boundary,
                "analysis.candidate_hard_boundaries.boundaries[]",
                &["source", "level", "range"],
            )?;
            reject_unknown_object_fields(
                boundary
                    .get("range")
                    .ok_or_else(|| "hard boundary range is missing".to_string())?,
                "analysis.candidate_hard_boundaries.boundaries[].range",
                &["start", "end"],
            )?;
        }
    }
    for region in object
        .get("review_regions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "analysis.review_regions must be an array".to_string())?
    {
        reject_unknown_object_fields(region, "analysis.review_regions[]", REVIEW)?;
        reject_unknown_object_fields(
            region
                .get("range")
                .ok_or_else(|| "review region range is missing".to_string())?,
            "analysis.review_regions[].range",
            &["start", "end"],
        )?;
    }
    let provenance = object
        .get("provenance")
        .ok_or_else(|| "analysis.provenance is missing".to_string())?;
    reject_unknown_object_fields(provenance, "analysis.provenance", PROVENANCE)?;
    let decision = provenance
        .get("fusion_decision")
        .ok_or_else(|| "analysis fusion decision is missing".to_string())?;
    let mode = decision
        .get("decision_mode")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "analysis fusion decision mode is missing".to_string())?;
    let allowed = match mode {
        "algorithm" => ALGORITHM,
        "ai_judgment" => AI,
        _ => return Err("unsupported SingingAnalysis fusion decision mode".to_string()),
    };
    reject_unknown_object_fields(decision, "analysis.provenance.fusion_decision", allowed)?;
    let selected_count = decision
        .get("selected_candidate_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "analysis selected candidate identities must be an array".to_string())?
        .len();
    if selected_count > MAX_CANDIDATE_STATES {
        return Err(format!(
            "analysis selected candidate identities exceed the {MAX_CANDIDATE_STATES} state limit"
        ));
    }
    Ok(())
}

fn validate_selected_candidate_coverage(
    candidates: &BTreeMap<String, SingingCandidateWireV1>,
    selected: &[&SingingCandidateWireV1],
) -> Result<(), String> {
    let mut primary_ranges = candidates
        .values()
        .filter(|candidate| candidate.boundary_role == "primary")
        .map(|candidate| (candidate.range.start, candidate.range.end))
        .collect::<Vec<_>>();
    if primary_ranges.is_empty() {
        primary_ranges.extend(
            candidates
                .values()
                .map(|candidate| (candidate.range.start, candidate.range.end)),
        );
    }
    primary_ranges.sort_unstable();
    let mut components = Vec::<(u64, u64)>::new();
    for (start, end) in primary_ranges {
        if let Some(component) = components.last_mut()
            && start <= component.1
        {
            component.1 = component.1.max(end);
        } else {
            components.push((start, end));
        }
    }
    for candidate in selected {
        if !components
            .iter()
            .any(|(start, end)| candidate.range.start >= *start && candidate.range.end <= *end)
        {
            return Err(
                "SingingAnalysis selected candidate lies outside voiced coverage".to_string(),
            );
        }
    }
    for (start, end) in components {
        let covering = selected
            .iter()
            .filter(|candidate| candidate.range.start < end && candidate.range.end > start)
            .collect::<Vec<_>>();
        if covering.first().map(|candidate| candidate.range.start) != Some(start)
            || covering.last().map(|candidate| candidate.range.end) != Some(end)
            || covering
                .windows(2)
                .any(|pair| pair[0].range.end != pair[1].range.start)
        {
            return Err(
                "SingingAnalysis selected candidate path does not exactly cover voiced components"
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Convert the independently owned Engine SingingAnalysis contract into the
/// Editor's read-only evidence representation. This is an explicit protocol
/// boundary: unknown/malformed units or selected IDs fail instead of silently
/// becoming an empty all-default bundle.
pub fn singing_analysis_evidence_bundle(
    bytes: &[u8],
    source: ArtifactRef,
) -> Result<SingingEvidenceBundle, String> {
    const CONTRACT: &str = "uta.analysis-engine.singing-analysis";
    const VERSION: u32 = 1;
    const FORMAT_VERSION: &str = "0.3.0";
    const TIMEBASE: u32 = 1_000_000;

    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    validate_singing_analysis_wire_shape(&value)?;
    let analysis: SingingAnalysisWireV1 =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    if analysis.contract != CONTRACT
        || analysis.version != VERSION
        || analysis.format_version != FORMAT_VERSION
        || analysis.timebase != TIMEBASE
    {
        return Err("unsupported SingingAnalysis evidence contract".to_string());
    }
    if analysis.provenance.fusion_algorithm != "fusion-v17" {
        return Err("invalid SingingAnalysis execution provenance".to_string());
    }
    analysis.provenance.fusion_decision.validate()?;

    let mut previous_hard_boundary = None;
    let mut hard_boundary_times = Vec::with_capacity(
        analysis
            .candidate_hard_boundaries
            .boundaries
            .len()
            .saturating_mul(2),
    );
    for boundary in &analysis.candidate_hard_boundaries.boundaries {
        if boundary.source.trim().is_empty()
            || !matches!(
                boundary.level.as_str(),
                "phrase" | "word" | "syllable" | "phoneme"
            )
            || boundary.range.start >= boundary.range.end
        {
            return Err("invalid SingingAnalysis hard boundary evidence".to_string());
        }
        let level_rank = match boundary.level.as_str() {
            "phrase" => 0u8,
            "word" => 1,
            "syllable" => 2,
            "phoneme" => 3,
            _ => unreachable!("validated boundary level"),
        };
        let key = (
            boundary.range.start,
            boundary.range.end,
            boundary.source.as_str(),
            level_rank,
        );
        if previous_hard_boundary.is_some_and(|previous| previous >= key) {
            return Err("SingingAnalysis hard boundaries are not normalized".to_string());
        }
        previous_hard_boundary = Some(key);
        hard_boundary_times.extend([boundary.range.start, boundary.range.end]);
    }
    hard_boundary_times.sort_unstable();
    hard_boundary_times.dedup();

    let mut candidates = BTreeMap::new();
    for candidate in analysis.candidate_evidence {
        candidate.validate()?;
        if candidates.insert(candidate.id.clone(), candidate).is_some() {
            return Err("duplicate SingingAnalysis candidate identity".to_string());
        }
    }

    let selected_ids = analysis.provenance.fusion_decision.selected_candidate_ids();
    let mut unique = BTreeSet::new();
    if selected_ids.is_empty()
        || selected_ids
            .iter()
            .any(|id| id.trim().is_empty() || !unique.insert(id.as_str()))
    {
        return Err("invalid SingingAnalysis selected candidate identity".to_string());
    }
    let selected = selected_ids
        .iter()
        .map(|id| {
            candidates
                .get(id)
                .ok_or_else(|| "SingingAnalysis selected candidate is missing".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if selected
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err("SingingAnalysis selected candidate path is not ordered".to_string());
    }
    validate_selected_candidate_coverage(&candidates, &selected)?;
    if selected.iter().any(|candidate| {
        let next_edge = hard_boundary_times.partition_point(|time| *time <= candidate.range.start);
        hard_boundary_times
            .get(next_edge)
            .is_some_and(|time| *time < candidate.range.end)
    }) {
        return Err("SingingAnalysis selected candidate crosses a hard boundary".to_string());
    }

    let mut tracks = vec![f0_track(
        "selected-fused-f0",
        "Selected candidate F0",
        EvidenceKind::FusedF0,
        &selected,
        &source,
        |candidate| Some(candidate.center_pitch_hz),
    )];
    for (id, label, kind, value) in [
        (
            "selected-rmvpe-f0",
            "RMVPE candidate F0",
            EvidenceKind::RmvpeF0,
            (|candidate: &SingingCandidateWireV1| candidate.rmvpe_center_hz)
                as fn(&SingingCandidateWireV1) -> Option<f32>,
        ),
        (
            "selected-fcpe-f0",
            "FCPE candidate F0",
            EvidenceKind::FcpeF0,
            (|candidate: &SingingCandidateWireV1| candidate.fcpe_center_hz)
                as fn(&SingingCandidateWireV1) -> Option<f32>,
        ),
    ] {
        let track = f0_track(id, label, kind, &selected, &source, value);
        if !track.points.is_empty() {
            tracks.push(track);
        }
    }

    let review_regions = analysis
        .review_regions
        .into_iter()
        .map(|region| review_region(region, &source))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SingingEvidenceBundle {
        timeline_step_ms: 10,
        tracks,
        review_regions,
    })
}

fn valid_hz(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn f0_track(
    id: &str,
    label: &str,
    kind: EvidenceKind,
    candidates: &[&SingingCandidateWireV1],
    source: &ArtifactRef,
    value: fn(&SingingCandidateWireV1) -> Option<f32>,
) -> EvidenceTrack {
    let points = candidates
        .iter()
        .filter_map(|candidate| {
            let hz = value(candidate)?;
            let midpoint =
                candidate.range.start + (candidate.range.end - candidate.range.start) / 2;
            Some(EvidencePoint {
                time: midpoint as f64 / 1_000_000.0,
                value: hz,
                pitch: Some(69.0 + 12.0 * (hz / 440.0).log2()),
                label: Some(candidate.id.clone()),
            })
        })
        .collect();
    EvidenceTrack {
        id: id.to_string(),
        label: label.to_string(),
        kind,
        source: source.clone(),
        points,
    }
}

fn review_region(
    region: SingingReviewRegionWireV1,
    source: &ArtifactRef,
) -> Result<ReviewRegion, String> {
    if region.id.trim().is_empty()
        || region.range.start >= region.range.end
        || region.reasons.is_empty()
        || region
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err("invalid SingingAnalysis review region".to_string());
    }
    Ok(ReviewRegion {
        id: region.id,
        start: region.range.start as f64 / 1_000_000.0,
        end: region.range.end as f64 / 1_000_000.0,
        severity: ReviewSeverity::Warning,
        reasons: region.reasons.into_iter().map(review_reason).collect(),
        confidence: region.confidence,
        evidence_refs: vec![source.clone()],
        reviewed: region.reviewed,
    })
}

fn review_reason(reason: SingingReviewReasonWireV1) -> ReviewReason {
    match reason {
        SingingReviewReasonWireV1::PitchDisagreement
        | SingingReviewReasonWireV1::PitchInstability => ReviewReason::PitchDisagreement,
        SingingReviewReasonWireV1::BoundaryDisagreement
        | SingingReviewReasonWireV1::F0SegmentationFallback => ReviewReason::BoundaryDisagreement,
        SingingReviewReasonWireV1::OctaveRisk => ReviewReason::OctaveRisk,
        SingingReviewReasonWireV1::WordNoteMismatch => ReviewReason::WordNoteMismatch,
        SingingReviewReasonWireV1::VoicingConflict => ReviewReason::VoicingConflict,
        SingingReviewReasonWireV1::LeadHarmonyLeak
        | SingingReviewReasonWireV1::VocalTopologyUnknown
        | SingingReviewReasonWireV1::ForegroundOverlap
        | SingingReviewReasonWireV1::SupportVocalActivity => ReviewReason::LeadHarmonyLeak,
        SingingReviewReasonWireV1::TechniqueAmbiguous => ReviewReason::TechniqueAmbiguous,
        SingingReviewReasonWireV1::UnknownConfidence
        | SingingReviewReasonWireV1::LowConfidence
        | SingingReviewReasonWireV1::LowPitchCoverage
        | SingingReviewReasonWireV1::TranscriptLowConfidence => ReviewReason::LowConfidence,
        SingingReviewReasonWireV1::TranscriptReferenceMismatch
        | SingingReviewReasonWireV1::TranscriptLanguageMismatch
        | SingingReviewReasonWireV1::TranscriptCoverageMismatch => {
            ReviewReason::LyricBoundaryLowConfidence
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

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence_source() -> ArtifactRef {
        ArtifactRef {
            file_hash: "song".to_string(),
            kind: crate::analysis_graph::ArtifactKind::EvidenceBundle,
            revision_id: "singing-revision".to_string(),
        }
    }

    fn candidate(
        id: &str,
        center_pitch_hz: f32,
        rmvpe_center_hz: Option<f32>,
        fcpe_center_hz: Option<f32>,
    ) -> SingingCandidateWireV1 {
        SingingCandidateWireV1 {
            id: id.to_string(),
            range: SingingRangeWireV1 {
                start: 1_000_000,
                end: 2_000_000,
            },
            target_midi: 69,
            boundary_source: "rmvpe".to_string(),
            boundary_kind: "f0_derived".to_string(),
            boundary_role: "primary".to_string(),
            boundary_fractional_midi: None,
            boundary_decision_parameter: None,
            presence_decision_parameter: None,
            boundary_hard: false,
            boundary_support: None,
            boundary_calibrated_confidence: None,
            target_pitch_source: "rmvpe".to_string(),
            center_pitch_hz,
            rmvpe_center_hz,
            rmvpe_confidence: None,
            rmvpe_cents_difference: None,
            rmvpe_voiced_ratio: None,
            rmvpe_pitch_mad_cents: None,
            fcpe_center_hz,
            fcpe_observed_ratio: None,
            fcpe_pitch_mad_cents: None,
            fcpe_cents_from_rmvpe: None,
            fcpe_supports_rmvpe: None,
            acoustic: None,
            basic_pitch: None,
            boundary_alternatives: Vec::new(),
            boundary_constraints: Vec::new(),
            technique_evidence: Vec::new(),
            techniques: SingingTechniqueScoresWireV1::default(),
            word_id: None,
            alternatives: Vec::new(),
        }
    }

    fn singing_analysis(selected_id: &str) -> Vec<u8> {
        let candidates = vec![
            candidate("selected", 440.0, Some(439.0), Some(441.0)),
            candidate("alternative", 880.0, None, None),
        ];
        let hard_boundaries = SingingHardBoundarySetWireV1::default();
        let candidate_pool = SingingCandidatePoolDigestWireV2 {
            schema_version: 2,
            candidates: &candidates,
            hard_boundaries: &hard_boundaries,
        };
        let candidate_set_digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&candidate_pool).unwrap())
        );
        serde_json::to_vec(&serde_json::json!({
            "contract":"uta.analysis-engine.singing-analysis",
            "version":1,
            "format_version":"0.3.0",
            "timebase":1000000,
            "chart_references":{},
            "candidate_evidence":candidates,
            "candidate_hard_boundaries":hard_boundaries,
            "review_regions":[{
                "id":"review-1",
                "range":{"start":1000000,"end":2000000},
                "reasons":["pitch_disagreement"],
                "evidence_experts":["rmvpe","fcpe"],
                "reviewed":false
            }],
            "provenance":{
                "execution_fingerprint":"f".repeat(64),
                "fusion_algorithm":"fusion-v17",
                "fusion_decision":{
                    "decision_mode":"ai_judgment",
                    "adapter_resource":"tool:fusion_agent_adapter",
                    "adapter_protocol":"uta.fusion_agent_request/uta.fusion_agent_response",
                    "adapter_protocol_version":4,
                    "adapter_identity":"fusion_agent_adapter",
                    "adapter_version":"1.0.0-test",
                    "candidate_set_digest":candidate_set_digest,
                    "selected_candidate_ids":[selected_id],
                    "response_digest":"a".repeat(64),
                    "reuse_policy":"preserved_revision_only"
                }
            }
        }))
        .unwrap()
    }

    fn refresh_candidate_pool_digest(value: &mut serde_json::Value) {
        let candidates: Vec<SingingCandidateWireV1> =
            serde_json::from_value(value["candidate_evidence"].clone()).unwrap();
        let hard_boundaries: SingingHardBoundarySetWireV1 =
            serde_json::from_value(value["candidate_hard_boundaries"].clone()).unwrap();
        let pool = SingingCandidatePoolDigestWireV2 {
            schema_version: 2,
            candidates: &candidates,
            hard_boundaries: &hard_boundaries,
        };
        value["provenance"]["fusion_decision"]["candidate_set_digest"] = serde_json::json!(
            format!("{:x}", Sha256::digest(serde_json::to_vec(&pool).unwrap()))
        );
    }

    #[test]
    fn singing_analysis_projects_selected_hz_and_preserves_unknown_confidence() {
        let bytes = singing_analysis("selected");
        let encoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(encoded["candidate_hard_boundaries"], serde_json::json!({}));
        let bundle = singing_analysis_evidence_bundle(&bytes, evidence_source()).unwrap();
        let fused = bundle
            .tracks
            .iter()
            .find(|track| track.kind == EvidenceKind::FusedF0)
            .unwrap();
        assert_eq!(fused.points.len(), 1);
        assert_eq!(fused.points[0].time, 1.5);
        assert_eq!(fused.points[0].value, 440.0);
        assert_eq!(fused.points[0].pitch, Some(69.0));
        assert_eq!(bundle.review_regions.len(), 1);
        assert_eq!(bundle.review_regions[0].confidence, None);
        assert_eq!(
            bundle.review_regions[0].reasons,
            vec![ReviewReason::PitchDisagreement]
        );
    }

    #[test]
    fn singing_analysis_accepts_any_nonempty_engine_validated_adapter_identity() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        value["provenance"]["fusion_decision"]["adapter_identity"] =
            serde_json::json!("uta-test-adapter");
        singing_analysis_evidence_bundle(&serde_json::to_vec(&value).unwrap(), evidence_source())
            .unwrap();
    }

    #[test]
    fn singing_analysis_accepts_engine_typed_hard_boundary_order() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        value["candidate_hard_boundaries"]["boundaries"] = serde_json::json!([
            {"source":"caller","level":"phrase","range":{"start":2000000,"end":2100000}},
            {"source":"caller","level":"word","range":{"start":2000000,"end":2100000}}
        ]);
        refresh_candidate_pool_digest(&mut value);
        singing_analysis_evidence_bundle(&serde_json::to_vec(&value).unwrap(), evidence_source())
            .unwrap();
    }

    #[test]
    fn singing_analysis_accepts_typed_soft_phrase_start_context() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        value["candidate_evidence"][0]["boundary_constraints"] = serde_json::json!([{
            "source_expert":"constraint.user.phrase.p1",
            "kind":"phrase_start",
            "time":100000,
            "source_local_strength":0.6,
            "correlation_group":"constraint.user",
            "depends_on":[]
        }]);
        refresh_candidate_pool_digest(&mut value);
        singing_analysis_evidence_bundle(&serde_json::to_vec(&value).unwrap(), evidence_source())
            .unwrap();
    }

    #[test]
    fn singing_analysis_rejects_missing_selected_candidate_and_wrong_contract() {
        assert!(
            singing_analysis_evidence_bundle(&singing_analysis("missing"), evidence_source())
                .unwrap_err()
                .contains("selected candidate is missing")
        );
        let mut value: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        value["contract"] = serde_json::json!("not-singing-analysis");
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&value).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("unsupported SingingAnalysis")
        );
        let mut value: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        value["candidate_evidence"][0]["future_confidence"] = serde_json::json!(0.5);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&value).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("unsupported SingingAnalysis field")
        );

        let mut nested: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        nested["candidate_evidence"][0]["alternatives"] = serde_json::json!([{
            "source_expert":"rmvpe",
            "center_hz":440.0,
            "cents_from_target":0.0,
            "future_confidence":0.5
        }]);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&nested).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("unknown field")
        );
    }

    #[test]
    fn singing_analysis_rejects_semantically_invalid_candidate_evidence() {
        let mut invalid_midi: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        invalid_midi["candidate_evidence"][0]["target_midi"] = serde_json::json!(255);
        refresh_candidate_pool_digest(&mut invalid_midi);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&invalid_midi).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("invalid singing-analysis candidate")
        );

        let mut inconsistent_fcpe: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        inconsistent_fcpe["candidate_evidence"][0]["fcpe_supports_rmvpe"] = serde_json::json!(true);
        refresh_candidate_pool_digest(&mut inconsistent_fcpe);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&inconsistent_fcpe).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("invalid singing-analysis candidate")
        );
    }

    #[test]
    fn singing_analysis_uses_structural_candidate_and_provenance_validation() {
        let mut candidate_tamper: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        candidate_tamper["candidate_evidence"][0]["center_pitch_hz"] = serde_json::json!(466.16);
        singing_analysis_evidence_bundle(
            &serde_json::to_vec(&candidate_tamper).unwrap(),
            evidence_source(),
        )
        .unwrap();

        let mut hard_boundary_tamper: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        hard_boundary_tamper["candidate_hard_boundaries"]["boundaries"] = serde_json::json!([{
            "source":"caller",
            "level":"word",
            "range":{"start":1010000,"end":1020000}
        }]);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&hard_boundary_tamper).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("crosses a hard boundary")
        );

        let mut coverage_tamper: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        coverage_tamper["candidate_evidence"][1]["range"] =
            serde_json::json!({"start":2000000,"end":3000000});
        refresh_candidate_pool_digest(&mut coverage_tamper);
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&coverage_tamper).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("exactly cover voiced components")
        );

        let mut provenance_tamper: serde_json::Value =
            serde_json::from_slice(&singing_analysis("selected")).unwrap();
        provenance_tamper["provenance"]["fusion_decision"]["adapter_resource"] =
            serde_json::json!("tool:untrusted");
        assert!(
            singing_analysis_evidence_bundle(
                &serde_json::to_vec(&provenance_tamper).unwrap(),
                evidence_source()
            )
            .unwrap_err()
            .contains("fusion decision provenance")
        );
    }

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
