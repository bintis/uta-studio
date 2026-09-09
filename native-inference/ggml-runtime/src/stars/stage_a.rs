use std::ffi::c_void;
use std::sync::Arc;

use super::model::{HIDDEN_DIM, MEL_BINS, Stars, tensor_ref};
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
        // SAFETY: pointers belong to the live STARS model and graph run.
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

impl Stars {
    /// Runs the first neural slice shared by every STARS stage.
    ///
    /// Input is frame-major `[frames, 80]`, matching [`super::frontend::mel_80`].
    pub fn encode_mel(&self, mel: &[f32], frames: usize) -> Result<MelEncoding, String> {
        if frames == 0 || mel.len() != frames * MEL_BINS {
            return Err("STARS mel input shape is invalid".to_string());
        }
        if mel.iter().any(|value| !value.is_finite()) {
            return Err("STARS mel input contains non-finite values".to_string());
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
            || valid_frames > frames
            || mel.len() != frames * MEL_BINS
            || pitch_coarse.len() != frames
            || uv.len() != frames
        {
            return Err("STARS mel/pitch input shape is invalid".to_string());
        }
        if mel.iter().any(|value| !value.is_finite())
            || pitch_coarse.iter().any(|value| !(0..300).contains(value))
            || uv.iter().any(|value| !(0..3).contains(value))
        {
            return Err("STARS mel/pitch input values are invalid".to_string());
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
        let nonpadding = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, HIDDEN_DIM as i64, frames as i64)
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(pitch_indices));
        ggml!(api, ggml_set_input(uv_indices));
        ggml!(api, ggml_set_input(nonpadding));
        let (projected, encoded) = self.build_mel_graph(run.context, input)?;
        let encoded_features = ggml!(api, ggml_transpose(run.context, encoded));
        let encoded_features = ggml!(api, ggml_cont(run.context, encoded_features));
        let encoded_features = ggml!(api, ggml_mul(run.context, encoded_features, nonpadding));
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
        let pitch = ggml!(api, ggml_mul(run.context, pitch, nonpadding));
        let embedded = ggml!(api, ggml_add(run.context, encoded_features, pitch));
        ggml!(api, ggml_set_output(projected));
        ggml!(api, ggml_set_output(encoded));
        ggml!(api, ggml_set_output(embedded));
        ggml!(api, ggml_build_forward_expand(run.graph, embedded));
        run.allocate(&self.backend)?;
        set_f32(api, input, &to_channel_major(mel, frames, MEL_BINS))?;
        set_i32(api, pitch_indices, pitch_coarse)?;
        set_i32(api, uv_indices, uv)?;
        let mut mask = vec![0.0_f32; frames * HIDDEN_DIM];
        mask[..valid_frames * HIDDEN_DIM].fill(1.0);
        set_f32(api, nonpadding, &mask)?;
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
        // im2col buffer. STARS parity needs the public F32 im2col path.
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
            "STARS GGML {label} shape cannot repeat: {:?} into {:?}",
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
        return Err("STARS GGML mel encoding shape is invalid".to_string());
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
        return Err("STARS GGML input tensor size mismatch".to_string());
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
        return Err("STARS GGML index tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

pub(super) fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "STARS tensor element count is invalid".to_string())?;
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
            return Err("could not allocate STARS GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_NODES, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate STARS GGML graph".to_string());
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
            return Err("could not allocate STARS GGML graph".to_string());
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
                "STARS GGML graph compute failed with status {status}"
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
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct MelFixture {
        mel: Vec<Vec<f32>>,
        annotation_uv: Vec<i32>,
        annotation_pitch_coarse: Vec<i32>,
    }

    #[derive(Deserialize)]
    struct StageFixture {
        mel_proj_out: Vec<Vec<f32>>,
        mel_encoder_out: Vec<Vec<f32>>,
        mel_embed_a0: Vec<Vec<f32>>,
        feat_a1: Vec<Vec<f32>>,
        a2_ph_bd_sigmoid: Vec<f32>,
        a3_ph_frame_logits: Vec<Vec<f32>>,
    }

    #[test]
    fn layout_roundtrip_preserves_frame_major_values() {
        let source = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let channel_major = to_channel_major(&source, 2, 3);
        assert_eq!(channel_major, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
        assert_eq!(to_frame_major(&channel_major, 2, 3).unwrap(), source);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, GGUF, and PyTorch fixture"]
    fn stage_a_matches_pytorch_on_explicit_device() {
        let runtime_path = PathBuf::from(
            std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR").expect("set UTA_TEST_GGML_RUNTIME_DIR"),
        );
        let model_path = PathBuf::from(
            std::env::var_os("UTA_TEST_STARS_GGUF").expect("set UTA_TEST_STARS_GGUF"),
        );
        let fixture_path = PathBuf::from(
            std::env::var_os("UTA_TEST_STARS_STAGE_A_FIXTURE")
                .expect("set UTA_TEST_STARS_STAGE_A_FIXTURE"),
        );
        let requested_kind = std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND to cpu or integrated_gpu");
        let description_filter = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => crate::DeviceKind::Cpu,
            "integrated_gpu" => crate::DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let mel_fixture: MelFixture = serde_json::from_str(include_str!(
            "../../fixtures/stars/shared-singing-frontend-upstream.json"
        ))
        .unwrap();
        let expected: StageFixture = serde_json::from_slice(
            &std::fs::read(fixture_path).expect("read STARS Stage A fixture"),
        )
        .unwrap();
        let frames = expected.mel_proj_out.len();
        assert_eq!(frames, 256);
        let valid_frames = mel_fixture.mel.len();
        let mut mel = mel_fixture.mel.into_iter().flatten().collect::<Vec<_>>();
        mel.resize(frames * MEL_BINS, 0.0);
        let mut pitch_coarse = mel_fixture.annotation_pitch_coarse;
        pitch_coarse.truncate(valid_frames);
        pitch_coarse.resize(frames, 0);
        let mut uv = mel_fixture.annotation_uv;
        uv.truncate(valid_frames);
        uv.resize(frames, 0);
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description_filter)
            })
            .expect("requested GGML test device is unavailable");
        let model = Stars::load(runtime, &device, &model_path).unwrap();
        let actual = model
            .encode_mel_with_pitch(&mel, &pitch_coarse, &uv, valid_frames, frames)
            .unwrap();
        let utterance = model
            .encode_utterance(&actual.embedded, valid_frames, frames)
            .unwrap();
        let expected_projected = expected
            .mel_proj_out
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_encoded = expected
            .mel_encoder_out
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_embedded = expected
            .mel_embed_a0
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_features = expected.feat_a1.into_iter().flatten().collect::<Vec<_>>();
        let expected_logits = expected
            .a3_ph_frame_logits
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let projected_error = max_abs_difference(&actual.mel.projected, &expected_projected);
        let encoded_error = max_abs_difference(&actual.mel.encoded, &expected_encoded);
        let embedded_error = max_abs_difference(&actual.embedded, &expected_embedded);
        let feature_error = max_abs_difference(
            &utterance.features[..valid_frames * HIDDEN_DIM],
            &expected_features[..valid_frames * HIDDEN_DIM],
        );
        let boundary_error = max_abs_difference(
            &utterance.boundary_probabilities[..valid_frames],
            &expected.a2_ph_bd_sigmoid[..valid_frames],
        );
        let logits_error = max_abs_difference(
            &utterance.phoneme_logits[..valid_frames * 61],
            &expected_logits[..valid_frames * 61],
        );
        eprintln!(
            "STARS Stage A on {}: projected {projected_error:.8}, encoded {encoded_error:.8}, embedded {embedded_error:.8}, features {feature_error:.8}, boundaries {boundary_error:.8}, logits {logits_error:.8}",
            device.description
        );
        let projected_tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 1.0e-3,
            crate::DeviceKind::IntegratedGpu => 3.0e-3,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(projected_error < projected_tolerance);
        assert!(encoded_error < 5.0e-3);
        assert!(embedded_error < 5.0e-3);
        let logits_tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 1.0e-2,
            crate::DeviceKind::IntegratedGpu => 2.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(feature_error < 5.0e-3);
        assert!(boundary_error < 5.0e-3);
        assert!(logits_error < logits_tolerance);
    }

    fn max_abs_difference(actual: &[f32], expected: &[f32]) -> f32 {
        assert_eq!(actual.len(), expected.len());
        actual
            .iter()
            .zip(expected)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0, f32::max)
    }
}
