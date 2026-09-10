//! Native resident ROSVOT stages with canonical transcript-boundary decoding.
mod model;
mod decode;
#[path = "../../../ggml-runtime/src/rosvot/pipeline.rs"]
mod pipeline;
pub use model::{Rosvot, FrameFeatures, ConditionEncoding, FrameHeadEncoding, PitchHeadEncoding, HIDDEN_DIM, MEL_BINS, PITCH_CLASSES};
pub use decode::{AggregatedNotes, NOTE_END, NOTE_START, aggregate_notes, decode_pitch, regulate_boundaries};
pub use pipeline::{FRAME_BUCKET, NOTE_BUCKET, RawNote, RosvotResult, SharedInputs, TranscriptWord, prepare_inputs, prepare_wav_inputs};
