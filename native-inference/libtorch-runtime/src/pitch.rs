use crate::{Input, Model};
use std::path::Path;

fn frame_major(channel_major: &[f32], channels: usize, frames: usize) -> Result<Vec<f32>, String> {
    if channel_major.len()
        != channels
            .checked_mul(frames)
            .ok_or_else(|| "mel shape overflow".to_string())?
    {
        return Err("canonical mel buffer does not match its dimensions".to_string());
    }
    let mut result = vec![0.0; channel_major.len()];
    for frame in 0..frames {
        for channel in 0..channels {
            result[frame * channels + channel] = channel_major[channel * frames + frame];
        }
    }
    Ok(result)
}

pub mod rmvpe {
    use super::*;
    pub use uta_ggml_runtime::rmvpe::PitchFrame;
    pub struct Rmvpe {
        model: Model,
    }
    impl Rmvpe {
        pub fn from_model(model: Model) -> Self {
            Self { model }
        }
        pub fn process_wav(
            &self,
            path: &Path,
            progress: impl FnMut(u64, u64),
        ) -> Result<Vec<PitchFrame>, String> {
            uta_ggml_runtime::rmvpe::host::process_wav(path, progress, |mel, frames| {
                let input = frame_major(mel, 128, frames)?;
                self.model
                    .forward(
                        "forward",
                        &[Input::f32("mel", &[frames as i64, 128], &input)],
                    )?
                    .take("salience")?
                    .into_f32()
            })
        }
    }
}
pub mod fcpe {
    use super::*;
    pub use uta_ggml_runtime::fcpe::PitchFrame;
    pub struct Fcpe {
        model: Model,
        cents: Vec<f32>,
    }
    impl Fcpe {
        pub fn from_model(model: Model) -> Result<Self, String> {
            let cents = model
                .forward("constants", &[])?
                .take("cents_mapping")?
                .into_f32()?;
            Ok(Self { model, cents })
        }
        pub fn process_wav(
            &self,
            path: &Path,
            progress: impl FnMut(u64, u64),
        ) -> Result<Vec<PitchFrame>, String> {
            self.process_wav_with_threshold(path, 0.006, progress)
        }
        pub fn process_wav_with_threshold(
            &self,
            path: &Path,
            threshold: f32,
            progress: impl FnMut(u64, u64),
        ) -> Result<Vec<PitchFrame>, String> {
            uta_ggml_runtime::fcpe::host::process_wav(
                path,
                progress,
                &self.cents,
                threshold,
                |mel| {
                    let input = frame_major(mel, 128, 201)?;
                    self.model
                        .forward("forward", &[Input::f32("mel", &[201, 128], &input)])?
                        .take("salience")?
                        .into_f32()
                },
            )
        }
    }
}
pub mod basic_pitch {
    use super::*;
    pub use uta_ggml_runtime::basic_pitch::ActivationFrame;
    pub struct BasicPitch {
        model: Model,
    }
    impl BasicPitch {
        pub fn from_model(model: Model) -> Self {
            Self { model }
        }
        pub fn process_wav(
            &self,
            path: &Path,
            progress: impl FnMut(u64, u64),
        ) -> Result<Vec<ActivationFrame>, String> {
            uta_ggml_runtime::basic_pitch::host::process_wav(path, progress, |audio| {
                let mut output = self.model.forward(
                    "forward",
                    &[Input::f32("audio", &[1, audio.len() as i64], audio)],
                )?;
                Ok(uta_ggml_runtime::basic_pitch::WindowActivations {
                    notes: output.take("frame")?.into_f32()?,
                    onsets: output.take("onset")?.into_f32()?,
                    contours: output.take("contour")?.into_f32()?,
                })
            })
        }
    }
}
