use serde::{Deserialize, Serialize};

use super::{CanonicalSingingTrack, TimeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SingingReviewReason {
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
    TechniqueAmbiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingReviewRegion {
    pub id: String,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub reasons: Vec<SingingReviewReason>,
    #[serde(default)]
    pub evidence_experts: Vec<String>,
    #[serde(default)]
    pub reviewed: bool,
}

fn minimum_known(left: Option<f32>, right: Option<f32>) -> Option<f32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        _ => None,
    }
}

fn merge_regions(mut regions: Vec<SingingReviewRegion>) -> Vec<SingingReviewRegion> {
    regions.sort_by_key(|region| region.range.start);
    let mut merged: Vec<SingingReviewRegion> = Vec::new();
    for region in regions {
        if let Some(previous) = merged.last_mut()
            && region.range.start <= previous.range.end.saturating_add(50_000)
        {
            previous.range = previous.range.union(region.range);
            previous.confidence = minimum_known(previous.confidence, region.confidence);
            previous.reasons.extend(region.reasons);
            previous.reasons.sort();
            previous.reasons.dedup();
            previous.evidence_experts.extend(region.evidence_experts);
            previous.evidence_experts.sort();
            previous.evidence_experts.dedup();
            previous.id = format!("review-{}-{}", previous.range.start, previous.range.end);
            continue;
        }
        merged.push(region);
    }
    merged
}

pub fn build_review_regions(track: &CanonicalSingingTrack) -> Vec<SingingReviewRegion> {
    let mut regions = Vec::new();
    for note in &track.notes {
        let mut reasons = Vec::new();
        let unknown_confidence = note.confidence.is_none();
        if note.confidence.is_some_and(|confidence| confidence < 0.65) {
            reasons.push(SingingReviewReason::LowConfidence);
        }
        if !note.alternatives.is_empty() {
            reasons.push(SingingReviewReason::PitchDisagreement);
        }
        if note
            .evidence
            .rmvpe_voiced_ratio
            .is_some_and(|ratio| ratio < 0.5)
        {
            reasons.push(SingingReviewReason::LowPitchCoverage);
        }
        if note
            .evidence
            .rmvpe_pitch_mad_cents
            .is_some_and(|mad| mad > 60.0)
        {
            reasons.push(SingingReviewReason::PitchInstability);
        }
        if note
            .alternatives
            .iter()
            .any(|item| (item.cents_from_target.abs() - 1_200.0).abs() <= 100.0)
        {
            reasons.push(SingingReviewReason::OctaveRisk);
        }
        if note
            .evidence
            .acoustic
            .as_ref()
            .and_then(|features| features.onset_supported)
            == Some(false)
        {
            reasons.push(SingingReviewReason::BoundaryDisagreement);
        }
        if note
            .evidence
            .acoustic
            .as_ref()
            .is_some_and(|features| features.mean_periodicity < 0.1)
        {
            reasons.push(SingingReviewReason::VoicingConflict);
        }
        if note.word_id.is_none() {
            reasons.push(SingingReviewReason::WordNoteMismatch);
        }
        if note.techniques.vibrato.is_some_and(|value| value >= 0.55)
            && note.techniques.glissando.is_some_and(|value| value >= 0.55)
        {
            reasons.push(SingingReviewReason::TechniqueAmbiguous);
        }
        if reasons.is_empty() {
            continue;
        }
        if unknown_confidence {
            reasons.push(SingingReviewReason::UnknownConfidence);
        }
        reasons.sort();
        reasons.dedup();
        regions.push(SingingReviewRegion {
            id: format!("review-{}-{}", note.range.start, note.range.end),
            range: note.range,
            confidence: note.confidence,
            reasons,
            evidence_experts: note.evidence.source_experts.clone(),
            reviewed: false,
        });
    }
    if let Some(probability) = track.harmony_metadata.lead_harmony_leak_probability
        && probability >= 0.5
        && let (Some(first), Some(last)) = (track.notes.first(), track.notes.last())
    {
        regions.push(SingingReviewRegion {
            id: "review-harmony-leak".to_string(),
            range: TimeRange {
                start: first.range.start,
                end: last.range.end,
            },
            confidence: Some(1.0 - probability),
            reasons: vec![SingingReviewReason::LeadHarmonyLeak],
            evidence_experts: Vec::new(),
            reviewed: false,
        });
    }
    merge_regions(regions)
}
