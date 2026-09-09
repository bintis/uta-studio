use std::ffi::c_void;
use std::sync::Arc;

use crate::ffi::{
    AllocatorPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GGML_TYPE_I32,
    GgmlInitParams, GraphPtr, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

use super::weights::tensor_ref;
use super::{
    D_INNER, D_K, D_MODEL, ENCODER_FRAMES, FEATURE_FRAMES, FireRed, KERNEL_SIZE, MEL_BINS, N_HEAD,
    N_LAYERS_ENC, SUBSAMPLE_PAD_FRAMES,
};

const GRAPH_MEMORY_BYTES: usize = 128 * 1024 * 1024;
const GRAPH_NODES: usize = 32_768;
const EPSILON: f32 = 1.0e-5;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: graph tensors and the model backend remain live for this call.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone)]
pub struct EncodedAudio {
    pub values: Vec<f32>,
    pub rows: usize,
    pub width: usize,
}

struct EncoderGraph {
    output: TensorPtr,
    relative_indices: TensorPtr,
    #[cfg(test)]
    stages: Vec<(String, TensorPtr)>,
}

/// Every field beyond `output` exists for the encoder parity tests; the
/// production graph reads only the block output.
#[cfg_attr(not(test), allow(dead_code))]
struct AttentionDiagnostics {
    output: TensorPtr,
    input: TensorPtr,
    query: TensorPtr,
    key: TensorPtr,
    position: TensorPtr,
    matrix_ac: TensorPtr,
    matrix_bd: TensorPtr,
    attention: TensorPtr,
    context: TensorPtr,
}

impl FireRed {
    pub fn encode(&self, features: &[f32]) -> Result<EncodedAudio, String> {
        if features.len() != FEATURE_FRAMES * MEL_BINS
            || features.iter().any(|value| !value.is_finite())
        {
            return Err(format!(
                "FireRed encoder expects {FEATURE_FRAMES} finite 80-bin feature frames"
            ));
        }
        let padded_frames = FEATURE_FRAMES + SUBSAMPLE_PAD_FRAMES;
        let mut padded = vec![0.0_f32; padded_frames * MEL_BINS];
        padded[..features.len()].copy_from_slice(features);
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_4d(
                run.context,
                GGML_TYPE_F32,
                MEL_BINS as i64,
                padded_frames as i64,
                1,
                1
            )
        );
        ggml!(api, ggml_set_input(input));
        let graph = self.build_encoder_graph(run.context, input)?;
        ggml!(api, ggml_set_input(graph.relative_indices));
        ggml!(api, ggml_set_output(graph.output));
        #[cfg(test)]
        for (_, tensor) in &graph.stages {
            ggml!(api, ggml_set_output(*tensor));
        }
        ggml!(api, ggml_build_forward_expand(run.graph, graph.output));
        run.allocate(&self.backend)?;
        set_f32(api, input, &padded)?;
        set_i32(api, graph.relative_indices, &relative_shift_indices())?;
        run.compute(&self.backend)?;
        #[cfg(test)]
        if let Some(directory) = std::env::var_os("UTA_TEST_FIRERED_ENCODER_DEBUG_DIR") {
            for (name, tensor) in &graph.stages {
                let values = get_f32(api, *tensor)?;
                let bytes: Vec<u8> = values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                std::fs::write(std::path::PathBuf::from(&directory).join(name), bytes).map_err(
                    |error| format!("could not write FireRed encoder diagnostic: {error}"),
                )?;
            }
        }
        let values = get_f32(api, graph.output)?;
        if values.len() != ENCODER_FRAMES * D_MODEL {
            return Err("FireRed encoder output tensor shape is invalid".to_string());
        }
        Ok(EncodedAudio {
            values,
            rows: ENCODER_FRAMES,
            width: D_MODEL,
        })
    }

    fn build_encoder_graph(
        &self,
        context: ContextPtr,
        input: TensorPtr,
    ) -> Result<EncoderGraph, String> {
        let api = self.api();
        let conv0 = self.weight("encoder.input_preprocessor.conv.0.weight")?;
        #[cfg(test)]
        let mut conv0_columns = None;
        let mut hidden = self.conv_2d_f32(
            context,
            conv0,
            input,
            2,
            #[cfg(test)]
            &mut conv0_columns,
        )?;
        hidden = add_conv_bias(
            self,
            context,
            hidden,
            "encoder.input_preprocessor.conv.0.bias",
        )?;
        hidden = ggml!(api, ggml_relu(context, hidden));
        #[cfg(test)]
        let conv0_output = hidden;
        let conv2 = self.weight("encoder.input_preprocessor.conv.2.weight")?;
        #[cfg(test)]
        let mut conv2_columns = None;
        hidden = self.conv_2d_f32(
            context,
            conv2,
            hidden,
            2,
            #[cfg(test)]
            &mut conv2_columns,
        )?;
        hidden = add_conv_bias(
            self,
            context,
            hidden,
            "encoder.input_preprocessor.conv.2.bias",
        )?;
        hidden = ggml!(api, ggml_relu(context, hidden));
        #[cfg(test)]
        let conv2_output = hidden;
        let shape = tensor_ref(hidden)?.ne;
        if shape[..3] != [19, ENCODER_FRAMES as i64, 32] {
            return Err(format!(
                "FireRed convolution output shape is invalid: {shape:?}"
            ));
        }
        // `[frequency, time, channel]` -> contiguous `[frequency, channel, time]`.
        hidden = ggml!(api, ggml_permute(context, hidden, 0, 2, 1, 3));
        hidden = ggml!(api, ggml_cont(context, hidden));
        hidden = ggml!(
            api,
            ggml_reshape_2d(context, hidden, 608, ENCODER_FRAMES as i64)
        );
        hidden = self.linear(context, "encoder.input_preprocessor.out.weight", hidden)?;
        #[cfg(test)]
        let mut stages = vec![
            ("encoder-conv0.f32le".to_string(), conv0_output),
            ("encoder-conv2.f32le".to_string(), conv2_output),
            ("encoder-subsample.f32le".to_string(), hidden),
            (
                "encoder-conv0-columns.f32le".to_string(),
                conv0_columns.ok_or("FireRed conv0 columns stage is missing")?,
            ),
            (
                "encoder-conv2-columns.f32le".to_string(),
                conv2_columns.ok_or("FireRed conv2 columns stage is missing")?,
            ),
        ];
        let position = self.position_slice(context)?;
        let relative_indices = ggml!(
            api,
            ggml_new_tensor_1d(
                context,
                GGML_TYPE_I32,
                (ENCODER_FRAMES * ENCODER_FRAMES * N_HEAD) as i64
            )
        );
        for layer in 0..N_LAYERS_ENC {
            let (next, _attention) =
                self.conformer_block(context, hidden, position, relative_indices, layer)?;
            hidden = next;
            #[cfg(test)]
            {
                if layer == 0 {
                    stages.extend([
                        (
                            "encoder-layer-0-attention-input.f32le".to_string(),
                            _attention.input,
                        ),
                        ("encoder-layer-0-query.f32le".to_string(), _attention.query),
                        ("encoder-layer-0-key.f32le".to_string(), _attention.key),
                        (
                            "encoder-layer-0-position.f32le".to_string(),
                            _attention.position,
                        ),
                        ("encoder-layer-0-ac.f32le".to_string(), _attention.matrix_ac),
                        ("encoder-layer-0-bd.f32le".to_string(), _attention.matrix_bd),
                        (
                            "encoder-layer-0-attention.f32le".to_string(),
                            _attention.attention,
                        ),
                        (
                            "encoder-layer-0-context.f32le".to_string(),
                            _attention.context,
                        ),
                    ]);
                }
                stages.push((format!("encoder-layer-{layer}.f32le"), hidden));
            }
        }
        Ok(EncoderGraph {
            output: hidden,
            relative_indices,
            #[cfg(test)]
            stages,
        })
    }

    fn conformer_block(
        &self,
        context: ContextPtr,
        mut hidden: TensorPtr,
        position: TensorPtr,
        relative_indices: TensorPtr,
        layer: usize,
    ) -> Result<(TensorPtr, AttentionDiagnostics), String> {
        let prefix = format!("encoder.layer_stack.{layer}");
        hidden = self.conformer_ffn(context, hidden, &format!("{prefix}.ffn1"))?;
        let attention = self.relative_attention(
            context,
            hidden,
            position,
            relative_indices,
            &format!("{prefix}.mhsa"),
        )?;
        hidden = attention.output;
        hidden = self.conformer_convolution(context, hidden, &format!("{prefix}.conv"))?;
        hidden = self.conformer_ffn(context, hidden, &format!("{prefix}.ffn2"))?;
        hidden = self.layer_norm(context, hidden, &format!("{prefix}.layer_norm.weight"))?;
        Ok((hidden, attention))
    }

    fn conformer_ffn(
        &self,
        context: ContextPtr,
        hidden: TensorPtr,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normed = self.layer_norm(context, hidden, &format!("{prefix}.net.0.weight"))?;
        let expanded = self.linear(context, &format!("{prefix}.net.1.weight"), normed)?;
        let activated = ggml!(api, ggml_silu(context, expanded));
        let projected = self.linear(context, &format!("{prefix}.net.4.weight"), activated)?;
        let projected = ggml!(api, ggml_scale_bias(context, projected, 0.5, 0.0));
        Ok(ggml!(api, ggml_add(context, hidden, projected)))
    }

    fn relative_attention(
        &self,
        context: ContextPtr,
        hidden: TensorPtr,
        position: TensorPtr,
        relative_indices: TensorPtr,
        prefix: &str,
    ) -> Result<AttentionDiagnostics, String> {
        let api = self.api();
        let query = self.layer_norm(context, hidden, &format!("{prefix}.layer_norm_q.weight"))?;
        let key = self.layer_norm(context, hidden, &format!("{prefix}.layer_norm_k.weight"))?;
        let value = self.layer_norm(context, hidden, &format!("{prefix}.layer_norm_v.weight"))?;
        let query = self.linear(context, &format!("{prefix}.w_qs.weight"), query)?;
        let query_projection = query;
        let query = self.attention_heads(context, query, ENCODER_FRAMES);
        let key = self.linear(context, &format!("{prefix}.w_ks.weight"), key)?;
        let key_projection = key;
        let key = self.attention_heads(context, key, ENCODER_FRAMES);
        let value = self.attention_heads(
            context,
            self.linear(context, &format!("{prefix}.w_vs.weight"), value)?,
            ENCODER_FRAMES,
        );
        let position = self.linear(context, &format!("{prefix}.linear_pos.weight"), position)?;
        let position_projection = position;
        let position = self.attention_heads(context, position, 2 * ENCODER_FRAMES - 1);
        let query_u = add_attention_bias(self, context, query, &format!("{prefix}.pos_bias_u"))?;
        let query_v = add_attention_bias(self, context, query, &format!("{prefix}.pos_bias_v"))?;
        let matrix_ac = ggml!(api, ggml_mul_mat(context, key, query_u));
        let matrix_bd = ggml!(api, ggml_mul_mat(context, position, query_v));
        let matrix_bd = ggml!(
            api,
            ggml_reshape_2d(
                context,
                matrix_bd,
                1,
                ((2 * ENCODER_FRAMES - 1) * ENCODER_FRAMES * N_HEAD) as i64
            )
        );
        let matrix_bd = ggml!(api, ggml_get_rows(context, matrix_bd, relative_indices));
        let matrix_bd = ggml!(
            api,
            ggml_reshape_3d(
                context,
                matrix_bd,
                ENCODER_FRAMES as i64,
                ENCODER_FRAMES as i64,
                N_HEAD as i64
            )
        );
        let scores = ggml!(api, ggml_add(context, matrix_ac, matrix_bd));
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
            ggml_reshape_2d(context, attended, D_MODEL as i64, ENCODER_FRAMES as i64)
        );
        let projected = self.linear(context, &format!("{prefix}.fc.weight"), attended)?;
        Ok(AttentionDiagnostics {
            output: ggml!(api, ggml_add(context, hidden, projected)),
            input: hidden,
            query: query_projection,
            key: key_projection,
            position: position_projection,
            matrix_ac,
            matrix_bd,
            attention,
            context: attended,
        })
    }

    fn conformer_convolution(
        &self,
        context: ContextPtr,
        hidden: TensorPtr,
        prefix: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normed =
            self.layer_norm(context, hidden, &format!("{prefix}.pre_layer_norm.weight"))?;
        let expanded = self.linear(context, &format!("{prefix}.pointwise_conv1.weight"), normed)?;
        let stride = tensor_ref(expanded)?.nb[1];
        let first = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                (D_INNER / 2) as i64,
                ENCODER_FRAMES as i64,
                stride,
                0
            )
        );
        let second = ggml!(
            api,
            ggml_view_2d(
                context,
                expanded,
                (D_INNER / 2) as i64,
                ENCODER_FRAMES as i64,
                stride,
                (D_INNER / 2) * std::mem::size_of::<f32>()
            )
        );
        let gate = ggml!(api, ggml_sigmoid(context, second));
        let glu = ggml!(api, ggml_mul(context, first, gate));
        let convolution_input = ggml!(api, ggml_transpose(context, glu));
        let convolution_input = ggml!(api, ggml_cont(context, convolution_input));
        let convolution_input = ggml!(
            api,
            ggml_reshape_3d(
                context,
                convolution_input,
                ENCODER_FRAMES as i64,
                (D_INNER / 2) as i64,
                1
            )
        );
        let weight = self.weight(&format!("{prefix}.depthwise_conv.weight"))?;
        let convolved = ggml!(
            api,
            ggml_conv_1d_dw(
                context,
                weight,
                convolution_input,
                1,
                (KERNEL_SIZE / 2) as i32,
                1
            )
        );
        let convolved = ggml!(
            api,
            ggml_reshape_2d(
                context,
                convolved,
                ENCODER_FRAMES as i64,
                (D_INNER / 2) as i64
            )
        );
        let convolved = ggml!(api, ggml_transpose(context, convolved));
        let convolved = ggml!(api, ggml_cont(context, convolved));
        let normed = self.layer_norm(context, convolved, &format!("{prefix}.batch_norm.weight"))?;
        let activated = ggml!(api, ggml_silu(context, normed));
        let projected = self.linear(
            context,
            &format!("{prefix}.pointwise_conv2.weight"),
            activated,
        )?;
        Ok(ggml!(api, ggml_add(context, hidden, projected)))
    }

    fn attention_heads(&self, context: ContextPtr, tensor: TensorPtr, rows: usize) -> TensorPtr {
        let api = self.api();
        let tensor = ggml!(
            api,
            ggml_reshape_3d(context, tensor, D_K as i64, N_HEAD as i64, rows as i64)
        );
        let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
        ggml!(api, ggml_cont(context, tensor))
    }

    fn position_slice(&self, context: ContextPtr) -> Result<TensorPtr, String> {
        let api = self.api();
        let position = self.weight("encoder.positional_encoding.pe")?;
        let descriptor = tensor_ref(position)?;
        if descriptor.ne[0] != D_MODEL as i64 || descriptor.ne[1] != 9_999 {
            return Err("FireRed relative position tensor shape is invalid".to_string());
        }
        let start = 9_999 / 2 - ENCODER_FRAMES + 1;
        Ok(ggml!(
            api,
            ggml_view_2d(
                context,
                position,
                D_MODEL as i64,
                (2 * ENCODER_FRAMES - 1) as i64,
                descriptor.nb[1],
                start * descriptor.nb[1]
            )
        ))
    }

    fn layer_norm(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        weight_name: &str,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let normalized = ggml!(api, ggml_norm(context, input, EPSILON));
        let weight = self.weight(weight_name)?;
        let weight = ggml!(api, ggml_repeat(context, weight, normalized));
        let normalized = ggml!(api, ggml_mul(context, normalized, weight));
        let prefix = weight_name
            .strip_suffix(".weight")
            .ok_or_else(|| format!("FireRed layer norm weight name is invalid: {weight_name}"))?;
        let bias = self.weight(&format!("{prefix}.bias"))?;
        let bias = ggml!(api, ggml_repeat(context, bias, normalized));
        Ok(ggml!(api, ggml_add(context, normalized, bias)))
    }

    /// Upstream `ggml_conv_2d` rounds its im2col patches to F16, which costs
    /// roughly 1e-3 of relative accuracy. FireRed feeds these two subsampling
    /// convolutions into a sixteen-layer conformer, so they keep F32 patches
    /// and F32 accumulation instead.
    fn conv_2d_f32(
        &self,
        context: ContextPtr,
        kernel: TensorPtr,
        input: TensorPtr,
        stride: std::ffi::c_int,
        #[cfg(test)] columns_stage: &mut Option<TensorPtr>,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let columns = ggml!(
            api,
            ggml_im2col(
                context,
                kernel,
                input,
                stride,
                stride,
                0,
                0,
                1,
                1,
                true,
                GGML_TYPE_F32
            )
        );
        let columns_shape = tensor_ref(columns)?.ne;
        let kernel_shape = tensor_ref(kernel)?.ne;
        let flat_columns = ggml!(
            api,
            ggml_reshape_2d(
                context,
                columns,
                columns_shape[0],
                columns_shape[3] * columns_shape[2] * columns_shape[1]
            )
        );
        let flat_kernel = ggml!(
            api,
            ggml_reshape_2d(
                context,
                kernel,
                kernel_shape[0] * kernel_shape[1] * kernel_shape[2],
                kernel_shape[3]
            )
        );
        let product = ggml!(api, ggml_mul_mat(context, flat_columns, flat_kernel));
        ggml!(api, ggml_mul_mat_set_prec(product, GGML_PREC_F32));
        let product = ggml!(
            api,
            ggml_reshape_4d(
                context,
                product,
                columns_shape[1],
                columns_shape[2],
                columns_shape[3],
                kernel_shape[3]
            )
        );
        #[cfg(test)]
        {
            *columns_stage = Some(columns);
        }
        let product = ggml!(api, ggml_permute(context, product, 0, 1, 3, 2));
        Ok(ggml!(api, ggml_cont(context, product)))
    }

    fn linear(
        &self,
        context: ContextPtr,
        weight_name: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let input_dimension = tensor_ref(input)?.ne[0];
        let mut weight = self.weight(weight_name)?;
        let descriptor = tensor_ref(weight)?;
        if descriptor.ne[0] == 1 && descriptor.ne[1] == input_dimension {
            weight = ggml!(
                api,
                ggml_reshape_2d(context, weight, descriptor.ne[1], descriptor.ne[2])
            );
        }
        if tensor_ref(weight)?.ne[0] != input_dimension {
            return Err(format!(
                "FireRed linear input mismatch for {weight_name}: expected {}, found {input_dimension}",
                tensor_ref(weight)?.ne[0]
            ));
        }
        let projected = ggml!(api, ggml_mul_mat(context, weight, input));
        // FireRed's reference `Linear` modules apply their bias. Only the
        // subsample output projection lost it here, but every projection is
        // covered so a future checkpoint cannot silently drop one again.
        let Some(prefix) = weight_name.strip_suffix(".weight") else {
            return Ok(projected);
        };
        match self.optional_weight(&format!("{prefix}.bias"))? {
            Some(bias) => Ok(ggml!(api, ggml_add(context, projected, bias))),
            None => Ok(projected),
        }
    }
}

fn add_conv_bias(
    model: &FireRed,
    context: ContextPtr,
    input: TensorPtr,
    name: &str,
) -> Result<TensorPtr, String> {
    let api = model.api();
    let bias = model.weight(name)?;
    let channels = tensor_ref(bias)?.ne[0];
    let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
    Ok(ggml!(api, ggml_add(context, input, bias)))
}

fn add_attention_bias(
    model: &FireRed,
    context: ContextPtr,
    input: TensorPtr,
    name: &str,
) -> Result<TensorPtr, String> {
    let api = model.api();
    let bias = model.weight(name)?;
    let bias = ggml!(
        api,
        ggml_reshape_3d(context, bias, D_K as i64, 1, N_HEAD as i64)
    );
    let bias = ggml!(api, ggml_repeat(context, bias, input));
    Ok(ggml!(api, ggml_add(context, input, bias)))
}

fn relative_shift_indices() -> Vec<i32> {
    let rows = ENCODER_FRAMES;
    let relative_rows = 2 * rows - 1;
    let mut indices = Vec::with_capacity(rows * rows * N_HEAD);
    for head in 0..N_HEAD {
        for query in 0..rows {
            for key in 0..rows {
                let relative = key + rows - 1 - query;
                indices
                    .push((relative + relative_rows * query + relative_rows * rows * head) as i32);
            }
        }
    }
    indices
}

pub(super) fn set_f32(
    api: &crate::ffi::ModelApi,
    tensor: TensorPtr,
    values: &[f32],
) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("FireRed F32 input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

pub(super) fn set_i32(
    api: &crate::ffi::ModelApi,
    tensor: TensorPtr,
    values: &[i32],
) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<i32>() {
        return Err("FireRed I32 input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

pub(super) fn get_f32(api: &crate::ffi::ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "FireRed tensor element count is invalid".to_string())?;
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

pub(super) struct GraphRun {
    runtime: Arc<GgmlRuntime>,
    pub(super) context: ContextPtr,
    pub(super) graph: GraphPtr,
    allocator: AllocatorPtr,
}

impl GraphRun {
    pub(super) fn new(runtime: Arc<GgmlRuntime>) -> Result<Self, String> {
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
            return Err("could not allocate FireRed GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_NODES, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate FireRed GGML graph".to_string());
        }
        Ok(Self {
            runtime,
            context,
            graph,
            allocator: std::ptr::null_mut(),
        })
    }

    pub(super) fn allocate(&mut self, backend: &GgmlBackendHandle) -> Result<(), String> {
        let api = &self.runtime.model_api;
        let buffer_type = ggml!(api, ggml_backend_get_default_buffer_type(backend.raw));
        self.allocator = ggml!(api, ggml_gallocr_new(buffer_type));
        if self.allocator.is_null()
            || !ggml!(api, ggml_gallocr_reserve(self.allocator, self.graph))
            || !ggml!(api, ggml_gallocr_alloc_graph(self.allocator, self.graph))
        {
            return Err("could not allocate FireRed GGML graph".to_string());
        }
        Ok(())
    }

    pub(super) fn compute(&self, backend: &GgmlBackendHandle) -> Result<(), String> {
        let status = ggml!(
            &self.runtime.model_api,
            ggml_backend_graph_compute(backend.raw, self.graph)
        );
        if status == GGML_STATUS_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "FireRed GGML graph compute failed with status {status}"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;
    use std::path::{Path, PathBuf};

    fn path(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("set {name}"))
    }

    fn mono_wav(path: &Path) -> Vec<f32> {
        let mut reader = hound::WavReader::open(path).unwrap();
        let specification = reader.spec();
        assert_eq!(specification.channels, 1);
        assert_eq!(specification.sample_rate, 16_000);
        match specification.sample_format {
            hound::SampleFormat::Int => {
                let scale = (1_u64 << (specification.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|sample| sample.unwrap() as f32 / scale)
                    .collect()
            }
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|sample| sample.unwrap())
                .collect(),
        }
    }

    #[test]
    fn relative_shift_indices_match_transformer_xl_slice() {
        let indices = relative_shift_indices();
        let relative_rows = 2 * ENCODER_FRAMES - 1;
        // GGML flattens the source as [relative, query, head]. For query q
        // and key k, Transformer-XL selects relative row T - 1 + k - q.
        assert_eq!(indices[0], (ENCODER_FRAMES - 1) as i32);
        assert_eq!(indices[ENCODER_FRAMES - 1], (relative_rows - 1) as i32);
        assert_eq!(
            indices[ENCODER_FRAMES],
            (relative_rows + ENCODER_FRAMES - 2) as i32
        );
        assert_eq!(
            indices[ENCODER_FRAMES * ENCODER_FRAMES - 1],
            (relative_rows * ENCODER_FRAMES - ENCODER_FRAMES) as i32
        );
    }

    /// Measures one plain F32 matrix multiply on the selected device against an
    /// exact f64 reference, using the same shape as FireRed's first subsampling
    /// convolution. It isolates backend matmul accuracy from any model graph.
    #[test]
    #[ignore = "requires an explicit packaged runtime and device"]
    fn selected_backend_multiplies_f32_matrices_accurately() {
        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let expected_kind = match std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND")
            .as_str()
        {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            "discrete_gpu" => DeviceKind::DiscreteGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION").unwrap_or_default();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested test device is unavailable");
        let backend = runtime.create_backend(&device).unwrap();

        let dimension = |name: &str, fallback: usize| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(fallback)
        };
        let depth = dimension("UTA_TEST_MATMUL_K", 9);
        let rows = dimension("UTA_TEST_MATMUL_M", 4_446);
        let columns = dimension("UTA_TEST_MATMUL_N", 32);
        let value = |index: usize| {
            let x = (index as f32).mul_add(0.37, 1.0).sin();
            x * 1.5
        };
        let left = (0..depth * rows).map(value).collect::<Vec<_>>();
        let right = (0..depth * columns)
            .map(|index| value(index + 7))
            .collect::<Vec<_>>();

        let mut run = GraphRun::new(Arc::clone(&runtime)).unwrap();
        let api = &runtime.model_api;
        let a = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, depth as i64, rows as i64)
        );
        let b = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, depth as i64, columns as i64)
        );
        let product = ggml!(api, ggml_mul_mat(run.context, a, b));
        if std::env::var_os("UTA_TEST_MATMUL_PREC_F32").is_some() {
            ggml!(api, ggml_mul_mat_set_prec(product, GGML_PREC_F32));
        }
        ggml!(api, ggml_build_forward_expand(run.graph, product));
        run.allocate(&backend).unwrap();
        set_f32(api, a, &left).unwrap();
        set_f32(api, b, &right).unwrap();
        run.compute(&backend).unwrap();
        let measured = get_f32(api, product).unwrap();
        assert_eq!(measured.len(), rows * columns);

        let mut worst = 0.0_f64;
        for column in 0..columns {
            for row in 0..rows {
                let mut exact = 0.0_f64;
                for index in 0..depth {
                    exact += f64::from(left[row * depth + index])
                        * f64::from(right[column * depth + index]);
                }
                let actual = f64::from(measured[column * rows + row]);
                let scale = exact.abs().max(1.0);
                worst = worst.max((actual - exact).abs() / scale);
            }
        }
        eprintln!(
            "F32 matmul m={rows} n={columns} k={depth} worst relative error on {}: {worst:.3e}",
            device.description
        );
        assert!(
            worst < 1.0e-5,
            "F32 matmul lost accuracy on {}: {worst:.3e}",
            device.description
        );
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, model, WAV, and CMVN"]
    fn actual_firered_encoder_runs_on_selected_backend() {
        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let expected_kind = match std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND")
            .as_str()
        {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            "discrete_gpu" => DeviceKind::DiscreteGpu,
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
            .expect("requested FireRed test device is unavailable");
        let model = FireRed::load(runtime, &device, &path("UTA_TEST_FIRERED_GGUF")).unwrap();
        let samples = mono_wav(&path("UTA_TEST_FIRERED_WAV"));
        assert!(
            (super::super::MIN_WINDOW_SAMPLES..=super::super::MAX_WINDOW_SAMPLES)
                .contains(&samples.len())
        );
        let cmvn = std::fs::read(path("UTA_TEST_FIRERED_CMVN")).unwrap();
        let (features, frames) = super::super::extract_features(&samples, &cmvn).unwrap();
        assert_eq!(frames, FEATURE_FRAMES);
        let reference_feature_bytes = std::fs::read(path("UTA_TEST_FIRERED_FEATURES")).unwrap();
        let reference_features: Vec<f32> = reference_feature_bytes
            .chunks_exact(4)
            .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
            .collect();
        let feature_difference = features
            .iter()
            .zip(&reference_features)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            feature_difference <= 3.0e-6,
            "feature difference: {feature_difference}"
        );
        let encoded = model.encode(&features).unwrap();
        assert_eq!((encoded.rows, encoded.width), (ENCODER_FRAMES, D_MODEL));
        assert!(encoded.values.iter().all(|value| value.is_finite()));
        let reference_bytes = std::fs::read(path("UTA_TEST_FIRERED_ENCODER")).unwrap();
        let reference_values: Vec<f32> = reference_bytes
            .chunks_exact(4)
            .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
            .collect();
        let reference = EncodedAudio {
            values: reference_values,
            rows: ENCODER_FRAMES,
            width: D_MODEL,
        };
        let expected = [super::super::SOS, 1202, 2246, 1019, 4710, super::super::EOS];
        assert_eq!(model.greedy_decode(&reference).unwrap(), expected);
        let max_difference = encoded
            .values
            .iter()
            .zip(&reference.values)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0_f32, f32::max);
        eprintln!("FireRed encoder max reference difference: {max_difference:.8}");
        assert_eq!(model.greedy_decode(&encoded).unwrap(), expected);
    }
}
