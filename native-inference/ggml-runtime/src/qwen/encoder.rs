//! Shared-upstream-GGML Qwen audio encoder.
//! Convolution is chunk-local; transformer attention spans all compacted rows.

use std::ffi::c_void;
use std::sync::Arc;

use super::frontend::Mel;
use super::model::ChunkGeometry;
use super::weights::{Qwen, tensor_ref};
use crate::ffi::{
    AllocatorPtr, ContextPtr, GGML_PREC_F32, GGML_STATUS_SUCCESS, GGML_TYPE_F16, GGML_TYPE_F32,
    GgmlInitParams, GraphPtr, ModelApi, TensorPtr,
};
use crate::{GgmlBackendHandle, GgmlRuntime};

const GRAPH_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const GRAPH_NODES: usize = 8_192;
const LAYER_NORM_EPSILON: f32 = 1.0e-5;

macro_rules! ggml {
    ($api:expr, $name:ident($($argument:expr),* $(,)?)) => {{
        // SAFETY: pointers belong to the live Qwen model and graph run.
        unsafe { ($api.$name)($($argument),*) }
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct EncodedAudio {
    /// Row-major `[rows, width]` audio embeddings.
    pub values: Vec<f32>,
    pub rows: usize,
    pub width: usize,
}

struct EncoderGraph {
    convolutions: [TensorPtr; 3],
    subsample: TensorPtr,
    positioned: TensorPtr,
    block_first: TensorPtr,
    block_last: TensorPtr,
    norm_post: TensorPtr,
    output: TensorPtr,
}

impl EncoderGraph {
    fn observed(&self) -> [(&'static str, TensorPtr); 9] {
        [
            ("enc.conv.0.out", self.convolutions[0]),
            ("enc.conv.1.out", self.convolutions[1]),
            ("enc.conv.2.out", self.convolutions[2]),
            ("enc.subsample.out", self.subsample),
            ("enc.pos_add.out", self.positioned),
            ("enc.block.0.out", self.block_first),
            ("enc.block.23.out", self.block_last),
            ("enc.ln_post.out", self.norm_post),
            ("enc.proj.out", self.output),
        ]
    }
}

impl Qwen {
    pub fn encode_audio(&self, mel: &Mel) -> Result<EncodedAudio, String> {
        self.encode_audio_inner(mel, false).map(|(audio, _)| audio)
    }

    fn encode_audio_inner(
        &self,
        mel: &Mel,
        observe: bool,
    ) -> Result<(EncodedAudio, Option<Vec<(&'static str, Vec<f32>)>>), String> {
        if mel.bins != self.config.mel_bins
            || mel.frames == 0
            || mel.data.len() != mel.bins.checked_mul(mel.frames).unwrap_or(usize::MAX)
            || mel.data.iter().any(|value| !value.is_finite())
        {
            return Err("Qwen mel/encoder input shape is invalid".to_string());
        }
        let geometry = ChunkGeometry::new(mel.frames, self.config.mel_per_chunk)?;
        if geometry.valid_rows == 0 {
            return Err("Qwen encoder received no mel frames".to_string());
        }
        let packed = pack_mel(mel, &geometry)?;
        let position = sinusoid_repeated(
            self.config.encoder_dim,
            geometry.valid_rows,
            geometry.rows_per_chunk,
        )?;
        let mut run = GraphRun::new(Arc::clone(&self.backend.runtime))?;
        let api = self.api();
        let input = ggml!(
            api,
            ggml_new_tensor_4d(
                run.context,
                GGML_TYPE_F32,
                geometry.padded_mel_frames as i64,
                mel.bins as i64,
                1,
                geometry.chunks as i64
            )
        );
        let positions = ggml!(
            api,
            ggml_new_tensor_2d(
                run.context,
                GGML_TYPE_F32,
                self.config.encoder_dim as i64,
                geometry.valid_rows as i64
            )
        );
        ggml!(api, ggml_set_input(input));
        ggml!(api, ggml_set_input(positions));
        let graph = self.build_encoder_graph(run.context, input, positions, &geometry)?;
        let observed = graph.observed();
        ggml!(api, ggml_set_output(graph.output));
        if observe {
            for (_, tensor) in observed {
                ggml!(api, ggml_set_output(tensor));
            }
        }
        ggml!(api, ggml_build_forward_expand(run.graph, graph.output));
        run.allocate(&self.backend)?;
        set_f32(api, input, &packed)?;
        set_f32(api, positions, &position)?;
        run.compute(&self.backend)?;
        let values = get_f32(api, graph.output)?;
        let expected = geometry
            .valid_rows
            .checked_mul(self.config.encoder_output_dim)
            .ok_or("Qwen encoder output shape overflow")?;
        if values.len() != expected {
            return Err("Qwen encoder output tensor shape is invalid".to_string());
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
            EncodedAudio {
                values,
                rows: geometry.valid_rows,
                width: self.config.encoder_output_dim,
            },
            observations,
        ))
    }

    fn build_encoder_graph(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        positions: TensorPtr,
        geometry: &ChunkGeometry,
    ) -> Result<EncoderGraph, String> {
        let api = self.api();
        let mut hidden = input;
        let mut convolutions = [std::ptr::null_mut(); 3];
        for layer in 0..3 {
            let prefix = self.convolution_prefix(layer);
            let weight = self.weight(&format!("{prefix}.weight"))?;
            hidden = ggml!(api, ggml_conv_2d(context, weight, hidden, 2, 2, 1, 1, 1, 1));
            let bias = self.weight(&format!("{prefix}.bias"))?;
            let channels = tensor_ref(bias)?.ne[0];
            let bias = ggml!(api, ggml_reshape_4d(context, bias, 1, 1, channels, 1));
            hidden = ggml!(api, ggml_add(context, hidden, bias));
            hidden = ggml!(api, ggml_gelu_erf(context, hidden));
            convolutions[layer] = hidden;
        }
        let hidden_shape = tensor_ref(hidden)?.ne;
        let frequency = super::model::after_cnn_len(self.config.mel_bins);
        if hidden_shape
            != [
                geometry.rows_per_chunk as i64,
                frequency as i64,
                self.config.conv_channels as i64,
                geometry.chunks as i64,
            ]
        {
            return Err(format!(
                "Qwen convolution output shape is invalid: {hidden_shape:?}"
            ));
        }
        // GGML's permutation arguments are destination axes: move source
        // `[time, frequency, channel, chunk]` to `[frequency, channel, time, chunk]`.
        // Contiguous storage is then `[chunk, time, channel, frequency]`,
        // matching conv_out with frequency contiguous.
        hidden = ggml!(api, ggml_permute(context, hidden, 2, 0, 1, 3));
        hidden = ggml!(api, ggml_cont(context, hidden));
        hidden = ggml!(
            api,
            ggml_reshape_2d(
                context,
                hidden,
                (frequency * self.config.conv_channels) as i64,
                geometry.padded_rows as i64
            )
        );
        if geometry.valid_rows < geometry.padded_rows {
            let stride = tensor_ref(hidden)?.nb[1];
            hidden = ggml!(
                api,
                ggml_view_2d(
                    context,
                    hidden,
                    (frequency * self.config.conv_channels) as i64,
                    geometry.valid_rows as i64,
                    stride,
                    0
                )
            );
        }
        let subsample = self.linear(
            context,
            &format!("{}.conv_out", self.encoder_prefix()),
            hidden,
            false,
        )?;
        let positioned = ggml!(api, ggml_add(context, subsample, positions));
        hidden = positioned;
        let mut block_first = std::ptr::null_mut();
        for layer in 0..self.config.encoder_layers {
            hidden = self.encoder_block(context, hidden, geometry.valid_rows, layer)?;
            if layer == 0 {
                block_first = hidden;
            }
        }
        let block_last = hidden;
        let norm_post = self.layer_norm(
            context,
            &format!("{}.ln_post", self.encoder_prefix()),
            hidden,
        )?;
        hidden = self.linear(
            context,
            &format!("{}.proj1", self.encoder_prefix()),
            norm_post,
            true,
        )?;
        hidden = ggml!(api, ggml_gelu_erf(context, hidden));
        let output = self.linear(
            context,
            &format!("{}.proj2", self.encoder_prefix()),
            hidden,
            true,
        )?;
        Ok(EncoderGraph {
            convolutions,
            subsample,
            positioned,
            block_first,
            block_last,
            norm_post,
            output,
        })
    }

    fn encoder_block(
        &self,
        context: ContextPtr,
        input: TensorPtr,
        rows: usize,
        index: usize,
    ) -> Result<TensorPtr, String> {
        let api = self.api();
        let (prefix, names) = self.encoder_block_names(index);
        let normalized = self.layer_norm(context, &format!("{prefix}.{}", names[0]), input)?;
        let query = self.linear(context, &format!("{prefix}.{}", names[1]), normalized, true)?;
        let key = self.linear(context, &format!("{prefix}.{}", names[2]), normalized, true)?;
        let value = self.linear(context, &format!("{prefix}.{}", names[3]), normalized, true)?;
        let attention_layout = |tensor| {
            let tensor = ggml!(
                api,
                ggml_reshape_3d(
                    context,
                    tensor,
                    (self.config.encoder_dim / self.config.encoder_heads) as i64,
                    self.config.encoder_heads as i64,
                    rows as i64
                )
            );
            let tensor = ggml!(api, ggml_permute(context, tensor, 0, 2, 1, 3));
            ggml!(api, ggml_cont(context, tensor))
        };
        let query = attention_layout(query);
        let key = ggml!(
            api,
            ggml_cast(context, attention_layout(key), GGML_TYPE_F16)
        );
        let value = ggml!(
            api,
            ggml_cast(context, attention_layout(value), GGML_TYPE_F16)
        );
        let attended = ggml!(
            api,
            ggml_flash_attn_ext(
                context,
                query,
                key,
                value,
                std::ptr::null_mut(),
                1.0 / ((self.config.encoder_dim / self.config.encoder_heads) as f32).sqrt(),
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
                self.config.encoder_dim as i64,
                rows as i64
            )
        );
        let projected = self.linear(context, &format!("{prefix}.{}", names[4]), attended, true)?;
        let residual = ggml!(api, ggml_add(context, input, projected));
        let normalized = self.layer_norm(context, &format!("{prefix}.{}", names[5]), residual)?;
        let expanded = self.linear(context, &format!("{prefix}.{}", names[6]), normalized, true)?;
        let expanded = ggml!(api, ggml_gelu_erf(context, expanded));
        let projected = self.linear(context, &format!("{prefix}.{}", names[7]), expanded, true)?;
        Ok(ggml!(api, ggml_add(context, residual, projected)))
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
        let weight_shape = tensor_ref(weight)?.ne;
        let input_shape = tensor_ref(input)?.ne;
        if weight_shape[0] != input_shape[0] {
            return Err(format!(
                "Qwen linear {prefix} shape mismatch: weight {weight_shape:?}, input {input_shape:?}"
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

    fn layer_norm(
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
}

fn pack_mel(mel: &Mel, geometry: &ChunkGeometry) -> Result<Vec<f32>, String> {
    let expected = mel
        .bins
        .checked_mul(mel.frames)
        .ok_or("Qwen mel shape overflow")?;
    if expected != mel.data.len() {
        return Err("Qwen mel storage shape mismatch".to_string());
    }
    let len = geometry
        .chunks
        .checked_mul(mel.bins)
        .and_then(|value| value.checked_mul(geometry.padded_mel_frames))
        .ok_or("Qwen convolution input overflow")?;
    let mut result = vec![0.0; len];
    for chunk in 0..geometry.chunks {
        for bin in 0..mel.bins {
            for time in 0..geometry.padded_mel_frames {
                let frame = chunk * geometry.padded_mel_frames + time;
                if frame < mel.frames {
                    result[(chunk * mel.bins + bin) * geometry.padded_mel_frames + time] =
                        mel.data[bin * mel.frames + frame];
                }
            }
        }
    }
    Ok(result)
}

fn sinusoid(channels: usize, rows: usize) -> Result<Vec<f32>, String> {
    if channels <= 2 || !channels.is_multiple_of(2) {
        return Err("Qwen sinusoid needs even width above 2".to_string());
    }
    let half = channels / 2;
    let scale = 10_000.0_f64.ln() / (half - 1) as f64;
    let mut output = vec![
        0.0;
        channels
            .checked_mul(rows)
            .ok_or("Qwen position shape overflow")?
    ];
    for row in 0..rows {
        for channel in 0..half {
            let angle = row as f64 * (-scale * channel as f64).exp();
            output[row * channels + channel] = angle.sin() as f32;
            output[row * channels + channel + half] = angle.cos() as f32;
        }
    }
    Ok(output)
}

fn sinusoid_repeated(
    channels: usize,
    rows: usize,
    rows_per_chunk: usize,
) -> Result<Vec<f32>, String> {
    let single = sinusoid(channels, rows_per_chunk)?;
    let mut output = vec![0.0; rows.checked_mul(channels).ok_or("Qwen position overflow")?];
    for row in 0..rows {
        output[row * channels..(row + 1) * channels].copy_from_slice(
            &single[(row % rows_per_chunk) * channels..(row % rows_per_chunk + 1) * channels],
        );
    }
    Ok(output)
}

fn set_f32(api: &ModelApi, tensor: TensorPtr, values: &[f32]) -> Result<(), String> {
    let bytes = ggml!(api, ggml_nbytes(tensor));
    if bytes != values.len() * std::mem::size_of::<f32>() {
        return Err("Qwen GGML input tensor size mismatch".to_string());
    }
    ggml!(
        api,
        ggml_backend_tensor_set(tensor, values.as_ptr().cast::<c_void>(), 0, bytes)
    );
    Ok(())
}

fn get_f32(api: &ModelApi, tensor: TensorPtr) -> Result<Vec<f32>, String> {
    let elements = usize::try_from(ggml!(api, ggml_nelements(tensor)))
        .map_err(|_| "Qwen tensor element count is invalid".to_string())?;
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
            return Err("could not allocate Qwen GGML graph context".to_string());
        }
        let graph = ggml!(api, ggml_new_graph_custom(context, GRAPH_NODES, false));
        if graph.is_null() {
            ggml!(api, ggml_free(context));
            return Err("could not allocate Qwen GGML graph".to_string());
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
            return Err("could not allocate Qwen GGML graph".to_string());
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
                "Qwen GGML graph compute failed with status {status}"
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
    fn mel_chunks_preserve_frequency_and_zero_only_padding() {
        let mel = Mel {
            bins: 2,
            frames: 5,
            data: vec![1., 2., 3., 4., 5., 11., 12., 13., 14., 15.],
        };
        let geometry = ChunkGeometry::new(5, 3).unwrap();
        assert_eq!(
            pack_mel(&mel, &geometry).unwrap(),
            [1., 2., 3., 11., 12., 13., 4., 5., 0., 14., 15., 0.]
        );
    }

    #[test]
    fn position_uses_sine_half_then_cosine_half_and_restarts_each_chunk() {
        let position = sinusoid_repeated(4, 5, 3).unwrap();
        assert_eq!(&position[..4], &[0., 0., 1., 1.]);
        assert!((position[4] - 1.0_f32.sin()).abs() < 1.0e-7);
        assert!((position[5] - 0.0001_f32.sin()).abs() < 1.0e-7);
        assert!((position[6] - 1.0_f32.cos()).abs() < 1.0e-7);
        assert_eq!(&position[12..16], &[0., 0., 1., 1.]);
        assert_eq!(&position[16..20], &position[4..8]);
        assert!(sinusoid(3, 2).is_err());
    }

    fn read_f32(path: impl AsRef<std::path::Path>) -> Vec<f32> {
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.len().is_multiple_of(4));
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect()
    }

    fn write_f32(path: impl AsRef<std::path::Path>, values: &[f32]) {
        let bytes = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, Qwen GGUF, mel, and encoder reference"]
    fn actual_encoder_matches_historical_rust_reference() {
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
        let mel = Mel {
            bins: model.config.mel_bins,
            frames: 1_200,
            data: read_f32(path("UTA_TEST_QWEN_MEL")),
        };
        let (actual, observations) = model.encode_audio_inner(&mel, true).unwrap();
        let expected = read_f32(path("UTA_TEST_QWEN_ENCODER_REFERENCE"));
        assert_eq!((actual.rows, actual.width), (156, 1_024));
        assert_eq!(actual.values.len(), expected.len());
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
        let stage_dir = path("UTA_TEST_QWEN_ENCODER_STAGE_DIR");
        let actual_dir =
            std::env::var_os("UTA_TEST_QWEN_ENCODER_ACTUAL_DIR").map(std::path::PathBuf::from);
        if let Some(directory) = &actual_dir {
            std::fs::create_dir_all(directory).unwrap();
        }
        for (name, values) in observations.unwrap() {
            if let Some(directory) = &actual_dir {
                write_f32(directory.join(format!("{name}.f32")), &values);
            }
            let expected_path = stage_dir.join(format!("{name}.f32"));
            if expected_path.exists() {
                let expected = read_f32(expected_path);
                compare(name, &values, &expected);
            }
        }
        if let Some(directory) = &actual_dir {
            write_f32(directory.join("encoder.f32"), &actual.values);
        }
        let maximum = compare("encoder", &actual.values, &expected);
        assert!(maximum < 0.1, "Qwen encoder max difference {maximum}");
    }
}
