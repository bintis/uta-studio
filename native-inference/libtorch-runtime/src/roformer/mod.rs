//! Native RoFormer learned graph with the canonical shared Rust STFT and
//! ordered overlap-add. No GGML library or graph is used by this adapter.
use std::path::Path;
use crate::{Input, Model};
use crate::stft::compute_stft;
use crate::wav::{read_f32_wav, write_f32_wav};
#[path = "../../../ggml-runtime/src/roformer/frames.rs"]
mod frames;
#[path = "../../../ggml-runtime/src/roformer/overlap.rs"]
mod overlap;

struct Config {
    fft_size: usize, hop_length: usize, window_length: usize,
    stem_count: usize, zero_dc: bool, sample_rate: u32,
    chunk_size: usize, overlap: usize,
    frequency_indices: Vec<usize>, bands_per_frequency: Vec<usize>,
}
pub struct Roformer { model: Model, config: Config }
impl Roformer {
    pub fn from_model(model: Model) -> Result<Self, String> {
        let architecture = model.metadata.get("general.architecture").and_then(serde_json::Value::as_str)
            .ok_or_else(|| "RoFormer GGUF lacks its architecture".to_string())?;
        let architecture = match architecture { "bs" | "bs-roformer" => "bs_roformer", "mel_band" | "mel-band-roformer" => "mel_band_roformer", value => value };
        let public = model.metadata.get(format!("{architecture}.n_bands")).is_some();
        let integer = |name: &str, fallback: usize| -> Result<usize, String> {
            match model.metadata.get(format!("{architecture}.{name}")) {
                None => Ok(fallback), Some(value) => value.as_u64().and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| format!("invalid RoFormer integer metadata: {name}")),
            }
        };
        let constants = model.forward("constants", &[])?;
        let integers = |name: &str| -> Result<Vec<usize>, String> {
            constants.get(name)?.i64()?.iter().map(|value| usize::try_from(*value).map_err(|_| format!("negative RoFormer {name} value"))).collect()
        };
        let config = Config {
            fft_size: integer(if public { "n_fft" } else { "stft_n_fft" }, 2048)?,
            hop_length: integer(if public { "hop_length" } else { "stft_hop_length" }, 441)?,
            window_length: integer(if public { "win_length" } else { "stft_win_length" }, 2048)?,
            stem_count: integer(if public { "n_stems" } else { "num_stems" }, 1)?,
            zero_dc: model.metadata.get(format!("{architecture}.zero_dc")).and_then(serde_json::Value::as_bool).unwrap_or(false),
            sample_rate: u32::try_from(integer("sample_rate", 44100)?).map_err(|_| "RoFormer sample rate is too large".to_string())?,
            chunk_size: integer(if public { "chunk_size" } else { "default_chunk_size" }, 352800)?,
            overlap: integer("default_num_overlap", 2)?,
            frequency_indices: integers("frequency_indices")?, bands_per_frequency: integers("bands_per_frequency")?,
        };
        if config.fft_size == 0 || config.hop_length == 0 || config.window_length > config.fft_size || config.chunk_size == 0
            || config.stem_count == 0 || config.bands_per_frequency.len() != config.fft_size / 2 + 1 {
            return Err("RoFormer native frontend configuration is invalid".to_string());
        }
        Ok(Self { model, config })
    }
    pub fn process_chunk(&self, interleaved: &[f32]) -> Result<Vec<Vec<f32>>, String> {
        if interleaved.is_empty() || interleaved.len() % 2 != 0 { return Err("RoFormer chunk must contain stereo frames".to_string()); }
        let channels: [Vec<f32>; 2] = std::array::from_fn(|channel| interleaved.iter().skip(channel).step_by(2).copied().collect());
        let spectra = channels.map(|audio| compute_stft(&audio, self.config.fft_size, self.config.hop_length, self.config.window_length));
        let frame_count = spectra[0].n_frames;
        let features = self.config.frequency_indices.len() * 2;
        let input = frames::prepare_model_input(&spectra, frame_count, features, &self.config.frequency_indices)?;
        let shape = [frame_count as i64, features as i64];
        let mut output = self.model.forward("mask", &[Input::f32("features", &shape, &input)])?;
        let mask = output.take("mask")?.into_f32()?;
        if mask.iter().any(|value| !value.is_finite()) { return Err("native RoFormer emitted nonfinite masks".to_string()); }
        frames::reconstruct_stems(&mask, &spectra, interleaved.len() / 2, &self.config)
    }
    pub fn process_wav(&mut self, input: &Path, output: &Path, progress: &mut impl FnMut(u64, u64)) -> Result<(), String> {
        let audio = read_f32_wav(input, self.config.sample_rate, 2)?;
        let stems = frames::process_overlap_add(&audio, self.config.chunk_size, self.config.overlap, |chunk| self.process_chunk(chunk), progress)?;
        if stems.len() != 1 { return Err("the catalog separation request requires one direct stem; residual is published by the worker".to_string()); }
        write_f32_wav(output, self.config.sample_rate, 2, &stems[0])
    }
}
