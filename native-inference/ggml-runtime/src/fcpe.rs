use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::ffi::{
    AllocatorPtr, BufferPtr, ContextPtr, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GgmlInitParams,
    GgmlTensor, GgufInitParams, GgufPtr, GraphPtr, ModelApi, TensorPtr,
};
use crate::wav::read_f32_wav;
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

const SAMPLE_RATE: u32 = 16_000;
const FFT_SIZE: usize = 1024;
const HOP_SIZE: usize = 160;
const MEL_BINS: usize = 128;
const INPUT_SAMPLES: usize = 32_000;
const WINDOW_FRAMES: usize = INPUT_SAMPLES / HOP_SIZE + 1;
const MODEL_CHANNELS: usize = 512;
const FEED_FORWARD_CHANNELS: usize = 2048;
const CONV_CHANNELS: usize = 1024;
const ENCODER_LAYERS: usize = 6;
const CONV_KERNEL: usize = 31;
const PITCH_CLASSES: usize = 360;
const VOICED_THRESHOLD: f32 = 0.006;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all raw handles are owned by the live model or graph scope
        // surrounding this call into the pinned GGML C ABI.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct PitchFrame {
    pub time: f64,
    pub hz: Option<f32>,
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
                mem_size: 32 * 1024 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate FCPE GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, 4096, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate FCPE GGML graph".to_string());
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
        if self.allocator.is_null()
            || !ggml!(api, ggml_gallocr_reserve(self.allocator, self.graph))
            || !ggml!(api, ggml_gallocr_alloc_graph(self.allocator, self.graph))
        {
            return Err("could not allocate FCPE GGML graph".to_string());
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
                "FCPE GGML graph compute failed with status {status}"
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

/// Rust-owned FCPE execution over the pinned upstream GGML ABI.
pub struct Fcpe {
    backend: GgmlBackendHandle,
    weight_context: ContextPtr,
    weight_buffer: BufferPtr,
    cents_mapping: Vec<f32>,
}

impl Drop for Fcpe {
    fn drop(&mut self) {
        let api = &self.backend.runtime.model_api;
        if !self.weight_buffer.is_null() {
            ggml!(api, ggml_backend_buffer_free(self.weight_buffer));
            self.weight_buffer = std::ptr::null_mut();
        }
        if !self.weight_context.is_null() {
            ggml!(api, ggml_free(self.weight_context));
            self.weight_context = std::ptr::null_mut();
        }
    }
}

impl Fcpe {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        let backend = runtime.create_backend(device)?;
        let mut model = Self {
            backend,
            weight_context: std::ptr::null_mut(),
            weight_buffer: std::ptr::null_mut(),
            cents_mapping: Vec::new(),
        };
        model.load_weights(model_path)?;
        model.cents_mapping = get_f32(model.api(), model.weight("cents_mapping")?)?;
        if model.cents_mapping.len() != PITCH_CLASSES
            || model.cents_mapping.iter().any(|value| !value.is_finite())
        {
            return Err("FCPE cents mapping is invalid".to_string());
        }
        Ok(model)
    }

    pub fn process_wav(
        &self,
        input_path: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<PitchFrame>, String> {
        let audio = read_f32_wav(input_path, SAMPLE_RATE, 1)?;
        let expected_frames = audio.len() / HOP_SIZE + 1;
        let window_count = audio.len().div_ceil(INPUT_SAMPLES);
        let mut frames = Vec::with_capacity(expected_frames);
        for window in 0..window_count {
            let source_sample_start = window * INPUT_SAMPLES;
            let source_sample_end = (source_sample_start + INPUT_SAMPLES).min(audio.len());
            let mut audio_window = vec![0.0_f32; INPUT_SAMPLES];
            audio_window[..source_sample_end - source_sample_start]
                .copy_from_slice(&audio[source_sample_start..source_sample_end]);
            let mel = log_mel_window(&audio_window)?;
            let input = channel_major_window(&mel, WINDOW_FRAMES, 0, WINDOW_FRAMES);
            let activations = self.run_window(&input)?;
            let decoded = decode_pitch(&activations, &self.cents_mapping)?;
            for (local_frame, hz) in decoded.into_iter().enumerate() {
                if window > 0 && local_frame == 0 {
                    continue;
                }
                let sample = source_sample_start + local_frame * HOP_SIZE;
                if sample > audio.len() {
                    break;
                }
                frames.push(PitchFrame {
                    time: sample as f64 / SAMPLE_RATE as f64,
                    hz,
                });
            }
            progress((window + 1) as u64, window_count as u64);
        }
        if frames.is_empty() || frames.len() != expected_frames {
            return Err("FCPE window stitching changed the evidence timeline".to_string());
        }
        Ok(frames)
    }

    fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "FCPE GGUF path")?;
        let mut context = std::ptr::null_mut();
        let gguf = ggml!(
            self.api(),
            gguf_init_from_file(
                encoded.as_ptr(),
                GgufInitParams {
                    no_alloc: true,
                    ctx: &mut context,
                }
            )
        );
        if gguf.is_null() || context.is_null() {
            return Err("could not open FCPE GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        if self.required_string(gguf, "general.architecture")? != "fcpe" {
            return Err("GGUF general.architecture is not fcpe".to_string());
        }
        for (key, expected) in [
            ("sample_rate", 16_000),
            ("hop_size", 160),
            ("window_size", 32_000),
            ("n_frames", 201),
            ("n_mel_bins", 128),
        ] {
            if self.required_u32(gguf, key)? != expected {
                return Err(format!("FCPE GGUF metadata mismatch: {key}"));
            }
        }
        if ggml!(self.api(), gguf_get_n_tensors(gguf)) != 59 {
            return Err("FCPE GGUF tensor count is not 59".to_string());
        }
        self.validate_weight_shapes()?;
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        let buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if buffer.is_null() {
            return Err("could not allocate FCPE GGML weight buffer".to_string());
        }
        self.weight_buffer = buffer;
        self.upload_tensors(path, gguf)
    }

    fn validate_weight_shapes(&self) -> Result<(), String> {
        for (name, dimensions) in [
            ("input_stack.0.weight", &[3, 128, 512][..]),
            ("input_stack.0.bias", &[512][..]),
            ("input_stack.1.weight", &[3, 512, 512][..]),
            ("input_stack.1.bias", &[512][..]),
            ("norm.weight", &[512][..]),
            ("norm.bias", &[512][..]),
            ("output_proj.weight", &[360, 512][..]),
            ("output_proj.bias", &[360][..]),
            ("cents_mapping", &[360][..]),
            ("mel_scale", &[512][..]),
            ("mel_bias", &[512][..]),
        ] {
            self.require_shape(name, dimensions)?;
        }
        for layer in 0..ENCODER_LAYERS {
            let prefix = format!("encoder_layers.{layer}");
            for (suffix, dimensions) in [
                ("norm.weight", &[512][..]),
                ("norm.bias", &[512][..]),
                ("fc1.weight", &[1, 512, 2048][..]),
                ("fc1.bias", &[2048][..]),
                ("conv.weight", &[31, 1, 1024][..]),
                ("conv.bias", &[1024][..]),
                ("fc2.weight", &[1, 1024, 512][..]),
                ("fc2.bias", &[512][..]),
            ] {
                self.require_shape(&format!("{prefix}.{suffix}"), dimensions)?;
            }
        }
        Ok(())
    }

    fn require_shape(&self, name: &str, expected: &[i64]) -> Result<(), String> {
        let tensor = tensor_ref(self.weight(name)?)?;
        let actual_dimensions = tensor
            .ne
            .iter()
            .rposition(|dimension| *dimension != 1)
            .map_or(1, |last| last + 1);
        if tensor.type_ != GGML_TYPE_F32 || &tensor.ne[..actual_dimensions] != expected {
            return Err(format!("FCPE GGUF tensor shape mismatch: {name}"));
        }
        Ok(())
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let api = self.api();
        let data_offset = ggml!(api, gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen FCPE GGUF: {error}"))?;
        let mut tensor = ggml!(api, ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!("FCPE tensor is not F32: {}", tensor_name(tensor)?));
            }
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "FCPE tensor name contains NUL".to_string())?;
            let tensor_id = ggml!(api, gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_id < 0 {
                return Err(format!("FCPE tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(api, ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(api, gguf_get_tensor_offset(gguf, tensor_id)))
                .ok_or_else(|| "FCPE tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek FCPE tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read FCPE tensor {name}: {error}"))?;
            ggml!(
                api,
                ggml_backend_tensor_set(tensor, bytes.as_ptr().cast::<c_void>(), 0, size)
            );
            tensor = ggml!(api, ggml_get_next_tensor(self.weight_context, tensor));
        }
        Ok(())
    }

    fn required_string(&self, gguf: GgufPtr, key: &str) -> Result<String, String> {
        let index = self.key_index(gguf, key)?;
        let raw = ggml!(self.api(), gguf_get_val_str(gguf, index));
        if raw.is_null() {
            return Err(format!("GGUF string {key} is null"));
        }
        // SAFETY: GGUF owns this NUL-terminated string during the call.
        Ok(unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned())
    }

    fn required_u32(&self, gguf: GgufPtr, key: &str) -> Result<u32, String> {
        Ok(ggml!(
            self.api(),
            gguf_get_val_u32(gguf, self.key_index(gguf, key)?)
        ))
    }

    fn key_index(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api(), gguf_find_key(gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("FCPE GGUF is missing {key}"))
        } else {
            Ok(index)
        }
    }

    fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        let encoded = CString::new(name).map_err(|_| "weight name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("FCPE GGUF is missing weight {name}"))
        } else {
            Ok(tensor)
        }
    }

    fn conv1d(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        depthwise: bool,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let output = if depthwise {
            ggml!(
                api,
                ggml_conv_1d_dw(context, weight, input, 1, CONV_KERNEL as i32 / 2, 1)
            )
        } else {
            ggml!(api, ggml_conv_1d(context, weight, input, 1, 1, 1))
        };
        let channels = tensor_ref(bias)?.ne[0];
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, channels, 1));
        Ok(ggml!(api, ggml_add(context, output, bias)))
    }

    fn feature_major(&self, context: ContextPtr, input: TensorPtr) -> TensorPtr {
        let transposed = ggml!(self.api(), ggml_transpose(context, input));
        ggml!(self.api(), ggml_cont(context, transposed))
    }

    fn layer_norm(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = ggml!(api, ggml_norm(context, input, 1.0e-5));
        let scaled = ggml!(
            api,
            ggml_mul(
                context,
                normalized,
                self.weight(&format!("{prefix}.weight"))?
            )
        );
        Ok(ggml!(
            api,
            ggml_add(context, scaled, self.weight(&format!("{prefix}.bias"))?)
        ))
    }

    fn pointwise(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        input_channels: usize,
        output_channels: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let weight = ggml!(
            api,
            ggml_reshape_2d(
                context,
                weight,
                input_channels as i64,
                output_channels as i64
            )
        );
        let output = ggml!(api, ggml_mul_mat(context, weight, input));
        Ok(ggml!(
            api,
            ggml_add(context, output, self.weight(&format!("{prefix}.bias"))?)
        ))
    }

    fn conformer_layer(
        &self,
        context: ContextPtr,
        layer: usize,
        input: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let prefix = format!("encoder_layers.{layer}");
        let normalized = self.layer_norm(context, &format!("{prefix}.norm"), input)?;
        let expanded = self.pointwise(
            context,
            &format!("{prefix}.fc1"),
            normalized,
            MODEL_CHANNELS,
            FEED_FORWARD_CHANNELS,
        )?;
        let descriptor = tensor_ref(expanded)?;
        let first = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                CONV_CHANNELS as i64,
                frames as i64,
                descriptor.nb[1],
                0
            )
        );
        let gate = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                CONV_CHANNELS as i64,
                frames as i64,
                descriptor.nb[1],
                CONV_CHANNELS * descriptor.nb[0]
            )
        );
        let gate = ggml!(api, ggml_sigmoid(context, gate));
        let gated = ggml!(api, ggml_mul(context, first, gate));
        let convolution_input = self.feature_major(context, gated);
        let convolved = self.conv1d(context, &format!("{prefix}.conv"), convolution_input, true)?;
        let convolved = ggml!(api, ggml_silu(context, convolved));
        let convolved = self.feature_major(context, convolved);
        let projected = self.pointwise(
            context,
            &format!("{prefix}.fc2"),
            convolved,
            CONV_CHANNELS,
            MODEL_CHANNELS,
        )?;
        Ok(ggml!(api, ggml_add(context, input, projected)))
    }

    fn build_graph(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let first = self.conv1d(context, "input_stack.0", input, false)?;
        // The checkpoint's input normalization reshapes 512 channels into
        // four groups and normalizes each complete 128-channel timeline.
        let group_elements = WINDOW_FRAMES * (MODEL_CHANNELS / 4);
        let grouped = ggml!(
            api,
            ggml_reshape_2d(context, first, group_elements as i64, 4)
        );
        let normalized = ggml!(api, ggml_norm(context, grouped, 1.0e-5));
        let normalized = ggml!(
            api,
            ggml_reshape_3d(
                context,
                normalized,
                WINDOW_FRAMES as i64,
                MODEL_CHANNELS as i64,
                1
            )
        );
        let scale = ggml!(
            api,
            ggml_reshape_3d(
                context,
                self.weight("mel_scale")?,
                1,
                MODEL_CHANNELS as i64,
                1
            )
        );
        let bias = ggml!(
            api,
            ggml_reshape_3d(
                context,
                self.weight("mel_bias")?,
                1,
                MODEL_CHANNELS as i64,
                1
            )
        );
        let normalized = ggml!(api, ggml_mul(context, normalized, scale));
        let normalized = ggml!(api, ggml_add(context, normalized, bias));
        let normalized = ggml!(api, ggml_leaky_relu(context, normalized, 0.01, false));
        let second = self.conv1d(context, "input_stack.1", normalized, false)?;
        let mut hidden = self.feature_major(context, second);
        for layer in 0..ENCODER_LAYERS {
            hidden = self.conformer_layer(context, layer, hidden, WINDOW_FRAMES)?;
        }
        let normalized = self.layer_norm(context, "norm", hidden)?;
        let projection = self.weight("output_proj.weight")?;
        let projection = ggml!(api, ggml_transpose(context, projection));
        let projection = ggml!(api, ggml_cont(context, projection));
        let output = ggml!(api, ggml_mul_mat(context, projection, normalized));
        let output = ggml!(
            api,
            ggml_add(context, output, self.weight("output_proj.bias")?)
        );
        let output = ggml!(api, ggml_sigmoid(context, output));
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(graph, output));
        Ok(output)
    }

    fn run_window(&self, mel: &[f32]) -> Result<Vec<f32>, String> {
        if mel.len() != WINDOW_FRAMES * MEL_BINS {
            return Err("FCPE mel window shape is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_3d(
                run.context,
                GGML_TYPE_F32,
                WINDOW_FRAMES as i64,
                MEL_BINS as i64,
                1
            )
        );
        ggml!(api, ggml_set_input(input));
        let output = self.build_graph(run.context, run.graph, input)?;
        run.allocate(&self.backend)?;
        set_f32(api, input, mel)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }
}

fn tensor_ref(raw: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers retain the context that owns this tensor.
    unsafe { raw.as_ref() }.ok_or_else(|| "GGML returned a null tensor".to_string())
}

fn tensor_name(raw: TensorPtr) -> Result<String, String> {
    let tensor = tensor_ref(raw)?;
    // SAFETY: GGML stores tensor names as NUL-terminated fixed arrays.
    Ok(unsafe { CStr::from_ptr(tensor.name.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("FCPE GGML input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "FCPE tensor element count is invalid".to_string())?;
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

#[derive(Clone)]
struct MelBand(Vec<(usize, f32)>);

fn mel_bands() -> &'static [MelBand] {
    static BANDS: OnceLock<Vec<MelBand>> = OnceLock::new();
    BANDS.get_or_init(|| {
        let minimum = hz_to_mel(0.0);
        let maximum = hz_to_mel(8000.0);
        let points = (0..MEL_BINS + 2)
            .map(|index| {
                mel_to_hz(minimum + index as f32 / (MEL_BINS + 1) as f32 * (maximum - minimum))
            })
            .collect::<Vec<_>>();
        (0..MEL_BINS)
            .map(|band| {
                let lower = points[band];
                let center = points[band + 1];
                let upper = points[band + 2];
                let normalization = 2.0 / (upper - lower);
                let weights = (0..=FFT_SIZE / 2)
                    .filter_map(|bin| {
                        let frequency = SAMPLE_RATE as f32 * bin as f32 / FFT_SIZE as f32;
                        let weight = if (lower..=center).contains(&frequency) {
                            (frequency - lower) / (center - lower)
                        } else if frequency > center && frequency <= upper {
                            (upper - frequency) / (upper - center)
                        } else {
                            0.0
                        } * normalization;
                        (weight > 0.0).then_some((bin, weight))
                    })
                    .collect();
                MelBand(weights)
            })
            .collect()
    })
}

fn hz_to_mel(hz: f32) -> f32 {
    const LINEAR_FREQUENCY_STEP: f32 = 200.0 / 3.0;
    const LOG_FREQUENCY_START: f32 = 1000.0;
    const LOG_MEL_START: f32 = LOG_FREQUENCY_START / LINEAR_FREQUENCY_STEP;
    let log_step = 6.4_f32.ln() / 27.0;
    if hz < LOG_FREQUENCY_START {
        hz / LINEAR_FREQUENCY_STEP
    } else {
        LOG_MEL_START + (hz / LOG_FREQUENCY_START).ln() / log_step
    }
}

fn mel_to_hz(mel: f32) -> f32 {
    const LINEAR_FREQUENCY_STEP: f32 = 200.0 / 3.0;
    const LOG_FREQUENCY_START: f32 = 1000.0;
    const LOG_MEL_START: f32 = LOG_FREQUENCY_START / LINEAR_FREQUENCY_STEP;
    let log_step = 6.4_f32.ln() / 27.0;
    if mel < LOG_MEL_START {
        mel * LINEAR_FREQUENCY_STEP
    } else {
        LOG_FREQUENCY_START * (log_step * (mel - LOG_MEL_START)).exp()
    }
}

fn reflected_sample(audio: &[f32], padded_index: usize) -> f32 {
    let padding = 432;
    if padded_index < padding {
        return audio[padding - padded_index];
    }
    let audio_index = padded_index - padding;
    if audio_index < audio.len() {
        audio[audio_index]
    } else {
        audio[audio.len() - 2 - (audio_index - audio.len())]
    }
}

fn log_mel_window(audio: &[f32]) -> Result<Vec<f32>, String> {
    if audio.len() != INPUT_SAMPLES {
        return Err("FCPE frontend requires an exact 32,000-sample window".to_string());
    }
    const REFLECT_PADDING: usize = 432;
    const STFT_FRAMES: usize = 200;
    if (STFT_FRAMES - 1) * HOP_SIZE + FFT_SIZE > audio.len() + REFLECT_PADDING * 2 {
        return Err("FCPE STFT frame calculation exceeded reflected padding".to_string());
    }
    let window = (0..FFT_SIZE)
        .map(|index| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * index as f32 / FFT_SIZE as f32).cos())
        })
        .collect::<Vec<_>>();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let mut spectrum = vec![Complex32::default(); FFT_SIZE];
    let mut output = vec![0.0_f32; WINDOW_FRAMES * MEL_BINS];
    for frame in 0..STFT_FRAMES {
        let start = frame * HOP_SIZE;
        for index in 0..FFT_SIZE {
            spectrum[index] =
                Complex32::new(reflected_sample(audio, start + index) * window[index], 0.0);
        }
        fft.process_with_scratch(&mut spectrum, &mut scratch);
        for (band, weights) in mel_bands().iter().enumerate() {
            let energy = weights
                .0
                .iter()
                .map(|(bin, weight)| (spectrum[*bin].norm_sqr() + 1.0e-9).sqrt() * weight)
                .sum::<f32>();
            output[frame * MEL_BINS + band] = energy.max(1.0e-5).ln();
        }
    }
    let last = (STFT_FRAMES - 1) * MEL_BINS;
    output.copy_within(last..last + MEL_BINS, STFT_FRAMES * MEL_BINS);
    Ok(output)
}

fn channel_major_window(
    frame_major: &[f32],
    frames: usize,
    start: usize,
    window_frames: usize,
) -> Vec<f32> {
    let mut output = vec![0.0_f32; MEL_BINS * window_frames];
    let copied = frames.saturating_sub(start).min(window_frames);
    for frame in 0..copied {
        for channel in 0..MEL_BINS {
            output[channel * window_frames + frame] =
                frame_major[(start + frame) * MEL_BINS + channel];
        }
    }
    output
}

fn decode_pitch(activations: &[f32], cents_mapping: &[f32]) -> Result<Vec<Option<f32>>, String> {
    if activations.len() != WINDOW_FRAMES * PITCH_CLASSES
        || cents_mapping.len() != PITCH_CLASSES
        || activations.iter().any(|value| !value.is_finite())
    {
        return Err("FCPE output activation shape or values are invalid".to_string());
    }
    let mut pitches = Vec::with_capacity(WINDOW_FRAMES);
    for row in activations.chunks_exact(PITCH_CLASSES) {
        let (peak, maximum) = row
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .ok_or_else(|| "FCPE activation row is empty".to_string())?;
        if maximum <= VOICED_THRESHOLD {
            pitches.push(None);
            continue;
        }
        let mut weighted_cents = 0.0_f64;
        let mut weight_sum = 0.0_f64;
        for offset in -4_isize..=4 {
            let index = (peak as isize + offset).clamp(0, PITCH_CLASSES as isize - 1) as usize;
            let weight = f64::from(row[index]);
            weighted_cents += f64::from(cents_mapping[index]) * weight;
            weight_sum += weight;
        }
        if !weight_sum.is_finite() || weight_sum <= 0.0 {
            pitches.push(None);
            continue;
        }
        let hz = (10.0 * 2.0_f64.powf(weighted_cents / weight_sum / 1200.0)) as f32;
        pitches.push((hz.is_finite() && hz > 0.0).then_some(hz));
    }
    Ok(pitches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_frontend_is_finite_and_duplicates_the_final_model_frame() {
        let mel = log_mel_window(&vec![0.0; INPUT_SAMPLES]).unwrap();
        assert_eq!(mel.len(), WINDOW_FRAMES * MEL_BINS);
        assert!(mel.iter().all(|value| value.is_finite()));
        assert!(
            mel.iter()
                .all(|value| (*value - 1.0e-5_f32.ln()).abs() < 1.0e-5)
        );
        assert_eq!(
            &mel[(WINDOW_FRAMES - 2) * MEL_BINS..(WINDOW_FRAMES - 1) * MEL_BINS],
            &mel[(WINDOW_FRAMES - 1) * MEL_BINS..]
        );
    }

    #[test]
    fn slaney_mel_frontend_matches_checkpoint_filter_anchors() {
        let bands = mel_bands();
        assert!((bands[0].0[0].1 - 0.02857778).abs() < 1.0e-7);
        assert!((bands[0].0[1].1 - 0.028377542).abs() < 1.0e-7);
        assert_eq!(bands[0].0[0].0, 1);
        assert_eq!(bands[0].0[1].0, 2);
    }

    #[test]
    fn decoder_uses_the_local_centroid_and_preserves_unvoiced_frames() {
        let cents = (0..PITCH_CLASSES)
            .map(|index| index as f32 * 20.0)
            .collect::<Vec<_>>();
        let mut activations = vec![0.0; WINDOW_FRAMES * PITCH_CLASSES];
        for offset in -4_isize..=4 {
            let index = (100_isize + offset) as usize;
            activations[index] = 0.5;
        }
        activations[100] = 0.6;
        let pitches = decode_pitch(&activations, &cents).unwrap();
        let expected = 10.0 * 2.0_f32.powf(2000.0 / 1200.0);
        assert!((pitches[0].unwrap() - expected).abs() < 1.0e-4);
        assert!(pitches[1..].iter().all(Option::is_none));
    }

    #[test]
    fn window_layout_is_channel_major_and_zero_padded() {
        let source = (0..3 * MEL_BINS)
            .map(|value| value as f32)
            .collect::<Vec<_>>();
        let window = channel_major_window(&source, 3, 1, 4);
        assert_eq!(window[0], MEL_BINS as f32);
        assert_eq!(window[1], (2 * MEL_BINS) as f32);
        assert_eq!(window[2], 0.0);
        assert_eq!(window[4], (MEL_BINS + 1) as f32);
    }
}
