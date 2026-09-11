//! Native speech execution with the canonical Rust Qwen tokenizer, prompts,
//! long-audio reconciliation and acoustic timestamp scheduling.
#[path = "../../../ggml-runtime/src/qwen/aligner.rs"]
pub mod aligner;
#[path = "../../../ggml-runtime/src/qwen/alignment_windows.rs"]
mod alignment_windows;
#[path = "../../../ggml-runtime/src/qwen/asr.rs"]
pub mod asr;
mod metadata;
#[path = "../../../ggml-runtime/src/qwen/progress.rs"]
mod progress;
mod weights {
    pub use super::Qwen;
}
pub use crate::speech::EncodedAudio;
use crate::speech::{Resident, matrix, position};
use crate::{Input, Model};
use model::{Config, ModelKind};
use std::cell::RefMut;
use tokenizer::Tokenizer;
use uta_ggml_runtime::qwen::decoder::DecoderLogits;
pub use uta_ggml_runtime::qwen::{frontend, model, timestamps, tokenizer};

pub struct Qwen {
    model: Model,
    pub config: Config,
    pub tokenizer: Tokenizer,
    resident: Resident,
}
impl Qwen {
    pub fn from_model(model: Model) -> Result<Self, String> {
        let metadata = metadata::NativeMetadata(&model.metadata);
        let config = Config::read(&metadata)?;
        let tokenizer = Tokenizer::from_gguf(&metadata)?;
        Ok(Self {
            model,
            config,
            tokenizer,
            resident: Resident::default(),
        })
    }
    pub fn encode_audio(&self, mel: &frontend::Mel) -> Result<EncodedAudio, String> {
        let _guard = self.resident.lock()?;
        self.resident.clear();
        let output = self.model.forward(
            "encode_outputs",
            &[Input::f32(
                "mel",
                &[mel.bins as i64, mel.frames as i64],
                &mel.data,
            )],
        )?;
        let (rows, _) = matrix(&output, "audio", self.config.encoder_output_dim)?;
        Ok(self.resident.remember(rows, self.config.encoder_output_dim))
    }
    pub fn decoder_session(&self, capacity: usize) -> Result<DecoderSession<'_>, String> {
        let guard = self.resident.lock()?;
        let output = self.model.forward(
            "session",
            &[Input::i64("@capacity", &[], &[capacity as i64])],
        )?;
        position(&output, 0)?;
        Ok(DecoderSession {
            model: self,
            past: 0,
            _guard: guard,
        })
    }
    pub fn classify_prompt(
        &self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
        selected: &[usize],
    ) -> Result<DecoderLogits, String> {
        let _guard = self.resident.lock()?;
        self.decode("classify", tokens, audio, Some(selected), 0)
    }
    fn decode(
        &self,
        operation: &str,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
        selected: Option<&[usize]>,
        past: usize,
    ) -> Result<DecoderLogits, String> {
        let tokens = tokens
            .iter()
            .map(|token| i64::from(*token))
            .collect::<Vec<_>>();
        let shape = [tokens.len() as i64];
        let expected = [past as i64];
        let mut inputs = vec![
            Input::i64("tokens", &shape, &tokens),
            Input::i64("@expected_position", &[], &expected),
        ];
        let indices = if let Some((audio, offset)) = audio {
            self.resident.validate(audio)?;
            Some(audio_indices(tokens.len(), offset, audio.rows)?)
        } else {
            None
        };
        if let Some(indices) = &indices {
            inputs.push(Input::i64("audio_indices", &shape, indices));
        }
        let selected = selected.map(|rows| rows.iter().map(|row| *row as i64).collect::<Vec<_>>());
        let selected_shape = [selected.as_ref().map_or(0, Vec::len) as i64];
        if let Some(selected) = &selected {
            inputs.push(Input::i64("selected_rows", &selected_shape, selected));
        }
        let output = self.model.forward(operation, &inputs)?;
        position(&output, past + tokens.len())?;
        let classes = match self.config.kind {
            ModelKind::Asr => self.config.vocab,
            ModelKind::Aligner => self
                .config
                .timestamp_classes
                .ok_or("missing native timestamp classes")?,
        };
        let (rows, values) = matrix(&output, "logits", classes)?;
        Ok(DecoderLogits {
            rows,
            classes,
            values,
        })
    }
}

pub struct DecoderSession<'a> {
    model: &'a Qwen,
    past: usize,
    _guard: RefMut<'a, ()>,
}
impl DecoderSession<'_> {
    pub fn decode(
        &mut self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
    ) -> Result<DecoderLogits, String> {
        let output = self
            .model
            .decode("decode", tokens, audio, None, self.past)?;
        self.past += tokens.len();
        Ok(output)
    }
}
impl Drop for DecoderSession<'_> {
    fn drop(&mut self) {
        // Native clear releases this session's KV/audio. Drop cannot report an
        // exception; the next native encode/session reports its own failures.
        let _ = self.model.model.forward("clear", &[]);
        self.model.resident.clear();
    }
}
fn audio_indices(tokens: usize, offset: usize, rows: usize) -> Result<Vec<i64>, String> {
    let end = offset
        .checked_add(rows)
        .filter(|end| *end <= tokens)
        .ok_or("Qwen audio injection exceeds the actual prompt")?;
    let mut indices = vec![-1; tokens];
    for (row, value) in indices[offset..end].iter_mut().enumerate() {
        *value = row as i64;
    }
    Ok(indices)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_injection_maps_only_actual_encoder_rows() {
        assert_eq!(audio_indices(6, 2, 3).unwrap(), [-1, -1, 0, 1, 2, -1]);
        assert!(audio_indices(4, 2, 3).is_err());
        assert!(audio_indices(4, usize::MAX, 1).is_err());
    }
}
