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
const MIN_INPUT_FRAMES: usize = 32;
const MAX_INPUT_FRAMES: usize = 1024;
const FRAME_STEP: usize = 32;
const OVERLAP_FRAMES: usize = 128;
const STRIDE_FRAMES: usize = MAX_INPUT_FRAMES - OVERLAP_FRAMES;
const PITCH_CLASSES: usize = 360;
const GRU_INPUT: usize = 384;
const GRU_HIDDEN: usize = 256;
const GRU_CHUNK_FRAMES: usize = 128;

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
    pub hz: f32,
    pub confidence: f32,
    pub voiced: bool,
}

struct GraphRun {
    runtime: Arc<GgmlRuntime>,
    context: ContextPtr,
    graph: GraphPtr,
    allocator: AllocatorPtr,
}

impl GraphRun {
    fn new(
        runtime: Arc<GgmlRuntime>,
        context_bytes: usize,
        capacity: usize,
    ) -> Result<Self, String> {
        let api = &runtime.model_api;
        let context = ggml!(
            api,
            ggml_init(GgmlInitParams {
                mem_size: context_bytes,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate RMVPE GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, capacity, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate RMVPE GGML graph".to_string());
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
            return Err("could not allocate RMVPE GGML graph on Vulkan".to_string());
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
                "RMVPE GGML graph compute failed with status {status}"
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

struct GruWeights {
    wz: TensorPtr,
    wr: TensorPtr,
    wh: TensorPtr,
    rz: TensorPtr,
    rr: TensorPtr,
    rh: TensorPtr,
    bias_z: TensorPtr,
    bias_r: TensorPtr,
    wbh: TensorPtr,
    rbh: TensorPtr,
}

struct GruGraphOutput {
    output: TensorPtr,
    hidden_final: TensorPtr,
    hidden_input: TensorPtr,
}

/// Rust-owned RMVPE execution over the upstream GGML shared-library ABI.
pub struct Rmvpe {
    backend: GgmlBackendHandle,
    weight_context: ContextPtr,
    weight_buffer: BufferPtr,
}

impl Drop for Rmvpe {
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

impl Rmvpe {
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
        };
        model.load_weights(model_path)?;
        Ok(model)
    }

    pub fn process_wav(
        &self,
        input_path: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<PitchFrame>, String> {
        let audio = read_f32_wav(input_path, SAMPLE_RATE, 1)?;
        let mel = log_mel_spectrogram(&audio)?;
        let frame_count = mel.len() / MEL_BINS;
        let window_count = if frame_count <= MAX_INPUT_FRAMES {
            1
        } else {
            (frame_count - MAX_INPUT_FRAMES).div_ceil(STRIDE_FRAMES) + 1
        };
        let mut evidence = Vec::with_capacity(frame_count);
        let mut start = 0_usize;
        for window in 0..window_count {
            let remaining = frame_count - start;
            let final_window = remaining <= MAX_INPUT_FRAMES;
            let clamped = remaining.clamp(MIN_INPUT_FRAMES, MAX_INPUT_FRAMES);
            let input_frames = clamped.div_ceil(FRAME_STEP) * FRAME_STEP;
            let mel_window = to_channel_major_window(&mel, frame_count, start, input_frames);
            let activations = self.run_window(&mel_window, input_frames)?;
            progress((window + 1) as u64, window_count as u64);
            let keep_start = if start == 0 { 0 } else { OVERLAP_FRAMES / 2 };
            let keep_end = if final_window {
                remaining
            } else {
                MAX_INPUT_FRAMES - OVERLAP_FRAMES / 2
            };
            for local_frame in keep_start..keep_end {
                let begin = local_frame * PITCH_CLASSES;
                let (hz, confidence) = local_average_hz(
                    activations
                        .get(begin..begin + PITCH_CLASSES)
                        .ok_or_else(|| "RMVPE activation timeline is truncated".to_string())?,
                )?;
                let frame = start + local_frame;
                evidence.push(PitchFrame {
                    time: frame as f64 * 0.01,
                    hz,
                    confidence,
                    voiced: confidence >= 0.03,
                });
            }
            if final_window {
                break;
            }
            start += STRIDE_FRAMES;
        }
        if evidence.len() != frame_count {
            return Err("RMVPE overlap stitching changed the evidence timeline".to_string());
        }
        Ok(evidence)
    }

    fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "RMVPE GGUF path")?;
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
            return Err("could not open RMVPE GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        if self.required_string(gguf, "general.architecture")? != "rmvpe" {
            return Err("GGUF general.architecture is not rmvpe".to_string());
        }
        for (key, expected) in [
            ("rmvpe.sample_rate", 16_000),
            ("rmvpe.n_fft", 1024),
            ("rmvpe.hop_length", 160),
            ("rmvpe.mel_bins", 128),
            ("rmvpe.pitch_classes", 360),
            ("rmvpe.gru_input_size", 384),
            ("rmvpe.gru_hidden_size", 256),
            ("rmvpe.cnn_head_out_channels", 3),
            ("rmvpe.encoder_stages", 5),
            ("rmvpe.bottleneck_stages", 4),
            ("rmvpe.bottleneck_channels", 512),
            ("rmvpe.decoder_stages", 5),
            ("rmvpe.blocks_per_stage", 4),
        ] {
            if self.required_u32(gguf, key)? != expected {
                return Err(format!("RMVPE GGUF metadata mismatch: {key}"));
            }
        }
        for key in ["rmvpe.gru_bidirectional", "rmvpe.gru_linear_before_reset"] {
            if !self.required_bool(gguf, key)? {
                return Err(format!("RMVPE GGUF metadata mismatch: {key}"));
            }
        }
        if ggml!(self.api(), gguf_get_n_tensors(gguf)) != 282 {
            return Err("RMVPE GGUF tensor count is not 282".to_string());
        }
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        let buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if buffer.is_null() {
            return Err("could not allocate RMVPE GGML weight buffer".to_string());
        }
        self.weight_buffer = buffer;
        self.upload_tensors(path, gguf)
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let api = self.api();
        let data_offset = ggml!(api, gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen RMVPE GGUF: {error}"))?;
        let mut tensor = ggml!(api, ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!("RMVPE tensor is not F32: {}", tensor_name(tensor)?));
            }
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "RMVPE tensor name contains NUL".to_string())?;
            let tensor_id = ggml!(api, gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_id < 0 {
                return Err(format!("RMVPE tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(api, ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(api, gguf_get_tensor_offset(gguf, tensor_id)))
                .ok_or_else(|| "RMVPE tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek RMVPE tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read RMVPE tensor {name}: {error}"))?;
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

    fn required_bool(&self, gguf: GgufPtr, key: &str) -> Result<bool, String> {
        Ok(ggml!(
            self.api(),
            gguf_get_val_bool(gguf, self.key_index(gguf, key)?)
        ))
    }

    fn key_index(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api(), gguf_find_key(gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("RMVPE GGUF is missing {key}"))
        } else {
            Ok(index)
        }
    }

    fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        self.maybe_weight(name)?
            .ok_or_else(|| format!("RMVPE GGUF is missing weight {name}"))
    }

    fn maybe_weight(&self, name: &str) -> Result<Option<TensorPtr>, String> {
        let encoded = CString::new(name).map_err(|_| "weight name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        Ok((!tensor.is_null()).then_some(tensor))
    }

    fn conv(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        stride: i32,
        padding: i32,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let mut output = ggml!(
            api,
            ggml_conv_2d(
                context, weight, input, stride, stride, padding, padding, 1, 1
            )
        );
        if let Some(bias) = self.maybe_weight(&format!("{prefix}.bias"))? {
            let channels = tensor_ref(bias)?.ne[0];
            let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
            output = ggml!(api, ggml_add(context, output, bias));
        }
        Ok(output)
    }

    fn batch_norm(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let scale = self.weight(&format!("{prefix}.weight"))?;
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let mean = self.weight(&format!("{prefix}.running_mean"))?;
        let variance = self.weight(&format!("{prefix}.running_var"))?;
        let channels = tensor_ref(scale)?.ne[0];
        let scale = ggml!(api, ggml_reshape_4d(context, scale, 1, 1, channels, 1));
        let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
        let mean = ggml!(api, ggml_reshape_4d(context, mean, 1, 1, channels, 1));
        let deviation = ggml!(api, ggml_scale_bias(context, variance, 1.0, 1.0e-5));
        let deviation = ggml!(api, ggml_sqrt(context, deviation));
        let deviation = ggml!(api, ggml_reshape_4d(context, deviation, 1, 1, channels, 1));
        let centered = ggml!(api, ggml_sub(context, input, mean));
        let normalized = ggml!(api, ggml_div(context, centered, deviation));
        let scaled = ggml!(api, ggml_mul(context, normalized, scale));
        Ok(ggml!(api, ggml_add(context, scaled, bias)))
    }

    fn residual_block(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let main = self.conv(context, &format!("{prefix}.conv.conv.0"), input, 1, 1)?;
        let main = ggml!(api, ggml_relu(context, main));
        let main = self.conv(context, &format!("{prefix}.conv.conv.3"), main, 1, 1)?;
        let main = ggml!(api, ggml_relu(context, main));
        let shortcut = format!("{prefix}.shortcut");
        let residual = if self.maybe_weight(&format!("{shortcut}.weight"))?.is_some() {
            self.conv(context, &shortcut, input, 1, 0)?
        } else {
            input
        };
        Ok(ggml!(api, ggml_add(context, main, residual)))
    }

    fn upsample(
        &self,
        context: ContextPtr,
        weight_name: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(weight_name)?;
        let full = ggml!(api, ggml_conv_transpose_2d_p0(context, weight, input, 2));
        let full_tensor = tensor_ref(full)?;
        let target_width = full_tensor.ne[0] - 1;
        let target_height = full_tensor.ne[1] - 1;
        let offset = full_tensor.nb[0] + full_tensor.nb[1];
        let cropped = ggml!(
            api,
            ggml_view_4d(
                context,
                full,
                target_width,
                target_height,
                full_tensor.ne[2],
                full_tensor.ne[3],
                full_tensor.nb[1],
                full_tensor.nb[2],
                full_tensor.nb[3],
                offset
            )
        );
        Ok(ggml!(api, ggml_cont(context, cropped)))
    }

    fn build_cnn_head(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        mel_input: TensorPtr,
        frame_count: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let transposed = ggml!(api, ggml_transpose(context, mel_input));
        let image = ggml!(api, ggml_cont(context, transposed));
        let mut value = self.batch_norm(context, "unet.encoder.bn", image)?;
        let mut skips = Vec::with_capacity(5);
        for stage in 0..5 {
            for block in 0..4 {
                value = self.residual_block(
                    context,
                    &format!("unet.encoder.layers.{stage}.conv.{block}"),
                    value,
                )?;
            }
            skips.push(value);
            value = ggml!(api, ggml_pool_2d(context, value, 1, 2, 2, 2, 2, 0.0, 0.0));
        }
        for stage in 0..4 {
            for block in 0..4 {
                value = self.residual_block(
                    context,
                    &format!("unet.intermediate.layers.{stage}.conv.{block}"),
                    value,
                )?;
            }
        }
        for stage in 0..5 {
            let prefix = format!("unet.decoder.layers.{stage}");
            let mut up =
                self.upsample(context, &format!("{prefix}.conv1.conv1.0.weight"), value)?;
            up = self.batch_norm(context, &format!("{prefix}.conv1.conv1.1"), up)?;
            up = ggml!(api, ggml_relu(context, up));
            value = ggml!(api, ggml_concat(context, up, skips[4 - stage], 2));
            for block in 0..4 {
                value = self.residual_block(context, &format!("{prefix}.conv2.{block}"), value)?;
            }
        }
        let head = self.conv(context, "cnn", value, 1, 1)?;
        let permuted = ggml!(api, ggml_permute(context, head, 0, 2, 1, 3));
        let permuted = ggml!(api, ggml_cont(context, permuted));
        let gru_input = ggml!(
            api,
            ggml_reshape_2d(context, permuted, GRU_INPUT as i64, frame_count as i64)
        );
        ggml!(api, ggml_set_output(gru_input));
        ggml!(api, ggml_build_forward_expand(graph, gru_input));
        Ok(gru_input)
    }

    fn gru_weights(&self, context: ContextPtr, direction: usize) -> Result<GruWeights, String> {
        let api = self.api();
        let input = self.weight("gru.weight_ih")?;
        let hidden = self.weight("gru.weight_hh")?;
        let bias = self.weight("gru.bias")?;
        let gate =
            |weight: TensorPtr, gate: usize, dimension: usize| -> Result<TensorPtr, String> {
                let descriptor = tensor_ref(weight)?;
                Ok(ggml!(
                    api,
                    ggml_view_2d(
                        context,
                        weight,
                        dimension as i64,
                        GRU_HIDDEN as i64,
                        descriptor.nb[1],
                        direction * descriptor.nb[2] + gate * GRU_HIDDEN * descriptor.nb[1]
                    )
                ))
            };
        let bias_part = |part: usize| -> Result<TensorPtr, String> {
            let descriptor = tensor_ref(bias)?;
            Ok(ggml!(
                api,
                ggml_view_1d(
                    context,
                    bias,
                    GRU_HIDDEN as i64,
                    direction * descriptor.nb[1] + part * GRU_HIDDEN * descriptor.nb[0]
                )
            ))
        };
        let wbz = bias_part(0)?;
        let wbr = bias_part(1)?;
        let wbh = bias_part(2)?;
        let rbz = bias_part(3)?;
        let rbr = bias_part(4)?;
        let rbh = bias_part(5)?;
        Ok(GruWeights {
            wz: gate(input, 0, GRU_INPUT)?,
            wr: gate(input, 1, GRU_INPUT)?,
            wh: gate(input, 2, GRU_INPUT)?,
            rz: gate(hidden, 0, GRU_HIDDEN)?,
            rr: gate(hidden, 1, GRU_HIDDEN)?,
            rh: gate(hidden, 2, GRU_HIDDEN)?,
            bias_z: ggml!(api, ggml_add(context, wbz, rbz)),
            bias_r: ggml!(api, ggml_add(context, wbr, rbr)),
            wbh,
            rbh,
        })
    }

    fn gru_cell(
        &self,
        context: ContextPtr,
        weights: &GruWeights,
        input: TensorPtr,
        previous: TensorPtr,
    ) -> TensorPtr {
        let api = self.api();
        let z_input = ggml!(api, ggml_mul_mat(context, weights.wz, input));
        let z_recurrent = ggml!(api, ggml_mul_mat(context, weights.rz, previous));
        let z = ggml!(api, ggml_add(context, z_input, z_recurrent));
        let z = ggml!(api, ggml_add(context, z, weights.bias_z));
        let z = ggml!(api, ggml_sigmoid(context, z));
        let r_input = ggml!(api, ggml_mul_mat(context, weights.wr, input));
        let r_recurrent = ggml!(api, ggml_mul_mat(context, weights.rr, previous));
        let r = ggml!(api, ggml_add(context, r_input, r_recurrent));
        let r = ggml!(api, ggml_add(context, r, weights.bias_r));
        let r = ggml!(api, ggml_sigmoid(context, r));
        let candidate_recurrent = ggml!(api, ggml_mul_mat(context, weights.rh, previous));
        let candidate_recurrent = ggml!(api, ggml_add(context, candidate_recurrent, weights.rbh));
        let candidate_recurrent = ggml!(api, ggml_mul(context, r, candidate_recurrent));
        let candidate = ggml!(api, ggml_mul_mat(context, weights.wh, input));
        let candidate = ggml!(api, ggml_add(context, candidate, candidate_recurrent));
        let candidate = ggml!(api, ggml_add(context, candidate, weights.wbh));
        let candidate = ggml!(api, ggml_tanh(context, candidate));
        let difference = ggml!(api, ggml_sub(context, previous, candidate));
        let retained = ggml!(api, ggml_mul(context, z, difference));
        ggml!(api, ggml_add(context, candidate, retained))
    }

    fn build_gru_chunk(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        input: TensorPtr,
        frame_count: usize,
        direction: usize,
    ) -> Result<GruGraphOutput, String> {
        let api = self.api();
        let hidden_input = ggml!(
            api,
            ggml_new_tensor_1d(context, GGML_TYPE_F32, GRU_HIDDEN as i64)
        );
        ggml!(api, ggml_set_input(hidden_input));
        let weights = self.gru_weights(context, direction)?;
        let element_size = ggml!(api, ggml_element_size(input));
        let mut sequence = vec![std::ptr::null_mut(); frame_count];
        let mut previous = hidden_input;
        if direction == 0 {
            for frame in 0..frame_count {
                let frame_input = ggml!(
                    api,
                    ggml_view_1d(
                        context,
                        input,
                        GRU_INPUT as i64,
                        frame * GRU_INPUT * element_size
                    )
                );
                previous = self.gru_cell(context, &weights, frame_input, previous);
                sequence[frame] = ggml!(
                    api,
                    ggml_reshape_2d(context, previous, GRU_HIDDEN as i64, 1)
                );
            }
        } else {
            for frame in (0..frame_count).rev() {
                let frame_input = ggml!(
                    api,
                    ggml_view_1d(
                        context,
                        input,
                        GRU_INPUT as i64,
                        frame * GRU_INPUT * element_size
                    )
                );
                previous = self.gru_cell(context, &weights, frame_input, previous);
                sequence[frame] = ggml!(
                    api,
                    ggml_reshape_2d(context, previous, GRU_HIDDEN as i64, 1)
                );
            }
        }
        let output = concat_balanced(api, context, &sequence, 1)?;
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_set_output(previous));
        ggml!(api, ggml_build_forward_expand(graph, output));
        ggml!(api, ggml_build_forward_expand(graph, previous));
        Ok(GruGraphOutput {
            output,
            hidden_final: previous,
            hidden_input,
        })
    }

    fn build_output_head(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight("fc.1.weight")?;
        let bias = self.weight("fc.1.bias")?;
        let output = ggml!(api, ggml_mul_mat(context, weight, input));
        let output = ggml!(api, ggml_add(context, output, bias));
        let output = ggml!(api, ggml_sigmoid(context, output));
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(graph, output));
        Ok(output)
    }

    fn run_cnn_head(&self, mel: &[f32], frames: usize) -> Result<Vec<f32>, String> {
        let capacity = frames * 30 + 20_000;
        let mut run = GraphRun::new(
            Arc::clone(&self.backend.runtime),
            64 * 1024 * 1024,
            capacity,
        )?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, frames as i64, MEL_BINS as i64)
        );
        ggml!(api, ggml_set_input(input));
        let output = self.build_cnn_head(run.context, run.graph, input, frames)?;
        run.allocate(&self.backend)?;
        set_f32(api, input, mel)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    fn run_gru_chunk(
        &self,
        direction: usize,
        start: usize,
        length: usize,
        input: &[f32],
        previous: &[f32],
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let capacity = length * 40 + 2000;
        let mut run = GraphRun::new(
            Arc::clone(&self.backend.runtime),
            32 * 1024 * 1024,
            capacity,
        )?;
        let api = self.api();
        let chunk = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, GRU_INPUT as i64, length as i64)
        );
        ggml!(api, ggml_set_input(chunk));
        let output = self.build_gru_chunk(run.context, run.graph, chunk, length, direction)?;
        run.allocate(&self.backend)?;
        let begin = start * GRU_INPUT;
        set_f32(api, chunk, &input[begin..begin + length * GRU_INPUT])?;
        set_f32(api, output.hidden_input, previous)?;
        run.compute(&self.backend)?;
        Ok((
            get_f32(api, output.output)?,
            get_f32(api, output.hidden_final)?,
        ))
    }

    fn run_output_head(&self, input: &[f32], frames: usize) -> Result<Vec<f32>, String> {
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime), 8 * 1024 * 1024, 200)?;
        let api = self.api();
        let gru = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, 512, frames as i64)
        );
        ggml!(api, ggml_set_input(gru));
        let output = self.build_output_head(run.context, run.graph, gru)?;
        run.allocate(&self.backend)?;
        set_f32(api, gru, input)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    fn run_window(&self, mel: &[f32], frames: usize) -> Result<Vec<f32>, String> {
        let gru_input = self.run_cnn_head(mel, frames)?;
        if gru_input.len() != GRU_INPUT * frames {
            return Err("RMVPE CNN output shape is invalid".to_string());
        }
        let chunks = (0..frames)
            .step_by(GRU_CHUNK_FRAMES)
            .map(|start| (start, GRU_CHUNK_FRAMES.min(frames - start)))
            .collect::<Vec<_>>();
        let mut forward = vec![0.0_f32; GRU_HIDDEN * frames];
        let mut backward = vec![0.0_f32; GRU_HIDDEN * frames];
        let mut hidden = vec![0.0_f32; GRU_HIDDEN];
        for &(start, length) in &chunks {
            let (output, final_hidden) =
                self.run_gru_chunk(0, start, length, &gru_input, &hidden)?;
            forward[start * GRU_HIDDEN..(start + length) * GRU_HIDDEN].copy_from_slice(&output);
            hidden = final_hidden;
        }
        hidden.fill(0.0);
        for &(start, length) in chunks.iter().rev() {
            let (output, final_hidden) =
                self.run_gru_chunk(1, start, length, &gru_input, &hidden)?;
            backward[start * GRU_HIDDEN..(start + length) * GRU_HIDDEN].copy_from_slice(&output);
            hidden = final_hidden;
        }
        let mut combined = vec![0.0_f32; GRU_HIDDEN * 2 * frames];
        for frame in 0..frames {
            let source = frame * GRU_HIDDEN;
            let destination = frame * GRU_HIDDEN * 2;
            combined[destination..destination + GRU_HIDDEN]
                .copy_from_slice(&forward[source..source + GRU_HIDDEN]);
            combined[destination + GRU_HIDDEN..destination + GRU_HIDDEN * 2]
                .copy_from_slice(&backward[source..source + GRU_HIDDEN]);
        }
        self.run_output_head(&combined, frames)
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

fn concat_balanced(
    api: &ModelApi,
    context: ContextPtr,
    tensors: &[TensorPtr],
    dimension: i32,
) -> Result<TensorPtr, String> {
    match tensors {
        [] => Err("cannot concatenate an empty RMVPE tensor list".to_string()),
        [only] => Ok(*only),
        _ => {
            let middle = tensors.len() / 2;
            let left = concat_balanced(api, context, &tensors[..middle], dimension)?;
            let right = concat_balanced(api, context, &tensors[middle..], dimension)?;
            Ok(ggml!(api, ggml_concat(context, left, right, dimension)))
        }
    }
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("RMVPE GGML input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "RMVPE tensor element count is invalid".to_string())?;
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
        let minimum = hz_to_mel(30.0);
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
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

fn reflected_sample(audio: &[f32], padded_index: usize) -> f32 {
    let pad = FFT_SIZE / 2;
    if padded_index < pad {
        return audio[pad - padded_index];
    }
    let audio_index = padded_index - pad;
    if audio_index < audio.len() {
        audio[audio_index]
    } else {
        audio[audio.len() - 2 - (audio_index - audio.len())]
    }
}

fn log_mel_spectrogram(audio: &[f32]) -> Result<Vec<f32>, String> {
    if audio.len() <= FFT_SIZE {
        return Err("RMVPE requires more than 64 ms of decoded audio".to_string());
    }
    let frame_count = audio.len() / HOP_SIZE + 1;
    if (frame_count - 1) * HOP_SIZE + FFT_SIZE > audio.len() + FFT_SIZE {
        return Err("RMVPE STFT frame calculation exceeded reflected padding".to_string());
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
    let mut output = vec![0.0_f32; frame_count * MEL_BINS];
    for frame in 0..frame_count {
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
                .map(|(bin, weight)| spectrum[*bin].norm() * weight)
                .sum::<f32>();
            output[frame * MEL_BINS + band] = energy.max(1.0e-5).ln();
        }
    }
    Ok(output)
}

fn to_channel_major_window(
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

fn local_average_hz(activation: &[f32]) -> Result<(f32, f32), String> {
    if activation.len() != PITCH_CLASSES
        || activation
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err("RMVPE activation frame is invalid".to_string());
    }
    let mut center = 0;
    let mut confidence = activation[0];
    for (index, value) in activation.iter().copied().enumerate().skip(1) {
        if value > confidence {
            center = index;
            confidence = value;
        }
    }
    let start = center.saturating_sub(4);
    let end = (center + 4).min(PITCH_CLASSES - 1);
    let mut weighted_cents = 0.0_f64;
    let mut weight = 0.0_f64;
    for (class, salience) in activation
        .iter()
        .copied()
        .enumerate()
        .take(end + 1)
        .skip(start)
    {
        weighted_cents += salience as f64 * (20.0 * class as f64 + 1997.3794);
        weight += salience as f64;
    }
    let cents = if weight > f32::EPSILON as f64 {
        weighted_cents / weight
    } else {
        20.0 * center as f64 + 1997.3794
    };
    Ok((
        (10.0 * 2.0_f64.powf(cents / 1200.0)) as f32,
        confidence.clamp(0.0, 1.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_average_produces_bounded_pitch_evidence() {
        let mut activation = vec![0.0; PITCH_CLASSES];
        activation[100] = 0.8;
        activation[101] = 0.4;
        let (hz, confidence) = local_average_hz(&activation).unwrap();
        assert!(hz.is_finite() && hz > 0.0);
        assert_eq!(confidence, 0.8);
    }

    #[test]
    fn mel_frontend_has_exact_ten_millisecond_timeline() {
        let audio = vec![0.0; SAMPLE_RATE as usize];
        let mel = log_mel_spectrogram(&audio).unwrap();
        assert_eq!(mel.len() / MEL_BINS, 101);
    }
}
