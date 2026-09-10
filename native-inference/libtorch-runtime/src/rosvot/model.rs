use std::sync::{Arc, Mutex};
use crate::{Input, Model};
use super::decode::NoteAggregation;
pub use uta_ggml_runtime::rosvot::{HIDDEN_DIM, MEL_BINS, PITCH_CLASSES};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage { Condition, Backbone, Attention, Weighted }
#[derive(Clone)]
pub struct FrameFeatures {
    pub(super) model: Model, pub(super) generation: u64, pub(super) stage: Stage,
    pub frames: usize, pub valid_frames: usize,
}
pub struct ConditionEncoding { pub conditioned: FrameFeatures, pub frames: usize }
pub struct FrameHeadEncoding { pub weighted_features: FrameFeatures, pub attention: FrameFeatures, pub boundary_logits: Vec<f32>, pub frames: usize }
pub struct PitchHeadEncoding { pub logits: Vec<f32>, pub notes: usize }
#[derive(Default)]
struct State { generation: u64, frames: usize, valid: usize, boundary_logits: Option<Vec<f32>> }
pub struct Rosvot { model: Model, state: Mutex<State> }
impl Rosvot {
    pub fn from_model(model: Model) -> Self { Self { model, state: Mutex::new(State::default()) } }
    fn feature(&self, state: &State, stage: Stage) -> FrameFeatures {
        FrameFeatures { model: self.model.clone(), generation: state.generation, stage, frames: state.frames, valid_frames: state.valid }
    }
    fn check(&self, state: &State, feature: &FrameFeatures, stage: Stage, valid: usize, frames: usize) -> Result<(), String> {
        if !Arc::ptr_eq(&self.model.inner, &feature.model.inner) || feature.generation != state.generation || feature.stage != stage
            || valid != state.valid || frames != state.frames || state.boundary_logits.is_none() {
            return Err("ROSVOT resident features belong to another model, expired window or incompatible stage".to_string());
        }
        Ok(())
    }
    pub fn encode_conditioning(&self, mel: &[f32], pitch: &[i32], uv: &[i32], word_boundaries: &[i32], valid_frames: usize, frames: usize) -> Result<ConditionEncoding, String> {
        let mut state = self.state.lock().map_err(|_| "ROSVOT feature ownership lock was poisoned".to_string())?;
        state.generation = state.generation.checked_add(1).ok_or_else(|| "ROSVOT generation overflow".to_string())?;
        state.boundary_logits = None;
        state.frames = frames;
        state.valid = valid_frames;
        let integers = |values: &[i32]| values.iter().copied().map(i64::from).collect::<Vec<_>>();
        let pitch = integers(pitch); let uv = integers(uv); let boundaries = integers(word_boundaries);
        let logits = self.model.forward("frames", &[
            Input::f32("mel", &[frames as i64, MEL_BINS as i64], mel), Input::i64("pitch", &[frames as i64], &pitch),
            Input::i64("uv", &[frames as i64], &uv), Input::i64("word_boundaries", &[frames as i64], &boundaries),
            Input::i64("@valid_frames", &[], &[valid_frames as i64]),
        ])?.take("boundary_logits")?.into_f32()?;
        state.boundary_logits = Some(logits);
        Ok(ConditionEncoding { conditioned: self.feature(&state, Stage::Condition), frames })
    }
    pub fn encode_backbone(&self, conditioned: &FrameFeatures, valid_frames: usize, frames: usize) -> Result<FrameFeatures, String> {
        let state = self.state.lock().map_err(|_| "ROSVOT feature ownership lock was poisoned".to_string())?;
        self.check(&state, conditioned, Stage::Condition, valid_frames, frames)?;
        Ok(self.feature(&state, Stage::Backbone))
    }
    pub fn encode_frame_heads(&self, features: &FrameFeatures, frames: usize) -> Result<FrameHeadEncoding, String> {
        let state = self.state.lock().map_err(|_| "ROSVOT feature ownership lock was poisoned".to_string())?;
        self.check(&state, features, Stage::Backbone, state.valid, frames)?;
        Ok(FrameHeadEncoding { weighted_features: self.feature(&state, Stage::Weighted), attention: self.feature(&state, Stage::Attention),
            boundary_logits: state.boundary_logits.clone().ok_or_else(|| "ROSVOT frame stage did not complete".to_string())?, frames })
    }
    pub fn encode_pitch_head(&self, aggregation: &NoteAggregation, notes: usize) -> Result<PitchHeadEncoding, String> {
        let state = self.state.lock().map_err(|_| "ROSVOT feature ownership lock was poisoned".to_string())?;
        self.check(&state, &aggregation.weighted, Stage::Weighted, state.valid, state.frames)?;
        if notes != aggregation.count { return Err("ROSVOT decoded note count changed before native aggregation".to_string()); }
        let logits = self.model.forward("pitch", &[
            Input::i64("boundaries", &[aggregation.boundaries.len() as i64], &aggregation.boundaries), Input::i64("@note_count", &[], &[notes as i64]),
        ])?.take("logits")?.into_f32()?;
        Ok(PitchHeadEncoding { logits, notes })
    }
}
