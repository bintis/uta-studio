use serde::{Deserialize, Serialize};

use super::{CanonicalSingingTrack, TimeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SingingReviewReason {
    LowConfidence,
    PitchDisagreement,
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
    pub confidence: f32,
    pub reasons: Vec<SingingReviewReason>,
    #[serde(default)]
    pub evidence_experts: Vec<String>,
    #[serde(default)]
    pub reviewed: bool,
}

fn merge_regions(mut regions: Vec<SingingReviewRegion>) -> Vec<SingingReviewRegion> {
    regions.sort_by(|left, right| left.range.start.total_cmp(&right.range.start));
    let mut merged: Vec<SingingReviewRegion> = Vec::new();
    for region in regions {
        if let Some(previous) = merged.last_mut()
            && region.range.start <= previous.range.end + 0.05
        {
            previous.range = previous.range.union(region.range);
            previous.confidence = previous.confidence.min(region.confidence);
            previous.reasons.extend(region.reasons);
            previous.reasons.sort();
            previous.reasons.dedup();
            previous.evidence_experts.extend(region.evidence_experts);
            previous.evidence_experts.sort();
            previous.evidence_experts.dedup();
            previous.id = format!(
                "review-{:.0}-{:.0}",
                previous.range.start * 1_000.0,
                previous.range.end * 1_000.0
            );
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
        if note.confidence < 0.65 {
            reasons.push(SingingReviewReason::LowConfidence);
        }
        if note.uncertain
            || note
                .alternatives
                .iter()
                .any(|item| item.probability >= 0.35)
        {
            reasons.push(SingingReviewReason::PitchDisagreement);
        }
        if note
            .alternatives
            .iter()
            .any(|item| item.midi_note.abs_diff(note.midi_note) == 12 && item.probability >= 0.25)
        {
            reasons.push(SingingReviewReason::OctaveRisk);
        }
        if note.evidence.boundary_score < 0.45 {
            reasons.push(SingingReviewReason::BoundaryDisagreement);
        }
        if note.word_id.is_none() {
            reasons.push(SingingReviewReason::WordNoteMismatch);
        }
        if note.techniques.vibrato >= 0.55 && note.techniques.glissando >= 0.55 {
            reasons.push(SingingReviewReason::TechniqueAmbiguous);
        }
        if reasons.is_empty() {
            continue;
        }
        regions.push(SingingReviewRegion {
            id: format!(
                "review-{:.0}-{:.0}",
                note.range.start * 1_000.0,
                note.range.end * 1_000.0
            ),
            range: note.range,
            confidence: note.confidence,
            reasons,
            evidence_experts: note.evidence.source_experts.clone(),
            reviewed: false,
        });
    }
    if track.harmony_metadata.lead_harmony_leak_probability >= 0.5
        && let (Some(first), Some(last)) = (track.notes.first(), track.notes.last())
    {
        regions.push(SingingReviewRegion {
            id: "review-harmony-leak".to_string(),
            range: TimeRange {
                start: first.range.start,
                end: last.range.end,
            },
            confidence: 1.0
                - track
                    .harmony_metadata
                    .lead_harmony_leak_probability
                    .clamp(0.0, 1.0),
            reasons: vec![SingingReviewReason::LeadHarmonyLeak],
            evidence_experts: Vec::new(),
            reviewed: false,
        });
    }
    merge_regions(regions)
}
