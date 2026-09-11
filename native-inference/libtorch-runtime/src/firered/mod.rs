//! Native FireRed encoder and incremental AED sessions with canonical Rust
//! CMVN, vocabulary, full-song windows and unfinished-window accounting.
#[path = "../../../ggml-runtime/src/firered/transcript.rs"]
mod transcript;
pub use crate::speech::EncodedAudio;
use crate::speech::{Resident, matrix, position};
use crate::{Input, Model};
pub use transcript::{TranscriptWindow, Transcription, WINDOW_OVERLAP_SAMPLES};
pub use uta_ggml_runtime::firered::{
    D_MODEL, ENCODER_FRAMES, EOS, FEATURE_FRAMES, MAX_GENERATED_TOKENS, MAX_WINDOW_SAMPLES,
    MEL_BINS, MIN_WINDOW_SAMPLES, SAMPLE_RATE, SOS, VOCAB_SIZE, extract_features,
};

pub struct FireRed {
    model: Model,
    resident: Resident,
}
impl FireRed {
    pub fn from_model(model: Model) -> Self {
        Self {
            model,
            resident: Resident::default(),
        }
    }
    pub fn encode(&self, features: &[f32]) -> Result<EncodedAudio, String> {
        let _guard = self.resident.lock()?;
        self.resident.clear();
        let output = self.model.forward(
            "encode_outputs",
            &[Input::f32(
                "features",
                &[FEATURE_FRAMES as i64, MEL_BINS as i64],
                features,
            )],
        )?;
        let (rows, _) = matrix(&output, "audio", D_MODEL)?;
        Ok(self.resident.remember(rows, D_MODEL))
    }
    pub fn greedy_decode(&self, encoded: &EncodedAudio) -> Result<Vec<u32>, String> {
        self.greedy_decode_with_budget(encoded, MAX_GENERATED_TOKENS)
    }

    pub fn greedy_decode_with_budget(&self, encoded: &EncodedAudio, max_new_tokens: usize) -> Result<Vec<u32>, String> {
        if max_new_tokens == 0 || max_new_tokens > MAX_GENERATED_TOKENS {
            return Err("FireRed token budget exceeds the native decoder capacity".to_string());
        }
        let _guard = self.resident.lock()?;
        self.resident.validate(encoded)?;
        let output = self.model.forward(
            "session",
            &[Input::i64("@capacity", &[], &[max_new_tokens as i64])],
        )?;
        position(&output, 0)?;
        let mut tokens = vec![SOS];
        for step in 0..max_new_tokens {
            let last = i64::from(*tokens.last().expect("SOS initializes the token sequence"));
            let output = self.model.forward(
                "decode",
                &[
                    Input::i64("tokens", &[1], &[last]),
                    Input::i64("@expected_position", &[], &[step as i64]),
                ],
            )?;
            position(&output, step + 1)?;
            let (_, logits) = matrix(&output, "logits", VOCAB_SIZE)?;
            // Match the canonical FireRed decoder's last-maximum tie rule.
            let token = logits
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .ok_or("FireRed returned empty logits")?
                .0 as u32;
            tokens.push(token);
            if token == EOS {
                break;
            }
        }
        Ok(tokens)
    }
}
