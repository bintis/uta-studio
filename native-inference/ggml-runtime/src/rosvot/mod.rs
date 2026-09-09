//! ROSVOT advanced-note challenger using Rust-owned orchestration and shared GGML.

mod decode;
mod frame;
mod heads;
mod model;
mod net;
mod pipeline;

pub use decode::{
    AggregatedNotes, NOTE_END, NOTE_START, aggregate_notes, decode_pitch, regulate_boundaries,
};
pub use frame::{ConditionEncoding, MelEncoding, MelPitchEncoding};
pub use heads::{FrameHeadEncoding, PitchHeadEncoding};
pub use model::{HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, Rosvot};
pub use pipeline::{
    FRAME_BUCKET, NOTE_BUCKET, RawNote, RosvotResult, SharedInputs, TranscriptWord, prepare_inputs,
    prepare_wav_inputs,
};
