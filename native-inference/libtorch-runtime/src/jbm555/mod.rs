use crate::{Input, Model};
#[path = "../../../ggml-runtime/src/jbm555/decode.rs"]
mod decode;
#[path = "../../../ggml-runtime/src/jbm555/frontend.rs"]
mod frontend;
#[path = "../../../ggml-runtime/src/jbm555/pipeline.rs"]
mod pipeline;
pub use decode::{Note, NoteRange, OFFSET_THRESHOLD, ONSET_THRESHOLD, decode_notes};
pub use frontend::{FREQUENCY_BINS, Frontend, HOP_SAMPLES, INPUT_CHANNELS, SAMPLE_RATE};
pub struct NetworkOutput { pub frames: usize, pub on_off: Vec<f32>, pub octave: Vec<f32>, pub pitch_class: Vec<f32> }
pub struct Jbm555 { model: Model }
impl Jbm555 {
    pub fn from_model(model: Model) -> Self { Self { model } }
    pub fn run_features_chunked(&self, features: &[f32], frames: usize, progress: &mut impl FnMut(u64, u64)) -> Result<NetworkOutput, String> {
        progress(0, 1);
        // Chunk ownership/context is implemented by the native plan so every
        // intermediate CNN tensor remains on the requested accelerator.
        let mut output = self.model.forward("forward", &[Input::f32("features", &[6, frames as i64, 384], features)])?;
        progress(1, 1);
        Ok(NetworkOutput { frames, on_off: output.take("on_off")?.into_f32()?, octave: output.take("octave")?.into_f32()?, pitch_class: output.take("pitch_class")?.into_f32()? })
    }
}
