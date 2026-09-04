//! Backend-agnostic conformance tests for any `Tensor` implementation.
//!
//! Every expected value here is computed independently of the tensor
//! implementation under test (plain array indexing, or a small reference
//! formula written directly in this file) rather than by calling the
//! implementation's own methods, so these tests actually catch a wrong
//! implementation instead of just checking self-consistency. `CpuTensor`
//! wires all of these in `cpu/mod.rs`; a future `GpuTensor` should wire the
//! same functions the same way.

use super::Tensor;

const TOLERANCE: f32 = 1e-4;

fn assert_close(actual: &[f32], expected: &[f32], context: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{context}: length mismatch (actual {:?}, expected {:?})",
        actual,
        expected
    );
    for (index, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
        let tolerance = TOLERANCE.max(e.abs() * TOLERANCE);
        assert!(
            (a - e).abs() <= tolerance,
            "{context}: index {index} expected {e}, got {a}"
        );
    }
}

fn export<T: Tensor>(tensor: &T) -> Vec<f32> {
    let n: usize = tensor.shape().iter().product();
    let mut buffer = vec![0.0; n];
    tensor.export(&mut buffer).expect("export should succeed");
    buffer
}

pub fn run_roundtrip<T: Tensor>(device: &T::Device) {
    let data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let tensor = T::from_data(&data, &[2, 3], device).unwrap();
    assert_eq!(tensor.shape(), &[2, 3]);
    assert_close(&export(&tensor), &data, "roundtrip");
}

pub fn run_layout_ops_preserve_view_semantics<T: Tensor>(device: &T::Device) {
    let shape = [2usize, 3, 4];
    let n: usize = shape.iter().product();
    let data: Vec<f32> = (0..n).map(|value| value as f32).collect();

    let transposed = T::from_data(&data, &shape, device)
        .unwrap()
        .transpose(0, 2)
        .unwrap();
    assert_eq!(transposed.shape(), &[4, 3, 2]);
    let transposed_contiguous = transposed.contiguous().unwrap();
    let mut expected_transposed = vec![0.0f32; n];
    for k in 0..4 {
        for j in 0..3 {
            for i in 0..2 {
                expected_transposed[k * 6 + j * 2 + i] = (i * 12 + j * 4 + k) as f32;
            }
        }
    }
    assert_close(
        &export(&transposed_contiguous),
        &expected_transposed,
        "transpose+contiguous",
    );

    let flat = transposed_contiguous.reshape(&[24]).unwrap();
    assert_eq!(flat.shape(), &[24]);
    assert_close(&export(&flat), &expected_transposed, "reshape");

    let sliced = T::from_data(&data, &shape, device)
        .unwrap()
        .slice(1, 1, 3)
        .unwrap();
    assert_eq!(sliced.shape(), &[2, 2, 4]);
    let mut expected_sliced = vec![0.0f32; 2 * 2 * 4];
    for i in 0..2 {
        for (jo, j) in (1..3).enumerate() {
            for k in 0..4 {
                expected_sliced[i * 8 + jo * 4 + k] = (i * 12 + j * 4 + k) as f32;
            }
        }
    }
    assert_close(
        &export(&sliced.contiguous().unwrap()),
        &expected_sliced,
        "slice",
    );

    let a = T::from_data(&data, &shape, device).unwrap();
    let b = T::from_data(&data, &shape, device).unwrap();
    let concatenated = T::concat(&[&a, &b], 0).unwrap();
    assert_eq!(concatenated.shape(), &[4, 3, 4]);
    let mut expected_concat = data.clone();
    expected_concat.extend_from_slice(&data);
    assert_close(&export(&concatenated), &expected_concat, "concat");
}

pub fn run_broadcast_add_and_mul_match_expected_values<T: Tensor>(device: &T::Device) {
    let a = T::from_data(&[1.0, 2.0, 3.0, 4.0], &[2, 2], device).unwrap();
    let b = T::from_data(&[10.0, 20.0, 30.0, 40.0], &[2, 2], device).unwrap();
    assert_close(
        &export(&a.clone().add(&b).unwrap()),
        &[11.0, 22.0, 33.0, 44.0],
        "exact-shape add",
    );
    assert_close(
        &export(&a.mul(&b).unwrap()),
        &[10.0, 40.0, 90.0, 160.0],
        "exact-shape mul",
    );

    let c = T::from_data(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], &[2, 3], device).unwrap();
    let d = T::from_data(&[10.0, 20.0, 30.0], &[3], device).unwrap();
    assert_close(
        &export(&c.add(&d).unwrap()),
        &[10.0, 21.0, 32.0, 13.0, 24.0, 35.0],
        "trailing-feature broadcast add",
    );
}

pub fn run_matmul_supports_2d_and_batched_3d_inputs<T: Tensor>(device: &T::Device) {
    let a = T::from_data(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2], device).unwrap();
    let diag = T::from_data(&[2.0, 0.0, 0.0, 3.0], &[2, 2], device).unwrap();
    let out = a.matmul(&diag).unwrap();
    assert_eq!(out.shape(), &[3, 2]);
    assert_close(
        &export(&out),
        &[2.0, 6.0, 6.0, 12.0, 10.0, 18.0],
        "2d matmul",
    );

    let lhs = T::from_data(
        &[1.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 2.0],
        &[2, 2, 2],
        device,
    )
    .unwrap();
    let rhs = T::from_data(
        &[5.0, 6.0, 7.0, 8.0, 1.0, 1.0, 1.0, 1.0],
        &[2, 2, 2],
        device,
    )
    .unwrap();
    let batched = lhs.matmul(&rhs).unwrap();
    assert_eq!(batched.shape(), &[2, 2, 2]);
    assert_close(
        &export(&batched),
        &[5.0, 6.0, 7.0, 8.0, 2.0, 2.0, 2.0, 2.0],
        "batched 3d matmul",
    );
}

pub fn run_matmul_handles_views_and_rejects_unsupported_batch_shapes<T: Tensor>(
    device: &T::Device,
) {
    let a = T::from_data(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3], device).unwrap();
    let a_t = a.transpose(0, 1).unwrap();
    assert_eq!(a_t.shape(), &[3, 2]);
    let identity = T::from_data(&[1.0, 0.0, 0.0, 1.0], &[2, 2], device).unwrap();
    let out = a_t.matmul(&identity).unwrap();
    assert_eq!(out.shape(), &[3, 2]);
    assert_close(
        &export(&out),
        &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0],
        "transposed-view matmul",
    );

    let vector = T::from_data(&[1.0, 2.0], &[2], device).unwrap();
    let matrix = T::from_data(&[1.0, 0.0, 0.0, 1.0], &[2, 2], device).unwrap();
    assert!(
        vector.matmul(&matrix).is_err(),
        "rank-1 matmul should be rejected"
    );

    let bad_a = T::from_data(&[1.0, 2.0, 3.0], &[1, 3], device).unwrap();
    let bad_b = T::from_data(&[1.0, 2.0], &[2, 1], device).unwrap();
    assert!(
        bad_a.matmul(&bad_b).is_err(),
        "mismatched inner dimensions should be rejected"
    );
}

pub fn run_linear_applies_weight_rows_and_optional_bias<T: Tensor>(device: &T::Device) {
    let input = T::from_data(&[1.0, 1.0, 1.0, 2.0, 2.0, 2.0], &[2, 3], device).unwrap();
    let weight = T::from_data(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3], device).unwrap();

    let no_bias = input.linear(&weight, None).unwrap();
    assert_eq!(no_bias.shape(), &[2, 2]);
    assert_close(&export(&no_bias), &[6.0, 15.0, 12.0, 30.0], "linear no bias");

    let bias = T::from_data(&[100.0, 200.0], &[2], device).unwrap();
    let with_bias = input.linear(&weight, Some(&bias)).unwrap();
    assert_close(
        &export(&with_bias),
        &[106.0, 215.0, 112.0, 230.0],
        "linear with bias",
    );
}

fn erf_approx_reference(x: f32) -> f32 {
    // Abramowitz & Stegun 7.1.26 — the exact approximation `gelu`/
    // `split_last_dim_two_gelu_mul` are specified against
    // (`cpu/elementwise.rs::erf_approx`). Any conforming backend must match
    // this specific polynomial, not "an" erf approximation.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152_1) * t + 1.421_413_8) * t - 0.284_496_72) * t
            + 0.254_829_6)
            * t)
            * (-x * x).exp();
    sign * y
}

pub fn run_normalization_and_activation_ops_match_reference_values<T: Tensor>(
    device: &T::Device,
) {
    let sigmoid_input = [0.0f32, 1.0, -1.0];
    let sig = T::from_data(&sigmoid_input, &[3], device)
        .unwrap()
        .sigmoid()
        .unwrap();
    let expected_sig: Vec<f32> = sigmoid_input
        .iter()
        .map(|&v| 1.0 / (1.0 + (-v).exp()))
        .collect();
    assert_close(&export(&sig), &expected_sig, "sigmoid");

    let scaled = T::from_data(&[1.0, 2.0, 3.0], &[3], device)
        .unwrap()
        .scale(2.0)
        .unwrap();
    assert_close(&export(&scaled), &[2.0, 4.0, 6.0], "scale");

    let gelu_input = [0.0f32, 1.0, -2.0];
    let gelu_out = T::from_data(&gelu_input, &[3], device)
        .unwrap()
        .gelu()
        .unwrap();
    let expected_gelu: Vec<f32> = gelu_input
        .iter()
        .map(|&v| 0.5 * v * (1.0 + erf_approx_reference(v / std::f32::consts::SQRT_2)))
        .collect();
    assert_close(&export(&gelu_out), &expected_gelu, "gelu");

    let norm_input = [1.0f32, 2.0, 3.0, 4.0];
    let weight = T::from_data(&[1.0, 1.0, 1.0, 1.0], &[4], device).unwrap();
    let eps = 1e-6f32;
    let normed = T::from_data(&norm_input, &[1, 4], device)
        .unwrap()
        .rms_norm(&weight, eps)
        .unwrap();
    let mean_square = norm_input.iter().map(|v| v * v).sum::<f32>() / 4.0;
    let inv_rms = 1.0 / (mean_square + eps).sqrt();
    let expected_norm: Vec<f32> = norm_input.iter().map(|&v| v * inv_rms).collect();
    assert_close(&export(&normed), &expected_norm, "rms_norm");

    let softmax_out = T::from_data(&[0.0, 1.0], &[1, 2], device)
        .unwrap()
        .softmax(-1)
        .unwrap();
    let e1 = 1.0f32.exp();
    let denom = 1.0 + e1;
    assert_close(&export(&softmax_out), &[1.0 / denom, e1 / denom], "softmax");
}

pub fn run_rope_rotates_each_head_using_global_positions<T: Tensor>(device: &T::Device) {
    let data = [1.0f32, 0.0, 1.0, 0.0];
    let tensor = T::from_data(&data, &[1, 2, 2], device).unwrap();
    let positions = [0i32, 1];
    let out = tensor.rope(&positions, 2, 1, 2, 10_000.0).unwrap();
    let (sin1, cos1) = 1.0f32.sin_cos();
    assert_close(&export(&out), &[1.0, 0.0, cos1, sin1], "rope");
}

pub fn run_region_rope_splits_global_and_region_rotation_halves<T: Tensor>(device: &T::Device) {
    let data = [1.0f32, 0.0, 1.0, 0.0];
    let tensor = T::from_data(&data, &[1, 1, 4], device).unwrap();
    let global_pos = [0i32];
    let region_ids = [1i32];
    let out = tensor
        .region_rope(&global_pos, &region_ids, 4, 1, 4, 10_000.0)
        .unwrap();
    let (sin1, cos1) = 1.0f32.sin_cos();
    // First half rotates by the (zero) global position and is unchanged;
    // second half rotates by the (nonzero) region id.
    assert_close(&export(&out), &[1.0, 0.0, cos1, sin1], "region_rope");
}

fn reference_conv1d_dw(
    input: &[f32],
    time: usize,
    channels: usize,
    kernel: &[f32],
    kernel_size: usize,
    bias: Option<&[f32]>,
    stride: usize,
    padding: usize,
) -> (Vec<f32>, usize) {
    let padded = time + 2 * padding;
    let out_time = if padded < kernel_size {
        0
    } else {
        (padded - kernel_size) / stride + 1
    };
    let mut out = vec![0.0f32; out_time * channels];
    for out_t in 0..out_time {
        for channel in 0..channels {
            let mut sum = bias.map_or(0.0, |b| b[channel]);
            for k in 0..kernel_size {
                let input_index = out_t * stride + k;
                if input_index < padding {
                    continue;
                }
                let input_t = input_index - padding;
                if input_t >= time {
                    continue;
                }
                sum += input[input_t * channels + channel] * kernel[channel * kernel_size + k];
            }
            out[out_t * channels + channel] = sum;
        }
    }
    (out, out_time)
}

pub fn run_depthwise_conv_applies_per_channel_kernels<T: Tensor>(device: &T::Device) {
    let time = 4;
    let channels = 2;
    let input_data = [1.0f32, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0];
    let kernel_data = [1.0f32, 1.0, 0.0, 1.0];
    let bias_data = [100.0f32, 200.0];

    let input = T::from_data(&input_data, &[time, channels], device).unwrap();
    let kernel = T::from_data(&kernel_data, &[channels, 2], device).unwrap();
    let bias = T::from_data(&bias_data, &[channels], device).unwrap();

    let out_plain = input.clone().conv1d_dw(&kernel, None, 1, 0).unwrap();
    let (expected_plain, time_plain) =
        reference_conv1d_dw(&input_data, time, channels, &kernel_data, 2, None, 1, 0);
    assert_eq!(out_plain.shape(), &[time_plain, channels]);
    assert_close(&export(&out_plain), &expected_plain, "conv1d_dw plain");

    let out_padded = input.conv1d_dw(&kernel, Some(&bias), 1, 1).unwrap();
    let (expected_padded, time_padded) = reference_conv1d_dw(
        &input_data,
        time,
        channels,
        &kernel_data,
        2,
        Some(&bias_data),
        1,
        1,
    );
    assert_eq!(out_padded.shape(), &[time_padded, channels]);
    assert_close(
        &export(&out_padded),
        &expected_padded,
        "conv1d_dw with bias and padding",
    );
}

pub fn run_embedding_and_repeat_return_expected_rows<T: Tensor>(device: &T::Device) {
    let table_data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
    let table = T::from_data(&table_data, &[3, 2], device).unwrap();
    let indices = [2i32, 0, 1];
    let gathered = T::embedding(&table, &indices).unwrap();
    assert_eq!(gathered.shape(), &[3, 2]);
    assert_close(
        &export(&gathered),
        &[5.0, 6.0, 1.0, 2.0, 3.0, 4.0],
        "embedding",
    );

    let base = T::from_data(&[1.0, 2.0, 3.0, 4.0], &[2, 2], device).unwrap();
    let repeated = base.repeat(0, 3).unwrap();
    assert_eq!(repeated.shape(), &[6, 2]);
    assert_close(
        &export(&repeated),
        &[
            1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0,
        ],
        "repeat",
    );
}

fn reference_fused_attention(
    q: &[f32],
    k: &[f32],
    v: &[f32],
    mask: Option<&[f32]>,
    q_len: usize,
    key_len: usize,
    head_dim: usize,
    scale: f32,
) -> Vec<f32> {
    let mut scores = vec![0.0f32; q_len * key_len];
    for i in 0..q_len {
        for j in 0..key_len {
            let mut dot = 0.0f32;
            for d in 0..head_dim {
                dot += q[i * head_dim + d] * k[j * head_dim + d];
            }
            scores[i * key_len + j] = dot * scale + mask.map_or(0.0, |m| m[i * key_len + j]);
        }
    }
    for row in scores.chunks_mut(key_len) {
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for value in row.iter_mut() {
            *value = (*value - max).exp();
            sum += *value;
        }
        for value in row.iter_mut() {
            *value /= sum;
        }
    }
    let mut out = vec![0.0f32; q_len * head_dim];
    for i in 0..q_len {
        for d in 0..head_dim {
            let mut acc = 0.0f32;
            for j in 0..key_len {
                acc += scores[i * key_len + j] * v[j * head_dim + d];
            }
            out[i * head_dim + d] = acc;
        }
    }
    out
}

pub fn run_fused_attention_matches_reference<T: Tensor>(device: &T::Device) {
    let q_data = [1.0f32, 0.0, 0.0, 1.0];
    let k_data = [1.0f32, 0.0, 0.0, 1.0];
    let v_data = [10.0f32, 20.0, 30.0, 40.0];

    let q = T::from_data(&q_data, &[1, 2, 2], device).unwrap();
    let k = T::from_data(&k_data, &[1, 2, 2], device).unwrap();
    let v = T::from_data(&v_data, &[1, 2, 2], device).unwrap();
    let out = T::fused_attention(&q, &k, &v, None, 1.0).unwrap();
    assert_eq!(out.shape(), &[1, 2, 2]);
    let expected = reference_fused_attention(&q_data, &k_data, &v_data, None, 2, 2, 2, 1.0);
    assert_close(&export(&out), &expected, "fused_attention no mask");

    let mask_data = [0.0f32, f32::NEG_INFINITY, 0.0, 0.0];
    let mask = T::from_data(&mask_data, &[2, 2], device).unwrap();
    let q2 = T::from_data(&q_data, &[1, 2, 2], device).unwrap();
    let k2 = T::from_data(&k_data, &[1, 2, 2], device).unwrap();
    let v2 = T::from_data(&v_data, &[1, 2, 2], device).unwrap();
    let out_masked = T::fused_attention(&q2, &k2, &v2, Some(&mask), 1.0).unwrap();
    let expected_masked = reference_fused_attention(
        &q_data,
        &k_data,
        &v_data,
        Some(&mask_data),
        2,
        2,
        2,
        1.0,
    );
    assert_close(
        &export(&out_masked),
        &expected_masked,
        "fused_attention masked",
    );
}
