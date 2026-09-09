use std::ffi::c_void;
use std::sync::Arc;

use crate::ffi::{
    AllocatorPtr, ContextPtr, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GgmlInitParams, GraphPtr,
    ModelApi, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

use super::frontend::{FREQUENCY_BINS, INPUT_CHANNELS};
use super::model::{Jbm555, tensor_ref};

const GRAPH_MEMORY_BYTES: usize = 16 * 1024 * 1024;
/// Frames one graph covers. JBM555 builds a single graph over its whole input,
/// so a 354.88-second song asked the Vulkan allocator to reserve a 13 GB
/// buffer and failed. Every convolution keeps the time axis, so the network is
/// exact on any contiguous frame range that carries enough neighbouring
/// context.
const CHUNK_FRAMES: usize = 1_024;
/// Context frames carried on each side of a chunk. The branch is five 9-tap
/// convolutions, so one output frame sees twenty input frames on each side;
/// sixty-four is comfortably beyond that, which makes the owned range of a
/// chunk bit-for-bit what a whole-input pass would have produced there.
const CONTEXT_FRAMES: usize = 64;
const GRAPH_NODE_CAPACITY: usize = 512;
const REDUCED_FREQUENCY_BINS: usize = FREQUENCY_BINS / 4;
const FLATTENED_FEATURES: usize = 32 * REDUCED_FREQUENCY_BINS;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: raw handles are owned by the live JBM555 model or graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkOutput {
    pub frames: usize,
    /// Frame-major probabilities `[frames, 4]`.
    pub on_off: Vec<f32>,
    /// Frame-major logits `[frames, 5]`.
    pub octave: Vec<f32>,
    /// Frame-major logits `[frames, 13]`.
    pub pitch_class: Vec<f32>,
}

impl Jbm555 {
    /// Runs the network over any number of frames by splitting the input into
    /// bounded chunks. Chunk outputs are concatenated frame-major, so the note
    /// decoder still sees one continuous sequence and needs no seam handling.
    pub fn run_features_chunked(
        &self,
        features: &[f32],
        frame_count: usize,
        mut report: impl FnMut(u64, u64),
    ) -> Result<NetworkOutput, String> {
        if frame_count <= CHUNK_FRAMES {
            let output = self.run_features(features, frame_count)?;
            report(1, 1);
            return Ok(output);
        }
        let chunks = frame_count.div_ceil(CHUNK_FRAMES);
        let mut on_off = Vec::with_capacity(frame_count * 4);
        let mut octave = Vec::with_capacity(frame_count * 5);
        let mut pitch_class = Vec::with_capacity(frame_count * 13);
        for index in 0..chunks {
            let owned_start = index * CHUNK_FRAMES;
            let owned_end = (owned_start + CHUNK_FRAMES).min(frame_count);
            let start = owned_start.saturating_sub(CONTEXT_FRAMES);
            let end = (owned_end + CONTEXT_FRAMES).min(frame_count);
            let chunk = frame_slice(features, frame_count, start, end)?;
            let output = self.run_features(&chunk, end - start)?;
            let offset = owned_start - start;
            let owned = owned_end - owned_start;
            on_off.extend_from_slice(&output.on_off[offset * 4..(offset + owned) * 4]);
            octave.extend_from_slice(&output.octave[offset * 5..(offset + owned) * 5]);
            pitch_class.extend_from_slice(&output.pitch_class[offset * 13..(offset + owned) * 13]);
            report(index as u64 + 1, chunks as u64);
        }
        Ok(NetworkOutput {
            frames: frame_count,
            on_off,
            octave,
            pitch_class,
        })
    }

    pub fn run_features(
        &self,
        features: &[f32],
        frame_count: usize,
    ) -> Result<NetworkOutput, String> {
        if frame_count == 0 {
            return Err("JBM555 graph requires at least one frame".to_string());
        }
        let expected = INPUT_CHANNELS
            .checked_mul(frame_count)
            .and_then(|value| value.checked_mul(FREQUENCY_BINS))
            .ok_or_else(|| "JBM555 feature dimensions overflowed".to_string())?;
        if features.len() != expected {
            return Err("JBM555 graph feature shape is invalid".to_string());
        }
        if features.iter().any(|value| !value.is_finite()) {
            return Err("JBM555 graph input contains a non-finite value".to_string());
        }

        let frames =
            i64::try_from(frame_count).map_err(|_| "JBM555 frame count exceeds i64".to_string())?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_4d(
                run.context,
                GGML_TYPE_F32,
                FREQUENCY_BINS as i64,
                frames,
                INPUT_CHANNELS as i64,
                1
            )
        );
        ggml!(api, ggml_set_input(input));
        let onset = self.build_branch(run.context, "onset_cnn", input, frames)?;
        let pitch = self.build_branch(run.context, "pitch_cnn", input, frames)?;
        for output in [onset, pitch] {
            ggml!(api, ggml_set_output(output));
            ggml!(api, ggml_build_forward_expand(run.graph, output));
        }
        run.allocate(&self.backend)?;
        set_f32(api, input, features, "JBM555 frontend features")?;
        run.compute(&self.backend)?;

        let mut on_off = get_f32(api, onset)?;
        if on_off.len() != frame_count * 4 {
            return Err("JBM555 onset graph returned an invalid shape".to_string());
        }
        for row in on_off.chunks_exact_mut(4) {
            softmax_in_place(row);
        }
        let pitch = get_f32(api, pitch)?;
        if pitch.len() != frame_count * 18 {
            return Err("JBM555 pitch graph returned an invalid shape".to_string());
        }
        let mut octave = Vec::with_capacity(frame_count * 5);
        let mut pitch_class = Vec::with_capacity(frame_count * 13);
        for row in pitch.chunks_exact(18) {
            octave.extend_from_slice(&row[..5]);
            pitch_class.extend_from_slice(&row[5..]);
        }
        if on_off
            .iter()
            .chain(&octave)
            .chain(&pitch_class)
            .any(|value| !value.is_finite())
        {
            return Err("JBM555 graph produced a non-finite value".to_string());
        }
        Ok(NetworkOutput {
            frames: frame_count,
            on_off,
            octave,
            pitch_class,
        })
    }

    fn build_branch(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let mut value = input;
        for layer in 1..=5 {
            value = self.conv(context, &format!("{prefix}.conv{layer}"), value, layer == 1)?;
            if layer < 5 {
                value = ggml!(api, ggml_relu(context, value));
            }
        }
        // Source is `[frequency, frames, channels]`; make channels the
        // innermost axis so each dense row is `[frequency, channels]`, matching
        // the historical ONNX transpose `[0, 2, 3, 1]`.
        let value = ggml!(api, ggml_permute(context, value, 1, 2, 0, 3));
        let value = ggml!(api, ggml_cont(context, value));
        let mut value = ggml!(
            api,
            ggml_reshape_2d(context, value, FLATTENED_FEATURES as i64, frames)
        );
        for layer in 1..=3 {
            value = self.linear(context, &format!("{prefix}.fc{layer}"), value)?;
            if layer < 3 {
                value = ggml!(api, ggml_relu(context, value));
            }
        }
        Ok(value)
    }

    fn conv(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        reduce_frequency: bool,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let stride = if reduce_frequency { 4 } else { 1 };
        let output = ggml!(
            api,
            ggml_conv_2d(
                context,
                self.weight(&format!("{prefix}.weight"))?,
                input,
                stride,
                1,
                4,
                4,
                1,
                1
            )
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let channels = tensor_ref(bias)?.ne[0];
        let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
        Ok(ggml!(api, ggml_add(context, output, bias)))
    }

    fn linear(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let output = ggml!(
            api,
            ggml_mul_mat(context, self.weight(&format!("{prefix}.weight"))?, input)
        );
        Ok(ggml!(
            api,
            ggml_add(context, output, self.weight(&format!("{prefix}.bias"))?)
        ))
    }
}

fn softmax_in_place(values: &mut [f32]) {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut total = 0.0;
    for value in values.iter_mut() {
        *value = (*value - maximum).exp();
        total += *value;
    }
    let total = total.max(f32::MIN_POSITIVE);
    for value in values {
        *value /= total;
    }
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32], label: &str) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err(format!("{label} byte length does not match GGML tensor"));
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "JBM555 tensor element count is invalid".to_string())?;
    let mut values = vec![0.0_f32; elements];
    ggml!(
        api,
        ggml_backend_tensor_get(
            tensor,
            values.as_mut_ptr().cast::<c_void>(),
            0,
            values.len() * std::mem::size_of::<f32>()
        )
    );
    Ok(values)
}

struct GraphRun {
    runtime: Arc<GgmlRuntime>,
    context: ContextPtr,
    graph: GraphPtr,
    allocator: AllocatorPtr,
}

impl GraphRun {
    fn new(runtime: Arc<GgmlRuntime>) -> Result<Self, String> {
        let api = &runtime.model_api;
        let context = ggml!(
            api,
            ggml_init(GgmlInitParams {
                mem_size: GRAPH_MEMORY_BYTES,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate JBM555 GGML graph context".to_string());
        }
        let graph = ggml!(
            api,
            ggml_new_graph_custom(context, GRAPH_NODE_CAPACITY, false)
        );
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate JBM555 GGML graph".to_string());
        }
        Ok(Self {
            runtime,
            context,
            graph,
            allocator: std::ptr::null_mut(),
        })
    }

    fn allocate(&mut self, backend: &GgmlBackendHandle) -> Result<(), String> {
        let api = &self.runtime.model_api;
        let buffer_type = ggml!(api, ggml_backend_get_default_buffer_type(backend.raw));
        self.allocator = ggml!(api, ggml_gallocr_new(buffer_type));
        if self.allocator.is_null() {
            return Err("could not create the JBM555 GGML graph allocator".to_string());
        }
        if !ggml!(api, ggml_gallocr_reserve(self.allocator, self.graph)) {
            return Err("could not reserve JBM555 GGML graph memory".to_string());
        }
        if !ggml!(api, ggml_gallocr_alloc_graph(self.allocator, self.graph)) {
            return Err("could not allocate the JBM555 GGML graph tensors".to_string());
        }
        Ok(())
    }

    fn compute(&self, backend: &GgmlBackendHandle) -> Result<(), String> {
        let status = ggml!(
            &self.runtime.model_api,
            ggml_backend_graph_compute(backend.raw, self.graph)
        );
        if status == GGML_STATUS_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "JBM555 GGML graph compute failed with status {status}"
            ))
        }
    }
}

impl Drop for GraphRun {
    fn drop(&mut self) {
        let api = &self.runtime.model_api;
        if !self.allocator.is_null() {
            ggml!(api, ggml_gallocr_free(self.allocator));
        }
        if !self.context.is_null() {
            ggml!(api, ggml_free(self.context));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_is_normalized_and_finite() {
        let mut values = [-3.0, 0.0, 2.0, 8.0];
        softmax_in_place(&mut values);
        assert!((values.iter().sum::<f32>() - 1.0).abs() < 1.0e-6);
        assert!(values.iter().all(|value| value.is_finite() && *value > 0.0));
    }

    #[test]
    fn a_frame_slice_keeps_the_channel_major_layout() {
        let frames = 4;
        let features: Vec<f32> = (0..INPUT_CHANNELS * frames * FREQUENCY_BINS)
            .map(|index| index as f32)
            .collect();
        let chunk = frame_slice(&features, frames, 1, 3).unwrap();
        assert_eq!(chunk.len(), INPUT_CHANNELS * 2 * FREQUENCY_BINS);
        for channel in 0..INPUT_CHANNELS {
            for frame in 0..2 {
                let expected =
                    features[channel * frames * FREQUENCY_BINS + (frame + 1) * FREQUENCY_BINS];
                let actual = chunk[channel * 2 * FREQUENCY_BINS + frame * FREQUENCY_BINS];
                assert_eq!(actual, expected);
            }
        }
        assert!(frame_slice(&features, frames, 3, 3).is_err());
        assert!(frame_slice(&features, frames, 1, 5).is_err());
    }

    /// The claim the chunking rests on: inside its owned range a chunk carries
    /// enough context that its output matches a whole-input pass.
    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and JBM555 GGUF"]
    fn chunked_network_matches_a_whole_input_pass() {
        use crate::{DeviceKind, GgmlRuntime};
        use std::path::PathBuf;

        fn path(name: &str) -> PathBuf {
            std::env::var_os(name)
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("set {name}"))
        }
        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let expected_kind = match std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND")
            .as_str()
        {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            "discrete_gpu" => DeviceKind::DiscreteGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION").unwrap_or_default();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested test device is unavailable");
        let model = Jbm555::load(runtime, &device, &path("UTA_TEST_JBM555_GGUF")).unwrap();

        let frames = CHUNK_FRAMES + CHUNK_FRAMES / 3;
        let features: Vec<f32> = (0..INPUT_CHANNELS * frames * FREQUENCY_BINS)
            .map(|index| ((index as f32) * 0.37).sin() * 0.5)
            .collect();
        let whole = model.run_features(&features, frames).unwrap();
        let chunked = model
            .run_features_chunked(&features, frames, |_, _| {})
            .unwrap();
        assert_eq!(chunked.frames, whole.frames);
        for (name, left, right) in [
            ("on_off", &whole.on_off, &chunked.on_off),
            ("octave", &whole.octave, &chunked.octave),
            ("pitch_class", &whole.pitch_class, &chunked.pitch_class),
        ] {
            assert_eq!(left.len(), right.len(), "{name} length");
            let worst = left
                .iter()
                .zip(right)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            eprintln!("JBM555 chunked {name} worst difference: {worst:.3e}");
            assert!(worst < 1.0e-4, "{name} differs by {worst:.3e}");
        }
    }
}

/// Copies the `[channel][frame][bin]` feature window `start..end` into a
/// standalone buffer with the same layout.
fn frame_slice(
    features: &[f32],
    frame_count: usize,
    start: usize,
    end: usize,
) -> Result<Vec<f32>, String> {
    if start >= end || end > frame_count {
        return Err("JBM555 chunk range is invalid".to_string());
    }
    let frames = end - start;
    let mut chunk = Vec::with_capacity(INPUT_CHANNELS * frames * FREQUENCY_BINS);
    for channel in 0..INPUT_CHANNELS {
        let base = channel * frame_count * FREQUENCY_BINS;
        chunk.extend_from_slice(
            &features[base + start * FREQUENCY_BINS..base + end * FREQUENCY_BINS],
        );
    }
    Ok(chunk)
}
