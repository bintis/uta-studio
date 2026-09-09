use std::sync::Arc;

use crate::ffi::{ContextPtr, GGML_TYPE_F32, GGML_TYPE_I32, TensorPtr};

use super::encoder::{GraphRun, get_f32, set_f32, set_i32};
use super::weights::tensor_ref;
use super::{
    D_K, D_MODEL, ENCODER_FRAMES, EOS, EncodedAudio, FireRed, MAX_GENERATED_TOKENS, N_HEAD,
    N_LAYERS_DEC, SOS, VOCAB_SIZE,
};

const EPSILON: f32 = 1.0e-5;

struct DecoderGraph {
    logits: TensorPtr,
    #[cfg(test)]
    stages: Vec<(String, TensorPtr)>,
}

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: graph tensors and the model backend remain live for this call.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

impl FireRed {
    pub fn greedy_decode(&self, encoded: &EncodedAudio) -> Result<Vec<u32>, String> {
        if encoded.rows != ENCODER_FRAMES
            || encoded.width != D_MODEL
            || encoded.values.len() != ENCODER_FRAMES * D_MODEL
            || encoded.values.iter().any(|value| !value.is_finite())
        {
            return Err("FireRed decoder received invalid encoder output".to_string());
        }
        let mut tokens = vec![SOS];
        for _ in 0..MAX_GENERATED_TOKENS {
            let logits = self.decode_last_logits(&tokens, encoded)?;
            let token = logits
                .iter()
                .copied()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(index, _)| index as u32)
                .ok_or_else(|| "FireRed decoder returned no logits".to_string())?;
            tokens.push(token);
            if token == EOS {
                break;
            }
        }
        Ok(tokens)
    }

    pub fn decode_last_logits(
        &self,
        tokens: &[u32],
        encoded: &EncodedAudio,
    ) -> Result<Vec<f32>, String> {
        if tokens.is_empty()
            || tokens.len() > MAX_GENERATED_TOKENS
            || tokens.iter().any(|token| *token as usize >= VOCAB_SIZE)
        {
            return Err("FireRed decoder tokens are invalid".to_string());
        }
        if encoded.rows != ENCODER_FRAMES
            || encoded.width != D_MODEL
            || encoded.values.len() != ENCODER_FRAMES * D_MODEL
        {
            return Err("FireRed decoder received invalid encoder output".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let token_input = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, tokens.len() as i64)
        );
        let encoder_input = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                D_MODEL as i64,
                ENCODER_FRAMES as i64
            )
        );
        ggml!(api, ggml_set_input(token_input));
        ggml!(api, ggml_set_input(encoder_input));
        let graph =
            self.build_decoder_graph(run.context, token_input, encoder_input, tokens.len())?;
        ggml!(api, ggml_set_output(graph.logits));
        #[cfg(test)]
        for (_, tensor) in &graph.stages {
            ggml!(api, ggml_set_output(*tensor));
        }
        ggml!(api, ggml_build_forward_expand(run.graph, graph.logits));
        run.allocate(&self.backend)?;
        let token_values: Vec<i32> = tokens.iter().map(|token| *token as i32).collect();
        set_i32(api, token_input, &token_values)?;
        set_f32(api, encoder_input, &encoded.values)?;
        run.compute(&self.backend)?;
        #[cfg(test)]
        if tokens.len() == 1 {
            if let Some(directory) = std::env::var_os("UTA_TEST_FIRERED_DECODER_DEBUG_DIR") {
                for (name, tensor) in &graph.stages {
                    let values = get_f32(api, *tensor)?;
                    let bytes: Vec<u8> = values
                        .iter()
                        .flat_map(|value| value.to_le_bytes())
                        .collect();
                    std::fs::write(std::path::PathBuf::from(&directory).join(name), bytes)
                        .map_err(|error| {
                            format!("could not write FireRed decoder diagnostic: {error}")
                        })?;
                }
            }
        }
        let logits = get_f32(api, graph.logits)?;
        if logits.len() != VOCAB_SIZE || logits.iter().any(|value| !value.is_finite()) {
            return Err("FireRed decoder produced invalid logits".to_string());
        }
        Ok(logits)
    }

    fn build_decoder_graph(
        &self,
        context: ContextPtr,
        token_input: TensorPtr,
        encoder: TensorPtr,
        rows: usize,
    ) -> Result<DecoderGraph, String> {
        let api = self.api();
        #[cfg(test)]
        let mut stages = Vec::new();
        let embedding = self.weight("decoder.tgt_word_emb.weight")?;
        let mut hidden = ggml!(api, ggml_get_rows(context, embedding, token_input));
        hidden = ggml!(
            api,
            ggml_scale_bias(context, hidden, (D_MODEL as f32).sqrt(), 0.0)
        );
        let position = self.weight("decoder.positional_encoding.pe")?;
        let descriptor = tensor_ref(position)?;
        if descriptor.ne[0] != D_MODEL as i64 || descriptor.ne[1] < rows as i64 {
            return Err("FireRed decoder positional encoding shape is invalid".to_string());
        }
        let position = ggml!(
            api,
            ggml_view_2d(
                context,
                position,
                D_MODEL as i64,
                rows as i64,
                descriptor.nb[1],
                0
            )
        );
        hidden = ggml!(api, ggml_add(context, hidden, position));
        #[cfg(test)]
        stages.push(("decoder-embed.f32le".to_string(), hidden));
        for layer in 0..N_LAYERS_DEC {
            let (next, _self_output, _cross_output) =
                self.decoder_layer(context, hidden, encoder, rows, layer)?;
            hidden = next;
            #[cfg(test)]
            {
                if layer == 0 {
                    stages.push(("decoder-layer-0-self.f32le".to_string(), _self_output));
                    stages.push(("decoder-layer-0-cross.f32le".to_string(), _cross_output));
                }
                stages.push((format!("decoder-layer-{layer}.f32le"), hidden));
            }
        }
        hidden = self.decoder_layer_norm(context, hidden, "decoder.layer_norm_out")?;
        #[cfg(test)]
        stages.push(("decoder-normalized.f32le".to_string(), hidden));
        let stride = tensor_ref(hidden)?.nb[1];
        let last = ggml!(
            api,
            ggml_view_2d(
                context,
                hidden,
                D_MODEL as i64,
                1,
                stride,
                (rows - 1) * stride
            )
        );
        let logits = self.decoder_linear(context, "decoder.tgt_word_prj", last, false)?;
        #[cfg(test)]
        stages.push(("decoder-logits.f32le".to_string(), logits));
        Ok(DecoderGraph {
            logits,
            #[cfg(test)]
            stages,
        })
    }

    fn decoder_layer(
        &self,
        context: ContextPtr,
        mut hidden: TensorPtr,
        encoder: TensorPtr,
        rows: usize,
        layer: usize,
    ) -> Result<(TensorPtr, TensorPtr, TensorPtr), String> {
        let api = self.api();
        let prefix = format!("decoder.layer_stack.{layer}");
        let normed =
            self.decoder_layer_norm(context, hidden, &format!("{prefix}.self_attn_norm"))?;
        let attended = self.decoder_attention(
            context,
            normed,
            normed,
            rows,
            rows,
            &format!("{prefix}.self_attn"),
        )?;
        hidden = ggml!(api, ggml_add(context, hidden, attended));
        let self_output = hidden;

        let normed =
            self.decoder_layer_norm(context, hidden, &format!("{prefix}.cross_attn_norm"))?;
        let attended = self.decoder_attention(
            context,
            normed,
            encoder,
            rows,
            ENCODER_FRAMES,
            &format!("{prefix}.cross_attn"),
        )?;
        hidden = ggml!(api, ggml_add(context, hidden, attended));
        let cross_output = hidden;

        let normed = self.decoder_layer_norm(context, hidden, &format!("{prefix}.mlp_norm"))?;
        let expanded = self.decoder_linear(context, &format!("{prefix}.mlp.w_1"), normed, true)?;
        let expanded = ggml!(api, ggml_gelu_erf(context, expanded));
        let projected =
            self.decoder_linear(context, &format!("{prefix}.mlp.w_2"), expanded, true)?;
        Ok((
            ggml!(api, ggml_add(context, hidden, projected)),
            self_output,
            cross_output,
        ))
    }

    fn decoder_attention(
        &self,
        context: ContextPtr,
        query_input: TensorPtr,
        key_value_input: TensorPtr,
        query_rows: usize,
        key_value_rows: usize,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let query = self.decoder_linear(context, &format!("{prefix}.w_qs"), query_input, true)?;
        let key =
            self.decoder_linear(context, &format!("{prefix}.w_ks"), key_value_input, false)?;
        let value =
            self.decoder_linear(context, &format!("{prefix}.w_vs"), key_value_input, true)?;
        let query = decoder_heads(self, context, query, query_rows);
        let key = decoder_heads(self, context, key, key_value_rows);
        let value = decoder_heads(self, context, value, key_value_rows);
        let scores = ggml!(api, ggml_mul_mat(context, key, query));
        let scores = ggml!(
            api,
            ggml_scale_bias(context, scores, 1.0 / (D_K as f32).sqrt(), 0.0)
        );
        let attention = ggml!(api, ggml_soft_max(context, scores));
        let value = ggml!(api, ggml_permute(context, value, 1, 0, 2, 3));
        let value = ggml!(api, ggml_cont(context, value));
        let attended = ggml!(api, ggml_mul_mat(context, attention, value));
        let attended = ggml!(api, ggml_permute(context, attended, 2, 0, 1, 3));
        let attended = ggml!(api, ggml_cont(context, attended));
        let attended = ggml!(
            api,
            ggml_reshape_2d(context, attended, D_MODEL as i64, query_rows as i64)
        );
        self.decoder_linear(context, &format!("{prefix}.fc"), attended, true)
    }

    fn decoder_layer_norm(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = ggml!(api, ggml_norm(context, input, EPSILON));
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let weight = ggml!(api, ggml_repeat(context, weight, normalized));
        let normalized = ggml!(api, ggml_mul(context, normalized, weight));
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_repeat(context, bias, normalized));
        Ok(ggml!(api, ggml_add(context, normalized, bias)))
    }

    fn decoder_linear(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        has_bias: bool,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let input_dimension = tensor_ref(input)?.ne[0];
        let weight = self.weight(&format!("{prefix}.weight"))?;
        if tensor_ref(weight)?.ne[0] != input_dimension {
            return Err(format!(
                "FireRed linear input mismatch for {prefix}: expected {}, found {input_dimension}",
                tensor_ref(weight)?.ne[0]
            ));
        }
        let mut output = ggml!(api, ggml_mul_mat(context, weight, input));
        if has_bias {
            let bias = self.weight(&format!("{prefix}.bias"))?;
            let bias = ggml!(api, ggml_repeat(context, bias, output));
            output = ggml!(api, ggml_add(context, output, bias));
        }
        Ok(output)
    }
}

fn decoder_heads(
    model: &FireRed,
    context: ContextPtr,
    tensor: TensorPtr,
    rows: usize,
) -> TensorPtr {
    let api = model.api();
    let tensor = ggml!(
        api,
        ggml_reshape_3d(context, tensor, D_K as i64, N_HEAD as i64, rows as i64)
    );
    let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
    ggml!(api, ggml_cont(context, tensor))
}
