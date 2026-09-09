use std::ffi::c_void;
use std::sync::Arc;

use super::model::{HIDDEN_DIM, MEL_BINS, Rosvot, tensor_ref};
use crate::ffi::{
    AllocatorPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F32, GGML_TYPE_I32,
    GgmlInitParams, GraphPtr, ModelApi, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

const GRAPH_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const GRAPH_NODES: usize = 2_048;
const LAYER_NORM_EPSILON: f32 = 1.0e-5;
const LEAKY_RELU_SLOPE: f32 = 0.01;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: pointers belong to the live ROSVOT model and graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct MelEncoding {
    /// Frame-major `[frames, 256]` values after `mel_proj`.
    pub projected: Vec<f32>,
    /// Frame-major `[frames, 256]` values after `mel_encoder`.
    pub encoded: Vec<f32>,
    pub frames: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MelPitchEncoding {
    pub mel: MelEncoding,
    /// Frame-major `[frames, 256]` mel, coarse-pitch, and UV embedding sum.
    pub embedded: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConditionEncoding {
    pub mel: MelEncoding,
    /// Frame-major mel, pitch, UV, and transcript-boundary embedding sum.
    pub embedded: Vec<f32>,
    /// Frame-major output from `cond_encoder`.
    pub conditioned: Vec<f32>,
}

impl Rosvot {
    /// Runs the ROSVOT mel frontend graph.
    ///
    /// Input is frame-major `[frames, 40]`, the lower half of the shared
    /// STARS 24 kHz magnitude-mel representation.
    pub fn encode_mel(&self, mel: &[f32], frames: usize) -> Result<MelEncoding, String> {
        if frames == 0 || mel.len() != frames * MEL_BINS {
            return Err("ROSVOT mel input shape is invalid".to_string());
        }
        if mel.iter().any(|value| !value.is_finite()) {
            return Err("ROSVOT mel input contains non-finite values".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_3d(
                run.context,
                GGML_TYPE_F32,
                frames as i64,
                MEL_BINS as i64,
                1
            )
        );
        ggml!(api, ggml_set_input(input));
        let (projected, encoded) = self.build_mel_graph(run.context, input)?;
        ggml!(api, ggml_set_output(projected));
        ggml!(api, ggml_set_output(encoded));
        ggml!(api, ggml_build_forward_expand(run.graph, encoded));
        run.allocate(&self.backend)?;
        set_f32(api, input, &to_channel_major(mel, frames, MEL_BINS))?;
        run.compute(&self.backend)?;
        Ok(MelEncoding {
            projected: to_frame_major(&get_f32(api, projected)?, frames, HIDDEN_DIM)?,
            encoded: to_frame_major(&get_f32(api, encoded)?, frames, HIDDEN_DIM)?,
            frames,
        })
    }

    /// Adds the annotation pitch channels consumed by Stage A.
    pub fn encode_mel_with_pitch(
        &self,
        mel: &[f32],
        pitch_coarse: &[i32],
        uv: &[i32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<MelPitchEncoding, String> {
        if frames == 0
            || valid_frames == 0
            || valid_frames > frames
            || mel.len() != frames * MEL_BINS
            || pitch_coarse.len() != frames
            || uv.len() != frames
        {
            return Err("ROSVOT mel/pitch input shape is invalid".to_string());
        }
        if mel.iter().any(|value| !value.is_finite())
            || pitch_coarse.iter().any(|value| !(0..300).contains(value))
            || uv.iter().any(|value| !(0..3).contains(value))
        {
            return Err("ROSVOT mel/pitch input values are invalid".to_string());
        }
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_3d(
                run.context,
                GGML_TYPE_F32,
                frames as i64,
                MEL_BINS as i64,
                1
            )
        );
        let pitch_indices = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames as i64)
        );
        let uv_indices = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames as i64)
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(pitch_indices));
        ggml!(api, ggml_set_input(uv_indices));
        let (projected, encoded) = self.build_mel_graph(run.context, input)?;
        let encoded_features = ggml!(api, ggml_transpose(run.context, encoded));
        let encoded_features = ggml!(api, ggml_cont(run.context, encoded_features));
        let pitch = ggml!(
            api,
            ggml_get_rows(
                run.context,
                self.weight("pitch_embed.weight")?,
                pitch_indices
            )
        );
        let uv_embedding = ggml!(
            api,
            ggml_get_rows(run.context, self.weight("uv_embed.weight")?, uv_indices)
        );
        let pitch = ggml!(api, ggml_add(run.context, pitch, uv_embedding));
        let embedded = ggml!(api, ggml_add(run.context, encoded_features, pitch));
        ggml!(api, ggml_set_output(projected));
        ggml!(api, ggml_set_output(encoded));
        ggml!(api, ggml_set_output(embedded));
        ggml!(api, ggml_build_forward_expand(run.graph, embedded));
        run.allocate(&self.backend)?;
        set_f32(api, input, &to_channel_major(mel, frames, MEL_BINS))?;
        set_i32(api, pitch_indices, pitch_coarse)?;
        set_i32(api, uv_indices, uv)?;
        run.compute(&self.backend)?;
        Ok(MelPitchEncoding {
            mel: MelEncoding {
                projected: to_frame_major(&get_f32(api, projected)?, frames, HIDDEN_DIM)?,
                encoded: to_frame_major(&get_f32(api, encoded)?, frames, HIDDEN_DIM)?,
                frames,
            },
            embedded: get_f32(api, embedded)?,
        })
    }

    /// Adds the transcript word-boundary channel and runs `cond_encoder`.
    pub fn encode_conditioning(
        &self,
        mel: &[f32],
        pitch_coarse: &[i32],
        uv: &[i32],
        word_boundaries: &[i32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<ConditionEncoding, String> {
        if word_boundaries.len() != frames
            || word_boundaries.iter().any(|value| !(0..3).contains(value))
        {
            return Err("ROSVOT word-boundary input is invalid".to_string());
        }
        let pitch = self.encode_mel_with_pitch(mel, pitch_coarse, uv, valid_frames, frames)?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        let indices = ggml!(
            api,
            ggml_new_tensor_1d(run.context, GGML_TYPE_I32, frames as i64)
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(indices));
        let word = ggml!(
            api,
            ggml_get_rows(run.context, self.weight("word_bd_embed.weight")?, indices)
        );
        let embedded = ggml!(api, ggml_add(run.context, input, word));
        ggml!(api, ggml_set_output(embedded));
        ggml!(api, ggml_build_forward_expand(run.graph, embedded));
        run.allocate(&self.backend)?;
        set_f32(api, input, &pitch.embedded)?;
        set_i32(api, indices, word_boundaries)?;
        run.compute(&self.backend)?;
        let embedded = get_f32(api, embedded)?;
        let conditioned = self.encode_conv_block_values(&embedded, frames, "cond_encoder")?;
        Ok(ConditionEncoding {
            mel: pitch.mel,
            embedded,
            conditioned,
        })
    }

    fn build_mel_graph(
        &self,
        context: ContextPtr,
        input: TensorPtr,
    ) -> Result<(TensorPtr, TensorPtr), String> {
        let projected = self.conv1d(context, "mel_proj", input, 1)?;
        let encoded = self.mel_encoder(context, projected)?;
        Ok((projected, encoded))
    }

    fn mel_encoder(&self, context: ContextPtr, input: TensorPtr) -> Result<TensorPtr, String> {
        let api = self.api();
        let mut hidden = input;
        for block in 0..2 {
            let prefix = format!("mel_encoder.res_blocks.0.blocks.{block}");
            let normalized = self.layer_norm_channels(context, &format!("{prefix}.0"), hidden)?;
            let expanded = self.conv1d(context, &format!("{prefix}.1"), normalized, 1)?;
            let expanded = ggml!(
                api,
                ggml_scale_bias(context, expanded, 3.0_f32.powf(-0.5), 0.0)
            );
            let activated = ggml!(
                api,
                ggml_leaky_relu(context, expanded, LEAKY_RELU_SLOPE, false)
            );
            let projected = self.conv1d(context, &format!("{prefix}.4"), activated, 0)?;
            ensure_can_repeat(projected, hidden, "mel encoder residual")?;
            hidden = ggml!(api, ggml_add(context, hidden, projected));
        }
        hidden = self.layer_norm_channels(context, "mel_encoder.last_norm", hidden)?;
        self.conv1d(context, "mel_encoder.post_net1", hidden, 1)
    }

    fn conv1d(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
        padding: i32,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let weight = self.weight(&format!("{prefix}.weight"))?;
        let bias = self.weight(&format!("{prefix}.bias"))?;
        // `ggml_conv_1d` always lowers F32 activations through an F16
        // im2col buffer. ROSVOT parity needs the public F32 im2col path.
        let columns = ggml!(
            api,
            ggml_im2col(
                context,
                weight,
                input,
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
        let channels = tensor_ref(bias)?.ne[0];
        let bias = ggml!(api, ggml_reshape_3d(context, bias, 1, channels, 1));
        ensure_can_repeat(bias, output, &format!("{prefix} bias"))?;
        Ok(ggml!(api, ggml_add(context, output, bias)))
    }

    fn layer_norm_channels(
        &self,
        context: ContextPtr,
        prefix: &str,
        input: TensorPtr,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let feature_major = ggml!(api, ggml_transpose(context, input));
        let feature_major = ggml!(api, ggml_cont(context, feature_major));
        let normalized = ggml!(api, ggml_norm(context, feature_major, LAYER_NORM_EPSILON));
        let scale = self.weight(&format!("{prefix}.weight"))?;
        ensure_can_repeat(scale, normalized, &format!("{prefix} scale"))?;
        let scaled = ggml!(api, ggml_mul(context, normalized, scale));
        let bias = self.weight(&format!("{prefix}.bias"))?;
        ensure_can_repeat(bias, scaled, &format!("{prefix} bias"))?;
        let biased = ggml!(api, ggml_add(context, scaled, bias));
        let channel_major = ggml!(api, ggml_transpose(context, biased));
        Ok(ggml!(api, ggml_cont(context, channel_major)))
    }
}

fn ensure_can_repeat(source: TensorPtr, target: TensorPtr, label: &str) -> Result<(), String> {
    let source_shape = tensor_ref(source)?.ne;
    let target_shape = tensor_ref(target)?.ne;
    if source_shape
        .iter()
        .zip(target_shape)
        .all(|(source, target)| *source > 0 && target % source == 0)
    {
        Ok(())
    } else {
        Err(format!(
            "ROSVOT GGML {label} shape cannot repeat: {:?} into {:?}",
            source_shape, target_shape
        ))
    }
}

fn to_channel_major(frame_major: &[f32], frames: usize, channels: usize) -> Vec<f32> {
    let mut result = vec![0.0; frame_major.len()];
    for frame in 0..frames {
        for channel in 0..channels {
            result[channel * frames + frame] = frame_major[frame * channels + channel];
        }
    }
    result
}

fn to_frame_major(
    channel_major: &[f32],
    frames: usize,
    channels: usize,
) -> Result<Vec<f32>, String> {
    if channel_major.len() != frames * channels {
        return Err("ROSVOT GGML mel encoding shape is invalid".to_string());
    }
    let mut result = vec![0.0; channel_major.len()];
    for frame in 0..frames {
        for channel in 0..channels {
            result[frame * channels + channel] = channel_major[channel * frames + frame];
        }
    }
    Ok(result)
}

pub(super) fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("ROSVOT GGML input tensor size mismatch".to_string());
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
        return Err("ROSVOT GGML index tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

pub(super) fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "ROSVOT tensor element count is invalid".to_string())?;
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
            return Err("could not allocate ROSVOT GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_NODES, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate ROSVOT GGML graph".to_string());
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
            return Err("could not allocate ROSVOT GGML graph".to_string());
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
                "ROSVOT GGML graph compute failed with status {status}"
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

    #[test]
    fn layout_roundtrip_preserves_frame_major_values() {
        let source = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let channel_major = to_channel_major(&source, 2, 3);
        assert_eq!(channel_major, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
        assert_eq!(to_frame_major(&channel_major, 2, 3).unwrap(), source);
    }
}
