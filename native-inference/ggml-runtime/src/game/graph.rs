use std::ffi::c_void;
use std::sync::Arc;

use crate::ffi::{
    AllocatorPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F16, GGML_TYPE_F32,
    GGML_TYPE_I32, GgmlInitParams, GgmlTensor, GraphPtr, ModelApi, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

use super::model::Game;

const GRAPH_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const GRAPH_NODE_CAPACITY: usize = 8_192;
#[cfg(test)]
const MODEL_DIM: i64 = 256;
const RMS_NORM_EPSILON: f32 = 1.0e-6;
const ROPE_THETA: f32 = 10_000.0;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: all raw handles are owned by the live GAME model or graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameEncoderOutput {
    pub frames: usize,
    /// Frame-major `[frames, model embedding dimension]` segmenter embeddings.
    pub segmenter_embeddings: Vec<f32>,
    /// Frame-major `[frames, model embedding dimension]` estimator embeddings.
    pub estimator_embeddings: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameEstimatorOutput {
    pub regions: usize,
    pub bins: usize,
    /// Region-major `[regions, bins]` pitch logits.
    pub pool_logits: Vec<f32>,
}

impl Game {
    pub fn encode_mel(&self, mel: &[f32], frame_count: usize) -> Result<GameEncoderOutput, String> {
        if frame_count == 0 {
            return Err("GAME encoder requires at least one mel frame".to_string());
        }
        if mel.len() != frame_count.saturating_mul(self.config().input_dim) {
            return Err("GAME encoder mel shape is invalid".to_string());
        }
        let frames = i64::try_from(frame_count)
            .map_err(|_| "GAME encoder frame count exceeds i64".to_string())?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.config().input_dim as i64,
                frames
            )
        );
        ggml!(api, ggml_set_input(input));
        let positions = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames));
        ggml!(api, ggml_set_input(positions));
        let (segmenter, estimator) = self.build_encoder(run.context, input, positions, frames)?;
        for output in [segmenter, estimator] {
            ggml!(api, ggml_set_output(output));
            ggml!(api, ggml_build_forward_expand(run.graph, output));
        }
        run.allocate(&self.backend)?;
        set_f32(api, input, mel, "GAME encoder mel")?;
        let positions_data = (0..frame_count)
            .map(|position| {
                i32::try_from(position).map_err(|_| "GAME encoder position exceeds i32".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        set_i32(api, positions, &positions_data, "GAME encoder positions")?;
        run.compute(&self.backend)?;
        Ok(GameEncoderOutput {
            frames: frame_count,
            segmenter_embeddings: get_f32(api, segmenter)?,
            estimator_embeddings: get_f32(api, estimator)?,
        })
    }

    /// Runs one GAME D3PM segmenter step. `x_seg` and the returned logits are
    /// frame-major; diffusion scheduling and boundary decoding stay in Rust.
    pub fn segmenter_logits(
        &self,
        x_seg: &[f32],
        noise_mod: &[i32],
        timestep: f32,
        language: i32,
    ) -> Result<Vec<f32>, String> {
        if noise_mod.is_empty() {
            return Err("GAME segmenter requires at least one frame".to_string());
        }
        let frame_count = noise_mod.len();
        if x_seg.len()
            != frame_count
                .checked_mul(self.config().embedding_dim)
                .ok_or_else(|| "GAME segmenter input length overflowed".to_string())?
        {
            return Err("GAME segmenter embedding shape is invalid".to_string());
        }
        if noise_mod
            .iter()
            .any(|value| *value < 0 || *value >= self.config().region_cycle_length as i32)
        {
            return Err("GAME segmenter noise index is invalid".to_string());
        }
        if language < 0 || language > self.config().language_count as i32 {
            return Err("GAME segmenter language index is invalid".to_string());
        }
        if !timestep.is_finite() {
            return Err("GAME segmenter timestep must be finite".to_string());
        }

        let frames = i64::try_from(frame_count)
            .map_err(|_| "GAME segmenter frame count exceeds i64".to_string())?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let embedding_dim = self.config().embedding_dim as i64;
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, embedding_dim, frames)
        );
        ggml!(api, ggml_set_input(input));
        let noise = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames));
        ggml!(api, ggml_set_input(noise));
        let time = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_F32, 1));
        ggml!(api, ggml_set_input(time));
        let language_input = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, 1));
        ggml!(api, ggml_set_input(language_input));
        let positions = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames));
        ggml!(api, ggml_set_input(positions));

        let noise_embedding = ggml!(
            api,
            ggml_get_rows(
                run.context,
                self.weight("noise_embedding.embedding.weight")?,
                noise
            )
        );
        let mut hidden = ggml!(api, ggml_add(run.context, input, noise_embedding));
        let mut time_embedding = self.linear(run.context, "time_embedding.0", time)?;
        time_embedding = ggml!(api, ggml_gelu_erf(run.context, time_embedding));
        time_embedding = self.linear(run.context, "time_embedding.2", time_embedding)?;
        hidden = ggml!(api, ggml_add(run.context, hidden, time_embedding));
        let language_embedding = ggml!(
            api,
            ggml_get_rows(
                run.context,
                self.weight("language_embedding.weight")?,
                language_input
            )
        );
        hidden = ggml!(api, ggml_add(run.context, hidden, language_embedding));
        hidden = self.linear(run.context, "segmenter.input_proj", hidden)?;
        for layer in 0..self.config().segmenter_layers {
            hidden = self.ebf_block(
                run.context,
                &format!("segmenter.layers.{layer}"),
                hidden,
                positions,
                frames,
            )?;
        }
        hidden = self.rms_norm(run.context, hidden, "segmenter.output_norm.weight")?;
        let logits = self.linear(run.context, "segmenter.output_proj", hidden)?;
        ggml!(api, ggml_set_output(logits));
        ggml!(api, ggml_build_forward_expand(run.graph, logits));

        run.allocate(&self.backend)?;
        set_f32(api, input, x_seg, "GAME segmenter embeddings")?;
        set_i32(api, noise, noise_mod, "GAME segmenter noise")?;
        set_f32(api, time, &[timestep], "GAME segmenter timestep")?;
        set_i32(api, language_input, &[language], "GAME segmenter language")?;
        let positions_data = (0..frame_count)
            .map(|position| {
                i32::try_from(position)
                    .map_err(|_| "GAME segmenter position exceeds i32".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        set_i32(api, positions, &positions_data, "GAME segmenter positions")?;
        run.compute(&self.backend)?;
        let logits = get_f32(api, logits)?;
        if logits.len() != frame_count {
            return Err("GAME segmenter output shape is invalid".to_string());
        }
        Ok(logits)
    }

    /// Runs GAME's joint region/frame estimator. Region IDs are zero before
    /// the first boundary and one-based afterward.
    pub fn estimate_pitch(
        &self,
        x_est: &[f32],
        regions: &[i32],
    ) -> Result<GameEstimatorOutput, String> {
        if regions.is_empty() {
            return Ok(GameEstimatorOutput {
                regions: 0,
                bins: self.config().estimator_output_dim,
                pool_logits: Vec::new(),
            });
        }
        let frame_count = regions.len();
        if x_est.len()
            != frame_count
                .checked_mul(self.config().embedding_dim)
                .ok_or_else(|| "GAME estimator input length overflowed".to_string())?
        {
            return Err("GAME estimator embedding shape is invalid".to_string());
        }
        if regions.iter().any(|region| *region < 0) {
            return Err("GAME estimator region ID is invalid".to_string());
        }
        let region_count = regions.iter().copied().max().unwrap_or(0) as usize;
        if region_count == 0 {
            return Ok(GameEstimatorOutput {
                regions: 0,
                bins: self.config().estimator_output_dim,
                pool_logits: Vec::new(),
            });
        }
        let frames = i64::try_from(frame_count)
            .map_err(|_| "GAME estimator frame count exceeds i64".to_string())?;
        let pool_length = i64::try_from(region_count)
            .map_err(|_| "GAME estimator region count exceeds i64".to_string())?;
        let total_length = frames
            .checked_add(pool_length)
            .ok_or_else(|| "GAME estimator sequence length overflowed".to_string())?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let embedding_dim = self.config().embedding_dim as i64;
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, embedding_dim, frames)
        );
        ggml!(api, ggml_set_input(input));
        let region_input = ggml!(api, ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames));
        ggml!(api, ggml_set_input(region_input));
        let global_positions = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, total_length)
        );
        ggml!(api, ggml_set_input(global_positions));
        let local_positions = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, total_length)
        );
        ggml!(api, ggml_set_input(local_positions));
        let attention_mask_input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, total_length, total_length)
        );
        ggml!(api, ggml_set_input(attention_mask_input));
        let attention_mask = ggml!(
            api,
            ggml_cast(run.context, attention_mask_input, GGML_TYPE_F16)
        );

        let region_embedding = ggml!(
            api,
            ggml_get_rows(
                run.context,
                self.weight("region_embedding.embedding.weight")?,
                region_input
            )
        );
        let mut frame_hidden = ggml!(api, ggml_add(run.context, input, region_embedding));
        frame_hidden = self.linear(run.context, "estimator.input_proj", frame_hidden)?;
        let pool_target = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, embedding_dim, pool_length)
        );
        let mut pool_hidden = ggml!(
            api,
            ggml_repeat(
                run.context,
                self.weight("estimator.pool_token_gen.emb")?,
                pool_target
            )
        );
        for layer in 0..self.config().estimator_layers {
            (pool_hidden, frame_hidden) = self.jebf_block(
                run.context,
                &format!("estimator.layers.{layer}"),
                pool_hidden,
                frame_hidden,
                global_positions,
                local_positions,
                attention_mask,
                pool_length,
                frames,
            )?;
        }
        pool_hidden = self.rms_norm(
            run.context,
            pool_hidden,
            "estimator.output_norm_pool.weight",
        )?;
        let logits = self.linear(run.context, "estimator.output_proj_pool", pool_hidden)?;
        ggml!(api, ggml_set_output(logits));
        ggml!(api, ggml_build_forward_expand(run.graph, logits));

        let region_mod = regions
            .iter()
            .map(|region| *region % self.config().region_cycle_length as i32)
            .collect::<Vec<_>>();
        let (global_data, local_data) = estimator_positions(regions, region_count)?;
        let mask_data = joint_attention_mask(regions, region_count)?;
        run.allocate(&self.backend)?;
        set_f32(api, input, x_est, "GAME estimator embeddings")?;
        set_i32(api, region_input, &region_mod, "GAME estimator regions")?;
        set_i32(
            api,
            global_positions,
            &global_data,
            "GAME estimator global positions",
        )?;
        set_i32(
            api,
            local_positions,
            &local_data,
            "GAME estimator local positions",
        )?;
        set_f32(api, attention_mask_input, &mask_data, "GAME estimator mask")?;
        run.compute(&self.backend)?;
        let pool_logits = get_f32(api, logits)?;
        let expected = region_count
            .checked_mul(self.config().estimator_output_dim)
            .ok_or_else(|| "GAME estimator output length overflowed".to_string())?;
        if pool_logits.len() != expected {
            return Err("GAME estimator output shape is invalid".to_string());
        }
        Ok(GameEstimatorOutput {
            regions: region_count,
            bins: self.config().estimator_output_dim,
            pool_logits,
        })
    }

    fn build_encoder(
        &self,
        context: ContextPtr,
        mel: TensorPtr,
        positions: TensorPtr,
        frames: i64,
    ) -> Result<(TensorPtr, TensorPtr), String> {
        let mut hidden = self.linear(context, "spectrogram_projection", mel)?;
        hidden = self.linear(context, "encoder.input_proj", hidden)?;
        for layer in 0..self.config().encoder_layers {
            hidden = self.ebf_block(
                context,
                &format!("encoder.layers.{layer}"),
                hidden,
                positions,
                frames,
            )?;
        }
        hidden = self.rms_norm(context, hidden, "encoder.output_norm.weight")?;
        let output = self.linear(context, "encoder.output_proj", hidden)?;
        split_feature_two(self.api(), context, output, frames)
    }

    fn ebf_block(
        &self,
        context: ContextPtr,
        prefix: &str,
        mut input: TensorPtr,
        positions: TensorPtr,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        input = self.residual_glu(
            context,
            input,
            &format!("{prefix}.norm1.weight"),
            &format!("{prefix}.ffn1"),
            Some(&format!("{prefix}.lay_scale1.scale")),
            0.5,
            frames,
        )?;
        let mut branch = self.pac(context, input, &format!("{prefix}.attn"), positions, frames)?;
        branch = self.multiply_weight(context, branch, &format!("{prefix}.lay_scale2.scale"))?;
        input = ggml!(self.api(), ggml_add(context, input, branch));
        self.residual_glu(
            context,
            input,
            &format!("{prefix}.norm2.weight"),
            &format!("{prefix}.ffn2"),
            Some(&format!("{prefix}.lay_scale3.scale")),
            0.5,
            frames,
        )
    }

    fn jebf_block(
        &self,
        context: ContextPtr,
        prefix: &str,
        mut pool: TensorPtr,
        mut frames: TensorPtr,
        global_positions: TensorPtr,
        local_positions: TensorPtr,
        mask: TensorPtr,
        pool_length: i64,
        frame_count: i64,
    ) -> Result<(TensorPtr, TensorPtr), String> {
        frames = self.residual_glu(
            context,
            frames,
            &format!("{prefix}.norm_ffn1_x.weight"),
            &format!("{prefix}.ffn1_x"),
            Some(&format!("{prefix}.lay_scale_ffn1_x.scale")),
            1.0,
            frame_count,
        )?;
        pool = self.residual_glu(
            context,
            pool,
            &format!("{prefix}.norm_ffn1_pool.weight"),
            &format!("{prefix}.ffn1_pool"),
            Some(&format!("{prefix}.lay_scale_ffn1_pool.scale")),
            1.0,
            pool_length,
        )?;
        let (mut pool_branch, mut frame_branch) = self.joint_pac(
            context,
            &format!("{prefix}.attn"),
            pool,
            frames,
            global_positions,
            local_positions,
            mask,
            pool_length,
            frame_count,
        )?;
        pool_branch = self.multiply_weight(
            context,
            pool_branch,
            &format!("{prefix}.lay_scale_jpac_pool.scale"),
        )?;
        frame_branch = self.multiply_weight(
            context,
            frame_branch,
            &format!("{prefix}.lay_scale_jpac_x.scale"),
        )?;
        pool = ggml!(self.api(), ggml_add(context, pool, pool_branch));
        frames = ggml!(self.api(), ggml_add(context, frames, frame_branch));
        frames = self.residual_glu(
            context,
            frames,
            &format!("{prefix}.norm_ffn2_x.weight"),
            &format!("{prefix}.ffn2_x"),
            Some(&format!("{prefix}.lay_scale_ffn2_x.scale")),
            1.0,
            frame_count,
        )?;
        pool = self.residual_glu(
            context,
            pool,
            &format!("{prefix}.norm_ffn2_pool.weight"),
            &format!("{prefix}.ffn2_pool"),
            Some(&format!("{prefix}.lay_scale_ffn2_pool.scale")),
            1.0,
            pool_length,
        )?;
        Ok((pool, frames))
    }

    #[allow(clippy::too_many_arguments)]
    fn joint_pac(
        &self,
        context: ContextPtr,
        prefix: &str,
        pool: TensorPtr,
        frames: TensorPtr,
        global_positions: TensorPtr,
        local_positions: TensorPtr,
        mask: TensorPtr,
        pool_length: i64,
        frame_count: i64,
    ) -> Result<(TensorPtr, TensorPtr), String> {
        let (attention_pool, attention_frames) = self.joint_attention(
            context,
            &format!("{prefix}.jattn"),
            pool,
            frames,
            global_positions,
            local_positions,
            mask,
            pool_length,
            frame_count,
        )?;
        let pool_norm = self.rms_norm(context, pool, &format!("{prefix}.c_norm_pool.weight"))?;
        let frame_norm = self.rms_norm(context, frames, &format!("{prefix}.c_norm_x.weight"))?;
        let pool_convolution =
            self.cgmlp(context, pool_norm, &format!("{prefix}.c_pool"), pool_length)?;
        let frame_convolution =
            self.cgmlp(context, frame_norm, &format!("{prefix}.c_x"), frame_count)?;
        Ok((
            self.merge_stream_named(
                context,
                attention_pool,
                pool_convolution,
                &format!("{prefix}.merge_dw_conv_pool"),
                &format!("{prefix}.merge_linear_pool"),
                pool_length,
            )?,
            self.merge_stream_named(
                context,
                attention_frames,
                frame_convolution,
                &format!("{prefix}.merge_dw_conv_x"),
                &format!("{prefix}.merge_linear_x"),
                frame_count,
            )?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn joint_attention(
        &self,
        context: ContextPtr,
        prefix: &str,
        pool: TensorPtr,
        frames: TensorPtr,
        global_positions: TensorPtr,
        local_positions: TensorPtr,
        mask: TensorPtr,
        pool_length: i64,
        frame_count: i64,
    ) -> Result<(TensorPtr, TensorPtr), String> {
        let api = self.api();
        let pool_norm = self.rms_norm(context, pool, &format!("{prefix}.pool_norm.weight"))?;
        let frame_norm = self.rms_norm(context, frames, &format!("{prefix}.x_norm.weight"))?;
        let pool_qkv = self.linear(context, &format!("{prefix}.pool_qkv"), pool_norm)?;
        let frame_qkv = self.linear(context, &format!("{prefix}.x_qkv"), frame_norm)?;
        let (pool_q, pool_k, pool_v) = split_feature_three(
            api,
            context,
            pool_qkv,
            pool_length,
            self.config().attention_heads as i64,
            self.config().attention_head_dim as i64,
        )?;
        let (frame_q, frame_k, frame_v) = split_feature_three(
            api,
            context,
            frame_qkv,
            frame_count,
            self.config().attention_heads as i64,
            self.config().attention_head_dim as i64,
        )?;
        let pool_q = self.rms_norm(context, pool_q, &format!("{prefix}.pool_q_norm.weight"))?;
        let pool_k = self.rms_norm(context, pool_k, &format!("{prefix}.pool_k_norm.weight"))?;
        let frame_q = self.rms_norm(context, frame_q, &format!("{prefix}.x_q_norm.weight"))?;
        let frame_k = self.rms_norm(context, frame_k, &format!("{prefix}.x_k_norm.weight"))?;
        let mut query = ggml!(api, ggml_concat(context, pool_q, frame_q, 2));
        let mut key = ggml!(api, ggml_concat(context, pool_k, frame_k, 2));
        let value = ggml!(api, ggml_concat(context, pool_v, frame_v, 2));
        query = mixed_rope(
            api,
            context,
            query,
            global_positions,
            local_positions,
            self.config().attention_head_dim as i64,
        )?;
        key = mixed_rope(
            api,
            context,
            key,
            global_positions,
            local_positions,
            self.config().attention_head_dim as i64,
        )?;
        let layout = |value| {
            let value = ggml!(api, ggml_permute(context, value, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, value))
        };
        query = layout(query);
        key = ggml!(api, ggml_cast(context, layout(key), GGML_TYPE_F16));
        let value = ggml!(api, ggml_cast(context, layout(value), GGML_TYPE_F16));
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                key,
                value,
                mask,
                1.0 / (self.config().attention_head_dim as f32).sqrt(),
                0.0,
                0.0
            )
        );
        ggml!(api, ggml_flash_attn_ext_set_prec(attended, GGML_PREC_F32));
        let total_length = pool_length + frame_count;
        let projection_dim =
            (self.config().attention_heads * self.config().attention_head_dim) as i64;
        let attended = ggml!(
            api,
            ggml_reshape_2d(context, attended, projection_dim, total_length)
        );
        let (pool_output, frame_output) =
            split_sequence(api, context, attended, pool_length, frame_count)?;
        Ok((
            self.linear(context, &format!("{prefix}.pool_out"), pool_output)?,
            self.linear(context, &format!("{prefix}.x_out"), frame_output)?,
        ))
    }

    fn merge_stream_named(
        &self,
        context: ContextPtr,
        attention: TensorPtr,
        convolution: TensorPtr,
        depthwise_name: &str,
        linear_name: &str,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let merged = ggml!(api, ggml_concat(context, attention, convolution, 0));
        let convolved = self.depthwise_conv(context, merged, depthwise_name, frames, false)?;
        let merged = ggml!(api, ggml_add(context, merged, convolved));
        self.linear(context, linear_name, merged)
    }

    fn residual_glu(
        &self,
        context: ContextPtr,
        residual: TensorPtr,
        norm_name: &str,
        ffn_prefix: &str,
        layer_scale_name: Option<&str>,
        branch_scale: f32,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let normalized = self.rms_norm(context, residual, norm_name)?;
        let expanded = self.linear(context, &format!("{ffn_prefix}.ln1"), normalized)?;
        let (left, right) = split_feature_two(self.api(), context, expanded, frames)?;
        let left = ggml!(self.api(), ggml_gelu_erf(context, left));
        let gated = ggml!(self.api(), ggml_mul(context, left, right));
        let mut branch = self.linear(context, &format!("{ffn_prefix}.ln2"), gated)?;
        if let Some(name) = layer_scale_name {
            branch = self.multiply_weight(context, branch, name)?;
        }
        if branch_scale != 1.0 {
            branch = ggml!(
                self.api(),
                ggml_scale_bias(context, branch, branch_scale, 0.0)
            );
        }
        Ok(ggml!(self.api(), ggml_add(context, residual, branch)))
    }

    fn pac(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        prefix: &str,
        positions: TensorPtr,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let attention_input = self.rms_norm(context, input, &format!("{prefix}.a_norm.weight"))?;
        let attention = self.attention(
            context,
            attention_input,
            &format!("{prefix}.attn"),
            positions,
            frames,
        )?;
        let cgmlp_input = self.rms_norm(context, input, &format!("{prefix}.c_norm.weight"))?;
        let convolution = self.cgmlp(context, cgmlp_input, &format!("{prefix}.c"), frames)?;
        self.merge_stream(context, attention, convolution, prefix, frames)
    }

    fn attention(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        prefix: &str,
        positions: TensorPtr,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let query = self.linear(context, &format!("{prefix}.q_linear"), input)?;
        let key_value = self.linear(context, &format!("{prefix}.kv_linear"), input)?;
        let head_dim = self.config().attention_head_dim as i64;
        let heads = self.config().attention_heads as i64;
        let projection_dim = head_dim * heads;
        let query = ggml!(
            api,
            ggml_reshape_4d(context, query, head_dim, heads, frames, 1)
        );
        let key_value_descriptor = tensor_ref(key_value)?;
        let mut parts = Vec::with_capacity(2);
        for part in 0..2 {
            let value = ggml!(
                api,
                ggml_view_3d(
                    context,
                    key_value,
                    projection_dim,
                    frames,
                    1,
                    key_value_descriptor.nb[1],
                    key_value_descriptor.nb[2],
                    part * projection_dim as usize * std::mem::size_of::<f32>()
                )
            );
            let value = ggml!(api, ggml_cont(context, value));
            parts.push(ggml!(
                api,
                ggml_reshape_4d(context, value, head_dim, heads, frames, 1)
            ));
        }
        let query = rope(api, context, query, positions, head_dim);
        let key = rope(api, context, parts[0], positions, head_dim);
        let layout = |value| {
            let value = ggml!(api, ggml_permute(context, value, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, value))
        };
        let query = layout(query);
        let key = ggml!(api, ggml_cast(context, layout(key), GGML_TYPE_F16));
        let value = ggml!(api, ggml_cast(context, layout(parts[1]), GGML_TYPE_F16));
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                key,
                value,
                std::ptr::null_mut(),
                1.0 / (head_dim as f32).sqrt(),
                0.0,
                0.0
            )
        );
        ggml!(api, ggml_flash_attn_ext_set_prec(attended, GGML_PREC_F32));
        let attended = ggml!(
            api,
            ggml_reshape_2d(context, attended, projection_dim, frames)
        );
        self.linear(context, &format!("{prefix}.out_linear"), attended)
    }

    fn cgmlp(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        prefix: &str,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let expanded = self.linear(context, &format!("{prefix}.pw1"), input)?;
        let expanded = ggml!(self.api(), ggml_gelu_erf(context, expanded));
        let (left, right) = split_feature_two(self.api(), context, expanded, frames)?;
        let right = self.rms_norm(context, right, &format!("{prefix}.norm.weight"))?;
        let right = self.depthwise_conv(context, right, &format!("{prefix}.dw"), frames, true)?;
        let gated = ggml!(self.api(), ggml_mul(context, left, right));
        self.linear(context, &format!("{prefix}.pw2"), gated)
    }

    fn merge_stream(
        &self,
        context: ContextPtr,
        attention: TensorPtr,
        convolution: TensorPtr,
        prefix: &str,
        frames: i64,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let merged = ggml!(api, ggml_concat(context, attention, convolution, 0));
        let convolved = self.depthwise_conv(
            context,
            merged,
            &format!("{prefix}.merge_dw_conv"),
            frames,
            false,
        )?;
        let merged = ggml!(api, ggml_add(context, merged, convolved));
        self.linear(context, &format!("{prefix}.merge_linear"), merged)
    }

    fn depthwise_conv(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        prefix: &str,
        frames: i64,
        gelu: bool,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let channels = tensor_ref(input)?.ne[0];
        let input = ggml!(api, ggml_transpose(context, input));
        let input = ggml!(api, ggml_cont(context, input));
        let input = ggml!(api, ggml_reshape_3d(context, input, frames, channels, 1));
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let kernel = tensor_ref(weight)?.ne[0];
        let output = ggml!(
            api,
            ggml_conv_1d_dw(context, weight, input, 1, (kernel / 2) as i32, 1)
        );
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, channels, 1));
        let output = ggml!(api, ggml_add(context, output, bias));
        let output = if gelu {
            ggml!(api, ggml_gelu_erf(context, output))
        } else {
            output
        };
        let output = ggml!(api, ggml_reshape_2d(context, output, frames, channels));
        let output = ggml!(api, ggml_transpose(context, output));
        Ok(ggml!(api, ggml_cont(context, output)))
    }

    fn linear(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let input_dimension = tensor_ref(input)?.ne[0];
        let mut weight = self.weight(&format!("{prefix}.weight"))?;
        let descriptor = tensor_ref(weight)?;
        if descriptor.ne[0] == 1 && descriptor.ne[1] == input_dimension {
            weight = ggml!(
                api,
                ggml_reshape_2d(context, weight, descriptor.ne[1], descriptor.ne[2])
            );
        }
        let output = ggml!(api, ggml_mul_mat(context, weight, input));
        let bias = self.weight(&format!("{prefix}.bias"))?;
        Ok(ggml!(api, ggml_add(context, output, bias)))
    }

    fn rms_norm(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        weight_name: &str,
    ) -> Result<TensorPtr, String> {
        let normalized = ggml!(self.api(), ggml_rms_norm(context, input, RMS_NORM_EPSILON));
        self.multiply_weight(context, normalized, weight_name)
    }

    fn multiply_weight(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        weight_name: &str,
    ) -> Result<TensorPtr, String> {
        Ok(ggml!(
            self.api(),
            ggml_mul(context, input, self.weight(weight_name)?)
        ))
    }
}

fn split_feature_three(
    api: &ModelApi,
    context: ContextPtr,
    input: TensorPtr,
    sequence_length: i64,
    heads: i64,
    head_dim: i64,
) -> Result<(TensorPtr, TensorPtr, TensorPtr), String> {
    let descriptor = tensor_ref(input)?;
    let projection_dim = heads
        .checked_mul(head_dim)
        .ok_or_else(|| "GAME attention projection dimension overflowed".to_string())?;
    if descriptor.ne[0] != projection_dim * 3 {
        return Err("GAME joint attention QKV shape is invalid".to_string());
    }
    let mut parts = Vec::with_capacity(3);
    for part in 0..3 {
        let value = ggml!(
            api,
            ggml_view_2d(
                context,
                input,
                projection_dim,
                sequence_length,
                descriptor.nb[1],
                part * projection_dim as usize * descriptor.nb[0]
            )
        );
        let value = ggml!(api, ggml_cont(context, value));
        parts.push(ggml!(
            api,
            ggml_reshape_4d(context, value, head_dim, heads, sequence_length, 1)
        ));
    }
    Ok((parts[0], parts[1], parts[2]))
}

fn split_sequence(
    api: &ModelApi,
    context: ContextPtr,
    input: TensorPtr,
    first_length: i64,
    second_length: i64,
) -> Result<(TensorPtr, TensorPtr), String> {
    let descriptor = tensor_ref(input)?;
    let first = ggml!(
        api,
        ggml_view_2d(
            context,
            input,
            descriptor.ne[0],
            first_length,
            descriptor.nb[1],
            0
        )
    );
    let second = ggml!(
        api,
        ggml_view_2d(
            context,
            input,
            descriptor.ne[0],
            second_length,
            descriptor.nb[1],
            first_length as usize * descriptor.nb[1]
        )
    );
    Ok((
        ggml!(api, ggml_cont(context, first)),
        ggml!(api, ggml_cont(context, second)),
    ))
}

fn mixed_rope(
    api: &ModelApi,
    context: ContextPtr,
    input: TensorPtr,
    global_positions: TensorPtr,
    local_positions: TensorPtr,
    head_dim: i64,
) -> Result<TensorPtr, String> {
    if head_dim % 4 != 0 {
        return Err("GAME mixed RoPE head dimension must be divisible by four".to_string());
    }
    let descriptor = tensor_ref(input)?;
    if descriptor.ne[0] != head_dim {
        return Err("GAME mixed RoPE input shape is invalid".to_string());
    }
    let half = head_dim / 2;
    let view_half = |offset: usize| {
        ggml!(
            api,
            ggml_view_4d(
                context,
                input,
                half,
                descriptor.ne[1],
                descriptor.ne[2],
                descriptor.ne[3],
                descriptor.nb[1],
                descriptor.nb[2],
                descriptor.nb[3],
                offset
            )
        )
    };
    let global = rope(api, context, view_half(0), global_positions, half);
    let local = rope(
        api,
        context,
        view_half(half as usize * descriptor.nb[0]),
        local_positions,
        half,
    );
    Ok(ggml!(api, ggml_concat(context, global, local, 0)))
}

fn estimator_positions(
    regions: &[i32],
    region_count: usize,
) -> Result<(Vec<i32>, Vec<i32>), String> {
    let total = region_count
        .checked_add(regions.len())
        .ok_or_else(|| "GAME estimator position count overflowed".to_string())?;
    let mut global = Vec::with_capacity(total);
    for position in 0..region_count {
        global.push(
            i32::try_from(position)
                .map_err(|_| "GAME estimator global position exceeds i32".to_string())?,
        );
    }
    for position in 0..regions.len() {
        global.push(
            i32::try_from(position)
                .map_err(|_| "GAME estimator global position exceeds i32".to_string())?,
        );
    }
    let mut local = vec![0_i32; total];
    let mut current_region = 0;
    let mut current_local = 0_usize;
    for (index, region) in regions.iter().copied().enumerate() {
        if region != current_region {
            current_region = region;
            current_local = 0;
        }
        if region > 0 {
            local[region_count + index] = i32::try_from(current_local + 1)
                .map_err(|_| "GAME estimator local position exceeds i32".to_string())?;
        }
        current_local += 1;
    }
    Ok((global, local))
}

fn joint_attention_mask(regions: &[i32], region_count: usize) -> Result<Vec<f32>, String> {
    let total = region_count
        .checked_add(regions.len())
        .ok_or_else(|| "GAME estimator mask dimension overflowed".to_string())?;
    let elements = total
        .checked_mul(total)
        .ok_or_else(|| "GAME estimator mask length overflowed".to_string())?;
    let region_of = |index: usize| {
        if index < region_count {
            i32::try_from(index + 1).unwrap_or(i32::MAX)
        } else {
            regions[index - region_count]
        }
    };
    let valid = |index: usize| index < region_count || regions[index - region_count] != 0;
    let is_pool = |index: usize| index < region_count;
    let mut mask = vec![-10_000.0_f32; elements];
    for query in 0..total {
        for key in 0..total {
            let query_region = region_of(query);
            let key_region = region_of(key);
            let same_region = query_region != 0 && query_region == key_region;
            if valid(query) && valid(key) && (is_pool(query) == is_pool(key) || same_region) {
                mask[query * total + key] = 0.0;
            }
        }
    }
    Ok(mask)
}

fn split_feature_two(
    api: &ModelApi,
    context: ContextPtr,
    input: TensorPtr,
    frames: i64,
) -> Result<(TensorPtr, TensorPtr), String> {
    let descriptor = tensor_ref(input)?;
    if descriptor.ne[0] % 2 != 0 {
        return Err("GAME tensor feature dimension is not even".to_string());
    }
    let half = descriptor.ne[0] / 2;
    let first = ggml!(
        api,
        ggml_view_2d(context, input, half, frames, descriptor.nb[1], 0)
    );
    let second = ggml!(
        api,
        ggml_view_2d(
            context,
            input,
            half,
            frames,
            descriptor.nb[1],
            half as usize * descriptor.nb[0]
        )
    );
    Ok((
        ggml!(api, ggml_cont(context, first)),
        ggml!(api, ggml_cont(context, second)),
    ))
}

fn rope(
    api: &ModelApi,
    context: ContextPtr,
    input: TensorPtr,
    positions: TensorPtr,
    rope_dimensions: i64,
) -> TensorPtr {
    ggml!(
        api,
        ggml_rope_ext(
            context,
            input,
            positions,
            std::ptr::null_mut(),
            rope_dimensions as i32,
            0,
            0,
            ROPE_THETA,
            1.0,
            0.0,
            1.0,
            0.0,
            0.0
        )
    )
}

fn tensor_ref(raw: TensorPtr) -> Result<&'static GgmlTensor, String> {
    // SAFETY: callers retain the GGML context that owns the tensor.
    unsafe { raw.as_ref() }.ok_or_else(|| "GGML returned a null GAME graph tensor".to_string())
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32], label: &str) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err(format!("{label} byte length does not match GGML tensor"));
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn set_i32(api: &ModelApi, tensor: TensorPtr, values: &[i32], label: &str) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<i32>() {
        return Err(format!("{label} byte length does not match GGML tensor"));
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "GAME tensor element count is invalid".to_string())?;
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
                mem_size: GRAPH_MEMORY_BYTES,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: true,
            })
        );
        if context.is_null() {
            return Err("could not allocate GAME GGML graph context".to_string());
        }
        let graph = ggml!(
            api,
            ggml_new_graph_custom(context, GRAPH_NODE_CAPACITY, false)
        );
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate GAME GGML graph".to_string());
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
            return Err("could not allocate GAME GGML graph".to_string());
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
                "GAME GGML graph compute failed with status {status}"
            ))
        }
    }
}

impl Drop for GraphRun {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_input_shape_is_frame_major() {
        let output = GameEncoderOutput {
            frames: 2,
            segmenter_embeddings: vec![0.0; 512],
            estimator_embeddings: vec![0.0; 512],
        };
        assert_eq!(
            output.segmenter_embeddings.len(),
            output.frames * MODEL_DIM as usize
        );
        assert_eq!(
            output.estimator_embeddings.len(),
            output.frames * MODEL_DIM as usize
        );
    }

    #[test]
    fn estimator_positions_match_mixed_rope_contract() {
        let (global, local) = estimator_positions(&[0, 1, 1, 2], 2).unwrap();
        assert_eq!(global, vec![0, 1, 0, 1, 2, 3]);
        assert_eq!(local, vec![0, 0, 0, 1, 2, 1]);
    }

    #[test]
    fn estimator_mask_connects_streams_and_matching_regions() {
        let mask = joint_attention_mask(&[0, 1, 1, 2], 2).unwrap();
        let total = 6;
        let at = |query: usize, key: usize| mask[query * total + key];
        assert_eq!(at(0, 1), 0.0);
        assert_eq!(at(0, 3), 0.0);
        assert_eq!(at(0, 5), -10_000.0);
        assert_eq!(at(3, 4), 0.0);
        assert_eq!(at(3, 5), 0.0);
        assert_eq!(at(2, 2), -10_000.0);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and GAME GGUF"]
    fn game_encoder_runs_on_explicit_device() {
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
        let mel = (0..80 * 16)
            .map(|index| ((index as f32) * 0.017).sin() * 3.0 - 5.0)
            .collect::<Vec<_>>();
        let output = model.encode_mel(&mel, 16).unwrap();
        assert_eq!(output.segmenter_embeddings.len(), 16 * MODEL_DIM as usize);
        assert_eq!(output.estimator_embeddings.len(), 16 * MODEL_DIM as usize);
        assert!(
            output
                .segmenter_embeddings
                .iter()
                .all(|value| value.is_finite())
        );
        assert!(
            output
                .estimator_embeddings
                .iter()
                .all(|value| value.is_finite())
        );
        let noise_mod = (0..output.frames)
            .map(|frame| (frame % model.config().region_cycle_length) as i32)
            .collect::<Vec<_>>();
        let segmenter = model
            .segmenter_logits(&output.segmenter_embeddings, &noise_mod, 0.5, 0)
            .unwrap();
        assert_eq!(segmenter.len(), output.frames);
        assert!(segmenter.iter().all(|value| value.is_finite()));
        if let Some(path) = std::env::var_os("UTA_TEST_GAME_SEGMENTER_OUTPUT") {
            let mut bytes = Vec::with_capacity(segmenter.len() * 4);
            for value in segmenter {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            std::fs::write(path, bytes).unwrap();
        }
        if let Some(path) = std::env::var_os("UTA_TEST_GAME_ENCODER_OUTPUT") {
            let mut bytes = Vec::with_capacity(2 * 16 * MODEL_DIM as usize * 4);
            for value in output
                .segmenter_embeddings
                .iter()
                .chain(&output.estimator_embeddings)
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            std::fs::write(path, bytes).unwrap();
        }
        let regions = [0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3];
        let estimator = model
            .estimate_pitch(&output.estimator_embeddings, &regions)
            .unwrap();
        assert_eq!(estimator.regions, 3);
        assert_eq!(estimator.bins, 257);
        assert_eq!(estimator.pool_logits.len(), 3 * 257);
        assert!(estimator.pool_logits.iter().all(|value| value.is_finite()));
        if let Some(path) = std::env::var_os("UTA_TEST_GAME_ESTIMATOR_OUTPUT") {
            let mut bytes = Vec::with_capacity(estimator.pool_logits.len() * 4);
            for value in estimator.pool_logits {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            std::fs::write(path, bytes).unwrap();
        }
        let inference = model
            .infer_mel(
                &mel,
                16,
                &super::super::GameInferParams {
                    seed: 42,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(inference.num_frames, 16);
        assert!(
            inference
                .notes
                .iter()
                .all(|note| note.pitch_midi.is_finite())
        );
        if let Some(path) = std::env::var_os("UTA_TEST_GAME_INFER_OUTPUT") {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(inference.boundaries.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&inference.boundaries);
            bytes.extend_from_slice(&(inference.notes.len() as u32).to_le_bytes());
            for note in inference.notes {
                bytes.extend_from_slice(&note.offset_seconds.to_le_bytes());
                bytes.extend_from_slice(&note.duration_seconds.to_le_bytes());
                bytes.extend_from_slice(&note.pitch_midi.to_le_bytes());
                bytes.push(u8::from(note.voiced));
            }
            std::fs::write(path, bytes).unwrap();
        }
        eprintln!(
            "ran GAME encoder on {} ({:?})",
            device.description, device.kind
        );
    }
}
