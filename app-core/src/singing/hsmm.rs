use serde::{Deserialize, Serialize};

use super::{TechniqueScores, TimeRange};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchAlternative {
    pub midi_note: u8,
    pub probability: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentCandidate {
    pub id: String,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_note: Option<u8>,
    pub center_pitch_hz: f32,
    pub pitch_score: f32,
    pub boundary_score: f32,
    pub duration_score: f32,
    pub alignment_score: f32,
    pub technique_score: f32,
    pub symbolic_prior_score: f32,
    #[serde(default)]
    pub onset_strength: f32,
    #[serde(default)]
    pub techniques: TechniqueScores,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<PitchAlternative>,
}

impl SegmentCandidate {
    pub fn emission_score(&self) -> Result<f32, String> {
        if self.range.start < 0.0
            || self.range.end <= self.range.start
            || !self.center_pitch_hz.is_finite()
        {
            return Err(format!("invalid segment candidate {}", self.id));
        }
        let components = [
            self.pitch_score,
            self.boundary_score,
            self.duration_score,
            self.alignment_score,
            self.technique_score,
            self.symbolic_prior_score,
        ];
        if components.iter().any(|score| !score.is_finite()) {
            return Err(format!(
                "segment candidate {} has a non-finite score",
                self.id
            ));
        }
        Ok(components.iter().sum())
    }
}

fn transition_score(previous: &SegmentCandidate, next: &SegmentCandidate) -> f32 {
    let gap = (next.range.start - previous.range.end).max(0.0) as f32;
    let gap_penalty = (gap * 0.05).min(0.5);
    let Some(previous_midi) = previous.midi_note else {
        return -gap_penalty;
    };
    let Some(next_midi) = next.midi_note else {
        return -gap_penalty;
    };
    let interval = previous_midi.abs_diff(next_midi) as f32;
    let mut penalty = match interval as u8 {
        0 => 0.0,
        1..=5 => interval * 0.025,
        6..=11 => 0.15 + (interval - 5.0) * 0.06,
        _ => 0.65 + (interval - 12.0) * 0.1,
    };
    let strong_new_attack =
        next.boundary_score.clamp(0.0, 1.0) * next.onset_strength.clamp(0.0, 1.0);
    penalty *= 1.0 - 0.65 * strong_new_attack;
    if interval <= 2.0 {
        penalty += previous.techniques.vibrato.clamp(0.0, 1.0) * interval * 0.2;
        penalty += previous.techniques.glissando.clamp(0.0, 1.0) * interval * 0.15;
    }
    -penalty - gap_penalty
}

/// Duration-aware Viterbi decode over explicit segment states. Each candidate
/// represents one NOTE/REST state with a duration, so decoding never rounds
/// independent frames into a run of accidental notes.
pub fn decode_candidate_graph(
    candidates: &[SegmentCandidate],
) -> Result<Vec<SegmentCandidate>, String> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut ordered = candidates.to_vec();
    ordered.sort_by(|left, right| {
        left.range
            .end
            .total_cmp(&right.range.end)
            .then_with(|| left.range.start.total_cmp(&right.range.start))
            .then_with(|| left.id.cmp(&right.id))
    });
    let emissions = ordered
        .iter()
        .map(SegmentCandidate::emission_score)
        .collect::<Result<Vec<_>, _>>()?;
    let mut best = vec![f32::NEG_INFINITY; ordered.len()];
    let mut previous = vec![None; ordered.len()];

    for index in 0..ordered.len() {
        best[index] = emissions[index];
        for predecessor in 0..index {
            if ordered[predecessor].range.end > ordered[index].range.start + 1.0e-6 {
                continue;
            }
            let score = best[predecessor]
                + transition_score(&ordered[predecessor], &ordered[index])
                + emissions[index];
            if score > best[index] {
                best[index] = score;
                previous[index] = Some(predecessor);
            }
        }
    }

    let Some(mut index) = best
        .iter()
        .enumerate()
        .filter(|(_, score)| **score > 0.0)
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
    else {
        return Ok(Vec::new());
    };
    let mut selected = Vec::new();
    loop {
        selected.push(ordered[index].clone());
        match previous[index] {
            Some(predecessor) => index = predecessor,
            None => break,
        }
    }
    selected.reverse();
    Ok(selected)
}
