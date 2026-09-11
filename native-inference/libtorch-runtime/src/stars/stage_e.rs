use super::model::{FrameFeatures, Stage};
use std::ops::Range;
use std::sync::Arc;

/// Deferred native interval aggregation, retaining the real device features and
/// the decoded host ranges. encode_techniques executes the aggregation and head
/// together; there is no fabricated intermediate feature vector.
pub struct TechniqueAggregation {
    pub(super) weighted: FrameFeatures,
    pub(super) intervals: Vec<Range<usize>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TechniqueEncoding {
    pub phonemes: usize,
    pub logits: Vec<f32>,
}
pub fn aggregate_technique_frames(
    weighted: &FrameFeatures,
    attention: &FrameFeatures,
    intervals: &[Range<usize>],
) -> Result<TechniqueAggregation, String> {
    if weighted.stage != Stage::Weighted
        || attention.stage != Stage::Attention
        || weighted.generation != attention.generation
        || !Arc::ptr_eq(&weighted.model.inner, &attention.model.inner)
        || weighted.frames != attention.frames
    {
        return Err(
            "STARS interval aggregation received incompatible resident features".to_string(),
        );
    }
    if intervals.is_empty()
        || intervals
            .iter()
            .any(|range| range.start >= range.end || range.end > weighted.frames)
    {
        return Err("STARS interval aggregation needs nonempty decoded frame ranges".to_string());
    }
    Ok(TechniqueAggregation {
        weighted: weighted.clone(),
        intervals: intervals.to_vec(),
    })
}
