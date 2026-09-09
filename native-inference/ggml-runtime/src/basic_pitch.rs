use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{
    AllocatorPtr, BufferPtr, ContextPtr, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GgmlInitParams,
    GgmlTensor, GgufInitParams, GgufPtr, GraphPtr, ModelApi, TensorPtr,
};
use crate::wav::read_f32_wav;
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

const MODEL_SIZE_BYTES: u64 = 144_512;
const SAMPLE_RATE: u32 = 22_050;
const FFT_HOP_SAMPLES: usize = 256;
const OVERLAP_FRAMES: usize = 30;
const HALF_OVERLAP_FRAMES: usize = OVERLAP_FRAMES / 2;
const OVERLAP_SAMPLES: usize = OVERLAP_FRAMES * FFT_HOP_SAMPLES;
const PADDING_SAMPLES: usize = OVERLAP_SAMPLES / 2;
const INPUT_SAMPLES: usize = 43_844;
const WINDOW_HOP_SAMPLES: usize = INPUT_SAMPLES - OVERLAP_SAMPLES;
const FRAMES_PER_WINDOW: usize = 172;
const OWNED_FRAMES_PER_WINDOW: usize = FRAMES_PER_WINDOW - OVERLAP_FRAMES;

const CQT_BINS_PER_OCTAVE: usize = 36;
const CQT_N_OCTAVES: usize = 9;
const CQT_N_BINS: usize = 309;
const CQT_KERNEL_LEN: usize = 256;

const N_HARMONIC_CHANNELS: usize = 8;
const N_FREQ_BINS_CONTOURS: usize = 264;
const N_NOTES: usize = 88;
const TENSOR_COUNT: i64 = 18;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: raw handles remain owned by the surrounding model or graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivationFrame {
    pub time: f64,
    pub note_max: f32,
    pub onset_max: f32,
    pub contour_class: usize,
    pub contour_score: f32,
}

struct CqtWeights {
    real: Vec<f32>,
    imaginary: Vec<f32>,
    lowpass: Vec<f32>,
    sqrt_lengths: Vec<f32>,
    batch_norm_scale: f32,
    batch_norm_shift: f32,
}

struct WindowActivations {
    notes: Vec<f32>,
    onsets: Vec<f32>,
    contours: Vec<f32>,
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
                mem_size: 16 * 1024 * 1024,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate Basic Pitch GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, 2048, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate Basic Pitch GGML graph".to_string());
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
            return Err("could not allocate Basic Pitch GGML graph".to_string());
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
                "Basic Pitch GGML graph compute failed with status {status}"
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

/// Rust-owned Basic Pitch frontend and graph over the pinned upstream GGML ABI.
pub struct BasicPitch {
    backend: GgmlBackendHandle,
    weight_context: ContextPtr,
    weight_buffer: BufferPtr,
    cqt: Option<CqtWeights>,
}

impl Drop for BasicPitch {
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

impl BasicPitch {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        let metadata = std::fs::metadata(model_path)
            .map_err(|error| format!("Basic Pitch GGUF is unavailable: {error}"))?;
        if !metadata.is_file() || metadata.len() != MODEL_SIZE_BYTES {
            return Err("Basic Pitch GGUF size is invalid".to_string());
        }
        let backend = runtime.create_backend(device)?;
        let mut model = Self {
            backend,
            weight_context: std::ptr::null_mut(),
            weight_buffer: std::ptr::null_mut(),
            cqt: None,
        };
        model.load_weights(model_path)?;
        Ok(model)
    }

    pub fn process_wav(
        &self,
        input_path: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Vec<ActivationFrame>, String> {
        let audio = read_f32_wav(input_path, SAMPLE_RATE, 1)?;
        if audio.len() < FFT_HOP_SAMPLES {
            return Err("Basic Pitch requires at least one 256-sample frame".to_string());
        }
        let count = window_count(audio.len());
        let mut frames = Vec::with_capacity(audio.len() / FFT_HOP_SAMPLES);
        let mut input = vec![0.0_f32; INPUT_SAMPLES];
        for window_index in 0..count {
            fill_padded_window(&mut input, &audio, window_index);
            let activations = self.run_window(&input)?;
            append_window_frames(&mut frames, &activations, window_index, audio.len());
            progress((window_index + 1) as u64, count as u64);
        }
        if frames.len() != audio.len() / FFT_HOP_SAMPLES {
            return Err("Basic Pitch window stitching changed the evidence timeline".to_string());
        }
        Ok(frames)
    }

    fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "Basic Pitch GGUF path")?;
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
            return Err("could not open Basic Pitch GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        if self.required_string(gguf, "general.architecture")? != "basic_pitch" {
            return Err("GGUF general.architecture is not basic_pitch".to_string());
        }
        for (key, expected) in [
            ("sample_rate", 22_050),
            ("window_samples", 43_844),
            ("fft_hop_samples", 256),
            ("n_output_frames", 172),
            ("n_notes", 88),
            ("n_contours", 264),
        ] {
            if self.required_u32(gguf, key)? != expected {
                return Err(format!("Basic Pitch GGUF metadata mismatch: {key}"));
            }
        }
        if ggml!(self.api(), gguf_get_n_tensors(gguf)) != TENSOR_COUNT {
            return Err("Basic Pitch GGUF tensor count is not 18".to_string());
        }
        self.validate_weight_shapes()?;
        self.cqt = Some(CqtWeights {
            real: self.read_tensor_f32(path, gguf, "cqt.conv_real.weight")?,
            imaginary: self.read_tensor_f32(path, gguf, "cqt.conv_imag.weight")?,
            lowpass: self.read_tensor_f32(path, gguf, "cqt.lowpass.weight")?,
            sqrt_lengths: self.read_tensor_f32(path, gguf, "cqt.sqrt_lengths")?,
            batch_norm_scale: self.read_tensor_f32(path, gguf, "cqt_bn.scale")?[0],
            batch_norm_shift: self.read_tensor_f32(path, gguf, "cqt_bn.shift")?[0],
        });
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        self.weight_buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if self.weight_buffer.is_null() {
            return Err("could not allocate Basic Pitch GGML weight buffer".to_string());
        }
        self.upload_tensors(path, gguf)
    }

    fn validate_weight_shapes(&self) -> Result<(), String> {
        for (name, dimensions) in [
            ("contour_conv1.bias", &[8][..]),
            ("contour_conv1.weight", &[8, 8, 3, 39][..]),
            ("contour_final.bias", &[1][..]),
            ("contour_final.weight", &[1, 8, 5, 5][..]),
            ("cqt.conv_imag.weight", &[36, 1, 1, 256][..]),
            ("cqt.conv_real.weight", &[36, 1, 1, 256][..]),
            ("cqt.lowpass.weight", &[1, 1, 1, 256][..]),
            ("cqt.sqrt_lengths", &[309][..]),
            ("cqt_bn.scale", &[1][..]),
            ("cqt_bn.shift", &[1][..]),
            ("note_conv1.bias", &[32][..]),
            ("note_conv1.weight", &[32, 1, 7, 7][..]),
            ("note_final.bias", &[1][..]),
            ("note_final.weight", &[1, 32, 7, 3][..]),
            ("onset_conv1.bias", &[32][..]),
            ("onset_conv1.weight", &[32, 8, 5, 5][..]),
            ("onset_final.bias", &[1][..]),
            ("onset_final.weight", &[1, 33, 3, 3][..]),
        ] {
            self.require_shape(name, dimensions)?;
        }
        Ok(())
    }

    fn require_shape(&self, name: &str, expected: &[i64]) -> Result<(), String> {
        let tensor = tensor_ref(self.weight(name)?)?;
        let dimensions = tensor
            .ne
            .iter()
            .rposition(|dimension| *dimension != 1)
            .map_or(1, |last| last + 1);
        if tensor.type_ != GGML_TYPE_F32 || &tensor.ne[..dimensions] != expected {
            return Err(format!("Basic Pitch GGUF tensor shape mismatch: {name}"));
        }
        Ok(())
    }

    fn read_tensor_f32(&self, path: &Path, gguf: GgufPtr, name: &str) -> Result<Vec<f32>, String> {
        let tensor = self.weight(name)?;
        let encoded = CString::new(name).map_err(|_| "tensor name contains NUL".to_string())?;
        let tensor_id = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
        if tensor_id < 0 {
            return Err(format!(
                "Basic Pitch tensor is absent from GGUF data: {name}"
            ));
        }
        let byte_count = ggml!(self.api(), ggml_nbytes(tensor));
        let mut bytes = vec![0_u8; byte_count];
        let offset = ggml!(self.api(), gguf_get_data_offset(gguf))
            .checked_add(ggml!(self.api(), gguf_get_tensor_offset(gguf, tensor_id)))
            .ok_or_else(|| "Basic Pitch tensor offset overflow".to_string())?;
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen Basic Pitch GGUF: {error}"))?;
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|error| format!("could not seek Basic Pitch tensor {name}: {error}"))?;
        file.read_exact(&mut bytes)
            .map_err(|error| format!("could not read Basic Pitch tensor {name}: {error}"))?;
        if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
            return Err(format!("Basic Pitch tensor byte count is invalid: {name}"));
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect())
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let api = self.api();
        let data_offset = ggml!(api, gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen Basic Pitch GGUF: {error}"))?;
        let mut tensor = ggml!(api, ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!(
                    "Basic Pitch tensor is not F32: {}",
                    tensor_name(tensor)?
                ));
            }
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "Basic Pitch tensor name contains NUL".to_string())?;
            let tensor_id = ggml!(api, gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_id < 0 {
                return Err(format!(
                    "Basic Pitch tensor is absent from GGUF data: {name}"
                ));
            }
            let size = ggml!(api, ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(api, gguf_get_tensor_offset(gguf, tensor_id)))
                .ok_or_else(|| "Basic Pitch tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek Basic Pitch tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read Basic Pitch tensor {name}: {error}"))?;
            ggml!(
                api,
                ggml_backend_tensor_set(tensor, bytes.as_ptr().cast::<c_void>(), 0, size)
            );
            tensor = ggml!(api, ggml_get_next_tensor(self.weight_context, tensor));
        }
        Ok(())
    }

    fn required_string(&self, gguf: GgufPtr, key: &str) -> Result<String, String> {
        let raw = ggml!(
            self.api(),
            gguf_get_val_str(gguf, self.key_index(gguf, key)?)
        );
        if raw.is_null() {
            return Err(format!("GGUF string {key} is null"));
        }
        // SAFETY: the GGUF context owns this NUL-terminated string.
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
            Err(format!("Basic Pitch GGUF is missing {key}"))
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
            Err(format!("Basic Pitch GGUF is missing weight {name}"))
        } else {
            Ok(tensor)
        }
    }

    fn conv2d(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        stride_width: i32,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let raw_weight = self.weight(&format!("{prefix}.weight"))?;
        let descriptor = tensor_ref(raw_weight)?;
        let input_descriptor = tensor_ref(input)?;
        let kernel_height = descriptor.ne[2] as i32;
        let kernel_width = descriptor.ne[3] as i32;
        let padding_width =
            same_padding_before(input_descriptor.ne[0] as i32, kernel_width, stride_width);
        let padding_height = same_padding_before(input_descriptor.ne[1] as i32, kernel_height, 1);
        // The converter preserves OpenVINO's contiguous NCHW bytes while the
        // GGUF descriptor records [O, I, H, W]. GGML Conv2D needs those same
        // bytes viewed as [W, H, I, O], not permuted through the descriptor's
        // misleading strides.
        let weight = ggml!(
            api,
            ggml_reshape_4d(
                context,
                raw_weight,
                descriptor.ne[3],
                descriptor.ne[2],
                descriptor.ne[1],
                descriptor.ne[0]
            )
        );
        let output = ggml!(
            api,
            ggml_conv_2d(
                context,
                weight,
                input,
                stride_width,
                1,
                padding_width,
                padding_height,
                1,
                1
            )
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let channels = tensor_ref(bias)?.ne[0];
        let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
        Ok(ggml!(api, ggml_add(context, output, bias)))
    }

    fn build_graph(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        input: TensorPtr,
    ) -> Result<(TensorPtr, TensorPtr, TensorPtr), String> {
        let api = self.api();
        let onset_first = self.conv2d(context, "onset_conv1", input, 3)?;
        let onset_first = ggml!(api, ggml_relu(context, onset_first));

        let contour_first = self.conv2d(context, "contour_conv1", input, 1)?;
        let contour_first = ggml!(api, ggml_relu(context, contour_first));
        let contours = self.conv2d(context, "contour_final", contour_first, 1)?;
        let contours = ggml!(api, ggml_sigmoid(context, contours));

        let note_first = self.conv2d(context, "note_conv1", contours, 3)?;
        let note_first = ggml!(api, ggml_relu(context, note_first));
        let note_head = self.conv2d(context, "note_final", note_first, 1)?;
        let note_head = ggml!(api, ggml_sigmoid(context, note_head));

        let onset_input = ggml!(api, ggml_concat(context, note_head, onset_first, 2));
        let onset_head = self.conv2d(context, "onset_final", onset_input, 1)?;
        let onset_head = ggml!(api, ggml_sigmoid(context, onset_head));

        // Preserve the established Studio evidence contract, whose note/onset
        // labels are opposite the source graph's raw declared output order.
        let notes = onset_head;
        let onsets = note_head;
        for output in [notes, onsets, contours] {
            ggml!(api, ggml_set_output(output));
            ggml!(api, ggml_build_forward_expand(graph, output));
        }
        Ok((notes, onsets, contours))
    }

    fn run_window(&self, window: &[f32]) -> Result<WindowActivations, String> {
        let cqt = self
            .cqt
            .as_ref()
            .ok_or_else(|| "Basic Pitch CQT weights are unavailable".to_string())?;
        let (mut magnitude, frame_count) = cqt_magnitude(window, cqt)?;
        if frame_count != FRAMES_PER_WINDOW {
            return Err(format!(
                "Basic Pitch CQT produced {frame_count} frames, expected {FRAMES_PER_WINDOW}"
            ));
        }
        normalized_log(&mut magnitude);
        for value in &mut magnitude {
            *value = *value * cqt.batch_norm_scale + cqt.batch_norm_shift;
        }
        let harmonics = harmonic_stack(&magnitude, frame_count);

        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_4d(
                run.context,
                GGML_TYPE_F32,
                N_FREQ_BINS_CONTOURS as i64,
                FRAMES_PER_WINDOW as i64,
                N_HARMONIC_CHANNELS as i64,
                1
            )
        );
        ggml!(api, ggml_set_input(input));
        let (notes, onsets, contours) = self.build_graph(run.context, run.graph, input)?;
        run.allocate(&self.backend)?;
        set_f32(api, input, &harmonics)?;
        run.compute(&self.backend)?;
        let result = WindowActivations {
            notes: get_f32(api, notes)?,
            onsets: get_f32(api, onsets)?,
            contours: get_f32(api, contours)?,
        };
        if result.notes.iter().any(|value| !value.is_finite())
            || result.onsets.iter().any(|value| !value.is_finite())
            || result.contours.iter().any(|value| !value.is_finite())
        {
            return Err("Basic Pitch produced non-finite activation evidence".to_string());
        }
        Ok(result)
    }
}

fn same_padding_before(input_size: i32, kernel_size: i32, stride: i32) -> i32 {
    let output_size = (input_size + stride - 1) / stride;
    ((output_size - 1) * stride + kernel_size - input_size).max(0) / 2
}

fn reflect_sample(data: &[f32], index: isize) -> f32 {
    if data.len() <= 1 {
        return data.first().copied().unwrap_or(0.0);
    }
    let period = 2 * (data.len() - 1) as isize;
    let folded = index.rem_euclid(period);
    data[if folded < data.len() as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }]
}

fn cqt_octave_conv1d_with_hop(
    signal: &[f32],
    real_kernel: &[f32],
    imaginary_kernel: &[f32],
    hop: usize,
) -> Result<(Vec<f32>, Vec<f32>, usize), String> {
    if real_kernel.len() != CQT_BINS_PER_OCTAVE * CQT_KERNEL_LEN
        || imaginary_kernel.len() != real_kernel.len()
    {
        return Err("Basic Pitch CQT kernel shape is invalid".to_string());
    }
    let padding = CQT_KERNEL_LEN / 2;
    let padded_len = signal.len() + padding * 2;
    let frame_count = (padded_len - CQT_KERNEL_LEN) / hop + 1;
    let mut real = vec![0.0_f32; frame_count * CQT_BINS_PER_OCTAVE];
    let mut imaginary = vec![0.0_f32; real.len()];
    for frame in 0..frame_count {
        let start = frame * hop;
        for frequency in 0..CQT_BINS_PER_OCTAVE {
            let mut real_sum = 0.0_f32;
            let mut imaginary_sum = 0.0_f32;
            let kernel_start = frequency * CQT_KERNEL_LEN;
            for kernel in 0..CQT_KERNEL_LEN {
                let sample = reflect_sample(signal, (start + kernel) as isize - padding as isize);
                real_sum += sample * real_kernel[kernel_start + kernel];
                imaginary_sum += sample * imaginary_kernel[kernel_start + kernel];
            }
            let index = frame * CQT_BINS_PER_OCTAVE + frequency;
            real[index] = real_sum;
            imaginary[index] = imaginary_sum;
        }
    }
    Ok((real, imaginary, frame_count))
}

fn downsample_by_2(signal: &[f32], lowpass: &[f32]) -> Result<Vec<f32>, String> {
    if lowpass.len() != CQT_KERNEL_LEN {
        return Err("Basic Pitch CQT lowpass shape is invalid".to_string());
    }
    let padding = (CQT_KERNEL_LEN - 1) / 2;
    let padded_len = signal.len() + 2 * padding;
    let output_len = (padded_len - CQT_KERNEL_LEN) / 2 + 1;
    let mut output = vec![0.0_f32; output_len];
    for (frame, value) in output.iter_mut().enumerate() {
        let start = frame * 2;
        let mut sum = 0.0_f32;
        for kernel in 0..CQT_KERNEL_LEN {
            let padded = start + kernel;
            if let Some(source) = padded
                .checked_sub(padding)
                .filter(|source| *source < signal.len())
            {
                sum += signal[source] * lowpass[kernel];
            }
        }
        *value = sum;
    }
    Ok(output)
}

fn cqt_magnitude(window: &[f32], weights: &CqtWeights) -> Result<(Vec<f32>, usize), String> {
    if window.len() != INPUT_SAMPLES || weights.sqrt_lengths.len() != CQT_N_BINS {
        return Err("Basic Pitch CQT input or normalization shape is invalid".to_string());
    }
    let mut octave_real = Vec::with_capacity(CQT_N_OCTAVES);
    let mut octave_imaginary = Vec::with_capacity(CQT_N_OCTAVES);
    let mut signal = window.to_vec();
    let mut hop = FFT_HOP_SAMPLES;
    let mut frame_count = 0;
    for octave in 0..CQT_N_OCTAVES {
        if octave > 0 {
            signal = downsample_by_2(&signal, &weights.lowpass)?;
            hop /= 2;
        }
        let (real, imaginary, frames) =
            cqt_octave_conv1d_with_hop(&signal, &weights.real, &weights.imaginary, hop)?;
        if octave == 0 {
            frame_count = frames;
        } else if frames != frame_count {
            return Err("Basic Pitch CQT octave timelines disagree".to_string());
        }
        octave_real.push(real);
        octave_imaginary.push(imaginary);
    }
    let total_bins = CQT_N_OCTAVES * CQT_BINS_PER_OCTAVE;
    let skip = total_bins - CQT_N_BINS;
    let mut magnitude = vec![0.0_f32; frame_count * CQT_N_BINS];
    for frame in 0..frame_count {
        for (output_bin, full_bin) in (skip..total_bins).enumerate() {
            let octave = CQT_N_OCTAVES - 1 - full_bin / CQT_BINS_PER_OCTAVE;
            let bin = full_bin % CQT_BINS_PER_OCTAVE;
            let index = frame * CQT_BINS_PER_OCTAVE + bin;
            let scale = weights.sqrt_lengths[output_bin];
            let real = octave_real[octave][index] * scale;
            let imaginary = octave_imaginary[octave][index] * scale;
            magnitude[frame * CQT_N_BINS + output_bin] =
                (real * real + imaginary * imaginary).sqrt();
        }
    }
    Ok((magnitude, frame_count))
}

fn normalized_log(magnitude: &mut [f32]) {
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for value in magnitude.iter_mut() {
        *value = 10.0 * (*value * *value + 1.0e-10).log10();
        minimum = minimum.min(*value);
        maximum = maximum.max(*value);
    }
    let range = maximum - minimum;
    if range == 0.0 {
        magnitude.fill(0.0);
    } else {
        for value in magnitude {
            *value = (*value - minimum) / range;
        }
    }
}

fn harmonic_stack(log_cqt: &[f32], frame_count: usize) -> Vec<f32> {
    const SHIFTS: [isize; N_HARMONIC_CHANNELS] = [-36, 0, 36, 57, 72, 84, 93, 101];
    let mut output = vec![0.0_f32; N_HARMONIC_CHANNELS * frame_count * N_FREQ_BINS_CONTOURS];
    for (channel, shift) in SHIFTS.into_iter().enumerate() {
        for frame in 0..frame_count {
            for bin in 0..N_FREQ_BINS_CONTOURS {
                let source = bin as isize + shift;
                if (0..CQT_N_BINS as isize).contains(&source) {
                    output[(channel * frame_count + frame) * N_FREQ_BINS_CONTOURS + bin] =
                        log_cqt[frame * CQT_N_BINS + source as usize];
                }
            }
        }
    }
    output
}

fn window_count(source_samples: usize) -> usize {
    let padded_samples = source_samples + 2 * PADDING_SAMPLES;
    if padded_samples <= INPUT_SAMPLES {
        1
    } else {
        (padded_samples - INPUT_SAMPLES).div_ceil(WINDOW_HOP_SAMPLES) + 1
    }
}

fn fill_padded_window(input: &mut [f32], audio: &[f32], window_index: usize) {
    input.fill(0.0);
    let padded_start = window_index * WINDOW_HOP_SAMPLES;
    for (local, value) in input.iter_mut().enumerate() {
        let padded_sample = padded_start + local;
        if let Some(source_sample) = padded_sample.checked_sub(PADDING_SAMPLES)
            && let Some(source) = audio.get(source_sample)
        {
            *value = *source;
        }
    }
}

fn maximum(values: &[f32]) -> f32 {
    values.iter().copied().fold(0.0, f32::max)
}

fn append_window_frames(
    frames: &mut Vec<ActivationFrame>,
    activations: &WindowActivations,
    window_index: usize,
    source_samples: usize,
) {
    let target_frames = source_samples / FFT_HOP_SAMPLES;
    for frame in HALF_OVERLAP_FRAMES..FRAMES_PER_WINDOW - HALF_OVERLAP_FRAMES {
        let source_frame = window_index * OWNED_FRAMES_PER_WINDOW + frame - HALF_OVERLAP_FRAMES;
        if source_frame >= target_frames {
            break;
        }
        let contour =
            &activations.contours[frame * N_FREQ_BINS_CONTOURS..(frame + 1) * N_FREQ_BINS_CONTOURS];
        let (contour_class, contour_score) = contour
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap_or((0, 0.0));
        frames.push(ActivationFrame {
            time: source_frame as f64 * FFT_HOP_SAMPLES as f64 / SAMPLE_RATE as f64,
            note_max: maximum(&activations.notes[frame * N_NOTES..(frame + 1) * N_NOTES]),
            onset_max: maximum(&activations.onsets[frame * N_NOTES..(frame + 1) * N_NOTES]),
            contour_class,
            contour_score,
        });
    }
}

fn tensor_ref(raw: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers retain the context that owns the tensor.
    unsafe { raw.as_ref() }.ok_or_else(|| "GGML returned a null tensor".to_string())
}

fn tensor_name(raw: TensorPtr) -> Result<String, String> {
    let tensor = tensor_ref(raw)?;
    // SAFETY: GGML tensor names are fixed NUL-terminated arrays.
    Ok(unsafe { CStr::from_ptr(tensor.name.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("Basic Pitch GGML input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "Basic Pitch tensor element count is invalid".to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_padding_matches_reference_stride_three_convolutions() {
        assert_eq!(same_padding_before(264, 5, 3), 1);
        assert_eq!(same_padding_before(264, 7, 3), 2);
        assert_eq!(same_padding_before(172, 39, 1), 19);
        assert_eq!(same_padding_before(88, 3, 1), 1);
    }

    #[test]
    fn window_stitching_matches_the_reference_grid() {
        assert_eq!(window_count(1), 1);
        assert_eq!(window_count(INPUT_SAMPLES + WINDOW_HOP_SAMPLES / 2), 2);
    }

    #[test]
    fn normalized_log_maps_extremes_to_zero_and_one() {
        let mut values = vec![0.001, 1.0, 0.5, 0.001];
        normalized_log(&mut values);
        assert!((values.iter().copied().fold(f32::INFINITY, f32::min) - 0.0).abs() < 1.0e-6);
        assert!((values.iter().copied().fold(f32::NEG_INFINITY, f32::max) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn harmonic_stack_uses_the_reference_shifts() {
        let mut input = vec![0.0; CQT_N_BINS];
        input[100] = 1.0;
        let stacked = harmonic_stack(&input, 1);
        assert_eq!(stacked[136], 1.0);
        assert_eq!(stacked[N_FREQ_BINS_CONTOURS + 100], 1.0);
        assert_eq!(stacked[2 * N_FREQ_BINS_CONTOURS + 64], 1.0);
    }
}
