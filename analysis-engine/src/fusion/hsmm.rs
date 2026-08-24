use serde::{Deserialize, Serialize};

use super::{TechniqueScores, TimeRange};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchAlternative {
    pub source_expert: String,
    pub center_hz: f32,
    pub cents_from_target: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcousticCandidateFeatures {
    pub frame_count: usize,
    pub mean_rms: f32,
    pub mean_periodicity: f32,
    pub mean_snr_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_flux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_flux: Option<f32>,
    /// Versioned deterministic feature, not a probability. See FUSION_VERSION.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_supported: Option<bool>,
    /// Maximum source-local Basic Pitch onset activation in this GAME region.
    /// This is not calibrated cross-model confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_pitch_onset_activation: Option<f32>,
    /// Versioned Basic Pitch source-local threshold decision, not probability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_pitch_onset_supported: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentCandidate {
    pub id: String,
    pub range: TimeRange,
    /// Explicit target-note decision derived only from fractional GAME MIDI.
    pub target_midi: u8,
    /// Original fractional GAME estimate retained through graph decoding.
    pub game_midi: f32,
    /// GAME decision configuration retained as configuration, never confidence.
    pub game_boundary_decision_threshold: f32,
    /// GAME decision configuration retained as configuration, never confidence.
    pub game_presence_decision_threshold: f32,
    pub center_pitch_hz: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_center_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_cents_difference: Option<f32>,
    /// Fraction of RMVPE grid frames in this segment that are voiced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_voiced_ratio: Option<f32>,
    /// Robust median absolute deviation around the RMVPE center, in cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmvpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_center_hz: Option<f32>,
    /// Fraction of FCPE grid frames in this segment that contain finite F0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_observed_ratio: Option<f32>,
    /// Robust median absolute deviation around the FCPE center, in cents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_pitch_mad_cents: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_cents_from_rmvpe: Option<f32>,
    /// `Some(true)` means FCPE is within 50 cents of RMVPE; `Some(false)`
    /// records material secondary-expert disagreement without replacing RMVPE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fcpe_supports_rmvpe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acoustic: Option<AcousticCandidateFeatures>,
    #[serde(default)]
    pub techniques: TechniqueScores,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<PitchAlternative>,
}

impl SegmentCandidate {
    pub fn emission_utility(&self) -> Result<f32, String> {
        if self.range.end <= self.range.start
            || self.target_midi > 127
            || !self.game_midi.is_finite()
            || !(0.0..128.0).contains(&self.game_midi)
            || !self.game_boundary_decision_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.game_boundary_decision_threshold)
            || !self.game_presence_decision_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.game_presence_decision_threshold)
            || !self.center_pitch_hz.is_finite()
            || self.center_pitch_hz <= 0.0
            || self.techniques.validated().is_none()
        {
            return Err(format!("invalid segment candidate {}", self.id));
        }
        if self
            .rmvpe_center_hz
            .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self
                .rmvpe_confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self
                .rmvpe_cents_difference
                .is_some_and(|value| !value.is_finite())
            || self
                .rmvpe_voiced_ratio
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self
                .rmvpe_pitch_mad_cents
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || self
                .fcpe_center_hz
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self
                .fcpe_observed_ratio
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self
                .fcpe_pitch_mad_cents
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || self
                .fcpe_cents_from_rmvpe
                .is_some_and(|value| !value.is_finite())
            || self.fcpe_supports_rmvpe.is_some() != self.fcpe_cents_from_rmvpe.is_some()
            || self.alternatives.iter().any(|alternative| {
                alternative.source_expert.trim().is_empty()
                    || !alternative.center_hz.is_finite()
                    || alternative.center_hz <= 0.0
                    || !alternative.cents_from_target.is_finite()
                    || alternative
                        .confidence
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            })
        {
            return Err(format!(
                "candidate {} has invalid optional evidence",
                self.id
            ));
        }
        if let Some(acoustic) = &self.acoustic
            && (acoustic.frame_count == 0
                || !acoustic.mean_rms.is_finite()
                || acoustic.mean_rms < 0.0
                || !acoustic.mean_periodicity.is_finite()
                || !(0.0..=1.0).contains(&acoustic.mean_periodicity)
                || !acoustic.mean_snr_db.is_finite()
                || acoustic
                    .onset_flux
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || acoustic
                    .preceding_flux
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || acoustic
                    .basic_pitch_onset_activation
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || acoustic.basic_pitch_onset_supported.is_some()
                    != acoustic.basic_pitch_onset_activation.is_some())
        {
            return Err(format!(
                "candidate {} has invalid acoustic evidence",
                self.id
            ));
        }

        // A valid GAME region is one explicit duration state. This structural
        // utility is deliberately not serialized or interpreted as confidence.
        let duration_seconds = self.range.end.saturating_sub(self.range.start) as f32 / 1_000_000.0;
        let mut utility = 0.5 + duration_seconds.min(2.0);
        if let Some(cents) = self.rmvpe_cents_difference {
            let absolute = cents.abs();
            if absolute <= 50.0 {
                utility += 0.25;
            } else if absolute >= 600.0 {
                utility -= 0.1;
            }
        }
        Ok(utility)
    }
}

fn transition_utility(previous: &SegmentCandidate, next: &SegmentCandidate) -> f32 {
    let gap_seconds = next.range.start.saturating_sub(previous.range.end) as f32 / 1_000_000.0;
    let gap_penalty = (gap_seconds * 0.05).min(0.5);
    let interval = previous.target_midi.abs_diff(next.target_midi) as f32;
    let mut penalty = match interval as u8 {
        0 => 0.0,
        1..=5 => interval * 0.025,
        6..=11 => 0.15 + (interval - 5.0) * 0.06,
        _ => 0.65 + (interval - 12.0) * 0.1,
    };
    if next.acoustic.as_ref().is_some_and(|features| {
        features.onset_supported == Some(true) || features.basic_pitch_onset_supported == Some(true)
    }) {
        penalty *= 0.35;
    }
    -penalty - gap_penalty
}

/// Duration-aware Viterbi decode over explicit GAME segment states. Optional
/// evidence changes utility only when present; unavailable evidence never
/// contributes a fabricated zero score.
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
            .cmp(&right.range.end)
            .then_with(|| left.range.start.cmp(&right.range.start))
            .then_with(|| left.id.cmp(&right.id))
    });
    let emissions = ordered
        .iter()
        .map(SegmentCandidate::emission_utility)
        .collect::<Result<Vec<_>, _>>()?;
    // Until the graph contains genuine overlapping boundary/pitch alternatives,
    // a validated non-overlapping GAME sequence is already the complete baseline
    // path. Transition smoothness must never delete a legitimate large melodic
    // leap merely because no competing state exists.
    if ordered
        .windows(2)
        .all(|pair| pair[0].range.end <= pair[1].range.start)
    {
        return Ok(ordered);
    }
    let mut best = vec![f32::NEG_INFINITY; ordered.len()];
    let mut previous = vec![None; ordered.len()];

    for index in 0..ordered.len() {
        best[index] = emissions[index];
        for predecessor in 0..index {
            if ordered[predecessor].range.end > ordered[index].range.start {
                continue;
            }
            let utility = best[predecessor]
                + transition_utility(&ordered[predecessor], &ordered[index])
                + emissions[index];
            if utility > best[index] {
                best[index] = utility;
                previous[index] = Some(predecessor);
            }
        }
    }

    let mut endpoint = None;
    for (index, utility) in best.iter().enumerate() {
        if *utility > 0.0 && endpoint.is_none_or(|current| *utility > best[current]) {
            endpoint = Some(index);
        }
    }
    let Some(mut index) = endpoint else {
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
