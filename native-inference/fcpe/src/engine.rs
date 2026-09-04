//! FCPE (Fast Context-based Pitch Estimation) native inference engine.
//!
//! Architecture: mel spectrogram → 2× Conv1D input stack → 6× Conformer
//! encoder layers → LayerNorm → Linear(512→360) → sigmoid → centroid
//! expectation → F0 in Hz.
//!
//! GGUF tensors (59 total, all F32):
//!   input_stack.{0,1}.{weight,bias}
//!   encoder_layers.{0..5}.{norm,fc1,conv,fc2}.{weight,bias}
//!   norm.{weight,bias}, output_proj.{weight,bias}
//!   cents_mapping, mel_scale, mel_bias

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;
use crate::mel;

const SAMPLE_RATE: usize = 16_000;
const INPUT_SAMPLES: usize = 32_000;
const FRAME_HOP_SAMPLES: usize = 160;
const OUTPUT_FRAMES: usize = INPUT_SAMPLES / FRAME_HOP_SAMPLES + 1; // 201
const D_MODEL: usize = 512;
const D_FF: usize = 2048;
const D_CONV_INNER: usize = 1024;
const N_ENCODER_LAYERS: usize = 6;
const CONV_KERNEL: usize = 31;
const N_PITCH_CLASSES: usize = 360;

// ---------------------------------------------------------------------------
// Tensor weight references (zero-copy slices into the GGUF mmap)
// ---------------------------------------------------------------------------
struct InputStack<'a> {
    w0: &'a [f32], // [3, 128, 512]
    b0: &'a [f32], // [512]
    w1: &'a [f32], // [3, 512, 512]
    b1: &'a [f32], // [512]
}

struct ConformerLayer<'a> {
    norm_w: &'a [f32],   // [512]
    norm_b: &'a [f32],   // [512]
    fc1_w: &'a [f32],    // [1, 512, 2048]
    fc1_b: &'a [f32],    // [2048]
    conv_w: &'a [f32],   // [31, 1, 1024]
    conv_b: &'a [f32],   // [1024]
    fc2_w: &'a [f32],    // [1, 1024, 512]
    fc2_b: &'a [f32],    // [512]
}

struct OutputHead<'a> {
    norm_w: &'a [f32],       // [512]
    norm_b: &'a [f32],       // [512]
    proj_w: &'a [f32],       // [360, 512]
    proj_b: &'a [f32],       // [360]
    cents_mapping: &'a [f32], // [360]
}

struct FcpeWeights<'a> {
    input_stack: InputStack<'a>,
    encoder_layers: [ConformerLayer<'a>; N_ENCODER_LAYERS],
    output_head: OutputHead<'a>,
}

fn load_weights(gguf: &GGUFFile) -> Result<FcpeWeights<'_>> {
    let t = |name: &str| -> Result<&[f32]> { gguf.tensor_data_f32(name) };

    let encoder_layers = std::array::from_fn(|i| {
        let p = format!("encoder_layers.{i}");
        ConformerLayer {
            norm_w: t(&format!("{p}.norm.weight")).unwrap(),
            norm_b: t(&format!("{p}.norm.bias")).unwrap(),
            fc1_w: t(&format!("{p}.fc1.weight")).unwrap(),
            fc1_b: t(&format!("{p}.fc1.bias")).unwrap(),
            conv_w: t(&format!("{p}.conv.weight")).unwrap(),
            conv_b: t(&format!("{p}.conv.bias")).unwrap(),
            fc2_w: t(&format!("{p}.fc2.weight")).unwrap(),
            fc2_b: t(&format!("{p}.fc2.bias")).unwrap(),
        }
    });

    Ok(FcpeWeights {
        input_stack: InputStack {
            w0: t("input_stack.0.weight")?,
            b0: t("input_stack.0.bias")?,
            w1: t("input_stack.1.weight")?,
            b1: t("input_stack.1.bias")?,
        },
        encoder_layers,
        output_head: OutputHead {
            norm_w: t("norm.weight")?,
            norm_b: t("norm.bias")?,
            proj_w: t("output_proj.weight")?,
            proj_b: t("output_proj.bias")?,
            cents_mapping: t("cents_mapping")?,
        },
    })
}

// ---------------------------------------------------------------------------
// Conv1D: shape [kernel, in_ch, out_ch] stored row-major
// ---------------------------------------------------------------------------
/// Conv1D with zero-padding (pad = kernel / 2).
/// Input shape: [frames, in_channels], output: [frames, out_channels].
/// Weight shape in GGUF: [kernel_size, in_channels, out_channels].
fn conv1d(
    input: &[f32],
    frames: usize,
    in_ch: usize,
    weight: &[f32],
    bias: &[f32],
    out_ch: usize,
    kernel: usize,
) -> Vec<f32> {
    let pad = kernel / 2;
    let mut output = vec![0.0_f32; frames * out_ch];

    for f in 0..frames {
        for oc in 0..out_ch {
            let mut sum = bias[oc];
            for k in 0..kernel {
                let input_frame = f as isize + k as isize - pad as isize;
                if input_frame < 0 || input_frame >= frames as isize {
                    continue;
                }
                let if_usize = input_frame as usize;
                for ic in 0..in_ch {
                    // weight layout: [kernel, in_ch, out_ch]
                    let w_idx = k * in_ch * out_ch + ic * out_ch + oc;
                    sum += input[if_usize * in_ch + ic] * weight[w_idx];
                }
            }
            output[f * out_ch + oc] = sum;
        }
    }
    output
}

/// Depthwise Conv1D: each channel is convolved independently.
/// Weight shape in GGUF: [kernel_size, 1, channels].
fn depthwise_conv1d(
    input: &[f32],
    frames: usize,
    channels: usize,
    weight: &[f32],
    bias: &[f32],
    kernel: usize,
) -> Vec<f32> {
    let pad = kernel / 2;
    let mut output = vec![0.0_f32; frames * channels];

    for f in 0..frames {
        for ch in 0..channels {
            let mut sum = bias[ch];
            for k in 0..kernel {
                let input_frame = f as isize + k as isize - pad as isize;
                if input_frame < 0 || input_frame >= frames as isize {
                    continue;
                }
                let if_usize = input_frame as usize;
                // weight layout: [kernel, 1, channels]
                let w_idx = k * channels + ch;
                sum += input[if_usize * channels + ch] * weight[w_idx];
            }
            output[f * channels + ch] = sum;
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Pointwise Conv1D (k=1) = matrix multiply: input [frames, in_ch] × weight → [frames, out_ch]
// ---------------------------------------------------------------------------
fn pointwise_conv1d(
    input: &[f32],
    frames: usize,
    in_ch: usize,
    weight: &[f32],
    bias: &[f32],
    out_ch: usize,
) -> Vec<f32> {
    // weight shape in GGUF: [1, in_ch, out_ch] → treat as [in_ch, out_ch]
    let mut output = vec![0.0_f32; frames * out_ch];

    // Use gemm for efficient matmul: input[frames, in_ch] × weight[in_ch, out_ch]
    // weight shape in GGUF: [1, in_ch, out_ch] → treat as [in_ch, out_ch] row-major
    // dst = input @ weight → [frames, out_ch]
    unsafe {
        gemm::gemm(
            frames,           // m
            out_ch,           // n
            in_ch,            // k
            output.as_mut_ptr(),
            1,                // dst_cs
            out_ch as isize,  // dst_rs
            false,            // read_dst
            input.as_ptr(),
            1,                // lhs_cs
            in_ch as isize,   // lhs_rs
            weight.as_ptr(),
            1,                // rhs_cs
            out_ch as isize,  // rhs_rs
            0.0,              // alpha (no accumulate)
            1.0,              // beta (scale factor)
            false,            // conj_dst
            false,            // conj_lhs
            false,            // conj_rhs
            gemm::Parallelism::None,
        );
    }

    // Add bias
    for f in 0..frames {
        for c in 0..out_ch {
            output[f * out_ch + c] += bias[c];
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Activations and normalization
// ---------------------------------------------------------------------------
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// GLU: split along channel dim, sigmoid on second half, multiply.
/// Input shape: [frames, 2*half_ch], output: [frames, half_ch].
fn glu(input: &[f32], frames: usize, double_ch: usize) -> Vec<f32> {
    let half = double_ch / 2;
    let mut output = vec![0.0_f32; frames * half];
    for f in 0..frames {
        for c in 0..half {
            let a = input[f * double_ch + c];
            let b = input[f * double_ch + half + c];
            output[f * half + c] = a * sigmoid(b);
        }
    }
    output
}

/// SiLU (Swish): x * sigmoid(x), applied element-wise in-place.
fn silu_inplace(data: &mut [f32]) {
    for v in data.iter_mut() {
        *v = *v * sigmoid(*v);
    }
}

/// Layer normalization along the channel dimension.
/// Input/output shape: [frames, channels].
fn layer_norm(data: &mut [f32], frames: usize, channels: usize, weight: &[f32], bias: &[f32]) {
    let eps = 1e-5_f32;
    for f in 0..frames {
        let row = &mut data[f * channels..(f + 1) * channels];
        let mean = row.iter().sum::<f32>() / channels as f32;
        let variance =
            row.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / channels as f32;
        let inv_std = 1.0 / (variance + eps).sqrt();
        for c in 0..channels {
            row[c] = (row[c] - mean) * inv_std * weight[c] + bias[c];
        }
    }
}

// ---------------------------------------------------------------------------
// Conformer block
// ---------------------------------------------------------------------------
fn conformer_block(
    input: &[f32],
    frames: usize,
    layer: &ConformerLayer<'_>,
) -> Vec<f32> {
    // 1. LayerNorm
    let mut normed = input.to_vec();
    layer_norm(&mut normed, frames, D_MODEL, layer.norm_w, layer.norm_b);

    // 2. fc1: pointwise Conv1D (512 → 2048)
    let expanded = pointwise_conv1d(&normed, frames, D_MODEL, layer.fc1_w, layer.fc1_b, D_FF);

    // 3. GLU: 2048 → 1024
    let gated = glu(&expanded, frames, D_FF);

    // 4. Depthwise Conv1D (1024, k=31) + SiLU
    let mut conv_out =
        depthwise_conv1d(&gated, frames, D_CONV_INNER, layer.conv_w, layer.conv_b, CONV_KERNEL);
    silu_inplace(&mut conv_out);

    // 5. fc2: pointwise Conv1D (1024 → 512)
    let projected =
        pointwise_conv1d(&conv_out, frames, D_CONV_INNER, layer.fc2_w, layer.fc2_b, D_MODEL);

    // 6. Residual add
    let mut output = input.to_vec();
    for (o, p) in output.iter_mut().zip(projected.iter()) {
        *o += *p;
    }
    output
}

// ---------------------------------------------------------------------------
// Output head: centroid-based pitch decoding
// ---------------------------------------------------------------------------
fn decode_pitch(
    features: &[f32],
    frames: usize,
    head: &OutputHead<'_>,
) -> Vec<Option<f32>> {
    // LayerNorm
    let mut normed = features.to_vec();
    layer_norm(&mut normed, frames, D_MODEL, head.norm_w, head.norm_b);

    // Linear projection (512 → 360) using matmul
    // proj_w shape: [360, 512] in GGUF (row-major: each row is one output neuron)
    // We need: normed[frames, 512] × proj_w^T → [frames, 360]
    let mut logits = vec![0.0_f32; frames * N_PITCH_CLASSES];
    for f in 0..frames {
        for p in 0..N_PITCH_CLASSES {
            let mut sum = head.proj_b[p];
            for d in 0..D_MODEL {
                sum += normed[f * D_MODEL + d] * head.proj_w[p * D_MODEL + d];
            }
            logits[f * N_PITCH_CLASSES + p] = sigmoid(sum);
        }
    }

    // Centroid expectation decoding (9-point neighborhood around argmax)
    let mut pitches = Vec::with_capacity(frames);
    for f in 0..frames {
        let row = &logits[f * N_PITCH_CLASSES..(f + 1) * N_PITCH_CLASSES];
        let max_activation = row.iter().copied().fold(0.0_f32, f32::max);

        if max_activation <= 0.006 {
            pitches.push(None);
            continue;
        }

        // Find argmax
        let peak_idx = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);

        // Build 9-point neighborhood centered on peak (peak-4 .. peak+4)
        let mut weighted_sum = 0.0_f64;
        let mut weight_sum = 0.0_f64;
        for offset in 0..9_usize {
            let raw_idx = peak_idx as isize - 4 + offset as isize;
            let clamped = raw_idx.clamp(0, N_PITCH_CLASSES as isize - 1) as usize;
            let activation = row[clamped] as f64;
            let cents = head.cents_mapping[clamped] as f64;
            weighted_sum += cents * activation;
            weight_sum += activation;
        }

        if weight_sum <= 0.0 {
            pitches.push(None);
            continue;
        }

        let cents = weighted_sum / weight_sum;
        // Convert cents to Hz: 10 * 2^(cents/1200)
        let hz = 10.0 * 2.0_f64.powf(cents / 1200.0);
        let hz_f32 = hz as f32;
        if hz_f32.is_finite() && hz_f32 > 0.0 {
            pitches.push(Some(hz_f32));
        } else {
            pitches.push(None);
        }
    }
    pitches
}

// ---------------------------------------------------------------------------
// Evidence serialization
// ---------------------------------------------------------------------------
#[derive(serde::Serialize)]
struct PitchFrame {
    time: f64,
    hz: Option<f32>,
}

#[derive(serde::Serialize)]
struct PitchEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_manifest_sha256: &'a str,
    model_xml_sha256: &'a str,
    model_bin_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    timeline_step_ms: u32,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    frames: Vec<PitchFrame>,
}

// ---------------------------------------------------------------------------
// Public inference entry point
// ---------------------------------------------------------------------------
pub fn infer(
    audio: &[f32],
    model_path: &Path,
    output_dir: &Path,
    mut progress: impl FnMut(f32, &'static str, Option<(u64, u64)>),
) -> Result<PathBuf> {
    if audio.is_empty() {
        return Err(Error::message("FCPE requires non-empty decoded audio"));
    }

    progress(0.0, "Loading FCPE model weights", None);
    let gguf = GGUFFile::open(model_path)?;
    if gguf.architecture() != "fcpe" {
        return Err(Error::message(format!(
            "expected GGUF architecture 'fcpe', got '{}'",
            gguf.architecture()
        )));
    }
    let weights = load_weights(&gguf)?;

    // Compute log-mel spectrogram
    progress(0.05, "Computing log-mel spectrogram", None);
    let (mel_data, total_mel_frames) = mel::log_mel_spectrogram(audio, |frac| {
        progress(
            0.05 + frac * 0.15,
            "Computing log-mel spectrogram",
            None,
        );
    })
    .map_err(Error::message)?;

    // Process in windows of OUTPUT_FRAMES (201 frames)
    let window_count = audio.len().div_ceil(INPUT_SAMPLES);
    let mut all_frames: Vec<PitchFrame> = Vec::with_capacity(
        total_mel_frames.min(window_count * OUTPUT_FRAMES),
    );

    for window_index in 0..window_count {
        let window_start_sample = window_index * INPUT_SAMPLES;
        let mel_frame_start = window_start_sample / mel::HOP_SIZE;
        let mel_frame_end = (mel_frame_start + OUTPUT_FRAMES).min(total_mel_frames);
        let window_mel_frames = mel_frame_end - mel_frame_start;

        if window_mel_frames == 0 {
            break;
        }

        // Extract mel window (pad with zeros if shorter than 201 frames)
        let mut mel_window = vec![0.0_f32; OUTPUT_FRAMES * mel::MEL_BINS];
        for f in 0..window_mel_frames {
            let src_offset = (mel_frame_start + f) * mel::MEL_BINS;
            let dst_offset = f * mel::MEL_BINS;
            mel_window[dst_offset..dst_offset + mel::MEL_BINS]
                .copy_from_slice(&mel_data[src_offset..src_offset + mel::MEL_BINS]);
        }

        // Forward pass through the model
        // 1. Input stack: two Conv1D layers
        let after_conv0 = conv1d(
            &mel_window,
            OUTPUT_FRAMES,
            mel::MEL_BINS,
            weights.input_stack.w0,
            weights.input_stack.b0,
            D_MODEL,
            3,
        );
        let after_conv1 = conv1d(
            &after_conv0,
            OUTPUT_FRAMES,
            D_MODEL,
            weights.input_stack.w1,
            weights.input_stack.b1,
            D_MODEL,
            3,
        );

        // 2. Conformer encoder layers
        let mut hidden = after_conv1;
        for (layer_idx, layer) in weights.encoder_layers.iter().enumerate() {
            hidden = conformer_block(&hidden, OUTPUT_FRAMES, layer);
            let layer_frac =
                (layer_idx + 1) as f32 / N_ENCODER_LAYERS as f32;
            progress(
                0.20 + 0.60 * (window_index as f32 + layer_frac) / window_count as f32,
                "Running FCPE conformer layers",
                Some(((window_index + 1) as u64, window_count as u64)),
            );
        }

        // 3. Decode pitch
        let pitches = decode_pitch(&hidden, OUTPUT_FRAMES, &weights.output_head);

        // Append frames with window stitching
        for (local_index, hz) in pitches.iter().take(window_mel_frames).enumerate() {
            // Skip duplicate boundary frame on subsequent windows
            if window_index > 0 && local_index == 0 {
                continue;
            }
            let source_sample = window_start_sample + local_index * FRAME_HOP_SAMPLES;
            if source_sample > audio.len() {
                break;
            }
            all_frames.push(PitchFrame {
                time: source_sample as f64 / SAMPLE_RATE as f64,
                hz: *hz,
            });
        }
    }

    if all_frames.is_empty() {
        return Err(Error::message(
            "FCPE produced no source-bounded pitch frames",
        ));
    }

    // Write evidence
    progress(0.95, "Writing FCPE pitch evidence", None);
    let destination = output_dir.join("fcpe-pitch-evidence.json");
    let temporary = output_dir.join("fcpe-pitch-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(Error::Io)?;
    serde_json::to_writer(
        &mut file,
        &PitchEvidence {
            schema_version: 3,
            model_id: "fcpe",
            source_model_sha256: "",
            model_manifest_sha256: "",
            model_xml_sha256: "",
            model_bin_sha256: "",
            runtime_manifest_sha256: "fcpe-native-recipe-v1",
            backend: "ggml_native",
            timeline_step_ms: 10,
            sample_rate: SAMPLE_RATE as u32,
            window_samples: INPUT_SAMPLES as u32,
            window_hop_samples: INPUT_SAMPLES as u32,
            frames: all_frames,
        },
    )
    .map_err(Error::Json)?;
    writeln!(file).map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    std::fs::rename(&temporary, &destination).map_err(Error::Io)?;

    progress(1.0, "FCPE inference complete", None);
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_is_correct_at_boundary_values() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
    }

    #[test]
    fn glu_halves_channels_and_gates() {
        let input = vec![1.0, 2.0, 0.0, 0.0]; // 1 frame, 4 channels → 2
        let output = glu(&input, 1, 4);
        assert_eq!(output.len(), 2);
        // gate = sigmoid(0) = 0.5
        assert!((output[0] - 0.5).abs() < 1e-6);
        assert!((output[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn layer_norm_normalizes_correctly() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0];
        let weight = vec![1.0; 4];
        let bias = vec![0.0; 4];
        layer_norm(&mut data, 1, 4, &weight, &bias);
        // Mean should be ~0 after normalization
        let mean: f32 = data.iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-5);
    }
}
