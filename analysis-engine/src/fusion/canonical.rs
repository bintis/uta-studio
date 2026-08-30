use serde::{Deserialize, Serialize};

use super::{
    AcousticCandidateFeatures, BasicPitchCandidateFeatures, BoundaryAlternative,
    BoundaryCandidateRole, BoundaryEvidenceKind, CanonicalLyrics, CanonicalWordBoundary,
    EvidenceProvenance, PitchAlternative, SegmentCandidate, TechniqueCandidateFeatures,
    TechniqueScores, TimeRange,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct F0Point {
    pub time: u64,
    pub hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PitchBendPoint {
    pub time: u64,
    pub cents: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FusionContextSignalV1 {
    F0SegmentationFallback,
    BoundaryDisagreement,
    BasicPitchOnset,
    AcousticOnset,
    TechniqueUncalibrated,
    LowPitchCoverage,
    PitchDisagreement,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PitchSelectionReasonV1 {
    #[default]
    Unknown,
    BoundaryProposal,
    F0DerivedProposal,
    GlobalPitchAlternative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FusionDecisionTraceV1 {
    pub policy_version: u32,
    pub continuous_f0_source: String,
    pub boundary_source: String,
    #[serde(default)]
    pub selected_target_pitch_source: String,
    #[serde(default)]
    pub considered_target_pitch_sources: Vec<String>,
    #[serde(default)]
    pub pitch_selection_reason: PitchSelectionReasonV1,
    #[serde(default)]
    pub onset_support_sources: Vec<String>,
    #[serde(default)]
    pub context_signals: Vec<FusionContextSignalV1>,
    pub degraded_fallback: bool,
}

impl Default for FusionDecisionTraceV1 {
    fn default() -> Self {
        Self {
            policy_version: 1,
            continuous_f0_source: "unknown".to_string(),
            boundary_source: "game".to_string(),
            selected_target_pitch_source: "unknown".to_string(),
            considered_target_pitch_sources: Vec::new(),
            pitch_selection_reason: PitchSelectionReasonV1::Unknown,
            onset_support_sources: Vec::new(),
            context_signals: Vec::new(),
            degraded_fallback: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalNoteEvidence {
    #[serde(default)]
    pub source_experts: Vec<String>,
    #[serde(default)]
    pub decision_trace: FusionDecisionTraceV1,
    #[serde(default = "legacy_game_source")]
    pub boundary_source: String,
    #[serde(default)]
    pub boundary_kind: BoundaryEvidenceKind,
    #[serde(default)]
    pub boundary_role: BoundaryCandidateRole,
    #[serde(
        default,
        alias = "game_fractional_midi",
        skip_serializing_if = "Option::is_none"
    )]
    pub boundary_fractional_midi: Option<f32>,
    /// Source-local decision configuration retained for audit; not confidence.
    #[serde(
        default,
        alias = "game_boundary_decision_threshold",
        skip_serializing_if = "Option::is_none"
    )]
    pub boundary_decision_parameter: Option<f32>,
    /// Source-local presence configuration retained for audit; not confidence.
    #[serde(
        default,
        alias = "game_presence_decision_threshold",
        skip_serializing_if = "Option::is_none"
    )]
    pub presence_decision_parameter: Option<f32>,
    /// Versioned calibrated boundary confidence, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_calibrated_confidence: Option<f32>,
    #[serde(default = "legacy_game_source")]
    pub target_pitch_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pitch_source_local_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pitch_calibrated_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_center_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_cents_difference: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_voiced_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_center_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_observed_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_cents_from_rmvpe: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_supports_rmvpe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acoustic: Option<AcousticCandidateFeatures>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_pitch: Option<BasicPitchCandidateFeatures>,
    #[serde(default)]
    pub boundary_alternatives: Vec<BoundaryAlternative>,
    #[serde(default)]
    pub technique_evidence: Vec<TechniqueCandidateFeatures>,
}

fn legacy_game_source() -> String {
    "game".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalNote {
    pub id: String,
    pub range: TimeRange,
    pub midi_note: u8,
    pub center_pitch_hz: f32,
    pub center_offset_cents: f32,
    /// Calibrated target-note confidence, when an expert actually supplies it.
    /// The current GAME baseline leaves this unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub uncertain: bool,
    #[serde(default)]
    pub alternatives: Vec<PitchAlternative>,
    #[serde(default)]
    pub f0_curve: Vec<F0Point>,
    #[serde(default)]
    pub pitch_bend: Vec<PitchBendPoint>,
    #[serde(default)]
    pub techniques: TechniqueScores,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    pub evidence: CanonicalNoteEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HarmonyMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_track_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_harmony_leak_probability: Option<f32>,
}

impl CanonicalNoteEvidence {
    pub fn effective_boundary_fractional_midi(&self) -> Option<f32> {
        self.boundary_fractional_midi
    }

    pub fn effective_boundary_decision_parameter(&self) -> Option<f32> {
        self.boundary_decision_parameter
    }

    pub fn effective_presence_decision_parameter(&self) -> Option<f32> {
        self.presence_decision_parameter
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalSingingTrack {
    pub schema_version: u32,
    pub transcript: CanonicalLyrics,
    #[serde(default)]
    pub words: Vec<CanonicalWordBoundary>,
    #[serde(default)]
    pub notes: Vec<CanonicalNote>,
    #[serde(default)]
    pub f0_curve: Vec<F0Point>,
    pub harmony_metadata: HarmonyMetadata,
    #[serde(default)]
    pub provenance: Vec<EvidenceProvenance>,
}

fn midi_frequency(midi: u8) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

fn invalid_unit(value: f32) -> bool {
    !value.is_finite() || !(0.0..=1.0).contains(&value)
}

fn validate_canonical_note(note: &CanonicalNote, index: usize) -> Result<(), String> {
    let field_error =
        |field: &str| format!("canonical note {index} ({}) has invalid {field}", note.id);
    if note.id.trim().is_empty() {
        return Err(format!("canonical note {index} has an empty id"));
    }
    if note.range.end <= note.range.start {
        return Err(field_error("time range"));
    }
    if !note.center_pitch_hz.is_finite() || note.center_pitch_hz <= 0.0 {
        return Err(field_error("center_pitch_hz"));
    }
    if !note.center_offset_cents.is_finite() {
        return Err(field_error("center_offset_cents"));
    }
    if note.confidence.is_some_and(invalid_unit) {
        return Err(field_error("confidence"));
    }
    if note.techniques.validated().is_none() {
        return Err(field_error("technique scores"));
    }
    if note.evidence.boundary_source.trim().is_empty()
        || note.evidence.target_pitch_source.trim().is_empty()
    {
        return Err(field_error("boundary/target source identity"));
    }
    if note
        .evidence
        .boundary_fractional_midi
        .is_some_and(|value| !value.is_finite() || !(0.0..128.0).contains(&value))
    {
        return Err(field_error("boundary_fractional_midi"));
    }
    if note
        .evidence
        .boundary_decision_parameter
        .is_some_and(invalid_unit)
        || note
            .evidence
            .presence_decision_parameter
            .is_some_and(invalid_unit)
        || [
            note.evidence.boundary_calibrated_confidence,
            note.evidence.target_pitch_source_local_score,
            note.evidence.target_pitch_calibrated_confidence,
        ]
        .into_iter()
        .flatten()
        .any(invalid_unit)
    {
        return Err(field_error("boundary/pitch confidence"));
    }
    if note.evidence.boundary_kind == BoundaryEvidenceKind::Game
        && note.evidence.effective_boundary_fractional_midi().is_none()
    {
        return Err(field_error("GAME fractional MIDI"));
    }
    if note.evidence.boundary_role == BoundaryCandidateRole::Challenger
        && matches!(
            note.evidence.boundary_kind,
            BoundaryEvidenceKind::Game | BoundaryEvidenceKind::F0Derived
        )
    {
        return Err(field_error("challenger boundary kind"));
    }
    if note.evidence.basic_pitch.as_ref().is_some_and(|evidence| {
        [
            evidence.onset_activation,
            evidence.note_activation,
            evidence.contour_activation,
        ]
        .into_iter()
        .any(invalid_unit)
            || evidence.contour_class >= 264
    }) {
        return Err(field_error("Basic Pitch evidence"));
    }
    for (alternative_index, alternative) in note.evidence.boundary_alternatives.iter().enumerate() {
        if alternative.source_expert.trim().is_empty()
            || alternative.range.end <= alternative.range.start
            || alternative
                .fractional_midi
                .is_some_and(|midi| !midi.is_finite() || !(0.0..128.0).contains(&midi))
            || [
                alternative.source_local_score,
                alternative.source_local_pitch_score,
                alternative.calibrated_boundary_confidence,
                alternative.calibrated_pitch_confidence,
            ]
            .into_iter()
            .flatten()
            .any(invalid_unit)
        {
            return Err(field_error(&format!(
                "boundary alternative {alternative_index}"
            )));
        }
    }
    for (technique_index, evidence) in note.evidence.technique_evidence.iter().enumerate() {
        if evidence.source_expert.trim().is_empty()
            || evidence.calibration.trim().is_empty()
            || [
                evidence.vibrato_activation,
                evidence.glissando_activation,
                evidence.falsetto_activation,
            ]
            .into_iter()
            .flatten()
            .any(invalid_unit)
        {
            return Err(field_error(&format!(
                "technique evidence {technique_index}"
            )));
        }
    }
    for (field, value, unit_interval, positive) in [
        (
            "rmvpe_center_hz",
            note.evidence.rmvpe_center_hz,
            false,
            true,
        ),
        (
            "rmvpe_confidence",
            note.evidence.rmvpe_confidence,
            true,
            false,
        ),
        (
            "rmvpe_cents_difference",
            note.evidence.rmvpe_cents_difference,
            false,
            false,
        ),
        (
            "rmvpe_voiced_ratio",
            note.evidence.rmvpe_voiced_ratio,
            true,
            false,
        ),
        (
            "rmvpe_pitch_mad_cents",
            note.evidence.rmvpe_pitch_mad_cents,
            false,
            false,
        ),
        ("fcpe_center_hz", note.evidence.fcpe_center_hz, false, true),
        (
            "fcpe_observed_ratio",
            note.evidence.fcpe_observed_ratio,
            true,
            false,
        ),
        (
            "fcpe_pitch_mad_cents",
            note.evidence.fcpe_pitch_mad_cents,
            false,
            false,
        ),
        (
            "fcpe_cents_from_rmvpe",
            note.evidence.fcpe_cents_from_rmvpe,
            false,
            false,
        ),
    ] {
        if value.is_some_and(|value| {
            !value.is_finite()
                || (unit_interval && !(0.0..=1.0).contains(&value))
                || (positive && value <= 0.0)
                || (field.ends_with("mad_cents") && value < 0.0)
        }) {
            return Err(field_error(field));
        }
    }
    if note.evidence.fcpe_supports_rmvpe.is_some() != note.evidence.fcpe_cents_from_rmvpe.is_some()
    {
        return Err(field_error("FCPE relation completeness"));
    }
    if note
        .f0_curve
        .windows(2)
        .any(|pair| pair[0].time >= pair[1].time)
        || note.f0_curve.iter().any(|point| {
            !point.hz.is_finite() || point.hz <= 0.0 || point.confidence.is_some_and(invalid_unit)
        })
    {
        return Err(field_error("note-local F0 curve"));
    }
    if note
        .pitch_bend
        .windows(2)
        .any(|pair| pair[0].time >= pair[1].time)
        || note.pitch_bend.iter().any(|point| !point.cents.is_finite())
    {
        return Err(field_error("pitch bend"));
    }
    Ok(())
}

pub fn validate_canonical_singing_track(track: &CanonicalSingingTrack) -> Result<(), String> {
    if track.schema_version != 1
        || track.transcript.text.trim().is_empty()
        || track
            .transcript
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || track.transcript.tokens.iter().any(|token| {
            token.text.trim().is_empty()
                || token
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        })
    {
        return Err("canonical singing track identity or transcript is invalid".to_string());
    }
    if track
        .words
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
        || track.words.iter().any(|word| {
            word.word_id.trim().is_empty()
                || word.text.trim().is_empty()
                || word.range.end <= word.range.start
                || word
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        })
    {
        return Err("canonical words are invalid or overlapping".to_string());
    }
    if let Some((index, pair)) = track
        .notes
        .windows(2)
        .enumerate()
        .find(|(_, pair)| pair[0].range.end > pair[1].range.start)
    {
        return Err(format!(
            "canonical notes {index} ({}) and {} ({}) overlap: {} > {}",
            pair[0].id,
            index + 1,
            pair[1].id,
            pair[0].range.end,
            pair[1].range.start
        ));
    }
    for (index, note) in track.notes.iter().enumerate() {
        validate_canonical_note(note, index)?;
    }
    if track
        .f0_curve
        .windows(2)
        .any(|pair| pair[0].time >= pair[1].time)
        || track.f0_curve.iter().any(|point| {
            !point.hz.is_finite()
                || point.hz <= 0.0
                || point
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        })
    {
        return Err("canonical F0 curve is invalid or unordered".to_string());
    }
    if track
        .harmony_metadata
        .lead_harmony_leak_probability
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err("harmony metadata probability is invalid".to_string());
    }
    Ok(())
}

fn fusion_decision_trace(
    candidate: &SegmentCandidate,
    continuous_f0_source: &str,
) -> FusionDecisionTraceV1 {
    let mut onset_support_sources = Vec::new();
    let mut context_signals = Vec::new();
    if candidate
        .basic_pitch
        .as_ref()
        .is_some_and(|features| features.onset_supported)
    {
        onset_support_sources.push("basic_pitch".to_string());
        context_signals.push(FusionContextSignalV1::BasicPitchOnset);
    }
    if candidate
        .acoustic
        .as_ref()
        .is_some_and(|features| features.onset_supported == Some(true))
    {
        onset_support_sources.push("acoustic_dsp".to_string());
        context_signals.push(FusionContextSignalV1::AcousticOnset);
    }
    let degraded_fallback = candidate.boundary_kind == BoundaryEvidenceKind::F0Derived;
    if degraded_fallback {
        context_signals.push(FusionContextSignalV1::F0SegmentationFallback);
    }
    if !candidate.boundary_alternatives.is_empty() {
        context_signals.push(FusionContextSignalV1::BoundaryDisagreement);
    }
    if candidate.technique_evidence.iter().any(|observation| {
        observation
            .calibration
            .to_ascii_lowercase()
            .contains("uncalibrated")
    }) {
        context_signals.push(FusionContextSignalV1::TechniqueUncalibrated);
    }
    let voiced_ratio = if continuous_f0_source == "rmvpe" {
        candidate.rmvpe_voiced_ratio
    } else {
        candidate.fcpe_observed_ratio
    };
    if voiced_ratio.is_some_and(|ratio| ratio < 0.35) {
        context_signals.push(FusionContextSignalV1::LowPitchCoverage);
    }
    if candidate.fcpe_supports_rmvpe == Some(false)
        || candidate
            .rmvpe_cents_difference
            .is_some_and(|cents| cents.abs() > 50.0)
    {
        context_signals.push(FusionContextSignalV1::PitchDisagreement);
    }
    onset_support_sources.sort();
    onset_support_sources.dedup();
    context_signals.sort_by_key(|signal| *signal as u8);
    context_signals.dedup();
    let mut considered_target_pitch_sources = candidate
        .alternatives
        .iter()
        .map(|alternative| alternative.source_expert.clone())
        .chain(std::iter::once(candidate.target_pitch_source.clone()))
        .collect::<Vec<_>>();
    considered_target_pitch_sources.sort();
    considered_target_pitch_sources.dedup();
    let pitch_selection_reason = if candidate.boundary_kind == BoundaryEvidenceKind::F0Derived {
        PitchSelectionReasonV1::F0DerivedProposal
    } else if candidate.target_pitch_source == candidate.boundary_source {
        PitchSelectionReasonV1::BoundaryProposal
    } else {
        PitchSelectionReasonV1::GlobalPitchAlternative
    };
    FusionDecisionTraceV1 {
        policy_version: 3,
        continuous_f0_source: continuous_f0_source.to_string(),
        boundary_source: candidate.boundary_source.clone(),
        selected_target_pitch_source: candidate.target_pitch_source.clone(),
        considered_target_pitch_sources,
        pitch_selection_reason,
        onset_support_sources,
        context_signals,
        degraded_fallback,
    }
}

pub fn build_canonical_singing_track(
    transcript: CanonicalLyrics,
    words: Vec<CanonicalWordBoundary>,
    candidates: Vec<SegmentCandidate>,
    f0_curve: Vec<F0Point>,
    continuous_f0_source: &str,
    harmony_metadata: HarmonyMetadata,
    provenance: Vec<EvidenceProvenance>,
) -> Result<CanonicalSingingTrack, String> {
    if candidates
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err("decoded note candidates overlap".to_string());
    }
    let source_experts = provenance
        .iter()
        .map(|item| item.expert_id.clone())
        .collect::<Vec<_>>();
    let mut notes = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        candidate.emission_utility()?;
        let reference_hz = midi_frequency(candidate.target_midi);
        let center_offset_cents = 1_200.0 * (candidate.center_pitch_hz / reference_hz).log2();
        let note_f0 = f0_curve
            .iter()
            .copied()
            .filter(|point| point.time >= candidate.range.start && point.time < candidate.range.end)
            .collect::<Vec<_>>();
        let pitch_bend = note_f0
            .iter()
            .map(|point| PitchBendPoint {
                time: point.time,
                cents: 1_200.0 * (point.hz / reference_hz).log2(),
            })
            .collect();
        let pitch_disagreement = candidate
            .alternatives
            .iter()
            .any(|alternative| alternative.cents_from_target.abs() > 50.0);
        let (voiced_ratio, pitch_mad_cents) = match continuous_f0_source {
            "rmvpe" => (
                candidate.rmvpe_voiced_ratio,
                candidate.rmvpe_pitch_mad_cents,
            ),
            "fcpe" => (
                candidate.fcpe_observed_ratio,
                candidate.fcpe_pitch_mad_cents,
            ),
            _ => (None, None),
        };
        let low_pitch_coverage = voiced_ratio.is_some_and(|ratio| ratio < 0.5);
        let pitch_instability = pitch_mad_cents.is_some_and(|mad| mad > 60.0);
        let boundary_disagreement = candidate.boundary_alternatives.iter().any(|alternative| {
            alternative.materially_disagrees_with(candidate.range, candidate.target_midi)
        });
        let uncertain = pitch_disagreement
            || low_pitch_coverage
            || pitch_instability
            || boundary_disagreement
            || candidate.boundary_kind == BoundaryEvidenceKind::F0Derived
            || candidate.boundary_role == BoundaryCandidateRole::Challenger;
        let decision_trace = fusion_decision_trace(&candidate, continuous_f0_source);
        notes.push(CanonicalNote {
            id: candidate.id,
            range: candidate.range,
            midi_note: candidate.target_midi,
            center_pitch_hz: candidate.center_pitch_hz,
            center_offset_cents,
            confidence: None,
            uncertain,
            alternatives: candidate.alternatives,
            f0_curve: note_f0,
            pitch_bend,
            techniques: candidate.techniques,
            word_id: candidate.word_id,
            evidence: CanonicalNoteEvidence {
                source_experts: source_experts.clone(),
                decision_trace,
                boundary_source: candidate.boundary_source,
                boundary_kind: candidate.boundary_kind,
                boundary_role: candidate.boundary_role,
                boundary_fractional_midi: candidate.boundary_fractional_midi,
                boundary_decision_parameter: candidate.boundary_decision_parameter,
                presence_decision_parameter: candidate.presence_decision_parameter,
                boundary_calibrated_confidence: candidate.boundary_calibrated_confidence,
                target_pitch_source: candidate.target_pitch_source,
                target_pitch_source_local_score: candidate.target_pitch_source_local_score,
                target_pitch_calibrated_confidence: candidate.target_pitch_calibrated_confidence,
                rmvpe_center_hz: candidate.rmvpe_center_hz,
                rmvpe_confidence: candidate.rmvpe_confidence,
                rmvpe_cents_difference: candidate.rmvpe_cents_difference,
                rmvpe_voiced_ratio: candidate.rmvpe_voiced_ratio,
                rmvpe_pitch_mad_cents: candidate.rmvpe_pitch_mad_cents,
                fcpe_center_hz: candidate.fcpe_center_hz,
                fcpe_observed_ratio: candidate.fcpe_observed_ratio,
                fcpe_pitch_mad_cents: candidate.fcpe_pitch_mad_cents,
                fcpe_cents_from_rmvpe: candidate.fcpe_cents_from_rmvpe,
                fcpe_supports_rmvpe: candidate.fcpe_supports_rmvpe,
                technique_evidence: candidate.technique_evidence,
                acoustic: candidate.acoustic,
                basic_pitch: candidate.basic_pitch,
                boundary_alternatives: candidate.boundary_alternatives,
            },
        });
    }
    let track = CanonicalSingingTrack {
        schema_version: 1,
        transcript,
        words,
        notes,
        f0_curve,
        harmony_metadata,
        provenance,
    };
    validate_canonical_singing_track(&track)?;
    Ok(track)
}
