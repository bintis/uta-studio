//! Canonical transcript/phoneme/note decoding with native resident STARS stages.
#[path = "../../../ggml-runtime/src/stars/decode.rs"]
pub mod decode;
#[path = "../../../ggml-runtime/src/stars/frontend.rs"]
pub mod frontend;
#[path = "../../../ggml-runtime/src/stars/pipeline.rs"]
mod pipeline;
mod model;
mod stage_e;
mod stage_d { pub use uta_ggml_runtime::stars::StyleLogits; }
pub use model::{Stars, FrameFeatures, MelPitchEncoding, UtteranceEncoding, RhythmEncoding, PitchEncoding, SentenceEncoding,
                HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, TECHNIQUE_CLASSES, TENSOR_COUNT};
pub use pipeline::{FRAME_BUCKET, NOTE_BUCKET, GlobalStyle, PhonemeInput, RawNote, RawTechnique, SharedInputs, StarsResult,
                   TECHNIQUE_TAXONOMY, TranscriptWord, prepare_inputs, prepare_wav_inputs};
pub use stage_d::StyleLogits;
pub use stage_e::{TechniqueAggregation, TechniqueEncoding, aggregate_technique_frames};
