//! STARS singing-note challenger using Rust-owned orchestration and shared GGML.

pub mod decode;
pub mod frontend;
mod model;
mod pipeline;
mod stage_a;
mod stage_b;
mod stage_c;
mod stage_d;
mod stage_e;
mod utterance;

pub use model::{HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, Stars, TECHNIQUE_CLASSES, TENSOR_COUNT};
pub use pipeline::{
    FRAME_BUCKET, GlobalStyle, NOTE_BUCKET, PhonemeInput, RawNote, RawTechnique, SharedInputs,
    StarsResult, TECHNIQUE_TAXONOMY, TranscriptWord, prepare_inputs, prepare_wav_inputs,
};
pub use stage_a::{MelEncoding, MelPitchEncoding};
pub use stage_b::RhythmEncoding;
pub use stage_c::PitchEncoding;
pub use stage_d::{SentenceEncoding, StyleLogits};
pub use stage_e::{TechniqueEncoding, aggregate_technique_frames};
pub use utterance::UtteranceEncoding;
