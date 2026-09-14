use serde::{Deserialize, Serialize};

#[path = "native_note_events.rs"]
mod native_note_events;
use native_note_events::NativeNoteEvents;

use super::candidate_states::{
    validate_candidate_evidence_relation_count, validate_candidate_state_count,
};
use super::{HardBoundarySet, TechniqueScores, TimeRange};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchAlternative {
    pub source_expert: String,
    pub center_hz: f32,
    pub cents_from_target: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryEvidenceKind {
    #[default]
    Game,
    F0Derived,
    AdvancedNote,
    BasicPitchOnset,
    AcousticOnset,
    Alignment,
    F0Transition,
    /// A coarser duration state backed by stable continuous-F0 evidence across
    /// an otherwise unsupported primary boundary.
    F0Consolidation,
    /// Duration geometry supported by a measured singing-activity boundary.
    Voicing,
    /// Caller-supplied phrase context. It is a soft melodic reset, not a hard
    /// structural cut unless separately present in `HardBoundarySet`.
    PhraseConstraint,
    Constraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryConstraintKind {
    /// Soft caller phrase-start context. Confidence attenuates only melody and
    /// short-octave-return priors; it is never a structural cut.
    PhraseStart,
    WordStart,
    WordEnd,
    VoicingTransition,
    PitchDiscontinuity,
    BasicPitchOnset,
    AcousticArticulation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryConstraintEvidence {
    pub source_expert: String,
    pub kind: BoundaryConstraintKind,
    pub time: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_group: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

impl BoundaryConstraintEvidence {
    fn is_valid(&self) -> bool {
        !self.source_expert.trim().is_empty()
            && self
                .source_local_strength
                .is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            && self
                .calibrated_confidence
                .is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            && (self.calibrated_confidence.is_none()
                || self
                    .calibration_version
                    .as_deref()
                    .is_some_and(|version| !version.trim().is_empty()))
            && self.depends_on.iter().all(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryCandidateRole {
    #[default]
    Primary,
    Challenger,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryAlternative {
    pub source_expert: String,
    pub range: TimeRange,
    #[serde(default)]
    pub kind: BoundaryEvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fractional_midi: Option<f32>,
    /// Source-local score or caller-supplied confidence. It is retained for audit
    /// and same-source thresholding; it is never compared across model families as
    /// though it were a calibrated global probability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local_pitch_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_boundary_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_pitch_confidence: Option<f32>,
    #[serde(default)]
    pub hard: bool,
}

impl BoundaryAlternative {
    pub fn materially_disagrees_with(&self, range: TimeRange, target_midi: u8) -> bool {
        const BOUNDARY_TOLERANCE: u64 = 50_000;
        self.range.start.abs_diff(range.start) > BOUNDARY_TOLERANCE
            || self.range.end.abs_diff(range.end) > BOUNDARY_TOLERANCE
            || self
                .fractional_midi
                .is_some_and(|midi| (midi - f32::from(target_midi)).abs() > 0.75)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AcousticCandidateFeatures {
    pub frame_count: usize,
    pub mean_rms: f32,
    pub mean_periodicity: f32,
    /// Robust center of independent Acoustic DSP fundamentals in this range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fundamental_center_hz: Option<f32>,
    pub mean_snr_db: f32,
    #[serde(default)]
    pub mean_vibrato_activation: f32,
    #[serde(default)]
    pub mean_glide_activation: f32,
    #[serde(default)]
    pub mean_ornament_activation: f32,
    #[serde(default)]
    pub mean_breath_activation: f32,
    #[serde(default)]
    pub max_voicing_transition_activation: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_flux: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_flux: Option<f32>,
    /// Versioned deterministic feature, not a probability. See FUSION_VERSION.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_supported: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BasicPitchCandidateFeatures {
    pub onset_activation: f32,
    #[serde(default)]
    pub note_activation: f32,
    #[serde(default)]
    pub contour_activation: f32,
    #[serde(default)]
    pub contour_class: usize,
    /// Source-local threshold decision, not a calibrated probability.
    pub onset_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TechniqueCandidateFeatures {
    pub source_expert: String,
    pub calibration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrato_activation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glissando_activation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falsetto_activation: Option<f32>,
}

impl TechniqueCandidateFeatures {
    fn continuity_activation(&self) -> Option<f32> {
        [self.vibrato_activation, self.glissando_activation]
            .into_iter()
            .flatten()
            .max_by(f32::total_cmp)
    }

    fn is_valid(&self) -> bool {
        !self.source_expert.trim().is_empty()
            && !self.calibration.trim().is_empty()
            && [
                self.vibrato_activation,
                self.glissando_activation,
                self.falsetto_activation,
            ]
            .into_iter()
            .flatten()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentCandidate {
    pub id: String,
    pub range: TimeRange,
    pub target: super::CandidateTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voicing_evidence: Option<super::voicing::VoicingCandidateEvidence>,
    pub boundary_source: String,
    pub boundary_kind: BoundaryEvidenceKind,
    #[serde(default)]
    pub boundary_role: BoundaryCandidateRole,
    /// Fractional target-pitch proposal supplied by the selected boundary expert.
    /// F0-derived segmentation leaves this absent and derives pitch candidates
    /// separately from the selected continuous-F0 evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_fractional_midi: Option<f32>,
    /// Source-local decision configuration retained for audit, never confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_decision_parameter: Option<f32>,
    /// Source-local presence configuration retained for audit, never confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_decision_parameter: Option<f32>,
    #[serde(default)]
    pub boundary_hard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_support: Option<f32>,
    /// Optional calibrated confidence for this source-local boundary decision.
    /// It is absent unless a versioned calibrator supplied it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_calibrated_confidence: Option<f32>,
    pub target_pitch_source: String,
    /// Source-local pitch support retained for audit, never treated as a
    /// cross-expert probability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pitch_source_local_score: Option<f32>,
    /// Versioned calibrated confidence for the selected discrete pitch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_pitch_calibrated_confidence: Option<f32>,
    /// Target error outside the 50-cent band, in semitone-seconds, from
    /// absolute-time primary F0 and reliable independent DSP observations.
    /// Repartitioning one target must preserve the sum of this observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuous_pitch_error_integral: Option<f32>,
    /// Trustworthy observed time in microseconds. Zero observed time is not
    /// evidence of a perfectly fitting target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuous_pitch_observed_duration: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic_pitch: Option<BasicPitchCandidateFeatures>,
    #[serde(default)]
    pub boundary_alternatives: Vec<BoundaryAlternative>,
    #[serde(default)]
    pub boundary_constraints: Vec<BoundaryConstraintEvidence>,
    #[serde(default)]
    pub technique_evidence: Vec<TechniqueCandidateFeatures>,
    #[serde(default)]
    pub techniques: TechniqueScores,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_id: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<PitchAlternative>,
}

const BOUNDARY_CONTEXT_TOLERANCE: u64 = 50_000;
const ATTACK_CONTEXT_TOLERANCE: u64 = 60_000;

fn constraint_group(constraint: &BoundaryConstraintEvidence) -> &str {
    match constraint.kind {
        BoundaryConstraintKind::BasicPitchOnset => "basic_pitch",
        BoundaryConstraintKind::AcousticArticulation => "acoustic",
        _ => constraint
            .correlation_group
            .as_deref()
            .or_else(|| constraint.depends_on.first().map(String::as_str))
            .unwrap_or(&constraint.source_expert),
    }
}

fn boundary_group(candidate: &SegmentCandidate) -> &str {
    match candidate.boundary_kind {
        BoundaryEvidenceKind::BasicPitchOnset => "basic_pitch",
        BoundaryEvidenceKind::AcousticOnset => "acoustic",
        BoundaryEvidenceKind::Alignment => "forced_alignment",
        BoundaryEvidenceKind::F0Derived
        | BoundaryEvidenceKind::F0Transition
        | BoundaryEvidenceKind::F0Consolidation => "continuous-pitch-neural",
        _ => &candidate.boundary_source,
    }
}

/// An event near both ends of a short state belongs to its nearer edge.
/// End evidence is retained on the candidate for audit, but cannot pay for
/// another onset at the beginning of the same state.
fn belongs_to_start(candidate: &SegmentCandidate, time: u64) -> bool {
    time.abs_diff(candidate.range.start) <= time.abs_diff(candidate.range.end)
}

fn start_attack_event(
    candidate: &SegmentCandidate,
    kind: BoundaryConstraintKind,
) -> Option<&BoundaryConstraintEvidence> {
    candidate
        .boundary_constraints
        .iter()
        .filter(|constraint| {
            constraint.kind == kind
                && constraint.time.abs_diff(candidate.range.start) <= ATTACK_CONTEXT_TOLERANCE
        })
        .min_by_key(|constraint| {
            (
                constraint.time.abs_diff(candidate.range.start),
                constraint.time,
            )
        })
}

fn attack_belongs_to_start(candidate: &SegmentCandidate, kind: BoundaryConstraintKind) -> bool {
    start_attack_event(candidate, kind)
        .is_none_or(|constraint| belongs_to_start(candidate, constraint.time))
}

#[derive(Default)]
struct BoundaryEventScore {
    strength: f32,
    reward: f32,
}

/// A boundary proposal, an attack feature and a context annotation can all be
/// views of the same model observation. Credit its strongest contribution once,
/// using the existing feature weights. Only independent source groups add.
fn boundary_event_groups(
    candidate: &SegmentCandidate,
) -> std::collections::BTreeMap<&str, BoundaryEventScore> {
    let mut groups = std::collections::BTreeMap::<&str, BoundaryEventScore>::new();
    let mut add = |group, strength: f32, reward: f32| {
        let entry = groups.entry(group).or_default();
        entry.strength = entry.strength.max(strength);
        entry.reward = entry.reward.max(reward);
    };
    if candidate.boundary_hard {
        add("caller-hard-boundary", 1.5, 0.65);
    } else {
        if let Some(support) = candidate.boundary_support {
            add(boundary_group(candidate), support * 0.6, support * 0.25);
        }
        if let Some(confidence) = candidate.boundary_calibrated_confidence {
            add(
                boundary_group(candidate),
                confidence,
                ((confidence - 0.5) * 0.8).max(0.0),
            );
        }
    }
    for constraint in &candidate.boundary_constraints {
        if constraint.time.abs_diff(candidate.range.start) > BOUNDARY_CONTEXT_TOLERANCE
            || !belongs_to_start(candidate, constraint.time)
        {
            continue;
        }
        if let Some(value) = constraint
            .calibrated_confidence
            .or(constraint.source_local_strength)
        {
            add(constraint_group(constraint), value * 0.6, value * 0.2);
        }
    }
    if candidate.basic_pitch.as_ref().is_some_and(|features| {
        features.onset_supported
            && features.onset_activation >= 0.9
            && features.note_activation >= 0.75
    }) && attack_belongs_to_start(candidate, BoundaryConstraintKind::BasicPitchOnset)
    {
        add("basic_pitch", 0.9, 0.7 * 0.9 / 1.5);
    }
    if candidate
        .acoustic
        .as_ref()
        .is_some_and(|features| features.onset_supported == Some(true))
        && attack_belongs_to_start(candidate, BoundaryConstraintKind::AcousticArticulation)
    {
        add("acoustic", 0.8, 0.7 * 0.8 / 1.5);
    }
    groups
}

fn boundary_event_score(candidate: &SegmentCandidate) -> BoundaryEventScore {
    boundary_event_groups(candidate).values().fold(
        BoundaryEventScore::default(),
        |mut combined, group| {
            combined.strength += group.strength;
            combined.reward += group.reward;
            combined
        },
    )
}

fn shares_start_attack(
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    kind: BoundaryConstraintKind,
) -> bool {
    start_attack_event(previous, kind)
        .zip(start_attack_event(next, kind))
        .is_some_and(|(prior, event)| {
            prior.time == event.time
                && belongs_to_start(previous, prior.time)
                && belongs_to_start(next, event.time)
        })
}

/// A path can observe the same peak from two neighboring state windows.
/// Their total reward is the maximum contribution of that observation, not
/// the sum; a different measured peak remains an independent note event.
fn repeated_attack_reward(previous: &SegmentCandidate, next: &SegmentCandidate) -> f32 {
    let shared = [
        (BoundaryConstraintKind::BasicPitchOnset, "basic_pitch"),
        (BoundaryConstraintKind::AcousticArticulation, "acoustic"),
    ];
    let shared = shared
        .into_iter()
        .filter(|(kind, _)| shares_start_attack(previous, next, *kind))
        .collect::<Vec<_>>();
    if shared.is_empty() {
        return 0.0;
    }
    let before = boundary_event_groups(previous);
    let after = boundary_event_groups(next);
    shared
        .into_iter()
        .filter_map(|(_, group)| before.get(group).zip(after.get(group)))
        .map(|(before, after)| before.reward.min(after.reward))
        .sum()
}

fn distinct_onset_supported(previous: &SegmentCandidate, next: &SegmentCandidate) -> bool {
    let acoustic = next
        .acoustic
        .as_ref()
        .is_some_and(|features| features.onset_supported == Some(true))
        && attack_belongs_to_start(next, BoundaryConstraintKind::AcousticArticulation)
        && !shares_start_attack(previous, next, BoundaryConstraintKind::AcousticArticulation);
    let basic_pitch = next
        .basic_pitch
        .as_ref()
        .is_some_and(|features| features.onset_supported)
        && attack_belongs_to_start(next, BoundaryConstraintKind::BasicPitchOnset)
        && !shares_start_attack(previous, next, BoundaryConstraintKind::BasicPitchOnset);
    acoustic || basic_pitch
}

/// Attaches only boundary-local contextual evidence to each duration state.
/// Correlated observations remain typed and are discounted during utility
/// evaluation rather than counted as independent votes.
pub fn attach_boundary_constraints(
    candidates: &mut [SegmentCandidate],
    constraints: &[BoundaryConstraintEvidence],
) -> Result<(), String> {
    // This executes after pitch-state expansion, so the same cumulative
    // candidate/evidence relation limit must protect this attachment pass too.
    validate_candidate_evidence_relation_count(candidates.len(), constraints.len())?;
    let mut indexed = constraints.iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by_key(|left| (left.1.time, left.0));
    for candidate in candidates {
        let mut matches = Vec::<(usize, &BoundaryConstraintEvidence)>::new();
        for edge in [candidate.range.start, candidate.range.end] {
            let lower = edge.saturating_sub(ATTACK_CONTEXT_TOLERANCE);
            let upper = edge.saturating_add(ATTACK_CONTEXT_TOLERANCE);
            let first = indexed.partition_point(|(_, constraint)| constraint.time < lower);
            let end = indexed.partition_point(|(_, constraint)| constraint.time <= upper);
            matches.extend(indexed[first..end].iter().copied().filter(|(_, event)| {
                matches!(
                    event.kind,
                    BoundaryConstraintKind::BasicPitchOnset
                        | BoundaryConstraintKind::AcousticArticulation
                ) || event.time.abs_diff(edge) <= BOUNDARY_CONTEXT_TOLERANCE
            }));
        }
        // Preserve the caller's deterministic constraint order and avoid a
        // duplicate when one event is local to both edges of a short state.
        matches.sort_by_key(|(index, _)| *index);
        matches.dedup_by_key(|(index, _)| *index);
        candidate.boundary_constraints = matches
            .into_iter()
            .map(|(_, constraint)| constraint.clone())
            .collect();
    }
    Ok(())
}

impl SegmentCandidate {
    fn validate(&self) -> Result<(), String> {
        if self.range.end <= self.range.start
            || self
                .voicing_evidence
                .as_ref()
                .is_some_and(|evidence| !evidence.valid_for(self.range))
            || (!self.target.is_pitched()
                && self.voicing_evidence.as_ref().is_none_or(|evidence| {
                    evidence.duration() != self.range.end - self.range.start
                }))
            || self.target.as_pitched().is_some_and(|(midi, center_hz)| {
                midi > 127 || !center_hz.is_finite() || center_hz <= 0.0
            })
            || self.boundary_source.trim().is_empty()
            || self.target_pitch_source.trim().is_empty()
            || self
                .boundary_fractional_midi
                .is_some_and(|value| !value.is_finite() || !(0.0..128.0).contains(&value))
            || self
                .boundary_decision_parameter
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self
                .presence_decision_parameter
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || (self.boundary_kind == BoundaryEvidenceKind::Game
                && self.boundary_fractional_midi.is_none())
            || (self.boundary_role == BoundaryCandidateRole::Challenger
                && matches!(
                    self.boundary_kind,
                    BoundaryEvidenceKind::Game | BoundaryEvidenceKind::F0Derived
                ))
            || self.boundary_alternatives.iter().any(|alternative| {
                alternative.source_expert.trim().is_empty()
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
                    .any(|score| !score.is_finite() || !(0.0..=1.0).contains(&score))
            })
            || self
                .boundary_constraints
                .iter()
                .any(|evidence| !evidence.is_valid())
            || self
                .technique_evidence
                .iter()
                .any(|evidence| !evidence.is_valid())
            || self
                .continuous_pitch_error_integral
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || self
                .boundary_support
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || [
                self.boundary_calibrated_confidence,
                self.target_pitch_source_local_score,
                self.target_pitch_calibrated_confidence,
            ]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
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
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || acoustic
                    .onset_flux
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || acoustic
                    .preceding_flux
                    .is_some_and(|value| !value.is_finite() || value < 0.0))
        {
            return Err(format!(
                "candidate {} has invalid acoustic evidence",
                self.id
            ));
        }
        if let Some(basic_pitch) = &self.basic_pitch
            && ([
                basic_pitch.onset_activation,
                basic_pitch.note_activation,
                basic_pitch.contour_activation,
            ]
            .into_iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || basic_pitch.contour_class >= 264)
        {
            return Err(format!(
                "candidate {} has invalid Basic Pitch evidence",
                self.id
            ));
        }

        Ok(())
    }

    pub fn emission_utility(&self) -> Result<f32, String> {
        self.validate()?;
        // Occupancy/pitch observations are rates integrated over covered time.
        // Repeating the same observation in shorter states must not multiply
        // its reward. Only boundary-local evidence can pay the state cost.
        let duration_seconds = self.range.end.saturating_sub(self.range.start) as f32 / 1_000_000.0;
        if !self.target.is_pitched() {
            return Ok(duration_seconds);
        }
        let unsupported_seconds = self
            .voicing_evidence
            .as_ref()
            .map_or(0.0, |evidence| evidence.duration() as f32 / 1_000_000.0);
        // Coverage credit is shared with explicit unpitched states. Only a
        // pitched target pays for time where all activity sources lack support.
        // This absolute-time term is additive under arbitrary state cuts.
        const UNSUPPORTED_PITCH_WEIGHT: f32 = 3.0;
        let mut utility = duration_seconds - 0.45 - unsupported_seconds * UNSUPPORTED_PITCH_WEIGHT;
        // Pitch proposals for the same duration geometry remain peers. A pitch
        // does not gain semantic authority merely because its expert also
        // supplied the boundary object.
        // A segment median can hide a shorter real pitch plateau inside a
        // long note. Integrate target error over fixed absolute-time
        // observations instead. The scale is a decoder utility weight, not a
        // calibrated model confidence. Fractional pitch targets and the
        // tolerance band reduce a short plateau's loss; its integrated cost
        // must still compete with the extra state and legato transition.
        const PITCH_ERROR_WEIGHT: f32 = 3.0;
        utility -= self.continuous_pitch_error_integral.unwrap_or(0.0) * PITCH_ERROR_WEIGHT;
        let event = boundary_event_score(self);
        utility += event.reward;
        let event_quality = (event.strength / 1.5).clamp(0.0, 1.0);
        let shortness = ((0.2 - duration_seconds) / 0.16).clamp(0.0, 1.0);
        if onset_supported(self) {
            utility -= shortness * (1.0 - event_quality) * 0.35;
        } else {
            utility -= shortness * 0.15;
        }
        // Acoustic fundamental is independent, target-relative pitch evidence.
        // It may support a peer proposal but never confers authority based on
        // which expert supplied the candidate's boundary geometry.
        utility += duration_seconds * acoustic_fundamental_support(self);
        if let Some(basic_pitch) = &self.basic_pitch {
            // These are source-local occupancy/contour activations. They are used as
            // fixed versioned features and never compared to another model's score.
            if basic_pitch.note_activation >= 0.5 {
                utility += duration_seconds * 0.1;
            }
            if basic_pitch.contour_activation >= 0.5 {
                utility += duration_seconds * 0.05;
            }
        }
        // Positive calibration support was credited to its source above.
        // Keep negative calibration evidence as a penalty.
        Ok(utility
            + self
                .boundary_calibrated_confidence
                .map_or(0.0, |confidence| ((confidence - 0.5) * 0.8).min(0.0)))
    }
}

fn onset_supported(candidate: &SegmentCandidate) -> bool {
    (candidate
        .acoustic
        .as_ref()
        .is_some_and(|features| features.onset_supported == Some(true))
        && attack_belongs_to_start(candidate, BoundaryConstraintKind::AcousticArticulation))
        || (candidate
            .basic_pitch
            .as_ref()
            .is_some_and(|features| features.onset_supported)
            && attack_belongs_to_start(candidate, BoundaryConstraintKind::BasicPitchOnset))
}

/// Boundary-local support for a semantic note event. Sustained pitch evidence
/// deliberately does not count here: stable F0 can validate a pitch target, but
/// it cannot turn a carried tone into a new onset.
fn note_event_support(candidate: &SegmentCandidate) -> f32 {
    boundary_event_score(candidate).strength
}

fn target_relative_expert_support(
    center_hz: Option<f32>,
    observed_ratio: Option<f32>,
    pitch_mad_cents: Option<f32>,
    target_hz: f32,
) -> f32 {
    const TARGET_AGREEMENT_CENTS: f32 = 50.0;
    let (Some(center_hz), Some(observed_ratio), Some(pitch_mad_cents)) =
        (center_hz, observed_ratio, pitch_mad_cents)
    else {
        return 0.0;
    };
    let target_distance = (1_200.0 * (center_hz / target_hz).log2()).abs();
    if !target_distance.is_finite() || target_distance > TARGET_AGREEMENT_CENTS {
        return 0.0;
    }
    observed_ratio * (1.0 - pitch_mad_cents / 180.0).clamp(0.0, 1.0)
}

pub(super) fn sustained_pitch_support(candidate: &SegmentCandidate) -> f32 {
    candidate.target.center_hz().map_or(0.0, |target_hz| {
        sustained_pitch_support_for_target(candidate, target_hz)
    })
}

fn sustained_pitch_support_for_target(candidate: &SegmentCandidate, target_hz: f32) -> f32 {
    let rmvpe = target_relative_expert_support(
        candidate.rmvpe_center_hz,
        candidate.rmvpe_voiced_ratio,
        candidate.rmvpe_pitch_mad_cents,
        target_hz,
    );
    let fcpe = target_relative_expert_support(
        candidate.fcpe_center_hz,
        candidate.fcpe_observed_ratio,
        candidate.fcpe_pitch_mad_cents,
        target_hz,
    );
    let peer_agreement = candidate
        .fcpe_cents_from_rmvpe
        .filter(|cents| rmvpe > 0.0 && fcpe > 0.0 && cents.abs() <= 50.0)
        .map_or(0.0, |_| 0.2);
    (rmvpe.max(fcpe) + peer_agreement).clamp(0.0, 1.0)
}

pub(super) fn acoustic_fundamental_support(candidate: &SegmentCandidate) -> f32 {
    candidate.target.center_hz().map_or(0.0, |target_hz| {
        acoustic_fundamental_support_for_target(candidate, target_hz)
    })
}

fn acoustic_fundamental_support_for_target(candidate: &SegmentCandidate, target_hz: f32) -> f32 {
    let Some(acoustic) = candidate.acoustic.as_ref() else {
        return 0.0;
    };
    let Some(fundamental_hz) = acoustic
        .fundamental_center_hz
        .filter(|hz| hz.is_finite() && *hz > 0.0)
    else {
        return 0.0;
    };
    if !acoustic.mean_periodicity.is_finite() || acoustic.mean_periodicity <= 0.0 {
        return 0.0;
    }
    let cents = (1_200.0_f32 * (fundamental_hz / target_hz).log2()).abs();
    if !cents.is_finite() || cents > 50.0 {
        return 0.0;
    }
    acoustic.mean_periodicity.clamp(0.0, 1.0) * (1.0 - cents / 50.0).clamp(0.0, 1.0)
}

fn expressive_continuity(previous: &SegmentCandidate, next: &SegmentCandidate) -> bool {
    let technique = previous
        .technique_evidence
        .iter()
        .chain(&next.technique_evidence)
        .filter_map(TechniqueCandidateFeatures::continuity_activation)
        .max_by(f32::total_cmp)
        .is_some_and(|activation| activation >= 0.6);
    let acoustic = previous
        .acoustic
        .iter()
        .chain(next.acoustic.iter())
        .flat_map(|features| {
            [
                features.mean_vibrato_activation,
                features.mean_glide_activation,
                features.mean_ornament_activation,
            ]
        })
        .max_by(f32::total_cmp)
        .is_some_and(|activation| activation >= 0.35);
    technique || acoustic
}

fn pitch_interval_cents(previous: &SegmentCandidate, next: &SegmentCandidate) -> f32 {
    previous
        .target
        .center_hz()
        .zip(next.target.center_hz())
        .map_or(0.0, |(previous_hz, next_hz)| {
            1_200.0 * (next_hz / previous_hz).log2().abs()
        })
}

fn phrase_start_strength(candidate: &SegmentCandidate) -> f32 {
    candidate
        .boundary_constraints
        .iter()
        .filter(|constraint| {
            constraint.kind == BoundaryConstraintKind::PhraseStart
                && constraint.time.abs_diff(candidate.range.start) <= CONTEXT_RESET_TOLERANCE
        })
        .filter_map(|constraint| {
            constraint
                .calibrated_confidence
                .or(constraint.source_local_strength)
        })
        .max_by(f32::total_cmp)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn has_typed_reset_between(
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    hard_boundaries: &HardBoundaryTimeIndex,
    voicing_reset_times: &[u64],
) -> bool {
    !previous.target.is_pitched()
        || !next.target.is_pitched()
        || hard_boundaries.resets_between(
            previous.range.end,
            next.range.start,
            HARD_BOUNDARY_TOLERANCE,
        )
        || voicing_reset_times.iter().any(|time| {
            *time >= previous.range.end.saturating_sub(CONTEXT_RESET_TOLERANCE)
                && *time <= next.range.start.saturating_add(CONTEXT_RESET_TOLERANCE)
        })
}

#[cfg(test)]
pub(super) fn transition_utility(
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    hard_boundaries: &HardBoundarySet,
    voicing_reset_times: &[u64],
) -> f32 {
    transition_utility_indexed(
        previous,
        next,
        &HardBoundaryTimeIndex::new(hard_boundaries),
        voicing_reset_times,
    )
}

fn transition_utility_indexed(
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    hard_boundaries: &HardBoundaryTimeIndex,
    voicing_reset_times: &[u64],
) -> f32 {
    if has_typed_reset_between(previous, next, hard_boundaries, voicing_reset_times) {
        return 0.0;
    }

    let duration = next.range.end.saturating_sub(next.range.start);
    let onset = distinct_onset_supported(previous, next)
        && (duration > 100_000 || note_event_support(next) >= 0.85);
    let contextual_boundary = matches!(
        next.boundary_kind,
        BoundaryEvidenceKind::Alignment | BoundaryEvidenceKind::F0Transition
    );
    let mut event_cost = if next.boundary_role == BoundaryCandidateRole::Challenger {
        0.5
    } else {
        0.35
    };
    let word_transition = previous
        .word_id
        .as_ref()
        .zip(next.word_id.as_ref())
        .is_some_and(|(previous_word, next_word)| previous_word != next_word);
    if onset {
        event_cost *= 0.08;
    } else if contextual_boundary {
        event_cost *= 0.45;
    } else if word_transition {
        event_cost *= 0.35;
    }
    if expressive_continuity(previous, next) && !onset {
        event_cost += 0.68;
    }

    // Melody distance is expressed in cents and intentionally not capped. A
    // measured attack can support a real leap, but unsupported octave churn
    // remains much more expensive than ordinary stepwise motion.
    let interval = pitch_interval_cents(previous, next);
    let pitch_support = sustained_pitch_support(next);
    let phrase_relaxation = phrase_start_strength(next);
    let melody_cost = ((interval - 350.0).max(0.0) / 1_200.0)
        * 0.45
        * (1.0 - 0.55 * pitch_support)
        * (1.0 - phrase_relaxation);
    -(event_cost + melody_cost + repeated_attack_reward(previous, next))
}

#[cfg(test)]
pub(super) fn short_octave_return_penalty_for_test(
    previous_previous: &SegmentCandidate,
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    hard_boundaries: &HardBoundarySet,
    voicing_reset_times: &[u64],
) -> f32 {
    short_octave_return_penalty(
        previous_previous,
        previous,
        next,
        &HardBoundaryTimeIndex::new(hard_boundaries),
        voicing_reset_times,
    )
}

fn short_octave_return_penalty(
    previous_previous: &SegmentCandidate,
    previous: &SegmentCandidate,
    next: &SegmentCandidate,
    hard_boundaries: &HardBoundaryTimeIndex,
    voicing_reset_times: &[u64],
) -> f32 {
    if has_typed_reset_between(
        previous_previous,
        previous,
        hard_boundaries,
        voicing_reset_times,
    ) || has_typed_reset_between(previous, next, hard_boundaries, voicing_reset_times)
    {
        return 0.0;
    }
    let middle_duration = previous.range.end.saturating_sub(previous.range.start) as f32;
    let outer_interval = pitch_interval_cents(previous_previous, next);
    let excursion =
        pitch_interval_cents(previous_previous, previous).min(pitch_interval_cents(previous, next));
    let octave_similarity = (1.0 - (excursion - 1_200.0).abs() / 350.0).clamp(0.0, 1.0);
    let return_similarity = (1.0 - outer_interval / 180.0).clamp(0.0, 1.0);
    let shortness = ((220_000.0 - middle_duration) / 180_000.0).clamp(0.0, 1.0);
    let weak_evidence = (1.0 - note_event_support(previous) / 2.0).clamp(0.15, 1.0);
    let phrase_relaxation = phrase_start_strength(previous).max(phrase_start_strength(next));
    -(1.25
        * octave_similarity
        * return_similarity
        * shortness
        * weak_evidence
        * (1.0 - phrase_relaxation))
}

const HARD_BOUNDARY_TOLERANCE: u64 = 0;
const CONTEXT_RESET_TOLERANCE: u64 = 20_000;

/// One immutable sorted edge index is shared across candidate validation and
/// every pair-state transition in a decode. This prevents repeated boundary
/// allocation/scanning from escaping the explicit graph-work budget.
struct HardBoundaryTimeIndex {
    times: Vec<u64>,
}

impl HardBoundaryTimeIndex {
    fn new(boundaries: &HardBoundarySet) -> Self {
        Self {
            times: boundaries.edge_times(),
        }
    }

    fn crosses(&self, range: TimeRange, tolerance: u64) -> bool {
        let lower = range.start.saturating_add(tolerance);
        let index = self.times.partition_point(|time| *time <= lower);
        self.times
            .get(index)
            .is_some_and(|time| time.saturating_add(tolerance) < range.end)
    }

    fn resets_between(&self, previous_end: u64, next_start: u64, tolerance: u64) -> bool {
        let lower = previous_end.saturating_sub(tolerance);
        let upper = next_start.saturating_add(tolerance);
        let index = self.times.partition_point(|time| *time < lower);
        self.times.get(index).is_some_and(|time| *time <= upper)
    }
}

fn voicing_reset_times(candidates: &[SegmentCandidate]) -> Vec<u64> {
    let mut times = candidates
        .iter()
        .flat_map(|candidate| &candidate.boundary_constraints)
        .filter(|constraint| constraint.kind == BoundaryConstraintKind::VoicingTransition)
        .map(|constraint| constraint.time)
        .collect::<Vec<_>>();
    times.sort_unstable();
    times.dedup();
    times
}

fn candidate_crosses_hard_boundary(
    candidate: &SegmentCandidate,
    hard_boundaries: &HardBoundaryTimeIndex,
) -> bool {
    hard_boundaries.crosses(candidate.range, HARD_BOUNDARY_TOLERANCE)
}

/// Validates the complete candidate pool before either selector runs. Structural
/// validity is intentionally separate from Algorithm scoring policy.
pub fn validate_candidate_pool(candidate_pool: &[SegmentCandidate]) -> Result<(), String> {
    if candidate_pool.is_empty() {
        return Err("candidate pool is empty".to_string());
    }
    validate_candidate_state_count(candidate_pool.len())?;
    let mut ids = std::collections::BTreeSet::new();
    for candidate in candidate_pool {
        if candidate.id.trim().is_empty() {
            return Err("candidate pool contains an empty candidate id".to_string());
        }
        if !ids.insert(candidate.id.as_str()) {
            return Err(format!(
                "candidate pool contains duplicate candidate id {}",
                candidate.id
            ));
        }
        candidate.validate()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct CoverageComponent {
    start: u64,
    end: u64,
}

/// Connected interval unions are the voiced regions represented by the pool.
/// A gap with no candidate is an observed pool-level rest; skipping a candidate
/// inside a connected union can no longer fabricate a rest or reset.
fn coverage_components(candidates: &[SegmentCandidate]) -> Vec<CoverageComponent> {
    let primary_ranges = candidates
        .iter()
        .filter(|candidate| candidate.boundary_role == BoundaryCandidateRole::Primary)
        .map(|candidate| candidate.range)
        .collect::<Vec<_>>();
    let mut ranges = if primary_ranges.is_empty() {
        candidates
            .iter()
            .map(|candidate| candidate.range)
            .collect::<Vec<_>>()
    } else {
        primary_ranges
    };
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut components: Vec<CoverageComponent> = Vec::new();
    for range in ranges {
        if let Some(component) = components.last_mut()
            && range.start <= component.end
        {
            component.end = component.end.max(range.end);
        } else {
            components.push(CoverageComponent {
                start: range.start,
                end: range.end,
            });
        }
    }
    components
}

/// Applies the selector-independent safety gate to a proposed final candidate
/// path. Algorithm and AI must return verbatim states, respect hard boundaries,
/// and exactly cover every connected voiced component represented by the pool.
pub fn validate_candidate_path(
    candidate_pool: &[SegmentCandidate],
    selected: &[SegmentCandidate],
) -> Result<(), String> {
    validate_candidate_path_with_boundaries(candidate_pool, selected, &HardBoundarySet::default())
}

pub fn validate_candidate_path_with_boundaries(
    candidate_pool: &[SegmentCandidate],
    selected: &[SegmentCandidate],
    hard_boundaries: &HardBoundarySet,
) -> Result<(), String> {
    validate_candidate_pool(candidate_pool)?;
    hard_boundaries.validate()?;
    let hard_boundary_times = HardBoundaryTimeIndex::new(hard_boundaries);
    validate_candidate_path_with_index(candidate_pool, selected, &hard_boundary_times)
}

fn validate_candidate_path_with_index(
    candidate_pool: &[SegmentCandidate],
    selected: &[SegmentCandidate],
    hard_boundary_times: &HardBoundaryTimeIndex,
) -> Result<(), String> {
    if selected.is_empty() {
        return Err("candidate path is empty".to_string());
    }
    let known = candidate_pool
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut selected_ids = std::collections::BTreeSet::new();
    let mut previous_end = None;
    for candidate in selected {
        if !selected_ids.insert(candidate.id.as_str()) {
            return Err(format!(
                "candidate path contains duplicate candidate id {}",
                candidate.id
            ));
        }
        match known.get(candidate.id.as_str()) {
            Some(original) if *original == candidate => {}
            _ => {
                return Err(format!(
                    "candidate path contains candidate {} that is not verbatim in the candidate pool",
                    candidate.id
                ));
            }
        }
        if previous_end.is_some_and(|end| end > candidate.range.start) {
            return Err("candidate path is not ordered and non-overlapping".to_string());
        }
        if candidate_crosses_hard_boundary(candidate, hard_boundary_times) {
            return Err(format!(
                "candidate path candidate {} crosses a hard boundary",
                candidate.id
            ));
        }
        previous_end = Some(candidate.range.end);
    }
    let components = coverage_components(candidate_pool);
    let mut selected_index = 0usize;
    for component in components {
        let mut cursor = component.start;
        while let Some(candidate) = selected.get(selected_index) {
            if candidate.range.start >= component.end {
                break;
            }
            if candidate.range.start != cursor || candidate.range.end > component.end {
                return Err(format!(
                    "candidate path does not exactly cover voiced component {}..{}",
                    component.start, component.end
                ));
            }
            cursor = candidate.range.end;
            selected_index += 1;
        }
        if cursor != component.end {
            return Err(format!(
                "candidate path does not exactly cover voiced component {}..{}",
                component.start, component.end
            ));
        }
    }
    if let Some(candidate) = selected.get(selected_index) {
        return Err(format!(
            "candidate path candidate {} lies outside represented voiced coverage",
            candidate.id
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct PairState {
    score: f32,
    previous_previous: Option<usize>,
}

const MAX_PAIR_STATES: usize = 65_536;
const MAX_PAIR_TRANSITIONS: usize = 2_000_000;

/// `expand_pitch_alternative_states` turns one boundary expert's segment
/// into one candidate *per distinct cross-expert pitch alternative* for
/// that same time slot -- with every boundary-detection expert enabled at
/// once (the "select every Step 3 model" configuration), several experts'
/// pitch estimates pile onto the same slot, so many candidates end up
/// sharing the exact same (start, end) range. `decode_component`'s pairing
/// at a junction is the full cross-product of everything ending there
/// against everything starting there, not a sum, so a handful of crowded
/// slots multiply into far more pair-states than the total candidate count
/// would suggest -- confirmed against a real "all three boundary experts +
/// maximum profile" run that blew through `MAX_PAIR_STATES` on total
/// candidate volume well under `MAX_EXPANDED_CANDIDATES`. The decoder never
/// benefits from weighing more than a few of the best-scoring alternatives
/// for one slot anyway, so keep only the top `MAX_RANGE_DUPLICATES` per
/// exact range -- ranked by the same emission score the decoder itself
/// uses to choose between them, so pruning can only discard candidates the
/// decoder would have lost to a same-range rival regardless.
const MAX_RANGE_DUPLICATES: usize = 8;

/// Indices into `ordered` to keep once same-range pitch-alternative
/// duplicates beyond `MAX_RANGE_DUPLICATES` are pruned. Every exact
/// (start, end) range keeps at least one candidate, so `coverage_components`
/// (computed on the unpruned pool) still exactly matches what decoding
/// actually sees.
fn prune_redundant_range_duplicates(
    ordered: &[SegmentCandidate],
    emissions: &[f32],
) -> std::collections::BTreeSet<usize> {
    let mut by_range = std::collections::BTreeMap::<(u64, u64), Vec<usize>>::new();
    for (index, candidate) in ordered.iter().enumerate() {
        by_range
            .entry((candidate.range.start, candidate.range.end))
            .or_default()
            .push(index);
    }
    let mut kept = std::collections::BTreeSet::new();
    for mut group in by_range.into_values() {
        if group.len() > MAX_RANGE_DUPLICATES {
            group.sort_by(|&left, &right| {
                emissions[right]
                    .partial_cmp(&emissions[left])
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| ordered[left].id.cmp(&ordered[right].id))
            });
            group.truncate(MAX_RANGE_DUPLICATES);
        }
        kept.extend(group);
    }
    kept
}

#[derive(Default)]
struct DecodeWorkBudget {
    /// Defensive work ceilings apply to one connected voiced component. A long
    /// song may contain hundreds of independent components; accumulating their
    /// work would reject duration rather than graph complexity.
    examined_pair_states: usize,
    examined_pair_transitions: usize,
}

fn decode_component(
    ordered: &[SegmentCandidate],
    emissions: &[f32],
    component: CoverageComponent,
    members: &[usize],
    hard_boundaries: &HardBoundaryTimeIndex,
    voicing_reset_times: &[u64],
    native_events: &NativeNoteEvents,
    budget: &mut DecodeWorkBudget,
) -> Result<Vec<usize>, String> {
    let mut members_by_end = std::collections::BTreeMap::<u64, Vec<usize>>::new();
    for &index in members {
        members_by_end
            .entry(ordered[index].range.end)
            .or_default()
            .push(index);
    }

    let mut states = std::collections::BTreeMap::<(usize, usize), PairState>::new();
    let mut predecessors_by_current = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for &next in members {
        let Some(previous_members) = members_by_end.get(&ordered[next].range.start) else {
            continue;
        };
        for &previous in previous_members {
            budget.examined_pair_states = budget.examined_pair_states.saturating_add(1);
            if budget.examined_pair_states > MAX_PAIR_STATES {
                return Err("candidate graph exceeds the bounded pair-state limit".to_string());
            }
            let transition = transition_utility_indexed(
                &ordered[previous],
                &ordered[next],
                hard_boundaries,
                voicing_reset_times,
            ) - native_events.repeated_reward(previous, next);
            let mut best = (ordered[previous].range.start == component.start).then(|| PairState {
                score: emissions[previous] + transition + emissions[next],
                previous_previous: None,
            });
            if let Some(before_members) = predecessors_by_current.get(&previous) {
                for &before in before_members {
                    budget.examined_pair_transitions =
                        budget.examined_pair_transitions.saturating_add(1);
                    if budget.examined_pair_transitions > MAX_PAIR_TRANSITIONS {
                        return Err(
                            "candidate graph exceeds the bounded transition limit".to_string()
                        );
                    }
                    let state = states
                        .get(&(before, previous))
                        .expect("indexed pair state exists");
                    let score = state.score
                        + transition
                        + short_octave_return_penalty(
                            &ordered[before],
                            &ordered[previous],
                            &ordered[next],
                            hard_boundaries,
                            voicing_reset_times,
                        )
                        + emissions[next];
                    if best.is_none_or(|candidate| score > candidate.score) {
                        best = Some(PairState {
                            score,
                            previous_previous: Some(before),
                        });
                    }
                }
            }
            if let Some(best) = best {
                states.insert((previous, next), best);
                predecessors_by_current
                    .entry(next)
                    .or_default()
                    .push(previous);
            }
        }
    }

    let mut endpoint = members
        .iter()
        .copied()
        .filter(|index| {
            ordered[*index].range.start == component.start
                && ordered[*index].range.end == component.end
        })
        .map(|index| (emissions[index], None, index))
        .max_by(|left, right| left.0.total_cmp(&right.0));
    for (&pair @ (_, current), state) in &states {
        if ordered[current].range.end == component.end
            && endpoint.is_none_or(|candidate| state.score > candidate.0)
        {
            endpoint = Some((state.score, Some(pair), current));
        }
    }
    let Some((_, pair, single)) = endpoint else {
        return Err(format!(
            "candidate graph cannot exactly cover voiced component {}..{}",
            component.start, component.end
        ));
    };
    let Some((mut previous, mut current)) = pair else {
        return Ok(vec![single]);
    };
    let mut selected = vec![current];
    loop {
        selected.push(previous);
        let state = states
            .get(&(previous, current))
            .expect("selected pair state exists");
        match state.previous_previous {
            Some(before) => {
                current = previous;
                previous = before;
            }
            None => break,
        }
    }
    selected.reverse();
    Ok(selected)
}

/// Exact second-order Viterbi decode over local covering paths. Pair states
/// preserve the history required by the octave-return prior, while exact
/// adjacency prevents skipped candidates from creating false rests.
pub fn decode_candidate_graph(
    candidates: &[SegmentCandidate],
) -> Result<Vec<SegmentCandidate>, String> {
    decode_candidate_graph_with_boundaries(candidates, &HardBoundarySet::default())
}

pub fn decode_candidate_graph_with_boundaries(
    candidates: &[SegmentCandidate],
    hard_boundaries: &HardBoundarySet,
) -> Result<Vec<SegmentCandidate>, String> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    validate_candidate_pool(candidates)?;
    hard_boundaries.validate()?;
    let hard_boundary_times = HardBoundaryTimeIndex::new(hard_boundaries);
    let mut ordered = candidates.to_vec();
    ordered.sort_by(|left, right| {
        left.range
            .end
            .cmp(&right.range.end)
            .then_with(|| left.range.start.cmp(&right.range.start))
            .then_with(|| left.id.cmp(&right.id))
    });
    let voicing_reset_times = voicing_reset_times(&ordered);
    let native_events = NativeNoteEvents::new(&ordered);
    let emissions = ordered
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            if candidate_crosses_hard_boundary(candidate, &hard_boundary_times) {
                Ok(f32::NEG_INFINITY)
            } else {
                candidate
                    .emission_utility()
                    .map(|utility| utility + native_events.reward(index))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let components = coverage_components(&ordered);
    let kept = prune_redundant_range_duplicates(&ordered, &emissions);
    let mut members_by_component = vec![Vec::new(); components.len()];
    for (index, candidate) in ordered.iter().enumerate() {
        if !kept.contains(&index) {
            continue;
        }
        let component_index =
            components.partition_point(|component| component.end <= candidate.range.start);
        if let Some(component) = components.get(component_index)
            && candidate.range.start >= component.start
            && candidate.range.end <= component.end
        {
            members_by_component[component_index].push(index);
        }
    }
    let mut selected = Vec::new();
    for (component, members) in components.into_iter().zip(members_by_component) {
        let mut budget = DecodeWorkBudget::default();
        selected.extend(decode_component(
            &ordered,
            &emissions,
            component,
            &members,
            &hard_boundary_times,
            &voicing_reset_times,
            &native_events,
            &mut budget,
        )?);
    }
    let decoded = selected
        .into_iter()
        .map(|index| ordered[index].clone())
        .collect::<Vec<_>>();
    validate_candidate_path_with_index(&ordered, &decoded, &hard_boundary_times)?;
    Ok(decoded)
}

#[cfg(test)]
#[path = "hsmm_event_tests.rs"]
mod event_tests;
