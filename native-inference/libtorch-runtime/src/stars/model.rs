use super::stage_d::StyleLogits;
use super::stage_e::{TechniqueAggregation, TechniqueEncoding};
use crate::{Input, Model};
use std::sync::{Arc, Mutex};
pub use uta_ggml_runtime::stars::{
    HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, TECHNIQUE_CLASSES, TENSOR_COUNT,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Mel,
    Utterance,
    Rhythm,
    Pitch,
    Sentence,
    Attention,
    Weighted,
}
/// Owned reference to real native frame features, scoped to one audio generation.
/// It is deliberately not a Vec and does not pretend that an omitted readback
/// is an empty feature tensor.
#[derive(Clone)]
pub struct FrameFeatures {
    pub(super) model: Model,
    pub(super) generation: u64,
    pub(super) stage: Stage,
    pub frames: usize,
    pub valid_frames: usize,
}
pub struct MelPitchEncoding {
    pub embedded: FrameFeatures,
    pub frames: usize,
}
#[derive(Clone)]
pub struct UtteranceEncoding {
    pub features: FrameFeatures,
    pub boundary_probabilities: Vec<f32>,
    pub phoneme_logits: Vec<f32>,
    pub frames: usize,
}
pub struct RhythmEncoding {
    pub features: FrameFeatures,
    pub note_boundary_logits: Vec<f32>,
    pub frames: usize,
}
pub struct PitchEncoding {
    pub features: FrameFeatures,
    pub note_logits: Vec<f32>,
    pub note_count: usize,
    pub frames: usize,
}
pub struct SentenceEncoding {
    pub features: FrameFeatures,
    pub weighted_features: FrameFeatures,
    pub attention: FrameFeatures,
    pub styles: StyleLogits,
    pub frames: usize,
}
#[derive(Default)]
struct State {
    generation: u64,
    frames: usize,
    valid: usize,
    utterance: Option<UtteranceEncoding>,
}
pub struct Stars {
    model: Model,
    state: Mutex<State>,
}
impl Stars {
    pub fn from_model(model: Model) -> Self {
        Self {
            model,
            state: Mutex::new(State::default()),
        }
    }
    fn feature(&self, state: &State, stage: Stage) -> FrameFeatures {
        FrameFeatures {
            model: self.model.clone(),
            generation: state.generation,
            stage,
            frames: state.frames,
            valid_frames: state.valid,
        }
    }
    fn check(
        &self,
        state: &State,
        feature: &FrameFeatures,
        stage: Stage,
        valid: usize,
        frames: usize,
    ) -> Result<(), String> {
        if !Arc::ptr_eq(&self.model.inner, &feature.model.inner)
            || feature.generation != state.generation
            || feature.stage != stage
            || state.frames != frames
            || state.valid != valid
            || state.utterance.is_none()
        {
            return Err("STARS resident features belong to another model, expired audio generation or incompatible stage".to_string());
        }
        Ok(())
    }
    pub fn encode_mel_with_pitch(
        &self,
        mel: &[f32],
        pitch: &[i32],
        uv: &[i32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<MelPitchEncoding, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| "STARS generation overflow".to_string())?;
        state.utterance = None;
        state.frames = frames;
        state.valid = valid_frames;
        let pitch = pitch.iter().copied().map(i64::from).collect::<Vec<_>>();
        let uv = uv.iter().copied().map(i64::from).collect::<Vec<_>>();
        // Fuse the mel and utterance learned stages; the next canonical host
        // step consumes these real small scores without repeating computation.
        let mut output = self.model.forward(
            "utterance",
            &[
                Input::f32("mel", &[frames as i64, MEL_BINS as i64], mel),
                Input::i64("pitch", &[frames as i64], &pitch),
                Input::i64("uv", &[frames as i64], &uv),
                Input::i64("@valid_frames", &[], &[valid_frames as i64]),
            ],
        )?;
        state.utterance = Some(UtteranceEncoding {
            features: self.feature(&state, Stage::Utterance),
            boundary_probabilities: output.take("boundary_probabilities")?.into_f32()?,
            phoneme_logits: output.take("phoneme_logits")?.into_f32()?,
            frames,
        });
        Ok(MelPitchEncoding {
            embedded: self.feature(&state, Stage::Mel),
            frames,
        })
    }
    pub fn encode_utterance(
        &self,
        mel: &FrameFeatures,
        valid_frames: usize,
        frames: usize,
    ) -> Result<UtteranceEncoding, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        self.check(&state, mel, Stage::Mel, valid_frames, frames)?;
        state
            .utterance
            .clone()
            .ok_or_else(|| "STARS utterance computation did not complete".to_string())
    }
    pub fn encode_rhythm(
        &self,
        mel: &FrameFeatures,
        utterance: &FrameFeatures,
        valid_frames: usize,
        frames: usize,
        phoneme_ids: &[i64],
        phoneme_count: usize,
        word_ids: &[i64],
        word_count: usize,
    ) -> Result<RhythmEncoding, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        self.check(&state, mel, Stage::Mel, valid_frames, frames)?;
        self.check(&state, utterance, Stage::Utterance, valid_frames, frames)?;
        let logits = self
            .model
            .forward(
                "rhythm",
                &[
                    Input::i64("phoneme_ids", &[frames as i64], phoneme_ids),
                    Input::i64("@phoneme_count", &[], &[phoneme_count as i64]),
                    Input::i64("word_ids", &[frames as i64], word_ids),
                    Input::i64("@word_count", &[], &[word_count as i64]),
                ],
            )?
            .take("boundary_logits")?
            .into_f32()?;
        Ok(RhythmEncoding {
            features: self.feature(&state, Stage::Rhythm),
            note_boundary_logits: logits,
            frames,
        })
    }
    pub fn encode_pitch(
        &self,
        mel: &FrameFeatures,
        rhythm: &FrameFeatures,
        valid_frames: usize,
        frames: usize,
        note_ids: &[i64],
        note_count: usize,
        boundaries: &[i64],
    ) -> Result<PitchEncoding, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        self.check(&state, mel, Stage::Mel, valid_frames, frames)?;
        self.check(&state, rhythm, Stage::Rhythm, valid_frames, frames)?;
        let logits = self
            .model
            .forward(
                "pitch",
                &[
                    Input::i64("note_ids", &[frames as i64], note_ids),
                    Input::i64("@note_count", &[], &[note_count as i64]),
                    Input::i64("boundaries", &[frames as i64], boundaries),
                ],
            )?
            .take("logits")?
            .into_f32()?;
        Ok(PitchEncoding {
            features: self.feature(&state, Stage::Pitch),
            note_logits: logits,
            note_count,
            frames,
        })
    }
    pub fn encode_sentence(
        &self,
        mel: &FrameFeatures,
        pitch: &FrameFeatures,
        valid_frames: usize,
        frames: usize,
    ) -> Result<SentenceEncoding, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        self.check(&state, mel, Stage::Mel, valid_frames, frames)?;
        self.check(&state, pitch, Stage::Pitch, valid_frames, frames)?;
        let mut output = self.model.forward("sentence", &[])?;
        let styles = StyleLogits {
            technique_group: output.take("technique_group")?.into_f32()?,
            language: output.take("language")?.into_f32()?,
            gender: output.take("gender")?.into_f32()?,
            emotion: output.take("emotion")?.into_f32()?,
            method: output.take("method")?.into_f32()?,
            pace: output.take("pace")?.into_f32()?,
            range: output.take("range")?.into_f32()?,
        };
        Ok(SentenceEncoding {
            features: self.feature(&state, Stage::Sentence),
            weighted_features: self.feature(&state, Stage::Weighted),
            attention: self.feature(&state, Stage::Attention),
            styles,
            frames,
        })
    }
    pub fn encode_techniques(
        &self,
        aggregation: &TechniqueAggregation,
        phonemes: usize,
    ) -> Result<TechniqueEncoding, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "STARS feature ownership lock was poisoned".to_string())?;
        self.check(
            &state,
            &aggregation.weighted,
            Stage::Weighted,
            state.valid,
            state.frames,
        )?;
        if phonemes != aggregation.intervals.len() {
            return Err("STARS technique aggregation interval count changed".to_string());
        }
        let ranges = aggregation
            .intervals
            .iter()
            .flat_map(|range| [range.start as i64, range.end as i64])
            .collect::<Vec<_>>();
        let logits = self
            .model
            .forward(
                "techniques",
                &[Input::i64("@intervals", &[phonemes as i64, 2], &ranges)],
            )?
            .take("logits")?
            .into_f32()?;
        Ok(TechniqueEncoding { phonemes, logits })
    }
}
