//! GAME's canonical host diffusion and note decoder over native resident
//! encoder features. The host receives feature handles, not fake empty vectors
//! or repeated copies of the accelerator's encoder output.
use std::sync::Mutex;
use crate::{Input, Model};
#[path = "../../../ggml-runtime/src/game/d3pm.rs"]
mod d3pm;
#[path = "../../../ggml-runtime/src/game/decode.rs"]
mod decode;
#[path = "../../../ggml-runtime/src/game/infer.rs"]
mod infer;
#[path = "../../../ggml-runtime/src/game/mel.rs"]
mod mel;
pub use d3pm::{GameRng, RandomSource, d3pm_time_schedule, remove_mutable_boundaries};
pub use decode::{GaussianBlurredResult, boundaries_to_regions, decode_gaussian_blurred_probs, decode_soft_boundaries};
pub use infer::{GameInferOutput, GameInferParams, GameNote};
pub use mel::{MelConfig, MelExtractor};
pub use uta_ggml_runtime::game::{GameConfig, GameEstimatorOutput};

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeatureKind { Segmenter, Estimator }
#[derive(Clone)]
pub struct ResidentEmbedding { model: Model, generation: u64, frames: usize, kind: FeatureKind }
pub struct GameEncoderOutput {
    pub frames: usize,
    pub segmenter_embeddings: ResidentEmbedding,
    pub estimator_embeddings: ResidentEmbedding,
}
pub struct Game { model: Model, config: GameConfig, generation: Mutex<u64> }
impl Game {
    pub fn from_model(model: Model) -> Result<Self, String> {
        let integer = |name: &str| -> Result<usize, String> { usize::try_from(model.integer(name)?).map_err(|_| format!("invalid GAME dimension: {name}")) };
        let config = GameConfig {
            variant: match model.resource.as_str() { "game_1_0_3_small" => "small", "game_1_0_3_medium" => "medium", "game_1_0_3_large" => "large", _ => return Err("native GAME resource is unknown".to_string()) },
            embedding_dim: integer("game.model.embedding_dim")?, input_dim: integer("game.model.in_dim")?,
            estimator_output_dim: integer("game.model.estimator_out_dim")?, region_cycle_length: integer("game.model.region_cycle_len")?,
            language_count: integer("game.model.num_languages")?, encoder_layers: integer("game.encoder.num_layers")?,
            segmenter_layers: integer("game.segmenter.num_layers")?, estimator_layers: integer("game.estimator.num_layers")?,
            model_dim: integer("game.encoder.dim")?, attention_heads: integer("game.encoder.num_heads")?,
            attention_head_dim: integer("game.encoder.head_dim")?, midi_minimum: model.number("game.inference.midi_min")? as f32,
            midi_maximum: model.number("game.inference.midi_max")? as f32, midi_bins: integer("game.inference.midi_num_bins")?,
            midi_deviation: model.number("game.inference.midi_std")? as f32,
        };
        Ok(Self { model, config, generation: Mutex::new(0) })
    }
    pub fn config(&self) -> &GameConfig { &self.config }
    pub fn encode_mel(&self, mel: &[f32], frames: usize) -> Result<GameEncoderOutput, String> {
        let mut generation = self.generation.lock().map_err(|_| "GAME resident feature ownership lock was poisoned".to_string())?;
        *generation = generation.checked_add(1).ok_or_else(|| "GAME feature generation overflow".to_string())?;
        let output = self.model.forward("encode", &[Input::f32("mel", &[frames as i64, self.config.input_dim as i64], mel)])?;
        if output.get("frames")?.i64()? != [frames as i64] { return Err("native GAME encoder changed the frame count".to_string()); }
        let feature = |kind| ResidentEmbedding { model: self.model.clone(), generation: *generation, frames, kind };
        Ok(GameEncoderOutput { frames, segmenter_embeddings: feature(FeatureKind::Segmenter), estimator_embeddings: feature(FeatureKind::Estimator) })
    }
    fn check_features(&self, features: &ResidentEmbedding, generation: u64, kind: FeatureKind) -> Result<(), String> {
        if !std::sync::Arc::ptr_eq(&self.model.inner, &features.model.inner) || generation != features.generation || features.kind != kind {
            return Err("GAME features belong to another model, stage or expired audio window".to_string());
        }
        Ok(())
    }
    pub fn segmenter_logits(&self, features: &ResidentEmbedding, noise: &[i32], time: f32, language: i32) -> Result<Vec<f32>, String> {
        let generation = self.generation.lock().map_err(|_| "GAME resident feature ownership lock was poisoned".to_string())?;
        self.check_features(features, *generation, FeatureKind::Segmenter)?;
        if noise.len() != features.frames { return Err("GAME noise timeline differs from its encoder features".to_string()); }
        let noise = noise.iter().copied().map(i64::from).collect::<Vec<_>>();
        self.model.forward("segment", &[
            Input::i64("noise", &[features.frames as i64], &noise), Input::f32("time", &[1], &[time]), Input::i64("language", &[1], &[i64::from(language)]),
        ])?.take("logits")?.into_f32()
    }
    pub fn estimate_pitch(&self, features: &ResidentEmbedding, regions: &[i32]) -> Result<GameEstimatorOutput, String> {
        let generation = self.generation.lock().map_err(|_| "GAME resident feature ownership lock was poisoned".to_string())?;
        self.check_features(features, *generation, FeatureKind::Estimator)?;
        if regions.len() != features.frames { return Err("GAME region timeline differs from encoder features".to_string()); }
        let regions = regions.iter().copied().map(i64::from).collect::<Vec<_>>();
        let count = regions.iter().copied().max().unwrap_or(0);
        let logits = self.model.forward("estimate", &[Input::i64("regions", &[features.frames as i64], &regions), Input::i64("@region_count", &[], &[count])])?
            .take("pool_logits")?.into_f32()?;
        Ok(GameEstimatorOutput { regions: usize::try_from(count).map_err(|_| "GAME region count is negative".to_string())?, bins: self.config.estimator_output_dim, pool_logits: logits })
    }
}
