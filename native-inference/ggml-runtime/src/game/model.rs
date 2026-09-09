use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{
    BufferPtr, ContextPtr, GGML_TYPE_F32, GgmlTensor, GgufInitParams, GgufPtr, ModelApi, TensorPtr,
};
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all handles are owned by this live GAME model load.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameConfig {
    pub variant: &'static str,
    pub embedding_dim: usize,
    pub input_dim: usize,
    pub estimator_output_dim: usize,
    pub region_cycle_length: usize,
    pub language_count: usize,
    pub encoder_layers: usize,
    pub segmenter_layers: usize,
    pub estimator_layers: usize,
    pub model_dim: usize,
    pub attention_heads: usize,
    pub attention_head_dim: usize,
    pub midi_minimum: f32,
    pub midi_maximum: f32,
    pub midi_bins: usize,
    pub midi_deviation: f32,
}

/// Loaded GAME weights on one explicitly selected GGML backend.
pub struct Game {
    pub(crate) backend: GgmlBackendHandle,
    pub(crate) weight_context: ContextPtr,
    weight_buffer: BufferPtr,
    config: GameConfig,
}

impl Drop for Game {
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

impl Game {
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
            config: expected_medium_config(),
        };
        model.load_weights(model_path)?;
        Ok(model)
    }

    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    pub(crate) fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    pub(crate) fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        let encoded =
            CString::new(name).map_err(|_| "GAME tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("GAME GGUF tensor is missing: {name}"))
        } else {
            Ok(tensor)
        }
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "GAME GGUF path")?;
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
            return Err("could not open GAME GGUF through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        self.validate_metadata(gguf)?;
        let expected_tensors = match self.config.variant {
            "small" | "medium" => 668,
            "large" => 1_308,
            _ => return Err("GAME runtime selected an unknown model profile".to_string()),
        };
        if ggml!(self.api(), gguf_get_n_tensors(gguf)) != expected_tensors {
            return Err(format!(
                "GAME GGUF tensor count does not match the {} profile ({expected_tensors})",
                self.config.variant
            ));
        }
        self.validate_anchor_shapes()?;
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        let buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if buffer.is_null() {
            return Err("could not allocate GAME GGML weight buffer".to_string());
        }
        self.weight_buffer = buffer;
        self.upload_tensors(path, gguf)
    }

    fn validate_metadata(&mut self, gguf: GgufPtr) -> Result<(), String> {
        for (key, expected) in [
            ("general.architecture", "game-me"),
            ("game.model.mode", "d3pm"),
            ("game.encoder.ffn_type", "glu"),
            ("game.segmenter.ffn_type", "glu"),
            ("game.estimator.ffn_type", "glu"),
            ("game.estimator.pool_merge_mode", "mean"),
            ("game.estimator.attn_type", "joint"),
            ("game.estimator.rope_mode", "mixed"),
            ("game.inference.spectrogram.type", "mel"),
        ] {
            if self.required_string(gguf, key)? != expected {
                return Err(format!("GAME GGUF metadata mismatch: {key}"));
            }
        }
        let embedding_dim = self.required_u32(gguf, "game.model.embedding_dim")? as usize;
        let encoder_dim = self.required_u32(gguf, "game.encoder.dim")? as usize;
        let encoder_layers = self.required_u32(gguf, "game.encoder.num_layers")? as usize;
        let encoder_heads = self.required_u32(gguf, "game.encoder.num_heads")? as usize;
        let encoder_head_dim = self.required_u32(gguf, "game.encoder.head_dim")? as usize;
        let segmenter_dim = self.required_u32(gguf, "game.segmenter.dim")? as usize;
        let segmenter_layers = self.required_u32(gguf, "game.segmenter.num_layers")? as usize;
        let segmenter_heads = self.required_u32(gguf, "game.segmenter.num_heads")? as usize;
        let segmenter_head_dim = self.required_u32(gguf, "game.segmenter.head_dim")? as usize;
        let estimator_dim = self.required_u32(gguf, "game.estimator.dim")? as usize;
        let estimator_layers = self.required_u32(gguf, "game.estimator.num_layers")? as usize;
        let estimator_heads = self.required_u32(gguf, "game.estimator.num_heads")? as usize;
        let estimator_head_dim = self.required_u32(gguf, "game.estimator.head_dim")? as usize;
        if encoder_dim != embedding_dim
            || segmenter_dim != embedding_dim
            || estimator_dim != embedding_dim
            || segmenter_heads != encoder_heads
            || estimator_heads != encoder_heads
            || segmenter_head_dim != encoder_head_dim
            || estimator_head_dim != encoder_head_dim
        {
            return Err("GAME GGUF backbone metadata is inconsistent".to_string());
        }
        let variant = match (
            embedding_dim,
            encoder_layers,
            segmenter_layers,
            estimator_layers,
            encoder_heads,
            encoder_head_dim,
        ) {
            (128, 4, 8, 4, 4, 64) => "small",
            (256, 4, 8, 4, 8, 64) => "medium",
            (256, 8, 16, 8, 8, 64) => "large",
            _ => return Err("GAME GGUF model profile is unsupported".to_string()),
        };
        for (key, expected) in [
            ("game.model.in_dim", 80),
            ("game.model.estimator_out_dim", 257),
            ("game.model.region_cycle_len", 3),
            ("game.model.num_languages", 127),
            ("game.encoder.c_kernel_size", 31),
            ("game.encoder.m_kernel_size", 31),
            ("game.segmenter.c_kernel_size", 31),
            ("game.segmenter.m_kernel_size", 31),
            ("game.estimator.region_token_num", 1),
            ("game.estimator.c_kernel_size_pool", 7),
            ("game.estimator.m_kernel_size_pool", 5),
            ("game.estimator.c_kernel_size_x", 31),
            ("game.estimator.m_kernel_size_x", 31),
            ("game.inference.audio_sample_rate", 44_100),
            ("game.inference.hop_size", 441),
            ("game.inference.fft_size", 2_048),
            ("game.inference.win_size", 2_048),
            ("game.inference.spectrogram.num_bins", 80),
            ("game.inference.midi_num_bins", 257),
        ] {
            if self.required_u32(gguf, key)? != expected {
                return Err(format!("GAME GGUF metadata mismatch: {key}"));
            }
        }
        for (key, expected) in [
            ("game.model.use_languages", true),
            ("game.encoder.use_ls", true),
            ("game.encoder.use_out_norm", true),
            ("game.encoder.skip_first_ffn", false),
            ("game.encoder.skip_out_ffn", false),
            ("game.segmenter.use_ls", true),
            ("game.segmenter.use_out_norm", true),
            ("game.segmenter.skip_first_ffn", false),
            ("game.segmenter.skip_out_ffn", false),
            ("game.estimator.use_ls", true),
            ("game.estimator.use_out_norm", true),
            ("game.estimator.skip_first_ffn", false),
            ("game.estimator.skip_out_ffn", false),
            ("game.estimator.qk_norm", true),
            ("game.estimator.use_region_bias", false),
            ("game.estimator.use_rope", true),
            ("game.estimator.use_pool_offset", false),
        ] {
            if self.required_bool(gguf, key)? != expected {
                return Err(format!("GAME GGUF metadata mismatch: {key}"));
            }
        }
        for (key, expected) in [
            ("game.estimator.theta", 10_000.0),
            ("game.inference.spectrogram.fmin", 0.0),
            ("game.inference.spectrogram.fmax", 8_000.0),
            ("game.inference.midi_min", 0.0),
            ("game.inference.midi_max", 128.0),
            ("game.inference.midi_std", 0.5),
        ] {
            if self.required_f32(gguf, key)?.to_bits() != f32::to_bits(expected) {
                return Err(format!("GAME GGUF metadata mismatch: {key}"));
            }
        }
        self.config = GameConfig {
            variant,
            embedding_dim,
            input_dim: 80,
            estimator_output_dim: 257,
            region_cycle_length: 3,
            language_count: 127,
            encoder_layers,
            segmenter_layers,
            estimator_layers,
            model_dim: embedding_dim,
            attention_heads: encoder_heads,
            attention_head_dim: encoder_head_dim,
            midi_minimum: 0.0,
            midi_maximum: 128.0,
            midi_bins: 257,
            midi_deviation: 0.5,
        };
        Ok(())
    }

    fn validate_anchor_shapes(&self) -> Result<(), String> {
        let dim = self.config.model_dim as i64;
        let embedding_dim = self.config.embedding_dim as i64;
        let projection_dim = (self.config.attention_heads * self.config.attention_head_dim) as i64;
        let final_segmenter = self.config.segmenter_layers - 1;
        let final_estimator = self.config.estimator_layers - 1;
        let anchors = [
            ("spectrogram_projection.weight".to_string(), vec![80, dim]),
            ("spectrogram_projection.bias".to_string(), vec![dim]),
            ("encoder.input_proj.weight".to_string(), vec![dim, dim]),
            (
                "encoder.output_proj.weight".to_string(),
                vec![dim, embedding_dim * 2],
            ),
            ("noise_embedding.embedding.weight".to_string(), vec![dim, 3]),
            ("language_embedding.weight".to_string(), vec![dim, 128]),
            ("time_embedding.0.weight".to_string(), vec![1, dim * 4]),
            ("time_embedding.2.weight".to_string(), vec![dim * 4, dim]),
            ("segmenter.input_proj.weight".to_string(), vec![dim, dim]),
            ("segmenter.output_proj.weight".to_string(), vec![dim]),
            ("estimator.input_proj.weight".to_string(), vec![dim, dim]),
            ("estimator.pool_token_gen.emb".to_string(), vec![dim]),
            (
                "region_embedding.embedding.weight".to_string(),
                vec![dim, 3],
            ),
            (
                "estimator.output_proj_pool.weight".to_string(),
                vec![dim, 257],
            ),
            (
                "encoder.layers.0.attn.attn.q_linear.weight".to_string(),
                vec![dim, projection_dim],
            ),
            (
                format!("segmenter.layers.{final_segmenter}.attn.c.dw.weight"),
                vec![31, 1, dim],
            ),
            (
                format!("estimator.layers.{final_estimator}.attn.jattn.pool_qkv.weight"),
                vec![dim, projection_dim * 3],
            ),
        ];
        for (name, shape) in anchors {
            self.require_shape(&name, &shape)?;
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
            return Err(format!(
                "GAME GGUF tensor shape mismatch: {name}; expected {expected:?}, found {:?}",
                &tensor.ne[..dimensions]
            ));
        }
        Ok(())
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let data_offset = ggml!(self.api(), gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen GAME GGUF: {error}"))?;
        let mut tensor = ggml!(self.api(), ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let descriptor = tensor_ref(tensor)?;
            let name = tensor_name(tensor)?;
            if descriptor.type_ != GGML_TYPE_F32 {
                return Err(format!("GAME tensor is not F32: {name}"));
            }
            let encoded = CString::new(name.as_str())
                .map_err(|_| "GAME tensor name contains NUL".to_string())?;
            let tensor_index = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_index < 0 {
                return Err(format!("GAME tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(self.api(), ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(
                    self.api(),
                    gguf_get_tensor_offset(gguf, tensor_index)
                ))
                .ok_or_else(|| "GAME tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek GAME tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read GAME tensor {name}: {error}"))?;
            ggml!(
                self.api(),
                ggml_backend_tensor_set(tensor, bytes.as_ptr().cast::<c_void>(), 0, size)
            );
            tensor = ggml!(
                self.api(),
                ggml_get_next_tensor(self.weight_context, tensor)
            );
        }
        Ok(())
    }

    fn required_string(&self, gguf: GgufPtr, key: &str) -> Result<String, String> {
        let raw = ggml!(
            self.api(),
            gguf_get_val_str(gguf, self.key_index(gguf, key)?)
        );
        if raw.is_null() {
            return Err(format!("GAME GGUF string metadata is null: {key}"));
        }
        // SAFETY: GGML owns a NUL-terminated metadata string while `gguf` lives.
        unsafe { CStr::from_ptr(raw) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("GAME GGUF string metadata is not UTF-8: {key}"))
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

    fn required_f32(&self, gguf: GgufPtr, key: &str) -> Result<f32, String> {
        Ok(ggml!(
            self.api(),
            gguf_get_val_f32(gguf, self.key_index(gguf, key)?)
        ))
    }

    fn key_index(&self, gguf: GgufPtr, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "GAME GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api(), gguf_find_key(gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("GAME GGUF metadata is missing: {key}"))
        } else {
            Ok(index)
        }
    }
}

fn expected_medium_config() -> GameConfig {
    GameConfig {
        variant: "medium",
        embedding_dim: 256,
        input_dim: 80,
        estimator_output_dim: 257,
        region_cycle_length: 3,
        language_count: 127,
        encoder_layers: 4,
        segmenter_layers: 8,
        estimator_layers: 4,
        model_dim: 256,
        attention_heads: 8,
        attention_head_dim: 64,
        midi_minimum: 0.0,
        midi_maximum: 128.0,
        midi_bins: 257,
        midi_deviation: 0.5,
    }
}

fn tensor_ref(tensor: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers pass a non-null tensor owned by the live weight context.
    unsafe { tensor.as_ref() }.ok_or_else(|| "GGML returned a null GAME tensor".to_string())
}

fn tensor_name(tensor: TensorPtr) -> Result<String, String> {
    let descriptor = tensor_ref(tensor)?;
    // SAFETY: GGML tensor names are fixed-size NUL-terminated arrays.
    unsafe { CStr::from_ptr(descriptor.name.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "GAME tensor name is not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_medium_configuration_is_explicit() {
        let config = expected_medium_config();
        assert_eq!(config.variant, "medium");
        assert_eq!(config.embedding_dim, 256);
        assert_eq!(config.input_dim, 80);
        assert_eq!(config.encoder_layers, 4);
        assert_eq!(config.segmenter_layers, 8);
        assert_eq!(config.estimator_layers, 4);
        assert_eq!(config.midi_bins, 257);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and GAME GGUF"]
    fn game_gguf_loads_on_explicit_device() {
        let runtime_path = std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_GGML_RUNTIME_DIR");
        let model_path = std::env::var_os("UTA_TEST_GAME_GGUF")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_GAME_GGUF");
        let requested_kind = std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND to cpu or integrated_gpu");
        let description_filter = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => crate::DeviceKind::Cpu,
            "integrated_gpu" => crate::DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description_filter)
            })
            .expect("requested GGML test device is unavailable");
        let model = Game::load(runtime, &device, &model_path).unwrap();
        assert_eq!(model.config().estimator_output_dim, 257);
        eprintln!("loaded GAME on {} ({:?})", device.description, device.kind);
    }
}
