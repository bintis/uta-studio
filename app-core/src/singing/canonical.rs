use serde::{Deserialize, Serialize};

use super::{
    CanonicalLyrics, CanonicalWordBoundary, EvidenceProvenance, PitchAlternative, SegmentCandidate,
    TechniqueScores, TimeRange,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct F0Point {
    pub time: f64,
    pub hz: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PitchBendPoint {
    pub time: f64,
    pub cents: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalNoteEvidence {
    #[serde(default)]
    pub source_experts: Vec<String>,
    pub pitch_score: f32,
    pub boundary_score: f32,
    pub alignment_score: f32,
    pub technique_score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalNote {
    pub id: String,
    pub range: TimeRange,
    pub midi_note: u8,
    pub center_pitch_hz: f32,
    pub center_offset_cents: f32,
    pub confidence: f32,
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
    #[serde(default)]
    pub lead_harmony_leak_probability: f32,
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

fn note_confidence(candidate: &SegmentCandidate) -> f32 {
    let scores = [
        candidate.pitch_score,
        candidate.boundary_score,
        candidate.duration_score,
        candidate.alignment_score,
    ];
    (scores
        .iter()
        .map(|score| score.clamp(0.0, 1.0))
        .sum::<f32>()
        / scores.len() as f32)
        .clamp(0.0, 1.0)
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
    if f0_curve.windows(2).any(|pair| pair[0].time >= pair[1].time)
        || f0_curve.iter().any(|point| {
            !point.time.is_finite()
                || point.time < 0.0
                || !point.hz.is_finite()
                || point.hz <= 0.0
                || !point.confidence.is_finite()
        })
    {
        return Err("canonical F0 curve is invalid or unordered".to_string());
    }
    let mut notes = Vec::new();
    for candidate in candidates {
        let Some(midi_note) = candidate.midi_note else {
            continue;
        };
        if !candidate.center_pitch_hz.is_finite() || candidate.center_pitch_hz <= 0.0 {
            return Err(format!(
                "candidate {} has invalid center pitch",
                candidate.id
            ));
        }
        let confidence = note_confidence(&candidate);
        let reference_hz = midi_frequency(midi_note);
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
        let close_alternative = candidate.alternatives.iter().any(|alternative| {
            alternative.midi_note != midi_note
                && alternative.probability >= (confidence - 0.12).max(0.35)
        });
        notes.push(CanonicalNote {
            id: candidate.id,
            range: candidate.range,
            midi_note,
            center_pitch_hz: candidate.center_pitch_hz,
            center_offset_cents,
            confidence,
            uncertain: confidence < 0.65 || close_alternative,
            alternatives: candidate.alternatives,
            f0_curve: note_f0,
            pitch_bend,
            techniques: candidate.techniques.clamped(),
            word_id: candidate.word_id,
            evidence: CanonicalNoteEvidence {
                source_experts: provenance
                    .iter()
                    .map(|item| item.expert_id.clone())
                    .collect(),
                pitch_score: candidate.pitch_score,
                boundary_score: candidate.boundary_score,
                alignment_score: candidate.alignment_score,
                technique_score: candidate.technique_score,
            },
        });
    }
    Ok(CanonicalSingingTrack {
        schema_version: 1,
        transcript,
        words,
        notes,
        f0_curve,
        harmony_metadata,
        provenance,
    })
}
