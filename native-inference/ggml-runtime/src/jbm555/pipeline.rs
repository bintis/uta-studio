use std::path::Path;

use crate::wav::read_f32_wav;

use super::{Frontend, Jbm555, Note, SAMPLE_RATE, decode_notes};

impl Jbm555 {
    /// Runs the published dual-input JBM555 route. Both WAVs must be mono
    /// 44.1 kHz float inputs prepared by the worker's local ffmpeg boundary.
    pub fn process_wavs(
        &self,
        mix_path: &Path,
        vocal_path: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(Vec<Note>, usize), String> {
        let mix = read_f32_wav(mix_path, SAMPLE_RATE as u32, 1)?;
        let vocal = read_f32_wav(vocal_path, SAMPLE_RATE as u32, 1)?;
        if mix.is_empty() || vocal.is_empty() {
            return Err("JBM555 requires non-empty mix and prepared-vocal inputs".to_string());
        }
        let sample_count = mix.len().min(vocal.len());
        let (features, frames) =
            Frontend::default().extract(&mix[..sample_count], &vocal[..sample_count])?;
        let output = self.run_features_chunked(&features, frames, &mut progress)?;
        let notes = decode_notes(
            &output.on_off,
            &output.octave,
            &output.pitch_class,
            output.frames,
        )?;
        Ok((notes, sample_count))
    }
}
