use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::Arc;

use crate::ffi::{
    GGUF_TYPE_ARRAY, GGUF_TYPE_FLOAT32, GGUF_TYPE_FLOAT64, GGUF_TYPE_INT32, GGUF_TYPE_INT64,
    GGUF_TYPE_STRING, GGUF_TYPE_UINT32, GGUF_TYPE_UINT64, GgufInitParams, GgufPtr, ModelApi,
};
use crate::{GgmlRuntime, path_c_string};

use super::tokenizer::Tokenizer;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: the metadata reader owns a live GGUF context for every call.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Asr,
    Aligner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderGelu {
    Erf,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub kind: ModelKind,
    pub encoder_layers: usize,
    pub encoder_dim: usize,
    pub encoder_heads: usize,
    pub encoder_ffn: usize,
    pub conv_channels: usize,
    pub mel_bins: usize,
    pub mel_per_chunk: usize,
    pub encoder_window_mel: usize,
    pub encoder_output_dim: usize,
    pub encoder_gelu: EncoderGelu,
    pub decoder_layers: usize,
    pub hidden: usize,
    pub intermediate: usize,
    pub heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    pub vocab: usize,
    pub rms_epsilon: f32,
    pub rope_theta: f32,
    pub audio_start: u32,
    pub audio_end: u32,
    pub audio_pad: u32,
    pub timestamp_token: Option<u32>,
    pub timestamp_classes: Option<usize>,
    pub timestamp_millis: Option<usize>,
}

impl Config {
    fn read(metadata: &MetadataReader<'_>) -> Result<Self, String> {
        let architecture = metadata.required_string("general.architecture")?;
        let integer = |key: &str| metadata.required_usize(key);
        let float = |key: &str| metadata.required_f32(key);
        let token = |key: &str| {
            u32::try_from(integer(key)?).map_err(|_| format!("Qwen token id is outside u32: {key}"))
        };
        let config = match architecture.as_str() {
            "qwen3_asr" => Self {
                kind: ModelKind::Asr,
                encoder_layers: integer("stt.qwen3_asr.encoder.n_layers")?,
                encoder_dim: integer("stt.qwen3_asr.encoder.d_model")?,
                encoder_heads: integer("stt.qwen3_asr.encoder.n_heads")?,
                encoder_ffn: integer("stt.qwen3_asr.encoder.ffn_dim")?,
                conv_channels: integer("stt.qwen3_asr.encoder.downsample_hidden")?,
                mel_bins: integer("stt.qwen3_asr.encoder.num_mel_bins")?,
                mel_per_chunk: integer("stt.qwen3_asr.encoder.n_window")?
                    .checked_mul(2)
                    .ok_or("Qwen convolution chunk size overflow")?,
                encoder_window_mel: integer("stt.qwen3_asr.encoder.n_window_infer")?,
                encoder_output_dim: integer("stt.qwen3_asr.encoder.output_dim")?,
                encoder_gelu: EncoderGelu::Erf,
                decoder_layers: integer("stt.qwen3_asr.decoder.n_layers")?,
                hidden: integer("stt.qwen3_asr.decoder.hidden_size")?,
                intermediate: integer("stt.qwen3_asr.decoder.intermediate_size")?,
                heads: integer("stt.qwen3_asr.decoder.n_heads")?,
                kv_heads: integer("stt.qwen3_asr.decoder.n_kv_heads")?,
                head_dim: integer("stt.qwen3_asr.decoder.head_dim")?,
                vocab: integer("stt.qwen3_asr.decoder.vocab_size")?,
                rms_epsilon: float("stt.qwen3_asr.decoder.rms_norm_eps")?,
                rope_theta: float("stt.qwen3_asr.decoder.rope_theta")?,
                audio_start: token("stt.qwen3_asr.audio_start_token_id")?,
                audio_end: token("stt.qwen3_asr.audio_end_token_id")?,
                audio_pad: token("stt.qwen3_asr.audio_token_id")?,
                timestamp_token: None,
                timestamp_classes: None,
                timestamp_millis: None,
            },
            "qwen3-asr" => Self {
                kind: ModelKind::Aligner,
                encoder_layers: integer("qwen3-asr.audio.encoder.layer_count")?,
                encoder_dim: integer("qwen3-asr.audio.encoder.embedding_length")?,
                encoder_heads: integer("qwen3-asr.audio.encoder.attention.head_count")?,
                encoder_ffn: integer("qwen3-asr.audio.encoder.feed_forward_length")?,
                conv_channels: integer("qwen3-asr.audio.conv_channels")?,
                mel_bins: integer("qwen3-asr.audio.num_mel_bins")?,
                // The official aligner converter does not carry n_window=50.
                mel_per_chunk: 100,
                encoder_window_mel: 800,
                encoder_output_dim: integer("qwen3-asr.embedding_length")?,
                encoder_gelu: EncoderGelu::Erf,
                decoder_layers: integer("qwen3-asr.block_count")?,
                hidden: integer("qwen3-asr.embedding_length")?,
                intermediate: integer("qwen3-asr.feed_forward_length")?,
                heads: integer("qwen3-asr.attention.head_count")?,
                kv_heads: integer("qwen3-asr.attention.head_count_kv")?,
                head_dim: integer("qwen3-asr.attention.key_length")?,
                vocab: integer("qwen3-asr.vocab_size")?,
                rms_epsilon: float("qwen3-asr.attention.layer_norm_rms_epsilon")?,
                rope_theta: float("qwen3-asr.rope.freq_base")?,
                audio_start: token("qwen3-asr.audio.start_token_id")?,
                audio_end: token("qwen3-asr.audio.end_token_id")?,
                audio_pad: token("qwen3-asr.audio.pad_token_id")?,
                timestamp_token: Some(token("qwen3-asr.timestamp_token_id")?),
                timestamp_classes: Some(integer("qwen3-asr.classify_num")?),
                timestamp_millis: Some(integer("qwen3-asr.timestamp_segment_time")?),
            },
            other => {
                return Err(format!(
                    "Qwen native graph does not support architecture {other}"
                ));
            }
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if [
            self.encoder_dim,
            self.encoder_heads,
            self.hidden,
            self.heads,
            self.kv_heads,
            self.head_dim,
            self.mel_per_chunk,
        ]
        .contains(&0)
            || !self.encoder_dim.is_multiple_of(self.encoder_heads)
            || !self.heads.is_multiple_of(self.kv_heads)
            || self.encoder_output_dim != self.hidden
            || self.heads.checked_mul(self.head_dim).is_none()
            || self.kv_heads.checked_mul(self.head_dim).is_none()
        {
            return Err("Qwen graph dimensions cannot form attention/audio injection".into());
        }
        Ok(())
    }

    pub fn query_width(&self) -> usize {
        self.heads * self.head_dim
    }

    pub fn kv_width(&self) -> usize {
        self.kv_heads * self.head_dim
    }
}

pub struct ModelMetadata {
    pub config: Config,
    pub tokenizer: Tokenizer,
}

impl ModelMetadata {
    pub fn read(runtime: &Arc<GgmlRuntime>, model_path: &Path) -> Result<Self, String> {
        if !model_path.is_file() {
            return Err("Qwen GGUF is unavailable".to_string());
        }
        let encoded = path_c_string(model_path, "Qwen GGUF path")?;
        let api = &runtime.model_api;
        let mut tensor_context = std::ptr::null_mut();
        let gguf = ggml!(
            api,
            gguf_init_from_file(
                encoded.as_ptr(),
                GgufInitParams {
                    no_alloc: true,
                    ctx: &mut tensor_context,
                }
            )
        );
        if gguf.is_null() || tensor_context.is_null() {
            if !gguf.is_null() {
                ggml!(api, gguf_free(gguf));
            }
            if !tensor_context.is_null() {
                ggml!(api, ggml_free(tensor_context));
            }
            return Err("could not open Qwen GGUF through GGML".to_string());
        }
        let reader = MetadataReader { api, gguf };
        let result = Config::read(&reader).and_then(|config| {
            Tokenizer::from_gguf(&reader).map(|tokenizer| Self { config, tokenizer })
        });
        ggml!(api, gguf_free(gguf));
        ggml!(api, ggml_free(tensor_context));
        result
    }
}

pub(crate) struct MetadataReader<'a> {
    api: &'a ModelApi,
    gguf: GgufPtr,
}

impl MetadataReader<'_> {
    fn key_index(&self, key: &str) -> Result<i64, String> {
        let encoded = CString::new(key).map_err(|_| "Qwen GGUF key contains NUL".to_string())?;
        let index = ggml!(self.api, gguf_find_key(self.gguf, encoded.as_ptr()));
        if index < 0 {
            Err(format!("Qwen GGUF is missing {key}"))
        } else {
            Ok(index)
        }
    }

    fn required_string(&self, key: &str) -> Result<String, String> {
        let index = self.key_index(key)?;
        if ggml!(self.api, gguf_get_kv_type(self.gguf, index)) != GGUF_TYPE_STRING {
            return Err(format!("Qwen GGUF string metadata has wrong type: {key}"));
        }
        let raw = ggml!(self.api, gguf_get_val_str(self.gguf, index));
        if raw.is_null() {
            return Err(format!("Qwen GGUF string metadata is null: {key}"));
        }
        // SAFETY: GGUF owns this NUL-terminated string while the reader lives.
        unsafe { CStr::from_ptr(raw) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("Qwen GGUF string metadata is not UTF-8: {key}"))
    }

    fn required_usize(&self, key: &str) -> Result<usize, String> {
        let index = self.key_index(key)?;
        let value = match ggml!(self.api, gguf_get_kv_type(self.gguf, index)) {
            GGUF_TYPE_UINT32 => u64::from(ggml!(self.api, gguf_get_val_u32(self.gguf, index))),
            GGUF_TYPE_UINT64 => ggml!(self.api, gguf_get_val_u64(self.gguf, index)),
            GGUF_TYPE_INT32 => {
                u64::try_from(ggml!(self.api, gguf_get_val_i32(self.gguf, index)))
                    .map_err(|_| format!("Qwen GGUF integer metadata is negative: {key}"))?
            }
            GGUF_TYPE_INT64 => {
                u64::try_from(ggml!(self.api, gguf_get_val_i64(self.gguf, index)))
                    .map_err(|_| format!("Qwen GGUF integer metadata is negative: {key}"))?
            }
            _ => return Err(format!("Qwen GGUF integer metadata has wrong type: {key}")),
        };
        usize::try_from(value)
            .map_err(|_| format!("Qwen GGUF metadata exceeds host address size: {key}"))
    }

    fn required_f32(&self, key: &str) -> Result<f32, String> {
        let index = self.key_index(key)?;
        match ggml!(self.api, gguf_get_kv_type(self.gguf, index)) {
            GGUF_TYPE_FLOAT32 => Ok(ggml!(self.api, gguf_get_val_f32(self.gguf, index))),
            GGUF_TYPE_FLOAT64 => Ok(ggml!(self.api, gguf_get_val_f64(self.gguf, index)) as f32),
            _ => Err(format!("Qwen GGUF float metadata has wrong type: {key}")),
        }
    }

    pub(crate) fn string_array(&self, key: &str) -> Result<Vec<String>, String> {
        let index = self.key_index(key)?;
        if ggml!(self.api, gguf_get_kv_type(self.gguf, index)) != GGUF_TYPE_ARRAY
            || ggml!(self.api, gguf_get_arr_type(self.gguf, index)) != GGUF_TYPE_STRING
        {
            return Err(format!("Qwen GGUF string array has wrong type: {key}"));
        }
        let count = ggml!(self.api, gguf_get_arr_n(self.gguf, index));
        (0..count)
            .map(|at| {
                let raw = ggml!(self.api, gguf_get_arr_str(self.gguf, index, at));
                if raw.is_null() {
                    return Err(format!("Qwen GGUF string array contains null: {key}[{at}]"));
                }
                // SAFETY: GGUF owns each NUL-terminated string while the reader lives.
                unsafe { CStr::from_ptr(raw) }
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| format!("Qwen GGUF string array is not UTF-8: {key}[{at}]"))
            })
            .collect()
    }

    pub(crate) fn i32_array(&self, key: &str) -> Result<Vec<i32>, String> {
        let index = self.key_index(key)?;
        if ggml!(self.api, gguf_get_kv_type(self.gguf, index)) != GGUF_TYPE_ARRAY
            || ggml!(self.api, gguf_get_arr_type(self.gguf, index)) != GGUF_TYPE_INT32
        {
            return Err(format!("Qwen GGUF i32 array has wrong type: {key}"));
        }
        let count = ggml!(self.api, gguf_get_arr_n(self.gguf, index));
        let raw = ggml!(self.api, gguf_get_arr_data(self.gguf, index)).cast::<i32>();
        if raw.is_null() && count != 0 {
            return Err(format!("Qwen GGUF i32 array is null: {key}"));
        }
        // SAFETY: GGUF owns `count` contiguous i32 values while the reader lives.
        Ok(unsafe { std::slice::from_raw_parts(raw, count) }.to_vec())
    }
}

/// Three stride-2, kernel-3, pad-1 convolutions.
pub fn after_cnn_len(mut frames: usize) -> usize {
    for _ in 0..3 {
        frames = frames.div_ceil(2);
    }
    frames
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChunkGeometry {
    pub chunks: usize,
    pub rows_per_chunk: usize,
    pub padded_mel_frames: usize,
    pub final_mel_frames: usize,
    pub final_rows: usize,
    pub valid_rows: usize,
    pub padded_rows: usize,
}

impl ChunkGeometry {
    pub fn new(frames: usize, chunk: usize) -> Result<Self, String> {
        if chunk == 0 {
            return Err("Qwen convolution chunk length is zero".into());
        }
        let chunks = frames.div_ceil(chunk);
        let padded_mel_frames = frames.min(chunk);
        let rows_per_chunk = after_cnn_len(padded_mel_frames);
        let final_mel_frames = if frames == 0 {
            0
        } else {
            (frames - 1) % chunk + 1
        };
        let final_rows = after_cnn_len(final_mel_frames);
        let padded_rows = chunks
            .checked_mul(rows_per_chunk)
            .ok_or("Qwen row count overflow")?;
        let valid_rows = if chunks == 0 {
            0
        } else {
            padded_rows - rows_per_chunk + final_rows
        };
        Ok(Self {
            chunks,
            rows_per_chunk,
            padded_mel_frames,
            final_mel_frames,
            final_rows,
            valid_rows,
            padded_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolution_chunk_ragged_rows_are_not_whole_sequence_downsampling() {
        let geometry = ChunkGeometry::new(1200, 100).unwrap();
        assert_eq!(geometry.valid_rows, 156);
        assert_eq!(after_cnn_len(1200), 150);
        assert_eq!(geometry.padded_rows, 156);
        let tail = ChunkGeometry::new(1201, 100).unwrap();
        assert_eq!(tail.valid_rows, 157);
        assert_eq!(tail.padded_rows, 169);
        assert_eq!(tail.final_rows, 1);
    }

    #[test]
    fn convolution_chunk_empty_and_exact_boundaries() {
        let empty = ChunkGeometry::new(0, 100).unwrap();
        assert_eq!(
            (empty.chunks, empty.valid_rows, empty.padded_rows),
            (0, 0, 0)
        );
        assert_eq!(ChunkGeometry::new(100, 100).unwrap().final_mel_frames, 100);
        assert_eq!(ChunkGeometry::new(99, 100).unwrap().valid_rows, 13);
        let short = ChunkGeometry::new(49, 100).unwrap();
        assert_eq!(
            (
                short.padded_mel_frames,
                short.rows_per_chunk,
                short.valid_rows
            ),
            (49, 7, 7)
        );
        assert!(ChunkGeometry::new(100, 0).is_err());
        assert_eq!(after_cnn_len(usize::MAX), usize::MAX.div_ceil(8));
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime and Qwen GGUF"]
    fn actual_gguf_metadata_and_tokenizer_load() {
        let runtime_path = std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_GGML_RUNTIME_DIR");
        let model_path = std::env::var_os("UTA_TEST_QWEN_GGUF")
            .map(std::path::PathBuf::from)
            .expect("set UTA_TEST_QWEN_GGUF");
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let metadata = ModelMetadata::read(&runtime, &model_path).unwrap();
        assert_eq!(metadata.config.kind, ModelKind::Aligner);
        assert_eq!(metadata.config.encoder_layers, 24);
        assert_eq!(metadata.config.decoder_layers, 28);
        assert_eq!(metadata.config.query_width(), 2_048);
        assert_eq!(metadata.config.kv_width(), 1_024);
        assert_eq!(metadata.tokenizer.id("<timestamp>").unwrap(), 151_705);
        let multilingual = "We're 世界 123";
        assert_eq!(
            metadata
                .tokenizer
                .decode(&metadata.tokenizer.encode(multilingual).unwrap(), false)
                .unwrap(),
            multilingual
        );
    }
}
