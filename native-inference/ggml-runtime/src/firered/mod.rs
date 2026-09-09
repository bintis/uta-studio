mod decoder;
mod encoder;
mod fbank;
mod transcript;
mod weights;

pub use encoder::EncodedAudio;
pub use transcript::{TranscriptWindow, Transcription, WINDOW_OVERLAP_SAMPLES};
pub use weights::FireRed;

pub const SAMPLE_RATE: usize = 16_000;
pub const MIN_WINDOW_SAMPLES: usize = 37_040;
pub const MAX_WINDOW_SAMPLES: usize = 37_199;
pub const FEATURE_FRAMES: usize = 230;
pub const ENCODER_FRAMES: usize = 58;
pub const D_MODEL: usize = 1_280;
pub const D_INNER: usize = 5_120;
pub const N_HEAD: usize = 20;
pub const D_K: usize = D_MODEL / N_HEAD;
pub const N_LAYERS_ENC: usize = 16;
pub const N_LAYERS_DEC: usize = 16;
pub const KERNEL_SIZE: usize = 33;
pub const VOCAB_SIZE: usize = 8_667;
pub const SOS: u32 = 3;
pub const EOS: u32 = 4;
/// Decoder token budget for one window.
///
/// The historical implementation used eleven, which matched its own
/// `step in 0..=10` cache bucket and was only ever exercised on a 2.3-second
/// "hello world" fixture. Sung audio needs far more: a full-song run failed
/// every window with `did not reach EOS within its token budget`.
///
/// The checkpoint's own ceiling is its decoder positional encoding, which has
/// 5,000 rows. The operational bound is acoustic: an attention encoder-decoder
/// cannot carry more output than the encoder frames it attends to, so one
/// token per encoder frame is already about 25 tokens per second of audio,
/// well beyond any speech or singing rate, while still stopping a runaway
/// decode long before the table ends.
pub const MAX_GENERATED_TOKENS: usize = ENCODER_FRAMES;
pub const SUBSAMPLE_PAD_FRAMES: usize = 6;
pub const FRAME_LENGTH: usize = 400;
pub const FRAME_SHIFT: usize = 160;
pub const MEL_BINS: usize = 80;

pub fn extract_features(audio: &[f32], cmvn: &[u8]) -> Result<(Vec<f32>, usize), String> {
    fbank::extract(audio, cmvn)
}
