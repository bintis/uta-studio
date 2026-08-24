use serde::{Deserialize, Serialize};

use super::{
    AcousticCandidateFeatures, CanonicalLyrics, CanonicalWordBoundary, EvidenceProvenance,
    PitchAlternative, SegmentCandidate, TechniqueScores, TimeRange,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalNoteEvidence {
    #[serde(default)]
    pub source_experts: Vec<String>,
    pub game_fractional_midi: f32,
    /// Decision configuration retained for audit; not measured confidence.
    pub game_boundary_decision_threshold: f32,
    /// Decision configuration retained for audit; not measured confidence.
    pub game_presence_decision_threshold: f32,
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
    if track
        .notes
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
        || track.notes.iter().any(|note| {
            note.range.end <= note.range.start
                || !note.center_pitch_hz.is_finite()
                || note.center_pitch_hz <= 0.0
                || !note.center_offset_cents.is_finite()
                || note
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || note.techniques.validated().is_none()
                || note
                    .evidence
                    .rmvpe_center_hz
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || note
                    .evidence
                    .rmvpe_confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || note
                    .evidence
                    .rmvpe_cents_difference
                    .is_some_and(|value| !value.is_finite())
                || note
                    .evidence
                    .rmvpe_voiced_ratio
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || note
                    .evidence
                    .rmvpe_pitch_mad_cents
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || note
                    .evidence
                    .fcpe_center_hz
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || note
                    .evidence
                    .fcpe_observed_ratio
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || note
                    .evidence
                    .fcpe_pitch_mad_cents
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || note
                    .evidence
                    .fcpe_cents_from_rmvpe
                    .is_some_and(|value| !value.is_finite())
                || note.evidence.fcpe_supports_rmvpe.is_some()
                    != note.evidence.fcpe_cents_from_rmvpe.is_some()
        })
    {
        return Err("canonical notes are invalid or overlapping".to_string());
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

pub fn build_canonical_singing_track(
    transcript: CanonicalLyrics,
    words: Vec<CanonicalWordBoundary>,
    candidates: Vec<SegmentCandidate>,
    f0_curve: Vec<F0Point>,
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
        let pitch_disagreement = !candidate.alternatives.is_empty();
        let low_pitch_coverage = candidate
            .rmvpe_voiced_ratio
            .is_some_and(|ratio| ratio < 0.5);
        let pitch_instability = candidate
            .rmvpe_pitch_mad_cents
            .is_some_and(|mad| mad > 60.0);
        let uncertain = pitch_disagreement || low_pitch_coverage || pitch_instability;
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
                game_fractional_midi: candidate.game_midi,
                game_boundary_decision_threshold: candidate.game_boundary_decision_threshold,
                game_presence_decision_threshold: candidate.game_presence_decision_threshold,
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
                acoustic: candidate.acoustic,
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
