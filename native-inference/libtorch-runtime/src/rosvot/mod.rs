//! Native resident ROSVOT stages with canonical transcript-boundary decoding.
mod decode;
mod model;
#[path = "../../../ggml-runtime/src/rosvot/pipeline.rs"]
mod pipeline;
pub use decode::{
    AggregatedNotes, NOTE_END, NOTE_START, aggregate_notes, decode_pitch, regulate_boundaries,
};
pub use model::{
    ConditionEncoding, FrameFeatures, FrameHeadEncoding, HIDDEN_DIM, MEL_BINS, PITCH_CLASSES,
    PitchHeadEncoding, Rosvot,
};
pub use pipeline::{
    FRAME_BUCKET, RawNote, RosvotResult, SharedInputs, TranscriptWord, prepare_inputs,
    prepare_wav_inputs,
};
