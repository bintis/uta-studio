use std::sync::Arc;

use super::frame::{GraphRun, get_f32, set_f32};
use super::model::{HIDDEN_DIM, Rosvot, tensor_ref};
use crate::ffi::{ContextPtr, GGML_PREC_F32, GGML_TYPE_F32, TensorPtr};

const LAYER_NORM_EPSILON: f32 = 1.0e-5;
const LEAKY_RELU_SLOPE: f32 = 0.01;
const HEADS: i64 = 4;
const HEAD_DIM: i64 = 64;
const POOL_AVG: u32 = 1;
const RELATIVE_POSITION_MAX_LEN: usize = 5_000;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: pointers belong to the live ROSVOT model and graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

impl Rosvot {
    /// Runs the ROSVOT U-Net and two-layer dense-FFN Conformer bottleneck.
    pub fn encode_backbone(
        &self,
        embedded: &[f32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<Vec<f32>, String> {
        if frames == 0
            || frames % 16 != 0
            || valid_frames == 0
            || valid_frames > frames
            || embedded.len() != frames * HIDDEN_DIM
            || embedded.iter().any(|value| !value.is_finite())
        {
            return Err("ROSVOT backbone input is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
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
        ggml!(api, ggml_set_input(relative_positions));
        let output =
            self.local_style_cmu(run.context, input, relative_positions, frames, "net.net", 2)?;
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, input, embedded)?;
        set_f32(
            api,
            relative_positions,
            &relative_position_values(bottleneck_frames),
        )?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    pub(super) fn encode_conv_block_values(
        &self,
        input: &[f32],
        rows: usize,
        prefix: &str,
    ) -> Result<Vec<f32>, String> {
        if rows == 0
            || input.len() != rows * HIDDEN_DIM
            || input.iter().any(|value| !value.is_finite())
        {
            return Err("ROSVOT ConvBlocks input is invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let tensor = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, rows as i64)
        );
        ggml!(api, ggml_set_input(tensor));
        let mut output = self.residual_block(
            run.context,
            &format!("{prefix}.res_blocks.0.blocks.0"),
            tensor,
        )?;
        output = self.layer_norm_features(run.context, &format!("{prefix}.last_norm"), output)?;
        output = self.conv_features(run.context, &format!("{prefix}.post_net1"), output, 1)?;
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, tensor, input)?;
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
        if rows == 0
            || input_dimensions == 0
            || output_dimensions == 0
            || input.len() != rows * input_dimensions
            || input.iter().any(|value| !value.is_finite())
        {
            return Err("ROSVOT linear projection input is invalid".to_string());
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
        let output = self.linear(run.context, prefix, tensor, true)?;
        let shape = tensor_ref(output)?.ne;
        if shape[0] != output_dimensions as i64 || shape[1] != rows as i64 {
            return Err(format!(
                "ROSVOT {prefix} output shape is invalid: {shape:?}"
            ));
        }
        ggml!(api, ggml_set_output(output));
        ggml!(api, ggml_build_forward_expand(run.graph, output));
        run.allocate(&self.backend)?;
        set_f32(api, tensor, input)?;
        run.compute(&self.backend)?;
        get_f32(api, output)
    }

    fn local_style_cmu(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        relative_positions: TensorPtr,
        frames: usize,
        adaptor: &str,
        conformer_layers: usize,
    ) -> Result<TensorPtr, String> {
        let prefix = adaptor.to_string();
        let mut current = input;
        let mut current_frames = frames;
        let mut skips = Vec::with_capacity(4);
        for stage in 0..4 {
            let stage_prefix = format!("{prefix}.down.layers.{stage}");
            current =
                self.residual_block(context, &format!("{stage_prefix}.0.blocks.0"), current)?;
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
            let branch = self.feed_forward_dense(
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
            let branch = self.feed_forward_dense(
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

    fn feed_forward_dense(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        _frames: usize,
    ) -> Result<TensorPtr, String> {
        let hidden = self.linear(context, &format!("{prefix}.w_1"), input, true)?;
        let hidden = ggml!(self.api(), ggml_relu(context, hidden));
        self.linear(context, &format!("{prefix}.w_2"), hidden, true)
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
        let api = self.api();
        let normalized = self.layer_norm_features(context, &format!("{prefix}.0"), input)?;
        let expanded = self.conv_features(context, &format!("{prefix}.1"), normalized, 1)?;
        let expanded = ggml!(
            api,
            ggml_scale_bias(context, expanded, 3.0_f32.powf(-0.5), 0.0)
        );
        let activated = ggml!(
            api,
            ggml_leaky_relu(context, expanded, LEAKY_RELU_SLOPE, false)
        );
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
            // ROSVOT' pointwise Conv1d experts are semantically linear but are
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
                "ROSVOT linear {prefix} shape mismatch: weight {weight_shape:?}, input {input_shape:?}"
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
                "ROSVOT transposed-convolution shape is invalid: {:?}",
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
        Err(format!("ROSVOT {label} shape is invalid: {shape:?}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_positions_use_the_fixed_training_origin() {
        let values = relative_position_values(2);
        assert_eq!(values.len(), 2 * HIDDEN_DIM);
        assert_eq!(values[0], 4_999.0_f32.sin());
        assert_eq!(values[HIDDEN_DIM], 4_998.0_f32.sin());
    }
}
