use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{
    AllocatorPtr, BufferPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F16,
    GGML_TYPE_F32, GGML_TYPE_I32, GGUF_TYPE_ARRAY, GGUF_TYPE_INT32, GgmlInitParams, GgmlTensor,
    GgufInitParams, GgufPtr, GraphPtr, ModelApi, TensorPtr,
};
use crate::stage_profile::StageProfile;
use crate::stft::compute_stft;
use crate::wav::{read_f32_wav, write_f32_wav};
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

mod frames;

use frames::{prepare_model_input, process_overlap_add, reconstruct_stems};

const GRAPH_CONTEXT_BYTES: usize = 1024 * 1024 * 1024;
const GRAPH_CAPACITY: usize = 65_536;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: model construction retains every GGML owner for the full
        // lifetime of the raw handles passed to the pinned C ABI.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Clone)]
struct Config {
    architecture: String,
    public_schema: bool,
    use_pope: bool,
    has_final_norm: bool,
    transformer_norm_output: bool,
    skip_connection: bool,
    fft_size: usize,
    hop_length: usize,
    window_length: usize,
    dimension: i64,
    band_count: i64,
    depth: usize,
    head_count: i64,
    head_dimension: i64,
    stem_count: usize,
    zero_dc: bool,
    mask_depth: usize,
    mask_mlp_layers: usize,
    sample_rate: u32,
    chunk_size: usize,
    overlap: usize,
    frequency_indices: Vec<usize>,
    bands_per_frequency: Vec<usize>,
    frequencies_per_band: Vec<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            architecture: String::new(),
            public_schema: false,
            use_pope: false,
            has_final_norm: true,
            transformer_norm_output: false,
            skip_connection: false,
            fft_size: 2048,
            hop_length: 441,
            window_length: 2048,
            dimension: 384,
            band_count: 60,
            depth: 6,
            head_count: 8,
            head_dimension: 64,
            stem_count: 1,
            zero_dc: false,
            mask_depth: 1,
            mask_mlp_layers: 3,
            sample_rate: 44_100,
            chunk_size: 352_800,
            overlap: 2,
            frequency_indices: Vec::new(),
            bands_per_frequency: Vec::new(),
            frequencies_per_band: Vec::new(),
        }
    }
}

impl Config {
    fn total_input_dimension(&self) -> i64 {
        if matches!(self.architecture.as_str(), "bs_roformer" | "bs_polarformer") {
            (self.fft_size / 2 + 1) as i64 * 4
        } else {
            self.frequencies_per_band.iter().sum::<usize>() as i64 * 4
        }
    }

    fn band_input_dimensions(&self) -> Vec<i64> {
        self.frequencies_per_band
            .iter()
            .map(|frequencies| (*frequencies * 4) as i64)
            .collect()
    }
}

struct GraphState {
    runtime: Arc<GgmlRuntime>,
    frame_count: i64,
    context: ContextPtr,
    graph: GraphPtr,
    allocator: AllocatorPtr,
    input: TensorPtr,
    time_positions: TensorPtr,
    frequency_positions: TensorPtr,
    output: TensorPtr,
}

impl Drop for GraphState {
    fn drop(&mut self) {
        let api = &self.runtime.model_api;
        if !self.allocator.is_null() {
            ggml!(api, ggml_gallocr_free(self.allocator));
            self.allocator = std::ptr::null_mut();
        }
        if !self.context.is_null() {
            ggml!(api, ggml_free(self.context));
            self.context = std::ptr::null_mut();
        }
    }
}

struct Model {
    backend: GgmlBackendHandle,
    weight_context: ContextPtr,
    weight_buffer: BufferPtr,
    config: Config,
    graph: Option<GraphState>,
    profile: StageProfile,
}

impl Drop for Model {
    fn drop(&mut self) {
        self.graph.take();
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

/// Rust-owned RoFormer/PolarFormer execution over the upstream GGML C ABI.
pub struct Roformer {
    model: Model,
}

impl Roformer {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        let backend = runtime.create_backend(device)?;
        let mut model = Model {
            backend,
            weight_context: std::ptr::null_mut(),
            weight_buffer: std::ptr::null_mut(),
            config: Config::default(),
            graph: None,
            profile: StageProfile::new("roformer"),
        };
        model.load_weights(model_path)?;
        Ok(Self { model })
    }

    pub fn process_wav(
        &mut self,
        input_path: &Path,
        output_path: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(), String> {
        let config = self.model.config.clone();
        let input = read_f32_wav(input_path, config.sample_rate, 2)?;
        let outputs = process_overlap_add(
            &input,
            config.chunk_size,
            config.overlap,
            |chunk| self.model.process_chunk(chunk),
            &mut progress,
        )?;
        self.model.profile.report();
        if outputs.len() != 1 {
            return Err(format!(
                "GGML separation model returned {} stems; this route requires one estimate",
                outputs.len()
            ));
        }
        write_f32_wav(output_path, config.sample_rate, 2, &outputs[0])
    }
}

impl Model {
    fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded_path = path_c_string(path, "RoFormer GGUF path")?;
        let mut weight_context = std::ptr::null_mut();
        let gguf = ggml!(
            self.api(),
            gguf_init_from_file(
                encoded_path.as_ptr(),
                GgufInitParams {
                    no_alloc: true,
                    ctx: &mut weight_context,
                }
            )
        );
        if gguf.is_null() || weight_context.is_null() {
            return Err("could not open RoFormer GGUF through GGML".to_string());
        }
        self.weight_context = weight_context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let mut config = Config::default();
        config.architecture = self.required_string(gguf, "general.architecture")?;
        config.architecture = match config.architecture.as_str() {
            "bs" | "bs-roformer" | "bs_roformer" => "bs_roformer".to_string(),
            "bs_polarformer" => "bs_polarformer".to_string(),
            "mel_band" | "mel-band-roformer" | "mel_band_roformer" => {
                "mel_band_roformer".to_string()
            }
            other => return Err(format!("unsupported RoFormer GGUF architecture: {other}")),
        };
        let prefix = format!("{}.", config.architecture);
        config.public_schema = matches!(
            config.architecture.as_str(),
            "bs_roformer" | "bs_polarformer"
        ) && self.find_key(gguf, &format!("{prefix}n_bands"))? >= 0;
        config.use_pope = config.architecture == "bs_polarformer";
        config.has_final_norm = config.architecture != "mel_band_roformer";
        config.transformer_norm_output = config.architecture == "mel_band_roformer";

        let value = |key: &str, fallback: u32| -> Result<u32, String> {
            Ok(self
                .maybe_u32(gguf, &format!("{prefix}{key}"))?
                .unwrap_or(fallback))
        };
        let fft_key = if config.public_schema {
            "n_fft"
        } else {
            "stft_n_fft"
        };
        let hop_key = if config.public_schema {
            "hop_length"
        } else {
            "stft_hop_length"
        };
        let window_key = if config.public_schema {
            "win_length"
        } else {
            "stft_win_length"
        };
        let bands_key = if config.public_schema {
            "n_bands"
        } else {
            "num_bands"
        };
        let stems_key = if config.public_schema {
            "n_stems"
        } else {
            "num_stems"
        };
        config.fft_size = value(fft_key, config.fft_size as u32)? as usize;
        config.hop_length = value(hop_key, config.hop_length as u32)? as usize;
        config.window_length = value(window_key, config.window_length as u32)? as usize;
        config.dimension = value("dim", config.dimension as u32)? as i64;
        config.band_count = value(bands_key, config.band_count as u32)? as i64;
        config.depth = value("depth", config.depth as u32)? as usize;
        config.stem_count = value(stems_key, config.stem_count as u32)? as usize;
        config.sample_rate = value("sample_rate", config.sample_rate)?;
        let chunk_key = if config.public_schema {
            "chunk_size"
        } else {
            "default_chunk_size"
        };
        config.chunk_size = value(chunk_key, config.chunk_size as u32)? as usize;
        config.overlap = value("default_num_overlap", config.overlap as u32)? as usize;
        if config.public_schema {
            config.head_count = value("heads", config.head_count as u32)? as i64;
            config.head_dimension = value("dim_head", config.head_dimension as u32)? as i64;
            config.mask_depth = value("mask_layers", config.mask_depth as u32)? as usize;
            config.mask_mlp_layers = config.mask_depth;
        }
        config.skip_connection = self
            .maybe_bool(gguf, &format!("{prefix}skip_connection"))?
            .unwrap_or(false);
        config.zero_dc = self
            .maybe_bool(gguf, &format!("{prefix}zero_dc"))?
            .unwrap_or(false);
        if let Some(value) = self.maybe_bool(gguf, &format!("{prefix}has_final_norm"))? {
            config.has_final_norm = value;
        }
        if self
            .maybe_u32(gguf, &format!("{prefix}linear_transformer_depth"))?
            .is_some_and(|depth| depth > 0)
        {
            return Err("linear-attention RoFormer GGUF is not supported".to_string());
        }
        if config.public_schema {
            self.load_public_band_tables(gguf, &prefix, &mut config)?;
        }

        let api = self.api();
        let buffer_type = ggml!(api, ggml_backend_get_default_buffer_type(self.backend.raw));
        let buffer = ggml!(
            api,
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if buffer.is_null() {
            return Err("could not allocate GGML RoFormer weight buffer".to_string());
        }
        self.weight_buffer = buffer;
        self.upload_tensors(path, gguf)?;
        if !config.public_schema {
            self.load_legacy_band_tables(&mut config)?;
            config.mask_mlp_layers = self.detect_legacy_mask_layers()?;
        }
        validate_config(&config)?;
        self.config = config;
        Ok(())
    }

    fn load_public_band_tables(
        &self,
        gguf: GgufPtr,
        prefix: &str,
        config: &mut Config,
    ) -> Result<(), String> {
        let key = self.find_key(gguf, &format!("{prefix}band_widths"))?;
        if key < 0 {
            return Err("public BS GGUF is missing band_widths".to_string());
        }
        let api = self.api();
        if ggml!(api, gguf_get_kv_type(gguf, key)) != GGUF_TYPE_ARRAY
            || ggml!(api, gguf_get_arr_type(gguf, key)) != GGUF_TYPE_INT32
        {
            return Err("public BS band_widths has an invalid GGUF type".to_string());
        }
        let count = ggml!(api, gguf_get_arr_n(gguf, key));
        let raw = ggml!(api, gguf_get_arr_data(gguf, key)).cast::<i32>();
        if raw.is_null() || count == 0 {
            return Err("public BS band_widths is empty".to_string());
        }
        // SAFETY: GGUF owns `count` contiguous int32 values until `gguf_free`.
        let widths = unsafe { std::slice::from_raw_parts(raw, count) };
        config.frequencies_per_band = widths
            .iter()
            .map(|width| {
                usize::try_from(*width)
                    .ok()
                    .filter(|width| width % 4 == 0)
                    .map(|width| width / 4)
                    .ok_or_else(|| "public BS band width is invalid".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let frequency_channels = config.frequencies_per_band.iter().sum::<usize>() * 2;
        config.frequency_indices = (0..frequency_channels).collect();
        config.bands_per_frequency = vec![1; config.fft_size / 2 + 1];
        Ok(())
    }

    fn load_legacy_band_tables(&self, config: &mut Config) -> Result<(), String> {
        config.frequency_indices = self.read_i32_weight("buffer_freq_indices")?;
        config.bands_per_frequency = self.read_i32_weight("buffer_num_bands_per_freq")?;
        config.frequencies_per_band = self.read_i32_weight("buffer_num_freqs_per_band")?;
        Ok(())
    }

    fn read_i32_weight(&self, name: &str) -> Result<Vec<usize>, String> {
        let raw = self.weight(name)?;
        if tensor(raw)?.type_ != GGML_TYPE_I32 {
            return Err(format!("RoFormer table {name} is not I32"));
        }
        let count = usize::try_from(ggml!(self.api(), ggml_nelements(raw)))
            .map_err(|_| format!("RoFormer table {name} is too large"))?;
        let mut values = vec![0_i32; count];
        ggml!(
            self.api(),
            ggml_backend_tensor_get(
                raw,
                values.as_mut_ptr().cast::<c_void>(),
                0,
                values.len() * std::mem::size_of::<i32>()
            )
        );
        values
            .into_iter()
            .map(|value| {
                usize::try_from(value)
                    .map_err(|_| format!("RoFormer table {name} contains a negative index"))
            })
            .collect()
    }

    fn detect_legacy_mask_layers(&self) -> Result<usize, String> {
        let mut layers = 0;
        for sequence_index in (0..=20).step_by(2) {
            let name = format!("mask_est.0.freq.0.mlp.{sequence_index}.weight");
            if self.maybe_weight(&name)?.is_some() {
                layers += 1;
            } else {
                break;
            }
        }
        if layers == 0 {
            Err("legacy RoFormer mask estimator has no MLP layers".to_string())
        } else {
            Ok(layers)
        }
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let api = self.api();
        let data_offset = ggml!(api, gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen RoFormer GGUF: {error}"))?;
        let mut tensor = ggml!(api, ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "GGUF tensor name contains NUL".to_string())?;
            let tensor_id = ggml!(api, gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_id < 0 {
                return Err(format!("GGUF tensor metadata is missing for {name}"));
            }
            let size = ggml!(api, ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(api, gguf_get_tensor_offset(gguf, tensor_id)))
                .ok_or_else(|| "GGUF tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek RoFormer tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read RoFormer tensor {name}: {error}"))?;
            ggml!(
                api,
                ggml_backend_tensor_set(tensor, bytes.as_ptr().cast::<c_void>(), 0, size)
            );
            tensor = ggml!(api, ggml_get_next_tensor(self.weight_context, tensor));
        }
        if ggml!(api, gguf_get_n_tensors(gguf)) <= 0 {
            return Err("RoFormer GGUF has no tensors".to_string());
        }
        Ok(())
    }

    fn required_string(&self, gguf: GgufPtr, key: &str) -> Result<String, String> {
        let index = self.find_key(gguf, key)?;
        if index < 0 {
            return Err(format!("GGUF is missing {key}"));
        }
        let raw = ggml!(self.api(), gguf_get_val_str(gguf, index));
        if raw.is_null() {
            return Err(format!("GGUF string {key} is null"));
        }
        // SAFETY: GGUF owns this NUL-terminated value through the call.
        Ok(unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned())
    }

    fn find_key(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let key = CString::new(key).map_err(|_| "GGUF key contains NUL".to_string())?;
        Ok(ggml!(self.api(), gguf_find_key(gguf, key.as_ptr())))
    }

    fn maybe_u32(&self, gguf: GgufPtr, key: &str) -> Result<Option<u32>, String> {
        let index = self.find_key(gguf, key)?;
        Ok((index >= 0).then(|| ggml!(self.api(), gguf_get_val_u32(gguf, index))))
    }

    fn maybe_bool(&self, gguf: GgufPtr, key: &str) -> Result<Option<bool>, String> {
        let index = self.find_key(gguf, key)?;
        Ok((index >= 0).then(|| ggml!(self.api(), gguf_get_val_bool(gguf, index))))
    }

    fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        self.maybe_weight(name)?
            .ok_or_else(|| format!("RoFormer GGUF is missing weight {name}"))
    }

    fn maybe_weight(&self, name: &str) -> Result<Option<TensorPtr>, String> {
        let name = CString::new(name).map_err(|_| "weight name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, name.as_ptr())
        );
        Ok((!tensor.is_null()).then_some(tensor))
    }

    fn ensure_graph(&mut self, frame_count: i64) -> Result<(), String> {
        if self
            .graph
            .as_ref()
            .is_some_and(|graph| graph.frame_count == frame_count)
        {
            return Ok(());
        }
        self.graph.take();
        let runtime = Arc::clone(&self.backend.runtime);
        let api = &runtime.model_api;
        let context = ggml!(
            api,
            ggml_init(GgmlInitParams {
                mem_size: GRAPH_CONTEXT_BYTES,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate GGML RoFormer graph context".to_string());
        }
        let mut state = GraphState {
            runtime: Arc::clone(&runtime),
            frame_count,
            context,
            graph: std::ptr::null_mut(),
            allocator: std::ptr::null_mut(),
            input: std::ptr::null_mut(),
            time_positions: std::ptr::null_mut(),
            frequency_positions: std::ptr::null_mut(),
            output: std::ptr::null_mut(),
        };
        state.graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_CAPACITY, false));
        state.input = ggml!(
            api,
            ggml_new_tensor_3d(
                context,
                GGML_TYPE_F32,
                self.config.total_input_dimension(),
                frame_count,
                1
            )
        );
        ggml!(api, ggml_set_input(state.input));
        let split = self.build_band_split(context, state.input, frame_count)?;
        state.time_positions = ggml!(
            api,
            ggml_new_tensor_1d(context, GGML_TYPE_I32, frame_count * self.config.band_count)
        );
        state.frequency_positions = ggml!(
            api,
            ggml_new_tensor_1d(context, GGML_TYPE_I32, self.config.band_count * frame_count)
        );
        ggml!(api, ggml_set_input(state.time_positions));
        ggml!(api, ggml_set_input(state.frequency_positions));
        let transformed = self.build_transformers(
            context,
            split,
            state.time_positions,
            state.frequency_positions,
            frame_count,
        )?;
        state.output = self.build_mask(context, state.graph, transformed, frame_count)?;
        let buffer_type = ggml!(api, ggml_backend_get_default_buffer_type(self.backend.raw));
        state.allocator = ggml!(api, ggml_gallocr_new(buffer_type));
        if state.allocator.is_null() {
            return Err("could not create GGML RoFormer graph allocator".to_string());
        }
        let _ = ggml!(api, ggml_gallocr_reserve(state.allocator, state.graph));
        if !ggml!(api, ggml_gallocr_alloc_graph(state.allocator, state.graph)) {
            return Err("could not allocate GGML RoFormer graph on Vulkan".to_string());
        }
        self.graph = Some(state);
        Ok(())
    }

    fn process_chunk(&mut self, input: &[f32]) -> Result<Vec<Vec<f32>>, String> {
        if input.is_empty() || input.len() % 2 != 0 {
            return Err("RoFormer input chunk must be non-empty interleaved stereo".to_string());
        }
        let frame_samples = input.len() / 2;
        let mark = self.profile.mark();
        let mut channels = [
            Vec::with_capacity(frame_samples),
            Vec::with_capacity(frame_samples),
        ];
        for frame in input.chunks_exact(2) {
            channels[0].push(frame[0]);
            channels[1].push(frame[1]);
        }
        self.profile.record("deinterleave", mark);
        let mark = self.profile.mark();
        let spectra = channels.map(|channel| {
            compute_stft(
                &channel,
                self.config.fft_size,
                self.config.hop_length,
                self.config.window_length,
            )
        });
        self.profile.record("stft", mark);
        if spectra[0].n_frames == 0 || spectra[0].n_frames != spectra[1].n_frames {
            return Err("RoFormer STFT produced an invalid frame count".to_string());
        }
        let frame_count = spectra[0].n_frames;
        let mark = self.profile.mark();
        let model_input = prepare_model_input(
            &spectra,
            frame_count,
            self.config.total_input_dimension() as usize,
            &self.config.frequency_indices,
        )?;
        self.profile.record("prepare_input", mark);
        let mark = self.profile.mark();
        self.ensure_graph(frame_count as i64)?;
        self.profile.record("ensure_graph", mark);
        // Detach the runtime borrow from `self` so the stage profile can still
        // record while the GGML calls below are in flight.
        let runtime = Arc::clone(&self.backend.runtime);
        let api = &runtime.model_api;
        let (
            graph_handle,
            graph_input,
            time_position_tensor,
            frequency_position_tensor,
            graph_output,
        ) = {
            let graph = self.graph.as_ref().expect("graph was ensured");
            (
                graph.graph,
                graph.input,
                graph.time_positions,
                graph.frequency_positions,
                graph.output,
            )
        };
        let input_bytes = ggml!(api, ggml_nbytes(graph_input));
        if input_bytes != model_input.len() * std::mem::size_of::<f32>() {
            return Err("RoFormer GGML input tensor size mismatch".to_string());
        }
        let mark = self.profile.mark();
        let time_positions = (0..frame_count * self.config.band_count as usize)
            .map(|index| (index % frame_count) as i32)
            .collect::<Vec<_>>();
        let frequency_positions = (0..self.config.band_count as usize * frame_count)
            .map(|index| (index % self.config.band_count as usize) as i32)
            .collect::<Vec<_>>();
        ggml!(
            api,
            ggml_backend_tensor_set(
                graph_input,
                model_input.as_ptr().cast::<c_void>(),
                0,
                input_bytes
            )
        );
        ggml!(
            api,
            ggml_backend_tensor_set(
                time_position_tensor,
                time_positions.as_ptr().cast::<c_void>(),
                0,
                time_positions.len() * std::mem::size_of::<i32>()
            )
        );
        ggml!(
            api,
            ggml_backend_tensor_set(
                frequency_position_tensor,
                frequency_positions.as_ptr().cast::<c_void>(),
                0,
                frequency_positions.len() * std::mem::size_of::<i32>()
            )
        );
        self.profile.record("upload", mark);
        let mark = self.profile.mark();
        let status = ggml!(
            api,
            ggml_backend_graph_compute(self.backend.raw, graph_handle)
        );
        self.profile.record("compute", mark);
        if status != GGML_STATUS_SUCCESS {
            return Err(format!(
                "GGML RoFormer graph compute failed with status {status}"
            ));
        }
        let mark = self.profile.mark();
        let output_elements = usize::try_from(ggml!(api, ggml_nelements(graph_output)))
            .map_err(|_| "RoFormer output element count is invalid".to_string())?;
        let mut mask = vec![0.0_f32; output_elements];
        ggml!(
            api,
            ggml_backend_tensor_get(
                graph_output,
                mask.as_mut_ptr().cast::<c_void>(),
                0,
                output_elements * std::mem::size_of::<f32>()
            )
        );
        self.profile.record("download", mark);
        let mark = self.profile.mark();
        let stems = reconstruct_stems(&mask, &spectra, frame_samples, &self.config);
        self.profile.record("reconstruct", mark);
        self.profile.chunk_done();
        stems
    }

    fn build_band_split(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        frame_count: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let input_tensor = tensor(input)?;
        let mut offset = 0_usize;
        let mut bands = Vec::with_capacity(self.config.band_count as usize);
        for (index, input_dimension) in self.config.band_input_dimensions().into_iter().enumerate()
        {
            let band = ggml!(
                api,
                ggml_view_3d(
                    context,
                    input,
                    input_dimension,
                    frame_count,
                    1,
                    input_tensor.nb[1],
                    input_tensor.nb[2],
                    offset * std::mem::size_of::<f32>()
                )
            );
            let band = if self.config.public_schema {
                ggml!(api, ggml_cont(context, band))
            } else {
                band
            };
            let gamma = self.weight(&format!(
                "band_split.{index}.{}",
                if self.config.public_schema {
                    "norm"
                } else {
                    "norm.weight"
                }
            ))?;
            let normalized = ggml!(api, ggml_rms_norm(context, band, 1.0e-12));
            let normalized = ggml!(api, ggml_mul(context, normalized, gamma));
            let weight = self.weight(&format!(
                "band_split.{index}.{}",
                if self.config.public_schema {
                    "w"
                } else {
                    "linear.weight"
                }
            ))?;
            let bias = self.weight(&format!(
                "band_split.{index}.{}",
                if self.config.public_schema {
                    "b"
                } else {
                    "linear.bias"
                }
            ))?;
            let projected = ggml!(api, ggml_mul_mat(context, weight, normalized));
            let projected = ggml!(api, ggml_add(context, projected, bias));
            bands.push(ggml!(
                api,
                ggml_reshape_4d(context, projected, self.config.dimension, 1, frame_count, 1)
            ));
            offset += input_dimension as usize;
        }
        concat_balanced(api, context, &bands, 1)
    }

    fn build_transformers(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        expanded_time_positions: TensorPtr,
        expanded_frequency_positions: TensorPtr,
        frame_count: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let time_positions = if self.config.public_schema {
            ggml!(
                api,
                ggml_view_1d(context, expanded_time_positions, frame_count, 0)
            )
        } else {
            expanded_time_positions
        };
        let frequency_positions = if self.config.public_schema {
            ggml!(
                api,
                ggml_view_1d(
                    context,
                    expanded_frequency_positions,
                    self.config.band_count,
                    0
                )
            )
        } else {
            expanded_frequency_positions
        };
        let mut value = input;
        let mut skip_outputs = Vec::new();
        for layer in 0..self.config.depth {
            if self.config.skip_connection {
                for skip in &skip_outputs {
                    value = ggml!(api, ggml_add(context, value, *skip));
                }
            }
            let block = format!("blk.{layer}");
            value = ggml!(api, ggml_permute(context, value, 0, 2, 1, 3));
            value = ggml!(api, ggml_cont(context, value));
            let time = ggml!(
                api,
                ggml_reshape_3d(
                    context,
                    value,
                    self.config.dimension,
                    frame_count,
                    self.config.band_count
                )
            );
            let time_attention_prefix = if self.config.public_schema {
                format!("{block}.time")
            } else {
                format!("{block}.time_attn")
            };
            let time_feed_forward_prefix = if self.config.public_schema {
                format!("{block}.time")
            } else {
                format!("{block}.time_ff")
            };
            let time = self.attention(context, time, time_positions, &time_attention_prefix)?;
            let mut time = self.feed_forward(context, time, &time_feed_forward_prefix)?;
            if self.config.transformer_norm_output {
                let norm = self.weight(&format!("{block}.time_norm.weight"))?;
                time = ggml!(api, ggml_rms_norm(context, time, 1.0e-12));
                time = ggml!(api, ggml_mul(context, time, norm));
            }
            value = ggml!(
                api,
                ggml_reshape_4d(
                    context,
                    time,
                    self.config.dimension,
                    frame_count,
                    self.config.band_count,
                    1
                )
            );
            value = ggml!(api, ggml_permute(context, value, 0, 2, 1, 3));
            value = ggml!(api, ggml_cont(context, value));
            let frequency = ggml!(
                api,
                ggml_reshape_3d(
                    context,
                    value,
                    self.config.dimension,
                    self.config.band_count,
                    frame_count
                )
            );
            let frequency_attention_prefix = if self.config.public_schema {
                format!("{block}.freq")
            } else {
                format!("{block}.freq_attn")
            };
            let frequency_feed_forward_prefix = if self.config.public_schema {
                format!("{block}.freq")
            } else {
                format!("{block}.freq_ff")
            };
            let frequency = self.attention(
                context,
                frequency,
                frequency_positions,
                &frequency_attention_prefix,
            )?;
            let mut frequency =
                self.feed_forward(context, frequency, &frequency_feed_forward_prefix)?;
            if self.config.transformer_norm_output {
                let norm = self.weight(&format!("{block}.freq_norm.weight"))?;
                frequency = ggml!(api, ggml_rms_norm(context, frequency, 1.0e-12));
                frequency = ggml!(api, ggml_mul(context, frequency, norm));
            }
            value = ggml!(
                api,
                ggml_reshape_4d(
                    context,
                    frequency,
                    self.config.dimension,
                    self.config.band_count,
                    frame_count,
                    1
                )
            );
            if self.config.skip_connection {
                skip_outputs.push(value);
            }
        }
        if self.config.has_final_norm {
            let norm = self.weight(if self.config.public_schema {
                "final_norm"
            } else {
                "final_norm.weight"
            })?;
            value = ggml!(api, ggml_rms_norm(context, value, 1.0e-12));
            value = ggml!(api, ggml_mul(context, value, norm));
        }
        Ok(value)
    }

    fn attention(
        &self,
        context: ContextPtr,
        sequence: TensorPtr,
        positions: TensorPtr,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let sequence_shape = tensor(sequence)?.ne;
        let sequence_length = sequence_shape[1];
        let sequence_batch = sequence_shape[2];
        let inner = self.config.head_count * self.config.head_dimension;
        let attention_name = |public_suffix: &str, legacy_suffix: &str| {
            if self.config.public_schema {
                format!("{prefix}.{public_suffix}")
            } else {
                format!("{prefix}_{legacy_suffix}")
            }
        };
        let norm = self.weight(&attention_name("attn_norm", "norm.weight"))?;
        let qkv_weight = self.weight(&attention_name("qkv", "qkv.weight"))?;
        let gate_weight = self.weight(&attention_name("gates_w", "gate.weight"))?;
        let gate_bias = self.weight(&attention_name("gates_b", "gate.bias"))?;
        let output_weight = self.weight(&attention_name("out", "out.weight"))?;
        let normalized = ggml!(api, ggml_rms_norm(context, sequence, 1.0e-12));
        let normalized = ggml!(api, ggml_mul(context, normalized, norm));
        let qkv = ggml!(api, ggml_mul_mat(context, qkv_weight, normalized));
        let qkv_tensor = tensor(qkv)?;
        let (mut query, mut key, value) = if self.config.public_schema {
            let mut parts = Vec::with_capacity(3);
            for index in 0..3 {
                let part = ggml!(
                    api,
                    ggml_view_3d(
                        context,
                        qkv,
                        inner,
                        sequence_length,
                        sequence_batch,
                        qkv_tensor.nb[1],
                        qkv_tensor.nb[2],
                        index * inner as usize * std::mem::size_of::<f32>()
                    )
                );
                let part = ggml!(api, ggml_cont(context, part));
                parts.push(ggml!(
                    api,
                    ggml_reshape_4d(
                        context,
                        part,
                        self.config.head_dimension,
                        self.config.head_count,
                        sequence_length,
                        sequence_batch
                    )
                ));
            }
            (parts[0], parts[1], parts[2])
        } else {
            let flattened_sequence = sequence_length * sequence_batch;
            let query = ggml!(
                api,
                ggml_view_4d(
                    context,
                    qkv,
                    self.config.head_dimension,
                    self.config.head_count,
                    flattened_sequence,
                    1,
                    self.config.head_dimension as usize * std::mem::size_of::<f32>(),
                    qkv_tensor.nb[1],
                    flattened_sequence as usize * qkv_tensor.nb[1],
                    0
                )
            );
            let key = ggml!(
                api,
                ggml_view_4d(
                    context,
                    qkv,
                    self.config.head_dimension,
                    self.config.head_count,
                    flattened_sequence,
                    1,
                    self.config.head_dimension as usize * std::mem::size_of::<f32>(),
                    qkv_tensor.nb[1],
                    flattened_sequence as usize * qkv_tensor.nb[1],
                    inner as usize * std::mem::size_of::<f32>()
                )
            );
            let value = ggml!(
                api,
                ggml_view_4d(
                    context,
                    qkv,
                    self.config.head_dimension,
                    sequence_length,
                    self.config.head_count,
                    sequence_batch,
                    qkv_tensor.nb[1],
                    self.config.head_dimension as usize * std::mem::size_of::<f32>(),
                    qkv_tensor.nb[2],
                    2 * inner as usize * std::mem::size_of::<f32>()
                )
            );
            (query, key, ggml!(api, ggml_cont(context, value)))
        };
        if self.config.use_pope {
            let inverse_frequencies = self.weight("pope.inv_freqs")?;
            let time = prefix.ends_with("time");
            let key_bias = self.weight(if time {
                "pope.time_k_phase_bias"
            } else {
                "pope.freq_k_phase_bias"
            })?;
            query = apply_pope(
                api,
                context,
                query,
                positions,
                inverse_frequencies,
                None,
                self.config.head_dimension,
                self.config.head_count,
            );
            key = apply_pope(
                api,
                context,
                key,
                positions,
                inverse_frequencies,
                Some(key_bias),
                self.config.head_dimension,
                self.config.head_count,
            );
        } else {
            query = rope(api, context, query, positions, self.config.head_dimension);
            key = rope(api, context, key, positions, self.config.head_dimension);
        }
        // Flash attention reads its row stride from nb1 and this permutation
        // leaves nb0 alone, so the rows it walks are still contiguous and the
        // view needs no materialising. Upstream's Vulkan support check asks
        // only about head size and element type, and a cast is a copy that
        // takes a strided source, so the half-precision K and V come straight
        // off the view in one pass instead of a copy and then a cast.
        let layout = |value| ggml!(api, ggml_permute(context, value, 0, 2, 1, 3));
        let (query, key, value) = if self.config.public_schema {
            query = layout(query);
            key = layout(key);
            let value = layout(value);
            (
                query,
                ggml!(api, ggml_cast(context, key, GGML_TYPE_F16)),
                ggml!(api, ggml_cast(context, value, GGML_TYPE_F16)),
            )
        } else {
            let restore = |value| {
                ggml!(
                    api,
                    ggml_view_4d(
                        context,
                        value,
                        self.config.head_dimension,
                        self.config.head_count,
                        sequence_length,
                        sequence_batch,
                        tensor(value).expect("RoPE tensor").nb[1],
                        tensor(value).expect("RoPE tensor").nb[2],
                        sequence_length as usize * tensor(value).expect("RoPE tensor").nb[2],
                        0
                    )
                )
            };
            (layout(restore(query)), layout(restore(key)), value)
        };
        let scale = 1.0 / (self.config.head_dimension as f32).sqrt();
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                key,
                value,
                std::ptr::null_mut(),
                scale,
                0.0,
                0.0
            )
        );
        // F32 accumulation. Half-precision accumulation was measured on 2026-09-09:
        // no change in dispatch time and a 3.6e-3 peak difference in the separated
        // output. See docs/PERFORMANCE_DIRECTION_2026-09-09.md.
        ggml!(api, ggml_flash_attn_ext_set_prec(attended, GGML_PREC_F32));
        let gates = ggml!(api, ggml_mul_mat(context, gate_weight, normalized));
        let gates = ggml!(api, ggml_add(context, gates, gate_bias));
        let gates = ggml!(api, ggml_sigmoid(context, gates));
        let gates = ggml!(
            api,
            ggml_reshape_4d(
                context,
                gates,
                1,
                self.config.head_count,
                sequence_length,
                sequence_batch
            )
        );
        let attended = ggml!(api, ggml_mul(context, attended, gates));
        let attended = ggml!(
            api,
            ggml_reshape_3d(context, attended, inner, sequence_length, sequence_batch)
        );
        let projected = ggml!(api, ggml_mul_mat(context, output_weight, attended));
        Ok(ggml!(api, ggml_add(context, sequence, projected)))
    }

    fn feed_forward(
        &self,
        context: ContextPtr,
        sequence: TensorPtr,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let feed_forward_name = |public_suffix: &str, legacy_suffix: &str| {
            if self.config.public_schema {
                format!("{prefix}.{public_suffix}")
            } else {
                format!("{prefix}_{legacy_suffix}")
            }
        };
        let norm = self.weight(&feed_forward_name("ff_norm", "norm.weight"))?;
        let first_weight = self.weight(&feed_forward_name("ff1_w", "in.weight"))?;
        let first_bias = self.weight(&feed_forward_name("ff1_b", "in.bias"))?;
        let second_weight = self.weight(&feed_forward_name("ff2_w", "out.weight"))?;
        let second_bias = self.weight(&feed_forward_name("ff2_b", "out.bias"))?;
        let hidden = ggml!(api, ggml_rms_norm(context, sequence, 1.0e-12));
        let hidden = ggml!(api, ggml_mul(context, hidden, norm));
        let hidden = ggml!(api, ggml_mul_mat(context, first_weight, hidden));
        let hidden = ggml!(api, ggml_add(context, hidden, first_bias));
        let hidden = ggml!(api, ggml_gelu_erf(context, hidden));
        let hidden = ggml!(api, ggml_mul_mat(context, second_weight, hidden));
        let hidden = ggml!(api, ggml_add(context, hidden, second_bias));
        Ok(ggml!(api, ggml_add(context, sequence, hidden)))
    }

    fn build_mask(
        &self,
        context: ContextPtr,
        graph: GraphPtr,
        input: TensorPtr,
        frame_count: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let mut output_dimensions = Vec::with_capacity(self.config.band_count as usize);
        let final_mlp_index = (self.config.mask_mlp_layers - 1) * 2;
        for band in 0..self.config.band_count {
            let final_weight = self.weight(&if self.config.public_schema {
                format!("mask.0.{band}.w2")
            } else {
                format!("mask_est.0.freq.{band}.mlp.{final_mlp_index}.weight")
            })?;
            output_dimensions.push(tensor(final_weight)?.ne[1] / 2);
        }
        let input_tensor = tensor(input)?;
        let mut stems = Vec::with_capacity(self.config.stem_count);
        for stem in 0..self.config.stem_count {
            let mut bands = Vec::with_capacity(self.config.band_count as usize);
            for band in 0..self.config.band_count as usize {
                let band_input = ggml!(
                    api,
                    ggml_view_3d(
                        context,
                        input,
                        self.config.dimension,
                        frame_count,
                        1,
                        input_tensor.nb[2],
                        input_tensor.nb[3],
                        band * input_tensor.nb[1]
                    )
                );
                let current = if self.config.public_schema {
                    let prefix = format!("mask.{stem}.{band}");
                    let first_weight = self.weight(&format!("{prefix}.w1"))?;
                    let first_bias = self.weight(&format!("{prefix}.b1"))?;
                    let second_weight = self.weight(&format!("{prefix}.w2"))?;
                    let second_bias = self.weight(&format!("{prefix}.b2"))?;
                    let current = ggml!(api, ggml_mul_mat(context, first_weight, band_input));
                    let current = ggml!(api, ggml_add(context, current, first_bias));
                    let current = ggml!(api, ggml_tanh(context, current));
                    let current = ggml!(api, ggml_mul_mat(context, second_weight, current));
                    ggml!(api, ggml_add(context, current, second_bias))
                } else {
                    let prefix = format!("mask_est.{stem}.freq.{band}.mlp");
                    let mut current = band_input;
                    for layer in 0..self.config.mask_mlp_layers {
                        let sequence_index = layer * 2;
                        let weight = self.weight(&format!("{prefix}.{sequence_index}.weight"))?;
                        let bias = self.weight(&format!("{prefix}.{sequence_index}.bias"))?;
                        current = ggml!(api, ggml_mul_mat(context, weight, current));
                        current = ggml!(api, ggml_add(context, current, bias));
                        if layer + 1 < self.config.mask_mlp_layers {
                            current = ggml!(api, ggml_tanh(context, current));
                        }
                    }
                    current
                };
                let current_tensor = tensor(current)?;
                let dimension = output_dimensions[band];
                let first = ggml!(
                    api,
                    ggml_view_3d(
                        context,
                        current,
                        dimension,
                        frame_count,
                        1,
                        current_tensor.nb[1],
                        current_tensor.nb[2],
                        0
                    )
                );
                let second = ggml!(
                    api,
                    ggml_view_3d(
                        context,
                        current,
                        dimension,
                        frame_count,
                        1,
                        current_tensor.nb[1],
                        current_tensor.nb[2],
                        dimension as usize * std::mem::size_of::<f32>()
                    )
                );
                let second = ggml!(api, ggml_cont(context, second));
                let second = ggml!(api, ggml_sigmoid(context, second));
                let output = ggml!(api, ggml_mul(context, first, second));
                bands.push(ggml!(
                    api,
                    ggml_reshape_4d(context, output, dimension, 1, frame_count, 1)
                ));
            }
            stems.push(concat_balanced(api, context, &bands, 0)?);
        }
        let output = concat_balanced(api, context, &stems, 1)?;
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(graph, output));
        Ok(output)
    }
}

fn validate_config(config: &Config) -> Result<(), String> {
    if config.fft_size == 0
        || config.hop_length == 0
        || config.window_length == 0
        || config.window_length > config.fft_size
        || config.dimension <= 0
        || config.band_count <= 0
        || config.depth == 0
        || config.head_count <= 0
        || config.head_dimension <= 0
        || config.stem_count != 1
        || config.mask_mlp_layers == 0
        || config.sample_rate != 44_100
        || config.chunk_size == 0
        || config.overlap == 0
        || config.chunk_size / config.overlap == 0
        || config.frequencies_per_band.len() != config.band_count as usize
        || config.frequency_indices.len() != config.total_input_dimension() as usize / 2
        || config.bands_per_frequency.len() != config.fft_size / 2 + 1
    {
        return Err(
            "RoFormer GGUF metadata does not match the Rust GGML execution contract".to_string(),
        );
    }
    Ok(())
}

fn tensor(raw: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers retain the GGML context that owns `raw`; null is
    // rejected before a reference is formed.
    unsafe { raw.as_ref() }.ok_or_else(|| "GGML returned a null tensor".to_string())
}

fn tensor_name(raw: TensorPtr) -> Result<String, String> {
    let tensor = tensor(raw)?;
    // SAFETY: GGML tensor names are fixed NUL-terminated arrays.
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
        [] => Err("cannot concatenate an empty GGML tensor list".to_string()),
        [only] => Ok(*only),
        _ => {
            let middle = tensors.len() / 2;
            let left = concat_balanced(api, context, &tensors[..middle], dimension)?;
            let right = concat_balanced(api, context, &tensors[middle..], dimension)?;
            Ok(ggml!(api, ggml_concat(context, left, right, dimension)))
        }
    }
}

fn rope(
    api: &ModelApi,
    context: ContextPtr,
    tensor: TensorPtr,
    positions: TensorPtr,
    dimensions: i64,
) -> TensorPtr {
    ggml!(
        api,
        ggml_rope_ext(
            context,
            tensor,
            positions,
            std::ptr::null_mut(),
            dimensions as i32,
            0,
            0,
            10_000.0,
            1.0,
            0.0,
            1.0,
            0.0,
            0.0
        )
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_pope(
    api: &ModelApi,
    context: ContextPtr,
    query_or_key: TensorPtr,
    positions: TensorPtr,
    inverse_frequencies: TensorPtr,
    bias: Option<TensorPtr>,
    head_dimension: i64,
    heads: i64,
) -> TensorPtr {
    let shape = tensor(query_or_key).expect("PoPE input tensor").ne;
    let sequence = shape[2];
    let batch = shape[3];
    let magnitude = ggml!(api, ggml_softplus(context, query_or_key));
    let positions = ggml!(api, ggml_cast(context, positions, GGML_TYPE_F32));
    let position_row = ggml!(api, ggml_reshape_2d(context, positions, 1, sequence));
    let frequency_row = ggml!(
        api,
        ggml_reshape_2d(context, inverse_frequencies, 1, head_dimension)
    );
    let phase = ggml!(api, ggml_mul_mat(context, frequency_row, position_row));
    let phase = ggml!(
        api,
        ggml_reshape_4d(context, phase, head_dimension, 1, sequence, 1)
    );
    let (cosine, sine) = if let Some(bias) = bias {
        let bias = ggml!(
            api,
            ggml_reshape_4d(context, bias, head_dimension, heads, 1, 1)
        );
        let target = ggml!(
            api,
            ggml_new_tensor_4d(context, GGML_TYPE_F32, head_dimension, heads, sequence, 1)
        );
        let phase = ggml!(api, ggml_repeat(context, phase, target));
        let bias = ggml!(api, ggml_repeat(context, bias, target));
        let biased = ggml!(api, ggml_add(context, phase, bias));
        (
            ggml!(api, ggml_cos(context, biased)),
            ggml!(api, ggml_sin(context, biased)),
        )
    } else {
        (
            ggml!(api, ggml_cos(context, phase)),
            ggml!(api, ggml_sin(context, phase)),
        )
    };
    let real = ggml!(api, ggml_mul(context, magnitude, cosine));
    let imaginary = ggml!(api, ggml_mul(context, magnitude, sine));
    let real = ggml!(
        api,
        ggml_reshape_4d(context, real, 1, head_dimension, heads, sequence * batch)
    );
    let imaginary = ggml!(
        api,
        ggml_reshape_4d(
            context,
            imaginary,
            1,
            head_dimension,
            heads,
            sequence * batch
        )
    );
    let stacked = ggml!(api, ggml_concat(context, real, imaginary, 0));
    ggml!(
        api,
        ggml_reshape_4d(context, stacked, 2 * head_dimension, heads, sequence, batch)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_config_cannot_enable_cpu_or_multiple_stems() {
        let mut config = Config::default();
        config.architecture = "bs_roformer".to_string();
        config.frequencies_per_band = vec![1; config.band_count as usize];
        config.frequency_indices = vec![0; config.total_input_dimension() as usize / 2];
        config.bands_per_frequency = vec![1; config.fft_size / 2 + 1];
        config.stem_count = 2;
        assert!(validate_config(&config).is_err());
    }
}
