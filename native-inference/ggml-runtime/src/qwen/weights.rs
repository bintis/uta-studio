use std::ffi::{CStr, CString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use super::model::{Config, ModelKind, ModelMetadata};
use super::tokenizer::Tokenizer;
use crate::ffi::{BufferPtr, ContextPtr, GgmlTensor, GgufInitParams, GgufPtr, ModelApi, TensorPtr};
use crate::{DeviceDescriptor, GgmlBackendHandle, GgmlRuntime, path_c_string};

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all pointers belong to this live Qwen model and GGUF load.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

/// Loaded Qwen audio encoder/decoder weights on one explicitly selected backend.
pub struct Qwen {
    pub config: Config,
    pub tokenizer: Tokenizer,
    pub(crate) backend: GgmlBackendHandle,
    pub(crate) weight_context: ContextPtr,
    weight_buffer: BufferPtr,
}

impl Drop for Qwen {
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

impl Qwen {
    pub fn load(
        runtime: Arc<GgmlRuntime>,
        device: &DeviceDescriptor,
        model_path: &Path,
    ) -> Result<Self, String> {
        let metadata = ModelMetadata::read(&runtime, model_path)?;
        let backend = runtime.create_backend(device)?;
        let mut model = Self {
            config: metadata.config,
            tokenizer: metadata.tokenizer,
            backend,
            weight_context: std::ptr::null_mut(),
            weight_buffer: std::ptr::null_mut(),
        };
        model.load_weights(model_path)?;
        Ok(model)
    }

    pub(crate) fn api(&self) -> &ModelApi {
        &self.backend.runtime.model_api
    }

    pub(crate) fn weight(&self, name: &str) -> Result<TensorPtr, String> {
        let encoded =
            CString::new(name).map_err(|_| "Qwen tensor name contains NUL".to_string())?;
        let tensor = ggml!(
            self.api(),
            ggml_get_tensor(self.weight_context, encoded.as_ptr())
        );
        if tensor.is_null() {
            Err(format!("Qwen GGUF tensor is missing: {name}"))
        } else {
            Ok(tensor)
        }
    }

    pub(crate) fn encoder_prefix(&self) -> &'static str {
        match self.config.kind {
            ModelKind::Asr => "enc",
            ModelKind::Aligner => "audio.encoder",
        }
    }

    pub(crate) fn convolution_prefix(&self, index: usize) -> String {
        match self.config.kind {
            ModelKind::Asr => format!("enc.conv.{index}"),
            ModelKind::Aligner => format!("audio.encoder.conv{}", index + 1),
        }
    }

    pub(crate) fn encoder_block_names(&self, index: usize) -> (String, [&'static str; 8]) {
        match self.config.kind {
            ModelKind::Asr => (
                format!("enc.blocks.{index}"),
                [
                    "norm_attn",
                    "attn.q",
                    "attn.k",
                    "attn.v",
                    "attn.out",
                    "norm_ffn",
                    "ffn.fc1",
                    "ffn.fc2",
                ],
            ),
            ModelKind::Aligner => (
                format!("audio.encoder.blk.{index}"),
                [
                    "attn_norm",
                    "attn_q",
                    "attn_k",
                    "attn_v",
                    "attn_out",
                    "ffn_norm",
                    "ffn_up",
                    "ffn_down",
                ],
            ),
        }
    }

    fn load_weights(&mut self, path: &Path) -> Result<(), String> {
        let encoded = path_c_string(path, "Qwen GGUF path")?;
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
            if !gguf.is_null() {
                ggml!(self.api(), gguf_free(gguf));
            }
            if !context.is_null() {
                ggml!(self.api(), ggml_free(context));
            }
            return Err("could not open Qwen GGUF weights through GGML".to_string());
        }
        self.weight_context = context;
        let result = self.load_open_gguf(path, gguf);
        ggml!(self.api(), gguf_free(gguf));
        result
    }

    fn load_open_gguf(&mut self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        self.validate_anchor_shapes()?;
        let buffer_type = ggml!(
            self.api(),
            ggml_backend_get_default_buffer_type(self.backend.raw)
        );
        self.weight_buffer = ggml!(
            self.api(),
            ggml_backend_alloc_ctx_tensors_from_buft(self.weight_context, buffer_type)
        );
        if self.weight_buffer.is_null() {
            return Err("could not allocate Qwen GGML weight buffer".to_string());
        }
        self.upload_tensors(path, gguf)
    }

    fn validate_anchor_shapes(&self) -> Result<(), String> {
        let config = &self.config;
        let conv0 = self.convolution_prefix(0);
        let conv_out = self.encoder_prefix();
        self.require_shape(
            &format!("{conv0}.weight"),
            &[3, 3, 1, config.conv_channels as i64],
        )?;
        self.require_shape(
            &format!("{conv_out}.conv_out.weight"),
            &[
                (super::model::after_cnn_len(config.mel_bins) * config.conv_channels) as i64,
                config.encoder_dim as i64,
            ],
        )?;
        let (block, names) = self.encoder_block_names(0);
        self.require_shape(
            &format!("{block}.{}.weight", names[1]),
            &[config.encoder_dim as i64, config.encoder_dim as i64],
        )?;
        self.require_shape(
            &format!("{conv_out}.proj2.weight"),
            &[config.encoder_dim as i64, config.encoder_output_dim as i64],
        )?;
        let (embedding, output_norm, first_block, query) = match config.kind {
            ModelKind::Asr => (
                "dec.token_embd.weight",
                "dec.output_norm.weight",
                "dec.blocks.0",
                "attn.q",
            ),
            ModelKind::Aligner => ("token_embd.weight", "output_norm.weight", "blk.0", "attn_q"),
        };
        self.require_shape(embedding, &[config.hidden as i64, config.vocab as i64])?;
        self.require_shape(output_norm, &[config.hidden as i64])?;
        self.require_shape(
            &format!("{first_block}.{query}.weight"),
            &[config.hidden as i64, config.query_width() as i64],
        )?;
        Ok(())
    }

    fn require_shape(&self, name: &str, expected: &[i64]) -> Result<(), String> {
        let tensor = tensor_ref(self.weight(name)?)?;
        let dimensions = tensor
            .ne
            .iter()
            .rposition(|dimension| *dimension != 1)
            .map_or(1, |last| last + 1);
        if &tensor.ne[..dimensions] != expected {
            Err(format!(
                "Qwen GGUF tensor shape mismatch: {name}; expected {expected:?}, found {:?}",
                &tensor.ne[..dimensions]
            ))
        } else {
            Ok(())
        }
    }

    fn upload_tensors(&self, path: &Path, gguf: GgufPtr) -> Result<(), String> {
        let data_offset = ggml!(self.api(), gguf_get_data_offset(gguf));
        let mut file = std::fs::File::open(path)
            .map_err(|error| format!("could not reopen Qwen GGUF: {error}"))?;
        let mut tensor = ggml!(self.api(), ggml_get_first_tensor(self.weight_context));
        let mut bytes = Vec::new();
        while !tensor.is_null() {
            let name = tensor_name(tensor)?;
            let encoded = CString::new(name.as_str())
                .map_err(|_| "Qwen tensor name contains NUL".to_string())?;
            let tensor_index = ggml!(self.api(), gguf_find_tensor(gguf, encoded.as_ptr()));
            if tensor_index < 0 {
                return Err(format!("Qwen tensor is absent from GGUF data: {name}"));
            }
            let size = ggml!(self.api(), ggml_nbytes(tensor));
            bytes.resize(size, 0);
            let offset = data_offset
                .checked_add(ggml!(
                    self.api(),
                    gguf_get_tensor_offset(gguf, tensor_index)
                ))
                .ok_or_else(|| "Qwen tensor offset overflow".to_string())?;
            file.seek(SeekFrom::Start(offset as u64))
                .map_err(|error| format!("could not seek Qwen tensor {name}: {error}"))?;
            file.read_exact(&mut bytes)
                .map_err(|error| format!("could not read Qwen tensor {name}: {error}"))?;
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
}

pub(crate) fn tensor_ref(tensor: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers pass a live GGML-owned tensor and keep its context alive.
    unsafe { tensor.as_ref() }.ok_or_else(|| "Qwen tensor pointer is null".to_string())
}

fn tensor_name(tensor: TensorPtr) -> Result<String, String> {
    let descriptor = tensor_ref(tensor)?;
    // SAFETY: GGML tensor names are inline NUL-terminated C strings.
    unsafe { CStr::from_ptr(descriptor.name.as_ptr()) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "Qwen tensor name is not UTF-8".to_string())
}
