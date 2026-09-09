use std::sync::Arc;

use super::model::{HIDDEN_DIM, Stars, tensor_ref};
use super::stage_a::{GraphRun, get_f32, set_f32};
use crate::ffi::{ContextPtr, GGML_PREC_F32, GGML_TYPE_F32, TensorPtr};

const LAYER_NORM_EPSILON: f32 = 1.0e-5;
const LEAKY_RELU_SLOPE: f32 = 0.01;
const HEADS: i64 = 4;
const HEAD_DIM: i64 = 64;
const SENTENCE_HEADS: i64 = 2;
const SENTENCE_HEAD_DIM: i64 = 128;
const SENTENCE_TOKENS: usize = 16;
const POOL_AVG: u32 = 1;
const RELATIVE_POSITION_MAX_LEN: usize = 5_000;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: pointers belong to the live STARS model and graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Clone, Copy)]
enum ResidualActivation {
    LeakyRelu,
    Silu,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UtteranceEncoding {
    /// Frame-major `[frames, 256]` utterance feature consumed by Stage B.
    pub features: Vec<f32>,
    pub boundary_probabilities: Vec<f32>,
    /// Frame-major `[frames, 61]` phoneme logits, excluding the boundary channel.
    pub phoneme_logits: Vec<f32>,
    pub frames: usize,
}

impl Stars {
    /// Runs the Stage A utterance U-Net, Conformer bottleneck, and phoneme head.
    ///
    /// `embedded` is frame-major `[frames, 256]`, as returned by
    /// [`Stars::encode_mel_with_pitch`]. Neural operations execute through the
    /// explicitly selected upstream-GGML backend; Rust supplies only masks,
    /// positional values, and output decoding.
    pub fn encode_utterance(
        &self,
        embedded: &[f32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<UtteranceEncoding, String> {
        if frames == 0
            || frames % 16 != 0
            || valid_frames > frames
            || embedded.len() != frames * HIDDEN_DIM
        {
            return Err("STARS utterance input shape is invalid".to_string());
        }
        if embedded.iter().any(|value| !value.is_finite()) {
            return Err("STARS utterance input contains non-finite values".to_string());
        }

        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let initial_mask = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let bottleneck_frames = frames / 16;
        let relative_positions = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                HIDDEN_DIM as i64,
                bottleneck_frames as i64
            )
        );
        let absolute_positions = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(initial_mask));
        ggml!(api, ggml_set_input(relative_positions));
        ggml!(api, ggml_set_input(absolute_positions));

        let utterance = self.local_style_utterance(
            run.context,
            input,
            initial_mask,
            relative_positions,
            frames,
        )?;
        let combined = ggml!(
            api,
            ggml_concat(run.context, utterance, absolute_positions, 0)
        );
        let features = self.linear(run.context, "l1_utter", combined, true)?;
        let logits = self.linear(run.context, "ph_frame_predictor.ph_head", features, true)?;
        ggml!(api, ggml_set_output(features));
        ggml!(api, ggml_set_output(logits));
        ggml!(api, ggml_build_forward_expand(run.graph, logits));
        run.allocate(&self.backend)?;

        set_f32(api, input, embedded)?;
        let mut mask = vec![0.0_f32; frames * HIDDEN_DIM];
        mask[..valid_frames * HIDDEN_DIM].fill(1.0);
        set_f32(api, initial_mask, &mask)?;
        set_f32(
            api,
            relative_positions,
            &relative_position_values(bottleneck_frames),
        )?;
        set_f32(
            api,
            absolute_positions,
            &absolute_position_values(valid_frames, frames),
        )?;
        run.compute(&self.backend)?;

        let features = get_f32(api, features)?;
        let raw_logits = get_f32(api, logits)?;
        if raw_logits.len() != frames * 62 {
            return Err("STARS phoneme-head output shape is invalid".to_string());
        }
        let mut boundary_probabilities = Vec::with_capacity(frames);
        let mut phoneme_logits = Vec::with_capacity(frames * 61);
        for row in raw_logits.chunks_exact(62) {
            boundary_probabilities.push(sigmoid(row[0].clamp(-16.0, 16.0)));
            phoneme_logits.extend_from_slice(&row[1..]);
        }
        Ok(UtteranceEncoding {
            features,
            boundary_probabilities,
            phoneme_logits,
            frames,
        })
    }

    pub(super) fn encode_local_style_frames(
        &self,
        embedded: &[f32],
        valid_frames: usize,
        frames: usize,
        adaptor: &str,
    ) -> Result<Vec<f32>, String> {
        self.encode_local_style_adaptor_frames(embedded, valid_frames, frames, adaptor, false)
    }

    pub(super) fn encode_sentence_style_frames(
        &self,
        embedded: &[f32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<Vec<f32>, String> {
        self.encode_local_style_adaptor_frames(
            embedded,
            valid_frames,
            frames,
            "prosody_extractor_sentence",
            true,
        )
    }

    fn encode_local_style_adaptor_frames(
        &self,
        embedded: &[f32],
        valid_frames: usize,
        frames: usize,
        adaptor: &str,
        include_encoder: bool,
    ) -> Result<Vec<f32>, String> {
        if frames == 0
            || frames % 16 != 0
            || valid_frames > frames
            || embedded.len() != frames * HIDDEN_DIM
            || embedded.iter().any(|value| !value.is_finite())
        {
            return Err("STARS local-style input shape is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let initial_mask = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let bottleneck_frames = frames / 16;
        let relative_positions = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                HIDDEN_DIM as i64,
                bottleneck_frames as i64
            )
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(initial_mask));
        ggml!(api, ggml_set_input(relative_positions));
        let mut output = self.local_style_cmu(
            run.context,
            input,
            initial_mask,
            relative_positions,
            frames,
            adaptor,
            if include_encoder { 1 } else { 2 },
        )?;
        if include_encoder {
            output = self.conv_blocks(run.context, &format!("{adaptor}.encoder"), output)?;
        }
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, input, embedded)?;
        let mut mask = vec![0.0_f32; frames * HIDDEN_DIM];
        mask[..valid_frames * HIDDEN_DIM].fill(1.0);
        set_f32(api, initial_mask, &mask)?;
        set_f32(
            api,
            relative_positions,
            &relative_position_values(bottleneck_frames),
        )?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    pub(super) fn encode_conv_blocks_values(
        &self,
        input: &[f32],
        rows: usize,
        prefix: &str,
    ) -> Result<Vec<f32>, String> {
        self.encode_conv_blocks_values_with_activation(
            input,
            rows,
            prefix,
            ResidualActivation::LeakyRelu,
        )
    }

    pub(super) fn encode_conv_blocks_silu_values(
        &self,
        input: &[f32],
        rows: usize,
        prefix: &str,
    ) -> Result<Vec<f32>, String> {
        self.encode_conv_blocks_values_with_activation(
            input,
            rows,
            prefix,
            ResidualActivation::Silu,
        )
    }

    fn encode_conv_blocks_values_with_activation(
        &self,
        input: &[f32],
        rows: usize,
        prefix: &str,
        activation: ResidualActivation,
    ) -> Result<Vec<f32>, String> {
        if rows == 0
            || input.len() != rows * HIDDEN_DIM
            || input.iter().any(|value| !value.is_finite())
        {
            return Err("STARS ConvBlocks input shape is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let tensor = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, rows as i64)
        );
        let mask = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, rows as i64)
        );
        ggml!(api, ggml_set_input(tensor));
        ggml!(api, ggml_set_input(mask));
        let mut output = self.residual_block_with_activation(
            run.context,
            &format!("{prefix}.res_blocks.0.blocks.0"),
            tensor,
            activation,
        )?;
        output = ggml!(api, ggml_mul(run.context, output, mask));
        output = self.layer_norm_features(run.context, &format!("{prefix}.last_norm"), output)?;
        output = ggml!(api, ggml_mul(run.context, output, mask));
        output = self.conv_features(run.context, &format!("{prefix}.post_net1"), output, 1)?;
        output = ggml!(api, ggml_mul(run.context, output, mask));
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, tensor, input)?;
        let mut mask_values = vec![0.0_f32; input.len()];
        for row in 0..rows {
            if input[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM]
                .iter()
                .any(|value| value.abs() > 0.0)
            {
                mask_values[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM].fill(1.0);
            }
        }
        set_f32(api, mask, &mask_values)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    pub(super) fn project_linear_values(
        &self,
        input: &[f32],
        rows: usize,
        input_dimensions: usize,
        output_dimensions: usize,
        prefix: &str,
    ) -> Result<Vec<f32>, String> {
        self.project_values(
            input,
            rows,
            input_dimensions,
            output_dimensions,
            None,
            prefix,
        )
    }

    pub(super) fn project_normalized_linear_values(
        &self,
        input: &[f32],
        output_dimensions: usize,
        norm_prefix: &str,
        linear_prefix: &str,
    ) -> Result<Vec<f32>, String> {
        self.project_values(
            input,
            1,
            HIDDEN_DIM,
            output_dimensions,
            Some(norm_prefix),
            linear_prefix,
        )
    }

    fn project_values(
        &self,
        input: &[f32],
        rows: usize,
        input_dimensions: usize,
        output_dimensions: usize,
        norm_prefix: Option<&str>,
        linear_prefix: &str,
    ) -> Result<Vec<f32>, String> {
        if rows == 0
            || input_dimensions == 0
            || output_dimensions == 0
            || input.len() != rows * input_dimensions
            || input.iter().any(|value| !value.is_finite())
        {
            return Err("STARS linear input shape is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let tensor = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                input_dimensions as i64,
                rows as i64
            )
        );
        ggml!(api, ggml_set_input(tensor));
        let projected_input = if let Some(prefix) = norm_prefix {
            self.layer_norm_features(run.context, prefix, tensor)?
        } else {
            tensor
        };
        let output = self.linear(run.context, linear_prefix, projected_input, true)?;
        ensure_shape(
            output,
            output_dimensions as i64,
            rows as i64,
            "linear output",
        )?;
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, tensor, input)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    pub(super) fn align_sentence_values(
        &self,
        features: &[f32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<Vec<f32>, String> {
        if frames == 0
            || valid_frames == 0
            || valid_frames > frames
            || features.len() != frames * HIDDEN_DIM
            || features.iter().any(|value| !value.is_finite())
        {
            return Err("STARS sentence-alignment input shape is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let feature_tensor = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let attention_mask = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                frames as i64,
                SENTENCE_TOKENS as i64
            )
        );
        ggml!(api, ggml_set_input(feature_tensor));
        ggml!(api, ggml_set_input(attention_mask));
        let mut output = self.weight("cls_tokens")?;
        for layer in 0..2 {
            output = self.sentence_alignment_layer(
                run.context,
                &format!("align_sentence.layers.{layer}"),
                output,
                feature_tensor,
                attention_mask,
                frames,
            )?;
        }
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, feature_tensor, features)?;
        let mut mask = vec![0.0_f32; frames * SENTENCE_TOKENS];
        for token in 0..SENTENCE_TOKENS {
            mask[token * frames + valid_frames..(token + 1) * frames].fill(-1.0e8);
        }
        set_f32(api, attention_mask, &mask)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    fn sentence_alignment_layer(
        &self,
        context: ContextPtr,
        prefix: &str,
        query: TensorPtr,
        features: TensorPtr,
        attention_mask: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let attended = self.sentence_cross_attention(
            context,
            &format!("{prefix}.multihead_attn"),
            query,
            features,
            attention_mask,
            frames,
        )?;
        let current = ggml!(api, ggml_add(context, query, attended));
        let current = self.layer_norm_features(context, &format!("{prefix}.norm1"), current)?;
        let expanded = self.linear(context, &format!("{prefix}.linear1"), current, true)?;
        let activated = ggml!(api, ggml_relu(context, expanded));
        let projected = self.linear(context, &format!("{prefix}.linear2"), activated, true)?;
        let current = ggml!(api, ggml_add(context, current, projected));
        self.layer_norm_features(context, &format!("{prefix}.norm2"), current)
    }

    fn sentence_cross_attention(
        &self,
        context: ContextPtr,
        prefix: &str,
        query: TensorPtr,
        features: TensorPtr,
        attention_mask: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let projection_weight = self.weight(&format!("{prefix}.in_proj_weight"))?;
        let projection_bias = self.weight(&format!("{prefix}.in_proj_bias"))?;
        let query_projection = ggml!(api, ggml_mul_mat(context, projection_weight, query));
        let query_projection = ggml!(api, ggml_add(context, query_projection, projection_bias));
        let feature_projection = ggml!(api, ggml_mul_mat(context, projection_weight, features));
        let feature_projection = ggml!(api, ggml_add(context, feature_projection, projection_bias));
        let query_stride = tensor_ref(query_projection)?.nb[1];
        let feature_stride = tensor_ref(feature_projection)?.nb[1];
        let segment_bytes = HIDDEN_DIM * size_of::<f32>();
        let query_view = ggml!(
            api,
            ggml_view_2d(
                context,
                query_projection,
                HIDDEN_DIM as i64,
                SENTENCE_TOKENS as i64,
                query_stride,
                0
            )
        );
        let key_view = ggml!(
            api,
            ggml_view_2d(
                context,
                feature_projection,
                HIDDEN_DIM as i64,
                frames as i64,
                feature_stride,
                segment_bytes
            )
        );
        let value_view = ggml!(
            api,
            ggml_view_2d(
                context,
                feature_projection,
                HIDDEN_DIM as i64,
                frames as i64,
                feature_stride,
                segment_bytes * 2
            )
        );
        let query_view = ggml!(api, ggml_cont(context, query_view));
        let key_view = ggml!(api, ggml_cont(context, key_view));
        let value_view = ggml!(api, ggml_cont(context, value_view));
        let query_heads = ggml!(
            api,
            ggml_reshape_3d(
                context,
                query_view,
                SENTENCE_HEAD_DIM,
                SENTENCE_HEADS,
                SENTENCE_TOKENS as i64
            )
        );
        let query_heads = ggml!(api, ggml_permute(context, query_heads, 0, 2, 1, 3));
        let query_heads = ggml!(api, ggml_cont(context, query_heads));
        let key_heads = ggml!(
            api,
            ggml_reshape_3d(
                context,
                key_view,
                SENTENCE_HEAD_DIM,
                SENTENCE_HEADS,
                frames as i64
            )
        );
        let key_heads = ggml!(api, ggml_permute(context, key_heads, 0, 2, 1, 3));
        let key_heads = ggml!(api, ggml_cont(context, key_heads));
        let value_heads = ggml!(
            api,
            ggml_reshape_3d(
                context,
                value_view,
                SENTENCE_HEAD_DIM,
                SENTENCE_HEADS,
                frames as i64
            )
        );
        let value_heads = ggml!(api, ggml_permute(context, value_heads, 0, 2, 1, 3));
        let value_heads = ggml!(api, ggml_cont(context, value_heads));
        let scores = ggml!(api, ggml_mul_mat(context, key_heads, query_heads));
        let scores = ggml!(
            api,
            ggml_scale_bias(context, scores, (SENTENCE_HEAD_DIM as f32).powf(-0.5), 0.0)
        );
        let scores = ggml!(api, ggml_add(context, scores, attention_mask));
        let probabilities = ggml!(api, ggml_soft_max(context, scores));
        let value_transposed = ggml!(api, ggml_transpose(context, value_heads));
        let value_transposed = ggml!(api, ggml_cont(context, value_transposed));
        let attended = ggml!(api, ggml_mul_mat(context, probabilities, value_transposed));
        let attended = ggml!(api, ggml_permute(context, attended, 2, 0, 1, 3));
        let attended = ggml!(api, ggml_cont(context, attended));
        let attended = ggml!(
            api,
            ggml_reshape_2d(context, attended, HIDDEN_DIM as i64, SENTENCE_TOKENS as i64)
        );
        self.linear(context, &format!("{prefix}.out_proj"), attended, true)
    }

    fn local_style_utterance(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        initial_mask: TensorPtr,
        relative_positions: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let current = self.local_style_cmu(
            context,
            input,
            initial_mask,
            relative_positions,
            frames,
            "prosody_extractor_utter",
            2,
        )?;
        self.conv_blocks(context, "prosody_extractor_utter.encoder", current)
    }

    fn local_style_cmu(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        initial_mask: TensorPtr,
        relative_positions: TensorPtr,
        frames: usize,
        adaptor: &str,
        conformer_layers: usize,
    ) -> Result<TensorPtr, String> {
        let prefix = format!("{adaptor}.cmuencoder.net");
        let mut current = input;
        let mut current_frames = frames;
        let mut skips = Vec::with_capacity(4);
        for stage in 0..4 {
            let stage_prefix = format!("{prefix}.down.layers.{stage}");
            current =
                self.residual_block(context, &format!("{stage_prefix}.0.blocks.0"), current)?;
            if stage == 0 {
                current = ggml!(self.api(), ggml_mul(context, current, initial_mask));
            }
            current = self.conv_features(context, &format!("{stage_prefix}.1"), current, 1)?;
            current =
                self.residual_block(context, &format!("{stage_prefix}.2.blocks.0"), current)?;
            skips.push(current);
            current = self.average_pool_time(context, current)?;
            current_frames /= 2;
        }
        current =
            self.layer_norm_features(context, &format!("{prefix}.down.last_norm"), current)?;
        current = self.conv_features(context, &format!("{prefix}.down.post_net"), current, 1)?;
        current = self.conv_features(context, &format!("{prefix}.mid.pre"), current, 1)?;
        let conformer = self.conformer(
            context,
            &format!("{prefix}.mid.net"),
            current,
            relative_positions,
            current_frames,
            conformer_layers,
        )?;
        current = self.conv_features(context, &format!("{prefix}.mid.post"), conformer, 1)?;

        for stage in 0..4 {
            let up_prefix = format!("{prefix}.up");
            current = self.conv_transpose_features(
                context,
                &format!("{up_prefix}.ups.{stage}.0"),
                current,
                current_frames,
            )?;
            current_frames *= 2;
            current =
                self.layer_norm_features(context, &format!("{up_prefix}.ups.{stage}.1"), current)?;
            current = ggml!(
                self.api(),
                ggml_leaky_relu(context, current, LEAKY_RELU_SLOPE, false)
            );
            let skip = skips[3 - stage];
            ensure_shape(skip, HIDDEN_DIM as i64, current_frames as i64, "U-Net skip")?;
            let merged = ggml!(self.api(), ggml_concat(context, current, skip, 0));
            current =
                self.conv_features(context, &format!("{up_prefix}.layers.{stage}.0"), merged, 1)?;
            current = self.residual_block(
                context,
                &format!("{up_prefix}.layers.{stage}.1.blocks.0"),
                current,
            )?;
        }
        current = self.layer_norm_features(context, &format!("{prefix}.up.last_norm"), current)?;
        current = self.conv_features(context, &format!("{prefix}.up.post_net"), current, 1)?;

        Ok(current)
    }

    fn conv_blocks(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let mut current =
            self.residual_block(context, &format!("{prefix}.res_blocks.0.blocks.0"), input)?;
        current = self.layer_norm_features(context, &format!("{prefix}.last_norm"), current)?;
        self.conv_features(context, &format!("{prefix}.post_net1"), current, 1)
    }

    fn conformer(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        positions: TensorPtr,
        frames: usize,
        layers: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let mut current = ggml!(
            api,
            ggml_scale_bias(context, input, (HIDDEN_DIM as f32).sqrt(), 0.0)
        );
        for layer in 0..layers {
            let layer_prefix = format!("{prefix}.encoder_layers.{layer}");
            let normalized = self.layer_norm_features(
                context,
                &format!("{layer_prefix}.norm_ff_macaron"),
                current,
            )?;
            let branch = self.feed_forward_moe(
                context,
                &format!("{layer_prefix}.feed_forward_macaron"),
                normalized,
                frames,
            )?;
            let branch = ggml!(api, ggml_scale_bias(context, branch, 0.5, 0.0));
            current = ggml!(api, ggml_add(context, current, branch));

            let normalized =
                self.layer_norm_features(context, &format!("{layer_prefix}.norm_mha"), current)?;
            let branch = self.relative_attention(
                context,
                &format!("{layer_prefix}.self_attn"),
                normalized,
                positions,
                frames,
            )?;
            current = ggml!(api, ggml_add(context, current, branch));

            let normalized =
                self.layer_norm_features(context, &format!("{layer_prefix}.norm_conv"), current)?;
            let branch = self.convolution_module(
                context,
                &format!("{layer_prefix}.conv_module"),
                normalized,
                frames,
            )?;
            current = ggml!(api, ggml_add(context, current, branch));

            let normalized =
                self.layer_norm_features(context, &format!("{layer_prefix}.norm_ff"), current)?;
            let branch = self.feed_forward_moe(
                context,
                &format!("{layer_prefix}.feed_forward"),
                normalized,
                frames,
            )?;
            let branch = ggml!(api, ggml_scale_bias(context, branch, 0.5, 0.0));
            current = ggml!(api, ggml_add(context, current, branch));
            current =
                self.layer_norm_features(context, &format!("{layer_prefix}.norm_final"), current)?;
        }
        self.layer_norm_features(context, &format!("{prefix}.layer_norm"), current)
    }

    fn relative_attention(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        positions: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let q = self.linear(context, &format!("{prefix}.linear_q"), input, true)?;
        let k = self.linear(context, &format!("{prefix}.linear_k"), input, true)?;
        let v = self.linear(context, &format!("{prefix}.linear_v"), input, true)?;
        let p = self.linear(context, &format!("{prefix}.linear_pos"), positions, false)?;
        let q = ggml!(
            api,
            ggml_reshape_3d(context, q, HEAD_DIM, HEADS, frames as i64)
        );
        let bias_u = self.weight(&format!("{prefix}.pos_bias_u"))?;
        let bias_v = self.weight(&format!("{prefix}.pos_bias_v"))?;
        let q_u = ggml!(api, ggml_add(context, q, bias_u));
        let q_v = ggml!(api, ggml_add(context, q, bias_v));
        let attention_layout = |tensor| {
            let tensor = ggml!(
                api,
                ggml_reshape_3d(context, tensor, HEAD_DIM, HEADS, frames as i64)
            );
            let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, tensor))
        };
        let q_u = attention_layout(q_u);
        let q_v = attention_layout(q_v);
        let k = attention_layout(k);
        let v = attention_layout(v);
        let p = attention_layout(p);
        let matrix_ac = ggml!(api, ggml_mul_mat(context, k, q_u));
        let matrix_bd = ggml!(api, ggml_mul_mat(context, p, q_v));
        let matrix_bd = self.relative_shift(context, matrix_bd, frames)?;
        let scores = ggml!(api, ggml_add(context, matrix_ac, matrix_bd));
        let scores = ggml!(
            api,
            ggml_scale_bias(context, scores, 1.0 / (HEAD_DIM as f32).sqrt(), 0.0)
        );
        let probabilities = ggml!(api, ggml_soft_max(context, scores));
        let value_transposed = ggml!(api, ggml_transpose(context, v));
        let value_transposed = ggml!(api, ggml_cont(context, value_transposed));
        let attended = ggml!(api, ggml_mul_mat(context, probabilities, value_transposed));
        let attended = ggml!(api, ggml_permute(context, attended, 2, 0, 1, 3));
        let attended = ggml!(api, ggml_cont(context, attended));
        let attended = ggml!(
            api,
            ggml_reshape_2d(context, attended, HIDDEN_DIM as i64, frames as i64)
        );
        self.linear(context, &format!("{prefix}.linear_out"), attended, true)
    }

    fn relative_shift(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let descriptor = tensor_ref(input)?;
        let zero = ggml!(
            api,
            ggml_view_3d(
                context,
                input,
                1,
                frames as i64,
                HEADS,
                descriptor.nb[1],
                descriptor.nb[2],
                0
            )
        );
        let zero = ggml!(api, ggml_cont(context, zero));
        let zero = ggml!(api, ggml_scale_bias(context, zero, 0.0, 0.0));
        let padded = ggml!(api, ggml_concat(context, zero, input, 0));
        let reshaped = ggml!(
            api,
            ggml_reshape_3d(context, padded, frames as i64, (frames + 1) as i64, HEADS)
        );
        Ok(ggml!(
            api,
            ggml_view_3d(
                context,
                reshaped,
                frames as i64,
                frames as i64,
                HEADS,
                frames * std::mem::size_of::<f32>(),
                frames * (frames + 1) * std::mem::size_of::<f32>(),
                frames * std::mem::size_of::<f32>()
            )
        ))
    }

    fn feed_forward_moe(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let input_stride = HIDDEN_DIM * std::mem::size_of::<f32>();
        let mut chunks = Vec::with_capacity(4);
        for expert in 0..4 {
            let chunk = ggml!(
                api,
                ggml_view_2d(
                    context,
                    input,
                    HEAD_DIM,
                    frames as i64,
                    input_stride,
                    expert * HEAD_DIM as usize * std::mem::size_of::<f32>()
                )
            );
            let hidden = self.linear(
                context,
                &format!("{prefix}.freq_experts.{expert}.w_1"),
                chunk,
                true,
            )?;
            let hidden = ggml!(api, ggml_relu(context, hidden));
            chunks.push(self.linear(
                context,
                &format!("{prefix}.freq_experts.{expert}.w_2"),
                hidden,
                true,
            )?);
        }
        let mut output = chunks[0];
        for chunk in chunks.into_iter().skip(1) {
            output = ggml!(api, ggml_concat(context, output, chunk, 0));
        }
        Ok(output)
    }

    fn convolution_module(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let expanded =
            self.conv_features(context, &format!("{prefix}.pointwise_conv1"), input, 0)?;
        let expanded_stride = 2 * HIDDEN_DIM * std::mem::size_of::<f32>();
        let left = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                HIDDEN_DIM as i64,
                frames as i64,
                expanded_stride,
                0
            )
        );
        let right = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                HIDDEN_DIM as i64,
                frames as i64,
                expanded_stride,
                HIDDEN_DIM * std::mem::size_of::<f32>()
            )
        );
        let right = ggml!(api, ggml_sigmoid(context, right));
        let gated = ggml!(api, ggml_mul(context, left, right));
        let depthwise =
            self.depthwise_conv_features(context, &format!("{prefix}.depthwise_conv"), gated, 4)?;
        let normalized = self.batch_norm_features(context, &format!("{prefix}.norm"), depthwise)?;
        let activated = ggml!(api, ggml_silu(context, normalized));
        self.conv_features(context, &format!("{prefix}.pointwise_conv2"), activated, 0)
    }

    fn residual_block(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        self.residual_block_with_activation(context, prefix, input, ResidualActivation::LeakyRelu)
    }

    fn residual_block_with_activation(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        activation: ResidualActivation,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = self.layer_norm_features(context, &format!("{prefix}.0"), input)?;
        let expanded = self.conv_features(context, &format!("{prefix}.1"), normalized, 1)?;
        let expanded = ggml!(
            api,
            ggml_scale_bias(context, expanded, 3.0_f32.powf(-0.5), 0.0)
        );
        let activated = match activation {
            ResidualActivation::LeakyRelu => ggml!(
                api,
                ggml_leaky_relu(context, expanded, LEAKY_RELU_SLOPE, false)
            ),
            ResidualActivation::Silu => ggml!(api, ggml_silu(context, expanded)),
        };
        let projected = self.conv_features(context, &format!("{prefix}.4"), activated, 0)?;
        Ok(ggml!(api, ggml_add(context, input, projected)))
    }

    fn linear(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        with_bias: bool,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let stored_shape = tensor_ref(weight)?.ne;
        let weight = if stored_shape[0] == 1 && stored_shape[2] > 1 {
            // STARS' pointwise Conv1d experts are semantically linear but are
            // stored as `[kernel=1, input, output]` in GGML order.
            ggml!(
                api,
                ggml_reshape_2d(context, weight, stored_shape[1], stored_shape[2])
            )
        } else {
            weight
        };
        let weight_shape = tensor_ref(weight)?.ne;
        let input_shape = tensor_ref(input)?.ne;
        if weight_shape[0] != input_shape[0] {
            return Err(format!(
                "STARS linear {prefix} shape mismatch: weight {weight_shape:?}, input {input_shape:?}"
            ));
        }
        let output = ggml!(api, ggml_mul_mat(context, weight, input));
        ggml!(api, ggml_mul_mat_set_prec(output, GGML_PREC_F32));
        if with_bias {
            Ok(ggml!(
                api,
                ggml_add(context, output, self.weight(&format!("{prefix}.bias"))?)
            ))
        } else {
            Ok(output)
        }
    }

    fn conv_features(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        padding: i32,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let channel_major = ggml!(api, ggml_transpose(context, input));
        let channel_major = ggml!(api, ggml_cont(context, channel_major));
        let input_shape = tensor_ref(channel_major)?.ne;
        let channel_major = ggml!(
            api,
            ggml_reshape_3d(context, channel_major, input_shape[0], input_shape[1], 1)
        );
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let columns = ggml!(
            api,
            ggml_im2col(
                context,
                weight,
                channel_major,
                1,
                0,
                padding,
                0,
                1,
                0,
                false,
                GGML_TYPE_F32
            )
        );
        let kernel = tensor_ref(weight)?;
        let column_shape = tensor_ref(columns)?.ne;
        let columns = ggml!(
            api,
            ggml_reshape_2d(
                context,
                columns,
                column_shape[0],
                column_shape[2] * column_shape[1]
            )
        );
        let weight = ggml!(
            api,
            ggml_reshape_2d(context, weight, kernel.ne[0] * kernel.ne[1], kernel.ne[2])
        );
        let output = ggml!(api, ggml_mul_mat(context, columns, weight));
        ggml!(api, ggml_mul_mat_set_prec(output, GGML_PREC_F32));
        let output = ggml!(
            api,
            ggml_reshape_3d(
                context,
                output,
                column_shape[1],
                kernel.ne[2],
                column_shape[2]
            )
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, kernel.ne[2], 1));
        let output = ggml!(api, ggml_add(context, output, bias));
        let feature_major = ggml!(api, ggml_transpose(context, output));
        Ok(ggml!(api, ggml_cont(context, feature_major)))
    }

    fn depthwise_conv_features(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        padding: i32,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let channel_major = ggml!(api, ggml_transpose(context, input));
        let channel_major = ggml!(api, ggml_cont(context, channel_major));
        let shape = tensor_ref(channel_major)?.ne;
        let channel_major = ggml!(
            api,
            ggml_reshape_3d(context, channel_major, shape[0], shape[1], 1)
        );
        let output = ggml!(
            api,
            ggml_conv_1d_dw(
                context,
                self.weight(&format!("{prefix}.weight"))?,
                channel_major,
                1,
                padding,
                1
            )
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, shape[1], 1));
        let output = ggml!(api, ggml_add(context, output, bias));
        let output = ggml!(api, ggml_transpose(context, output));
        Ok(ggml!(api, ggml_cont(context, output)))
    }

    fn conv_transpose_features(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        frames: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let channel_major = ggml!(api, ggml_transpose(context, input));
        let channel_major = ggml!(api, ggml_cont(context, channel_major));
        let channel_major = ggml!(
            api,
            ggml_reshape_3d(context, channel_major, frames as i64, HIDDEN_DIM as i64, 1)
        );
        // GGML has no output-padding argument. Full padding=0 output has one
        // sample on each side of the PyTorch padding=1/output_padding=1 result;
        // dropping only the left sample gives the exact 2*T interval.
        let raw = ggml!(
            api,
            ggml_conv_transpose_1d(
                context,
                self.weight(&format!("{prefix}.weight"))?,
                channel_major,
                2,
                0,
                1
            )
        );
        let descriptor = tensor_ref(raw)?;
        let wanted = (2 * frames) as i64;
        if descriptor.ne[0] < wanted + 1 || descriptor.ne[1] != HIDDEN_DIM as i64 {
            return Err(format!(
                "STARS transposed-convolution shape is invalid: {:?}",
                descriptor.ne
            ));
        }
        let cropped = ggml!(
            api,
            ggml_view_3d(
                context,
                raw,
                wanted,
                HIDDEN_DIM as i64,
                1,
                descriptor.nb[1],
                descriptor.nb[2],
                std::mem::size_of::<f32>()
            )
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, HIDDEN_DIM as i64, 1));
        let output = ggml!(api, ggml_add(context, cropped, bias));
        let output = ggml!(api, ggml_transpose(context, output));
        Ok(ggml!(api, ggml_cont(context, output)))
    }

    fn average_pool_time(
        &self,
        context: ContextPtr,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let channel_major = ggml!(api, ggml_transpose(context, input));
        let channel_major = ggml!(api, ggml_cont(context, channel_major));
        let pooled = ggml!(api, ggml_pool_1d(context, channel_major, POOL_AVG, 2, 2, 0));
        let pooled = ggml!(api, ggml_transpose(context, pooled));
        Ok(ggml!(api, ggml_cont(context, pooled)))
    }

    fn layer_norm_features(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = ggml!(api, ggml_norm(context, input, LAYER_NORM_EPSILON));
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

    fn batch_norm_features(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let centered = ggml!(
            api,
            ggml_sub(
                context,
                input,
                self.weight(&format!("{prefix}.running_mean"))?
            )
        );
        let variance = ggml!(
            api,
            ggml_scale_bias(
                context,
                self.weight(&format!("{prefix}.running_var"))?,
                1.0,
                LAYER_NORM_EPSILON
            )
        );
        let deviation = ggml!(api, ggml_sqrt(context, variance));
        let normalized = ggml!(api, ggml_div(context, centered, deviation));
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
}

fn ensure_shape(tensor: TensorPtr, features: i64, frames: i64, label: &str) -> Result<(), String> {
    let shape = tensor_ref(tensor)?.ne;
    if shape[0] == features && shape[1] == frames {
        Ok(())
    } else {
        Err(format!("STARS {label} shape is invalid: {shape:?}"))
    }
}

fn relative_position_values(frames: usize) -> Vec<f32> {
    let mut values = vec![0.0_f32; frames * HIDDEN_DIM];
    for row in 0..frames {
        let position = (RELATIVE_POSITION_MAX_LEN - 1 - row) as f32;
        for index in 0..HIDDEN_DIM / 2 {
            let divisor = (-((2 * index) as f32) * 10_000.0_f32.ln() / HIDDEN_DIM as f32).exp();
            let angle = position * divisor;
            values[row * HIDDEN_DIM + 2 * index] = angle.sin();
            values[row * HIDDEN_DIM + 2 * index + 1] = angle.cos();
        }
    }
    values
}

pub(super) fn absolute_position_values(valid_frames: usize, frames: usize) -> Vec<f32> {
    let half = HIDDEN_DIM / 2;
    let scale = 10_000.0_f32.ln() / (half as f32 - 1.0);
    let frequencies = (0..half)
        .map(|index| (-(index as f32) * scale).exp())
        .collect::<Vec<_>>();
    let mut values = vec![0.0_f32; frames * HIDDEN_DIM];
    for frame in 0..valid_frames {
        let position = (frame + 1) as f32;
        for (index, frequency) in frequencies.iter().enumerate() {
            let angle = position * frequency;
            values[frame * HIDDEN_DIM + index] = angle.sin();
            values[frame * HIDDEN_DIM + half + index] = angle.cos();
        }
    }
    values
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_positions_zero_padding() {
        let positions = absolute_position_values(2, 4);
        assert!(
            positions[..2 * HIDDEN_DIM]
                .iter()
                .any(|value| *value != 0.0)
        );
        assert!(
            positions[2 * HIDDEN_DIM..]
                .iter()
                .all(|value| *value == 0.0)
        );
    }

    #[test]
    fn relative_positions_use_fixed_reference_origin() {
        let positions = relative_position_values(2);
        assert_eq!(positions.len(), 2 * HIDDEN_DIM);
        assert_eq!(positions[0], 4_999.0_f32.sin());
        assert_eq!(positions[HIDDEN_DIM], 4_998.0_f32.sin());
    }
}
