use std::sync::Arc;
use super::model::{FrameFeatures, HIDDEN_DIM, Stage};
pub use uta_ggml_runtime::rosvot::{NOTE_END, NOTE_START, decode_pitch, regulate_boundaries};
/// Owned native aggregation request over actual resident features. The pitch
/// invocation performs its reduction on the same device, without readback.
pub struct NoteAggregation { pub(super) weighted: FrameFeatures, pub(super) boundaries: Vec<i64>, pub(super) count: usize }
pub struct AggregatedNotes { pub features: NoteAggregation, pub count: usize, pub frame_to_note: Vec<usize> }
pub fn aggregate_notes(weighted: &FrameFeatures, attention: &FrameFeatures, boundaries: &[i32], hidden: usize, valid_frames: usize) -> Result<AggregatedNotes, String> {
    if weighted.stage != Stage::Weighted || attention.stage != Stage::Attention || weighted.generation != attention.generation
        || !Arc::ptr_eq(&weighted.model.inner, &attention.model.inner) || weighted.frames != attention.frames || hidden != HIDDEN_DIM
        || valid_frames == 0 || valid_frames != weighted.valid_frames || boundaries.len() != weighted.frames
        || boundaries.iter().any(|value| !matches!(value, 0 | 1)) {
        return Err("ROSVOT native aggregation received incompatible resident features or boundary map".to_string());
    }
    let mut note = 0;
    let frame_to_note = boundaries[..valid_frames].iter().map(|value| { note += *value as usize; note }).collect::<Vec<_>>();
    let count = note + 1;
    Ok(AggregatedNotes { features: NoteAggregation { weighted: weighted.clone(), boundaries: boundaries.iter().copied().map(i64::from).collect(), count }, count, frame_to_note })
}
