use std::ffi::c_void;
use std::sync::Arc;

use super::encoder::EncodedAudio;
use super::model::ModelKind;
use super::weights::Qwen;
use crate::ffi::{
    AllocatorPtr, BufferPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F16,
    GGML_TYPE_F32, GGML_TYPE_I32, GgmlInitParams, GraphPtr, ModelApi, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

const GRAPH_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const SESSION_MEMORY_BYTES: usize = 4 * 1024 * 1024;
const GRAPH_NODES: usize = 8_192;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: tensors and graph objects belong to the live model/run contexts.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Clone, Debug)]
pub struct DecoderLogits {
    /// Row-major `[selected_rows, classes]` logits.
    pub values: Vec<f32>,
    pub rows: usize,
    pub classes: usize,
}

struct DecoderKv {
    key: TensorPtr,
    value: TensorPtr,
}

/// Temporary ASR generation state bound to its owning model and backend.
/// The fixed-capacity F16 KV tensors remain backend-resident across tokens.
pub struct DecoderSession<'a> {
    model: &'a Qwen,
    context: ContextPtr,
    buffer: BufferPtr,
    cache: Vec<DecoderKv>,
    capacity: usize,
    past: usize,
    failed: bool,
}

struct DecoderGraph {
    token_embedding: TensorPtr,
    audio_injected: TensorPtr,
    block_first: TensorPtr,
    block_last: TensorPtr,
    normalized: TensorPtr,
    logits: TensorPtr,
}

impl DecoderGraph {
    fn observed(&self) -> [(&'static str, TensorPtr); 6] {
        [
            ("dec.token_emb", self.token_embedding),
            ("dec.audio_injected", self.audio_injected),
            ("dec.block.0.out", self.block_first),
            ("dec.block.last.out", self.block_last),
            ("dec.out_before_head", self.normalized),
            ("aligner.timestamp_logits", self.logits),
        ]
    }
}

impl Qwen {
    /// Run the timestamp-classifier decoder over one complete causal prompt.
    /// Audio rows replace the corresponding token embeddings without changing
    /// token IDs. Incremental ASR decoding and its KV cache are a separate API.
    pub fn classify_prompt(
        &self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
        selected_rows: &[usize],
    ) -> Result<DecoderLogits, String> {
        if self.config.kind != ModelKind::Aligner {
            return Err("Qwen prompt classification requires the aligner model".to_string());
        }
        self.decode_prompt_inner(tokens, audio, selected_rows, false)
            .map(|(logits, _)| logits)
    }

    /// Decode one complete ASR prompt and return vocabulary logits for its
    /// final row. Incremental generation uses the same weights and math but a
    /// persistent KV session to avoid recomputing this prefill.
    pub fn next_token_logits(
        &self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
    ) -> Result<DecoderLogits, String> {
        if self.config.kind != ModelKind::Asr {
            return Err("Qwen next-token decoding requires the ASR model".to_string());
        }
        let selected = [tokens
            .len()
            .checked_sub(1)
            .ok_or("Qwen decoder token sequence is empty")?];
        self.decode_prompt_inner(tokens, audio, &selected, false)
            .map(|(logits, _)| logits)
    }

    /// Create a backend-resident KV session with room for the complete prompt
    /// and all generated tokens. Capacity is fixed so generation never changes
    /// backend allocation ownership mid-session.
    pub fn decoder_session(&self, capacity: usize) -> Result<DecoderSession<'_>, String> {
        if self.config.kind != ModelKind::Asr {
            return Err("Qwen decoder sessions require the ASR model".to_string());
        }
        if capacity == 0 {
            return Err("Qwen decoder session capacity must be positive".to_string());
        }
        let api = self.api();
        let context = ggml!(
            api,
            ggml_init(GgmlInitParams {
                mem_size: SESSION_MEMORY_BYTES,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate Qwen decoder session context".to_string());
        }
        let mut cache = Vec::with_capacity(self.config.decoder_layers);
        for _ in 0..self.config.decoder_layers {
            let key = ggml!(
                api,
                ggml_new_tensor_3d(
                    context,
                    GGML_TYPE_F16,
                    self.config.head_dim as i64,
                    capacity as i64,
                    self.config.kv_heads as i64
                )
            );
            let value = ggml!(
                api,
                ggml_new_tensor_3d(
                    context,
                    GGML_TYPE_F16,
                    self.config.head_dim as i64,
                    capacity as i64,
                    self.config.kv_heads as i64
                )
            );
            cache.push(DecoderKv { key, value });
        }
        let buffer_type = ggml!(api, ggml_backend_get_default_buffer_type(self.backend.raw));
        let buffer = ggml!(
            api,
            ggml_backend_alloc_ctx_tensors_from_buft(context, buffer_type)
        );
        if buffer.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate Qwen decoder session KV buffer".to_string());
        }
        Ok(DecoderSession {
            model: self,
            context,
            buffer,
            cache,
            capacity,
            past: 0,
            failed: false,
        })
    }

    fn decode_prompt_inner(
        &self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
        selected_rows: &[usize],
        observe: bool,
    ) -> Result<(DecoderLogits, Option<Vec<(&'static str, Vec<f32>)>>), String> {
        let rows = tokens.len();
        if rows == 0
            || tokens
                .iter()
                .any(|token| *token as usize >= self.config.vocab)
        {
            return Err("Qwen decoder token sequence is invalid".to_string());
        }
        if selected_rows.is_empty() || selected_rows.iter().any(|row| *row >= rows) {
            return Err("Qwen decoder selected rows are invalid".to_string());
        }
        if let Some((encoded, offset)) = audio {
            if encoded.width != self.config.hidden
                || offset
                    .checked_add(encoded.rows)
                    .is_none_or(|end| end > rows)
            {
                return Err("Qwen audio injection shape/range mismatch".to_string());
            }
        }
        let classes = match self.config.kind {
            ModelKind::Asr => self.config.vocab,
            ModelKind::Aligner => self
                .config
                .timestamp_classes
                .ok_or("Qwen aligner classifier width is missing")?,
        };
        let mut run = DecoderRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let token_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, rows as i64)
        );
        let position_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, rows as i64)
        );
        let embedding_mask = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.config.hidden as i64,
                rows as i64
            )
        );
        let embedding_override = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.config.hidden as i64,
                rows as i64
            )
        );
        let attention_mask_input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, rows as i64, rows as i64)
        );
        let selected_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, selected_rows.len() as i64)
        );
        for tensor in [
            token_input,
            position_input,
            embedding_mask,
            embedding_override,
            attention_mask_input,
            selected_input,
        ] {
            ggml!(api, ggml_set_input(tensor));
        }
        let graph = self.build_decoder_graph(
            run.context,
            token_input,
            position_input,
            embedding_mask,
            embedding_override,
            attention_mask_input,
            selected_input,
            rows,
        )?;
        let observed = graph.observed();
        ggml!(api, ggml_set_output(graph.logits));
        if observe {
            for (_, tensor) in observed {
                ggml!(api, ggml_set_output(tensor));
            }
        }
        ggml!(api, ggml_build_forward_expand(run.graph, graph.logits));

        let token_values = tokens
            .iter()
            .map(|token| i32::try_from(*token).map_err(|_| "Qwen token exceeds i32".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let positions = (0..rows)
            .map(|position| {
                i32::try_from(position).map_err(|_| "Qwen position exceeds i32".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let selected = selected_rows
            .iter()
            .map(|row| i32::try_from(*row).map_err(|_| "Qwen row exceeds i32".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut mask_values = vec![1.0_f32; rows * self.config.hidden];
        let mut override_values = vec![0.0_f32; rows * self.config.hidden];
        if let Some((encoded, offset)) = audio {
            let start = offset * self.config.hidden;
            let end = start + encoded.rows * self.config.hidden;
            mask_values[start..end].fill(0.0);
            override_values[start..end].copy_from_slice(&encoded.values);
        }
        let mut attention_values = vec![-1.0e8_f32; rows * rows];
        for query in 0..rows {
            attention_values[query * rows..query * rows + query + 1].fill(0.0);
        }

        run.allocate(&self.backend)?;
        set_i32(api, token_input, &token_values)?;
        set_i32(api, position_input, &positions)?;
        set_f32(api, embedding_mask, &mask_values)?;
        set_f32(api, embedding_override, &override_values)?;
        set_f32(api, attention_mask_input, &attention_values)?;
        set_i32(api, selected_input, &selected)?;
        run.compute(&self.backend)?;
        let values = get_f32(api, graph.logits)?;
        let expected = selected_rows
            .len()
            .checked_mul(classes)
            .ok_or("Qwen classifier output overflow")?;
        if values.len() != expected {
            return Err("Qwen classifier output shape is invalid".to_string());
        }
        let observations = observe
            .then(|| {
                observed
                    .into_iter()
                    .map(|(name, tensor)| get_f32(api, tensor).map(|values| (name, values)))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Ok((
            DecoderLogits {
                values,
                rows: selected_rows.len(),
                classes,
            },
            observations,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_decoder_graph(
        &self,
        context: ContextPtr,
        token_input: TensorPtr,
        positions: TensorPtr,
        embedding_mask: TensorPtr,
        embedding_override: TensorPtr,
        attention_mask_input: TensorPtr,
        selected_input: TensorPtr,
        rows: usize,
    ) -> Result<DecoderGraph, String> {
        let api = self.api();
        let token_embedding = ggml!(
            api,
            ggml_get_rows(
                context,
                self.weight(self.decoder_embedding_name())?,
                token_input
            )
        );
        let masked = ggml!(api, ggml_mul(context, token_embedding, embedding_mask));
        let mut hidden = ggml!(api, ggml_add(context, masked, embedding_override));
        let audio_injected = hidden;
        let attention_mask = ggml!(api, ggml_cast(context, attention_mask_input, GGML_TYPE_F16));
        let mut block_first = std::ptr::null_mut();
        for layer in 0..self.config.decoder_layers {
            hidden = self.decoder_block(context, hidden, positions, attention_mask, rows, layer)?;
            if layer == 0 {
                block_first = hidden;
            }
        }
        let block_last = hidden;
        let normalized = self.decoder_rms_norm(context, hidden, self.decoder_output_norm_name())?;
        let selected = ggml!(api, ggml_get_rows(context, normalized, selected_input));
        let logits = self.decoder_linear(context, self.decoder_head_name(), selected)?;
        Ok(DecoderGraph {
            token_embedding,
            audio_injected,
            block_first,
            block_last,
            normalized,
            logits,
        })
    }

    fn decoder_block(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        positions: TensorPtr,
        attention_mask: TensorPtr,
        rows: usize,
        layer: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let (prefix, names) = self.decoder_block_names(layer);
        let normalized =
            self.decoder_rms_norm(context, input, &format!("{prefix}.{}.weight", names[0]))?;
        let query = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[4]),
            normalized,
        )?;
        let key = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[5]),
            normalized,
        )?;
        let value = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[6]),
            normalized,
        )?;
        let query = ggml!(
            api,
            ggml_reshape_3d(
                context,
                query,
                self.config.head_dim as i64,
                self.config.heads as i64,
                rows as i64
            )
        );
        let key = ggml!(
            api,
            ggml_reshape_3d(
                context,
                key,
                self.config.head_dim as i64,
                self.config.kv_heads as i64,
                rows as i64
            )
        );
        let value = ggml!(
            api,
            ggml_reshape_3d(
                context,
                value,
                self.config.head_dim as i64,
                self.config.kv_heads as i64,
                rows as i64
            )
        );
        let query =
            self.decoder_rms_norm(context, query, &format!("{prefix}.{}.weight", names[2]))?;
        let key = self.decoder_rms_norm(context, key, &format!("{prefix}.{}.weight", names[3]))?;
        let query = self.decoder_rope(context, query, positions);
        let key = self.decoder_rope(context, key, positions);
        let layout = |tensor| {
            let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, tensor))
        };
        let query = layout(query);
        let key = ggml!(api, ggml_cast(context, layout(key), GGML_TYPE_F16));
        let value = ggml!(api, ggml_cast(context, layout(value), GGML_TYPE_F16));
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                key,
                value,
                attention_mask,
                1.0 / (self.config.head_dim as f32).sqrt(),
                0.0,
                0.0
            )
        );
        ggml!(api, ggml_flash_attn_ext_set_prec(attended, GGML_PREC_F32));
        let attended = ggml!(
            api,
            ggml_reshape_2d(
                context,
                attended,
                self.config.query_width() as i64,
                rows as i64
            )
        );
        let attended =
            self.decoder_linear(context, &format!("{prefix}.{}.weight", names[7]), attended)?;
        let residual = ggml!(api, ggml_add(context, input, attended));
        let normalized =
            self.decoder_rms_norm(context, residual, &format!("{prefix}.{}.weight", names[1]))?;
        let gate = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[8]),
            normalized,
        )?;
        let up = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[9]),
            normalized,
        )?;
        let gate = ggml!(api, ggml_silu(context, gate));
        let activated = ggml!(api, ggml_mul(context, gate, up));
        let down = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[10]),
            activated,
        )?;
        Ok(ggml!(api, ggml_add(context, residual, down)))
    }

    #[allow(clippy::too_many_arguments)]
    fn decoder_session_block(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        positions: TensorPtr,
        attention_mask: TensorPtr,
        rows: usize,
        past: usize,
        capacity: usize,
        layer: usize,
        cache: &DecoderKv,
        copies: &mut Vec<TensorPtr>,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let end = past
            .checked_add(rows)
            .ok_or("Qwen decoder session position overflow")?;
        let (prefix, names) = self.decoder_block_names(layer);
        let normalized =
            self.decoder_rms_norm(context, input, &format!("{prefix}.{}.weight", names[0]))?;
        let query = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[4]),
            normalized,
        )?;
        let key = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[5]),
            normalized,
        )?;
        let value = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[6]),
            normalized,
        )?;
        let reshape = |tensor, heads| {
            ggml!(
                api,
                ggml_reshape_3d(
                    context,
                    tensor,
                    self.config.head_dim as i64,
                    heads as i64,
                    rows as i64
                )
            )
        };
        let query = reshape(query, self.config.heads);
        let key = reshape(key, self.config.kv_heads);
        let value = reshape(value, self.config.kv_heads);
        let query =
            self.decoder_rms_norm(context, query, &format!("{prefix}.{}.weight", names[2]))?;
        let key = self.decoder_rms_norm(context, key, &format!("{prefix}.{}.weight", names[3]))?;
        let query = self.decoder_rope(context, query, positions);
        let key = self.decoder_rope(context, key, positions);
        let layout = |tensor| {
            let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, tensor))
        };
        let query = layout(query);
        let key = ggml!(api, ggml_cast(context, layout(key), GGML_TYPE_F16));
        let value = ggml!(api, ggml_cast(context, layout(value), GGML_TYPE_F16));

        let element = std::mem::size_of::<u16>();
        let row_stride = self
            .config
            .head_dim
            .checked_mul(element)
            .ok_or("Qwen KV row stride overflow")?;
        let head_stride = capacity
            .checked_mul(row_stride)
            .ok_or("Qwen KV head stride overflow")?;
        let offset = past
            .checked_mul(row_stride)
            .ok_or("Qwen KV offset overflow")?;
        let key_target = ggml!(
            api,
            ggml_view_3d(
                context,
                cache.key,
                self.config.head_dim as i64,
                rows as i64,
                self.config.kv_heads as i64,
                row_stride,
                head_stride,
                offset
            )
        );
        let value_target = ggml!(
            api,
            ggml_view_3d(
                context,
                cache.value,
                self.config.head_dim as i64,
                rows as i64,
                self.config.kv_heads as i64,
                row_stride,
                head_stride,
                offset
            )
        );
        copies.push(ggml!(api, ggml_cpy(context, key, key_target)));
        copies.push(ggml!(api, ggml_cpy(context, value, value_target)));
        let cached_key = ggml!(
            api,
            ggml_view_3d(
                context,
                cache.key,
                self.config.head_dim as i64,
                end as i64,
                self.config.kv_heads as i64,
                row_stride,
                head_stride,
                0
            )
        );
        let cached_value = ggml!(
            api,
            ggml_view_3d(
                context,
                cache.value,
                self.config.head_dim as i64,
                end as i64,
                self.config.kv_heads as i64,
                row_stride,
                head_stride,
                0
            )
        );
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                cached_key,
                cached_value,
                attention_mask,
                1.0 / (self.config.head_dim as f32).sqrt(),
                0.0,
                0.0
            )
        );
        ggml!(api, ggml_flash_attn_ext_set_prec(attended, GGML_PREC_F32));
        let attended = ggml!(
            api,
            ggml_reshape_2d(
                context,
                attended,
                self.config.query_width() as i64,
                rows as i64
            )
        );
        let attended =
            self.decoder_linear(context, &format!("{prefix}.{}.weight", names[7]), attended)?;
        let residual = ggml!(api, ggml_add(context, input, attended));
        let normalized =
            self.decoder_rms_norm(context, residual, &format!("{prefix}.{}.weight", names[1]))?;
        let gate = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[8]),
            normalized,
        )?;
        let up = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[9]),
            normalized,
        )?;
        let gate = ggml!(api, ggml_silu(context, gate));
        let activated = ggml!(api, ggml_mul(context, gate, up));
        let down = self.decoder_linear(
            context,
            &format!("{prefix}.{}.weight", names[10]),
            activated,
        )?;
        Ok(ggml!(api, ggml_add(context, residual, down)))
    }

    fn decoder_embedding_name(&self) -> &'static str {
        match self.config.kind {
            ModelKind::Asr => "dec.token_embd.weight",
            ModelKind::Aligner => "token_embd.weight",
        }
    }

    fn decoder_output_norm_name(&self) -> &'static str {
        match self.config.kind {
            ModelKind::Asr => "dec.output_norm.weight",
            ModelKind::Aligner => "output_norm.weight",
        }
    }

    fn decoder_head_name(&self) -> &'static str {
        match self.config.kind {
            ModelKind::Asr => "dec.token_embd.weight",
            ModelKind::Aligner => "output.weight",
        }
    }

    fn decoder_block_names(&self, layer: usize) -> (String, [&'static str; 11]) {
        match self.config.kind {
            ModelKind::Asr => (
                format!("dec.blocks.{layer}"),
                [
                    "norm_attn",
                    "norm_ffn",
                    "attn.q_norm",
                    "attn.k_norm",
                    "attn.q",
                    "attn.k",
                    "attn.v",
                    "attn.o",
                    "ffn.gate",
                    "ffn.up",
                    "ffn.down",
                ],
            ),
            ModelKind::Aligner => (
                format!("blk.{layer}"),
                [
                    "attn_norm",
                    "ffn_norm",
                    "attn_q_norm",
                    "attn_k_norm",
                    "attn_q",
                    "attn_k",
                    "attn_v",
                    "attn_output",
                    "ffn_gate",
                    "ffn_up",
                    "ffn_down",
                ],
            ),
        }
    }

    fn decoder_rope(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        positions: TensorPtr,
    ) -> TensorPtr {
        ggml!(
            self.api(),
            ggml_rope_ext(
                context,
                input,
                positions,
                std::ptr::null_mut(),
                self.config.head_dim as i32,
                2,
                0,
                self.config.rope_theta,
                1.0,
                0.0,
                1.0,
                0.0,
                0.0
            )
        )
    }

    fn decoder_rms_norm(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        weight_name: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = ggml!(api, ggml_rms_norm(context, input, self.config.rms_epsilon));
        let weight = ggml!(
            api,
            ggml_repeat(context, self.weight(weight_name)?, normalized)
        );
        Ok(ggml!(api, ggml_mul(context, normalized, weight)))
    }

    fn decoder_linear(
        &self,
        context: ContextPtr,
        weight_name: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let output = ggml!(
            self.api(),
            ggml_mul_mat(context, self.weight(weight_name)?, input)
        );
        ggml!(self.api(), ggml_mul_mat_set_prec(output, GGML_PREC_F32));
        Ok(output)
    }
}

impl DecoderSession<'_> {
    pub fn position(&self) -> usize {
        self.past
    }

    /// Append prompt or generated tokens and return vocabulary logits for the
    /// final appended row. Audio embedding replacement is allowed only during
    /// the initial prefill call.
    pub fn decode(
        &mut self,
        tokens: &[u32],
        audio: Option<(&EncodedAudio, usize)>,
    ) -> Result<DecoderLogits, String> {
        if self.failed {
            return Err("Qwen decoder session is unusable after a compute failure".to_string());
        }
        let rows = tokens.len();
        let end = self
            .past
            .checked_add(rows)
            .ok_or("Qwen decoder session position overflow")?;
        if rows == 0 || end > self.capacity {
            return Err("Qwen decoder session input exceeds its capacity".to_string());
        }
        if tokens
            .iter()
            .any(|token| *token as usize >= self.model.config.vocab)
        {
            return Err("Qwen decoder token sequence is invalid".to_string());
        }
        if audio.is_some() && self.past != 0 {
            return Err(
                "Qwen audio embedding replacement is only valid during prefill".to_string(),
            );
        }
        if let Some((encoded, offset)) = audio {
            if encoded.width != self.model.config.hidden
                || offset
                    .checked_add(encoded.rows)
                    .is_none_or(|audio_end| audio_end > rows)
            {
                return Err("Qwen audio injection shape/range mismatch".to_string());
            }
        }

        let mut run = DecoderRun::new(Arc::clone(&self.model.backend.runtime))?;
        let api = self.model.api();
        let token_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, rows as i64)
        );
        let position_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, rows as i64)
        );
        let embedding_mask = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.model.config.hidden as i64,
                rows as i64
            )
        );
        let embedding_override = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.model.config.hidden as i64,
                rows as i64
            )
        );
        let attention_mask_input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, end as i64, rows as i64)
        );
        let selected_input = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, 1));
        for tensor in [
            token_input,
            position_input,
            embedding_mask,
            embedding_override,
            attention_mask_input,
            selected_input,
        ] {
            ggml!(api, ggml_set_input(tensor));
        }

        let token_embedding = ggml!(
            api,
            ggml_get_rows(
                run.context,
                self.model.weight(self.model.decoder_embedding_name())?,
                token_input
            )
        );
        let masked = ggml!(api, ggml_mul(run.context, token_embedding, embedding_mask));
        let mut hidden = ggml!(api, ggml_add(run.context, masked, embedding_override));
        let attention_mask = ggml!(
            api,
            ggml_cast(run.context, attention_mask_input, GGML_TYPE_F16)
        );
        let mut copies = Vec::with_capacity(self.model.config.decoder_layers * 2);
        for (layer, cache) in self.cache.iter().enumerate() {
            hidden = self.model.decoder_session_block(
                run.context,
                hidden,
                position_input,
                attention_mask,
                rows,
                self.past,
                self.capacity,
                layer,
                cache,
                &mut copies,
            )?;
        }
        let normalized = self.model.decoder_rms_norm(
            run.context,
            hidden,
            self.model.decoder_output_norm_name(),
        )?;
        let selected = ggml!(api, ggml_get_rows(run.context, normalized, selected_input));
        let logits =
            self.model
                .decoder_linear(run.context, self.model.decoder_head_name(), selected)?;
        ggml!(api, ggml_set_output(logits));
        for copy in copies {
            ggml!(api, ggml_build_forward_expand(run.graph, copy));
        }
        ggml!(api, ggml_build_forward_expand(run.graph, logits));

        let token_values = tokens
            .iter()
            .map(|token| i32::try_from(*token).map_err(|_| "Qwen token exceeds i32".to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let positions = (self.past..end)
            .map(|position| {
                i32::try_from(position).map_err(|_| "Qwen position exceeds i32".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut mask_values = vec![1.0_f32; rows * self.model.config.hidden];
        let mut override_values = vec![0.0_f32; rows * self.model.config.hidden];
        if let Some((encoded, offset)) = audio {
            let start = offset * self.model.config.hidden;
            let audio_end = start + encoded.rows * self.model.config.hidden;
            mask_values[start..audio_end].fill(0.0);
            override_values[start..audio_end].copy_from_slice(&encoded.values);
        }
        let mut attention_values = vec![-1.0e8_f32; rows * end];
        for query in 0..rows {
            attention_values[query * end..query * end + self.past + query + 1].fill(0.0);
        }
        let selected_value =
            [i32::try_from(rows - 1).map_err(|_| "Qwen selected row exceeds i32".to_string())?];

        run.allocate(&self.model.backend)?;
        set_i32(api, token_input, &token_values)?;
        set_i32(api, position_input, &positions)?;
        set_f32(api, embedding_mask, &mask_values)?;
        set_f32(api, embedding_override, &override_values)?;
        set_f32(api, attention_mask_input, &attention_values)?;
        set_i32(api, selected_input, &selected_value)?;
        self.failed = true;
        run.compute(&self.model.backend)?;
        self.failed = false;
        self.past = end;
        let values = get_f32(api, logits)?;
        if values.len() != self.model.config.vocab {
            return Err("Qwen ASR vocabulary output shape is invalid".to_string());
        }
        Ok(DecoderLogits {
            values,
            rows: 1,
            classes: self.model.config.vocab,
        })
    }
}

impl Drop for DecoderSession<'_> {
    fn drop(&mut self) {
        let api = self.model.api();
        if !self.buffer.is_null() {
            ggml!(api, ggml_backend_buffer_free(self.buffer));
            self.buffer = std::ptr::null_mut();
        }
        if !self.context.is_null() {
            ggml!(api, ggml_free(self.context));
            self.context = std::ptr::null_mut();
        }
    }
}

struct DecoderRun {
    runtime: Arc<GgmlRuntime>,
    context: ContextPtr,
    graph: GraphPtr,
    allocator: AllocatorPtr,
}

impl DecoderRun {
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
            return Err("could not allocate Qwen decoder graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_NODES, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate Qwen decoder graph".to_string());
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
            return Err("could not allocate Qwen decoder graph".to_string());
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
                "Qwen decoder graph compute failed with status {status}"
            ))
        }
    }
}

impl Drop for DecoderRun {
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

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("Qwen decoder F32 input size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn set_i32(api: &ModelApi, tensor: TensorPtr, values: &[i32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<i32>() {
        return Err("Qwen decoder I32 input size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "Qwen decoder output element count is invalid".to_string())?;
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

    fn read_f32(path: impl AsRef<std::path::Path>) -> Vec<f32> {
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len().is_multiple_of(4));
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn causal_mask_keeps_current_and_previous_rows() {
        let rows = 4;
        let mut mask = vec![-1.0e8_f32; rows * rows];
        for query in 0..rows {
            mask[query * rows..query * rows + query + 1].fill(0.0);
        }
        assert_eq!(
            mask,
            [
                0.0, -1.0e8, -1.0e8, -1.0e8, 0.0, 0.0, -1.0e8, -1.0e8, 0.0, 0.0, 0.0, -1.0e8, 0.0,
                0.0, 0.0, 0.0,
            ]
        );
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, Qwen aligner GGUF, and historical decoder dump"]
    fn actual_aligner_decoder_matches_historical_rust_reference() {
        let path = |name: &str| {
            std::env::var_os(name)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| panic!("set {name}"))
        };
        let runtime = crate::GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let requested_kind =
            std::env::var("UTA_TEST_GGML_DEVICE_KIND").expect("set UTA_TEST_GGML_DEVICE_KIND");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => crate::DeviceKind::Cpu,
            "integrated_gpu" => crate::DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested Qwen test device is unavailable");
        let model = Qwen::load(runtime, &device, &path("UTA_TEST_QWEN_GGUF")).unwrap();
        let reference = path("UTA_TEST_QWEN_DECODER_REFERENCE_DIR");
        let audio = EncodedAudio {
            values: read_f32(reference.join("enc.proj.out.f32")),
            rows: 156,
            width: 1_024,
        };
        let mut tokens = vec![model.config.audio_start];
        tokens.extend(std::iter::repeat_n(model.config.audio_pad, audio.rows));
        tokens.push(model.config.audio_end);
        let timestamp = model.config.timestamp_token.unwrap();
        let mut selected = Vec::new();
        for word in ["All", "he", "just", "is", "for", "us"] {
            tokens.extend(model.tokenizer.encode(word).unwrap());
            selected.push(tokens.len());
            tokens.push(timestamp);
            selected.push(tokens.len());
            tokens.push(timestamp);
        }
        assert_eq!(tokens.len(), 176);
        let (actual, observations) = model
            .decode_prompt_inner(&tokens, Some((&audio, 1)), &selected, true)
            .unwrap();
        let compare = |name: &str, actual: &[f32], expected: &[f32]| {
            assert_eq!(actual.len(), expected.len(), "{name} shape");
            let differences = actual
                .iter()
                .zip(expected)
                .map(|(actual, expected)| (actual - expected).abs())
                .collect::<Vec<_>>();
            let maximum = differences.iter().copied().fold(0.0_f32, f32::max);
            let mean = differences.iter().sum::<f32>() / differences.len() as f32;
            eprintln!("Qwen {name} reference difference: max={maximum:.8} mean={mean:.8}");
            maximum
        };
        for (name, values) in observations.unwrap() {
            let expected_name = if name == "dec.block.last.out" {
                "dec.block.27.out"
            } else {
                name
            };
            let expected = read_f32(reference.join(format!("{expected_name}.f32")));
            compare(name, &values, &expected);
        }
        let expected = read_f32(reference.join("aligner.timestamp_logits.f32"));
        let maximum = compare("aligner.timestamp_logits", &actual.values, &expected);
        let argmax = |row: &[f32]| {
            row.iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .unwrap()
                .0
        };
        let classes = actual
            .values
            .chunks_exact(actual.classes)
            .map(argmax)
            .collect::<Vec<_>>();
        assert_eq!(
            classes,
            [97, 148, 148, 149, 112, 113, 115, 149, 143, 149, 149, 149]
        );
        assert!(maximum < 0.2, "Qwen decoder logit max difference {maximum}");
    }
}
