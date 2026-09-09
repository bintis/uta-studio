//! Rust-owned GAME frontend, diffusion control, decoding, and GGML graphs.
//!
//! The implementation intentionally keeps model orchestration outside C++.
//! Shared upstream GGML owns tensor execution through the runtime C ABI.

mod d3pm;
mod decode;
mod graph;
mod infer;
mod mel;
mod model;

pub use d3pm::{GameRng, RandomSource, d3pm_time_schedule, remove_mutable_boundaries};
pub use decode::{
    GaussianBlurredResult, boundaries_to_regions, decode_gaussian_blurred_probs,
    decode_soft_boundaries,
};
pub use graph::{GameEncoderOutput, GameEstimatorOutput};
pub use infer::{GameInferOutput, GameInferParams, GameNote};
pub use mel::{MelConfig, MelExtractor};
pub use model::{Game, GameConfig};

pub const SAMPLE_RATE: u32 = 44_100;
pub const HOP_SIZE: usize = 441;
pub const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 30;
pub const CHUNK_OVERLAP_SAMPLES: usize = SAMPLE_RATE as usize * 2;
pub const D3PM_STEPS: usize = 8;
pub const BOUNDARY_THRESHOLD: f32 = 0.2;
pub const PRESENCE_THRESHOLD: f32 = 0.2;
