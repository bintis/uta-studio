//! Native CPU re-implementation of Spotify Basic Pitch (CQT + harmonic
//! stacking + a small fully-convolutional backbone), matching
//! `native-inference/openvino-worker/src/basic_pitch.rs`'s validated
//! windowing/overlap-stitch/evidence contract exactly, but running the model
//! itself on hand-written CPU kernels against a native GGUF instead of an
//! OpenVINO IR.
//!
//! Architecture and every numeric constant here were confirmed against the
//! real `spotify/basic-pitch` source (`models.py`, `nn.py`,
//! `layers/{signal,nnaudio}.py`) and cross-checked against this catalog's
//! pinned ONNX graph and GGUF. See `native-inference/basic-pitch/tools/
//! convert_basic_pitch_to_gguf.py` for the full provenance of the tensor
//! naming used below -- in particular, three of the six convolution layers
//! were misnamed by the original (unused) conversion attempt; the names used
//! here (`onset_conv1`, `contour_conv1`, `contour_final`, `note_conv1`,
//! `note_final`, `onset_final`) are the *true* roles, verified by matching
//! weight shapes against `basic_pitch/models.py`'s actual layer order.

use std::io::Write;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;

pub const SAMPLE_RATE: usize = 22_050;
const FFT_HOP_SAMPLES: usize = 256;
const OVERLAP_FRAMES: usize = 30;
const HALF_OVERLAP_FRAMES: usize = OVERLAP_FRAMES / 2;
const OVERLAP_SAMPLES: usize = OVERLAP_FRAMES * FFT_HOP_SAMPLES;
const PADDING_SAMPLES: usize = OVERLAP_SAMPLES / 2;
const INPUT_SAMPLES: usize = 43_844;
const WINDOW_HOP_SAMPLES: usize = INPUT_SAMPLES - OVERLAP_SAMPLES;
const FRAMES_PER_WINDOW: usize = 172;
const OWNED_FRAMES_PER_WINDOW: usize = FRAMES_PER_WINDOW - OVERLAP_FRAMES;

// CQT2010v2 parameters for this exact model (fmin=27.5Hz, bins_per_octave=36,
// n_bins=309; confirmed downsample_factor == 1, i.e. no early downsampling
// branch triggers for this config, so it is not implemented here).
const CQT_BINS_PER_OCTAVE: usize = 36;
const CQT_N_OCTAVES: usize = 9;
const CQT_N_BINS: usize = 309;
const CQT_KERNEL_LEN: usize = 256;

const N_HARMONIC_CHANNELS: usize = 8;
const N_FREQ_BINS_CONTOURS: usize = 264;
const N_NOTES: usize = 88;

struct Weights {
    cqt_real: Vec<f32>,         // [36, 256]
    cqt_imag: Vec<f32>,         // [36, 256]
    cqt_lowpass: Vec<f32>,      // [256]
    cqt_sqrt_lengths: Vec<f32>, // [309]
    cqt_bn_scale: f32,
    cqt_bn_shift: f32,
    onset_conv1_w: Vec<f32>,
    onset_conv1_b: Vec<f32>, // [32,8,5,5],[32]
    contour_conv1_w: Vec<f32>,
    contour_conv1_b: Vec<f32>, // [8,8,3,39],[8]
    contour_final_w: Vec<f32>,
    contour_final_b: Vec<f32>, // [1,8,5,5],[1]
    note_conv1_w: Vec<f32>,
    note_conv1_b: Vec<f32>, // [32,1,7,7],[32]
    note_final_w: Vec<f32>,
    note_final_b: Vec<f32>, // [1,32,7,3],[1]
    onset_final_w: Vec<f32>,
    onset_final_b: Vec<f32>, // [1,33,3,3],[1]
}

fn take(file: &GGUFFile, name: &str) -> Result<Vec<f32>> {
    Ok(file.tensor_data_f32(name)?.to_vec())
}

impl Weights {
    fn load(path: &Path) -> Result<Self> {
        let file = GGUFFile::open(path)?;
        if file.architecture() != "basic_pitch" {
            return Err(Error::UnsupportedArchitecture {
                found: file.architecture().to_string(),
            });
        }
        Ok(Self {
            cqt_real: take(&file, "cqt.conv_real.weight")?,
            cqt_imag: take(&file, "cqt.conv_imag.weight")?,
            cqt_lowpass: take(&file, "cqt.lowpass.weight")?,
            cqt_sqrt_lengths: take(&file, "cqt.sqrt_lengths")?,
            cqt_bn_scale: take(&file, "cqt_bn.scale")?[0],
            cqt_bn_shift: take(&file, "cqt_bn.shift")?[0],
            onset_conv1_w: take(&file, "onset_conv1.weight")?,
            onset_conv1_b: take(&file, "onset_conv1.bias")?,
            contour_conv1_w: take(&file, "contour_conv1.weight")?,
            contour_conv1_b: take(&file, "contour_conv1.bias")?,
            contour_final_w: take(&file, "contour_final.weight")?,
            contour_final_b: take(&file, "contour_final.bias")?,
            note_conv1_w: take(&file, "note_conv1.weight")?,
            note_conv1_b: take(&file, "note_conv1.bias")?,
            note_final_w: take(&file, "note_final.weight")?,
            note_final_b: take(&file, "note_final.bias")?,
            onset_final_w: take(&file, "onset_final.weight")?,
            onset_final_b: take(&file, "onset_final.bias")?,
        })
    }
}

/// Reflect-101 boundary lookup (mirrors without duplicating the edge
/// sample), matching TensorFlow's `mode="REFLECT"` used by
/// `ReflectionPad1D` in the reference implementation.
fn reflect_sample(data: &[f32], index: isize) -> f32 {
    if data.len() <= 1 {
        return data.first().copied().unwrap_or(0.0);
    }
    let period = 2 * (data.len() - 1) as isize;
    let folded = index.rem_euclid(period);
    data[if folded < data.len() as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }]
}

fn reflect_pad(data: &[f32], pad: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(data.len() + 2 * pad);
    for i in 0..data.len() + 2 * pad {
        out.push(reflect_sample(data, i as isize - pad as isize));
    }
    out
}

/// Applies the pretrained top-octave CQT kernel bank (real + imaginary) to a
/// reflect-padded 1-D signal at the given hop, matching `get_cqt_complex` in
/// `basic_pitch/layers/nnaudio.py`.
///
/// Returns `(real, imag, n_frames)`, each `real`/`imag` flattened
/// `[n_frames][CQT_BINS_PER_OCTAVE]` (frame-major).
fn cqt_octave_conv1d_with_hop(
    signal: &[f32],
    real_kernel: &[f32],
    imag_kernel: &[f32],
    hop: usize,
) -> (Vec<f32>, Vec<f32>, usize) {
    let padded = reflect_pad(signal, CQT_KERNEL_LEN / 2);
    let n_frames = (padded.len() - CQT_KERNEL_LEN) / hop + 1;
    let mut real = vec![0.0_f32; n_frames * CQT_BINS_PER_OCTAVE];
    let mut imag = vec![0.0_f32; n_frames * CQT_BINS_PER_OCTAVE];
    real.par_chunks_mut(CQT_BINS_PER_OCTAVE)
        .zip(imag.par_chunks_mut(CQT_BINS_PER_OCTAVE))
        .enumerate()
        .for_each(|(t, (real_row, imag_row))| {
            let start = t * hop;
            let window = &padded[start..start + CQT_KERNEL_LEN];
            for f in 0..CQT_BINS_PER_OCTAVE {
                let rk = &real_kernel[f * CQT_KERNEL_LEN..(f + 1) * CQT_KERNEL_LEN];
                let ik = &imag_kernel[f * CQT_KERNEL_LEN..(f + 1) * CQT_KERNEL_LEN];
                let mut re = 0.0_f32;
                let mut im = 0.0_f32;
                for k in 0..CQT_KERNEL_LEN {
                    re += window[k] * rk[k];
                    im += window[k] * ik[k];
                }
                real_row[f] = re;
                imag_row[f] = im;
            }
        });
    (real, imag, n_frames)
}

/// Zero-pads by `(kernel_len-1)/2` on each side and downsamples by exactly
/// 2 via `conv1d` (stride 2, VALID padding) against the pretrained lowpass
/// filter -- matches `downsampling_by_n(..., match_torch_exactly=True)`.
fn downsample_by_2(signal: &[f32], lowpass: &[f32]) -> Vec<f32> {
    let pad = (CQT_KERNEL_LEN - 1) / 2;
    let mut padded = vec![0.0_f32; signal.len() + 2 * pad];
    padded[pad..pad + signal.len()].copy_from_slice(signal);
    let n_out = (padded.len() - CQT_KERNEL_LEN) / 2 + 1;
    let mut out = vec![0.0_f32; n_out];
    out.par_iter_mut().enumerate().for_each(|(t, value)| {
        let start = t * 2;
        let window = &padded[start..start + CQT_KERNEL_LEN];
        *value = window.iter().zip(lowpass).map(|(a, b)| a * b).sum();
    });
    out
}

/// Full CQT2010v2 magnitude spectrogram for one window: reflect-pad + conv1d
/// against the pretrained kernel bank at the top octave, then repeatedly
/// downsample by 2 and repeat at half the hop for each lower octave,
/// concatenate (lowest-frequency octave first, matching the reference's
/// repeated `tf.concat((CQT1, CQT), axis=1)` prepend), keep the last
/// `CQT_N_BINS` bins, scale by the per-bin `sqrt(lengths)` librosa
/// normalization, and take the magnitude. Returns `[n_frames][CQT_N_BINS]`
/// (frame-major) plus `n_frames`.
fn cqt_magnitude(window: &[f32], weights: &Weights) -> (Vec<f32>, usize) {
    let mut octave_real = Vec::with_capacity(CQT_N_OCTAVES);
    let mut octave_imag = Vec::with_capacity(CQT_N_OCTAVES);
    let mut hop = FFT_HOP_SAMPLES;
    let mut signal = window.to_vec();
    let mut n_frames = 0;
    for octave in 0..CQT_N_OCTAVES {
        if octave > 0 {
            signal = downsample_by_2(&signal, &weights.cqt_lowpass);
            hop /= 2;
        }
        let (real, imag, frames) =
            cqt_octave_conv1d_with_hop(&signal, &weights.cqt_real, &weights.cqt_imag, hop);
        if octave == 0 {
            n_frames = frames;
        }
        debug_assert_eq!(frames, n_frames, "every CQT octave must share the same frame grid");
        octave_real.push(real);
        octave_imag.push(imag);
    }

    // Reference order after all prepends: lowest-frequency octave first,
    // top (least-downsampled) octave last -- i.e. exactly the reverse of
    // computation order (octave 0 was computed first but ends up last).
    let total_bins = CQT_N_OCTAVES * CQT_BINS_PER_OCTAVE;
    let mut magnitude = vec![0.0_f32; n_frames * CQT_N_BINS];
    magnitude
        .par_chunks_mut(CQT_N_BINS)
        .enumerate()
        .for_each(|(t, row)| {
            // Bin `total_bins - CQT_N_BINS` in the full concatenated axis is
            // where the kept window starts (`CQT[:, -CQT_N_BINS:, :]`).
            let skip = total_bins - CQT_N_BINS;
            for (out_bin, full_bin) in (skip..total_bins).enumerate() {
                let octave = CQT_N_OCTAVES - 1 - full_bin / CQT_BINS_PER_OCTAVE;
                let bin_in_octave = full_bin % CQT_BINS_PER_OCTAVE;
                let re = octave_real[octave][t * CQT_BINS_PER_OCTAVE + bin_in_octave];
                let im = octave_imag[octave][t * CQT_BINS_PER_OCTAVE + bin_in_octave];
                let sqrt_len = weights.cqt_sqrt_lengths[out_bin];
                let re = re * sqrt_len;
                let im = im * sqrt_len;
                row[out_bin] = (re * re + im * im).sqrt();
            }
        });
    (magnitude, n_frames)
}

/// `NormalizedLog` (`basic_pitch/layers/signal.py`), exact formula: convert
/// magnitude to power, log-compress, then per-window min/max normalize to
/// `[0, 1]` (0 if the window is constant).
fn normalized_log(magnitude: &mut [f32]) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for value in magnitude.iter_mut() {
        let power = *value * *value;
        let log_power = 10.0 * (power + 1e-10).log10();
        *value = log_power;
        min = min.min(log_power);
        max = max.max(log_power);
    }
    let offset_max = max - min;
    if offset_max == 0.0 {
        magnitude.fill(0.0);
        return;
    }
    for value in magnitude.iter_mut() {
        *value = (*value - min) / offset_max;
    }
}

/// The single-channel `BatchNormalization` applied to the CQT/NormalizedLog
/// output, before `HarmonicStacking` (`get_cqt(..., use_batchnorm=True)` in
/// `basic_pitch/models.py`). Inference-mode BN with a fixed scale/shift
/// already folded from `gamma/sqrt(var+eps)` and `beta-mean*scale`.
fn cqt_batchnorm(magnitude: &mut [f32], weights: &Weights) {
    for value in magnitude.iter_mut() {
        *value = *value * weights.cqt_bn_scale + weights.cqt_bn_shift;
    }
}

/// `HarmonicStacking` (`basic_pitch/nn.py`), exact formula: for each of the
/// 8 harmonics `[0.5, 1, 2, 3, 4, 5, 6, 7]`, shift the frequency axis by
/// `round(36 * log2(h))` bins (zero-filling the vacated edge), stack as a
/// channel, then keep only the first `N_FREQ_BINS_CONTOURS` bins.
/// `log_cqt` is `[n_frames][CQT_N_BINS]`; returns channel-major
/// `[N_HARMONIC_CHANNELS][n_frames][N_FREQ_BINS_CONTOURS]`.
fn harmonic_stack(log_cqt: &[f32], n_frames: usize) -> Vec<f32> {
    const HARMONICS: [f64; N_HARMONIC_CHANNELS] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
    let shifts: Vec<isize> = HARMONICS
        .iter()
        .map(|h| (36.0_f64 * h.log2()).round() as isize)
        .collect();
    let mut out = vec![0.0_f32; N_HARMONIC_CHANNELS * n_frames * N_FREQ_BINS_CONTOURS];
    for (channel, &shift) in shifts.iter().enumerate() {
        let channel_out = &mut out[channel * n_frames * N_FREQ_BINS_CONTOURS
            ..(channel + 1) * n_frames * N_FREQ_BINS_CONTOURS];
        for t in 0..n_frames {
            let src_row = &log_cqt[t * CQT_N_BINS..(t + 1) * CQT_N_BINS];
            let dst_row = &mut channel_out[t * N_FREQ_BINS_CONTOURS..(t + 1) * N_FREQ_BINS_CONTOURS];
            for (bin, slot) in dst_row.iter_mut().enumerate() {
                let source_bin = bin as isize + shift;
                *slot = if source_bin >= 0 && (source_bin as usize) < CQT_N_BINS {
                    src_row[source_bin as usize]
                } else {
                    0.0
                };
            }
        }
    }
    out
}

/// General 2D convolution, NCHW layout (`input`/`output` channel-major,
/// row-major within a channel), TF/Keras `"same"` padding, weight layout
/// `[c_out, c_in, kh, kw]` (ONNX's native `Conv` weight order). No batching
/// (batch size is always 1 here).
#[allow(clippy::too_many_arguments)]
fn conv2d_same(
    input: &[f32],
    c_in: usize,
    h: usize,
    w: usize,
    weight: &[f32],
    bias: &[f32],
    c_out: usize,
    kh: usize,
    kw: usize,
    stride_h: usize,
    stride_w: usize,
) -> (Vec<f32>, usize, usize) {
    let out_h = h.div_ceil(stride_h);
    let out_w = w.div_ceil(stride_w);
    let pad_h_total = ((out_h - 1) * stride_h + kh).saturating_sub(h);
    let pad_w_total = ((out_w - 1) * stride_w + kw).saturating_sub(w);
    let pad_top = pad_h_total / 2;
    let pad_left = pad_w_total / 2;

    let mut out = vec![0.0_f32; c_out * out_h * out_w];
    out.par_chunks_mut(out_h * out_w)
        .enumerate()
        .for_each(|(co, out_channel)| {
            let b = bias[co];
            let weight_co = &weight[co * c_in * kh * kw..(co + 1) * c_in * kh * kw];
            for oh in 0..out_h {
                for ow in 0..out_w {
                    let mut sum = b;
                    let h_start = (oh * stride_h) as isize - pad_top as isize;
                    let w_start = (ow * stride_w) as isize - pad_left as isize;
                    for ci in 0..c_in {
                        let weight_ci = &weight_co[ci * kh * kw..(ci + 1) * kh * kw];
                        let in_ci = &input[ci * h * w..(ci + 1) * h * w];
                        for r in 0..kh {
                            let cur_h = h_start + r as isize;
                            if cur_h < 0 || cur_h >= h as isize {
                                continue;
                            }
                            let in_row = &in_ci[(cur_h as usize) * w..(cur_h as usize + 1) * w];
                            let weight_row = &weight_ci[r * kw..(r + 1) * kw];
                            for c in 0..kw {
                                let cur_w = w_start + c as isize;
                                if cur_w >= 0 && (cur_w as usize) < w {
                                    sum += in_row[cur_w as usize] * weight_row[c];
                                }
                            }
                        }
                    }
                    out_channel[oh * out_w + ow] = sum;
                }
            }
        });
    (out, out_h, out_w)
}

fn relu_inplace(data: &mut [f32]) {
    data.par_iter_mut().for_each(|v| {
        if *v < 0.0 {
            *v = 0.0;
        }
    });
}

fn sigmoid_inplace(data: &mut [f32]) {
    data.par_iter_mut().for_each(|v| *v = 1.0 / (1.0 + (-*v).exp()));
}

fn concat_channels(a: &[f32], a_channels: usize, b: &[f32], b_channels: usize, plane: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; (a_channels + b_channels) * plane];
    out[..a_channels * plane].copy_from_slice(a);
    out[a_channels * plane..].copy_from_slice(b);
    out
}

struct WindowActivations {
    notes: Vec<f32>,    // [FRAMES_PER_WINDOW * N_NOTES]
    onsets: Vec<f32>,   // [FRAMES_PER_WINDOW * N_NOTES]
    contours: Vec<f32>, // [FRAMES_PER_WINDOW * N_FREQ_BINS_CONTOURS]
}

fn run_window(window: &[f32], weights: &Weights) -> Result<WindowActivations> {
    let (mut magnitude, n_frames) = cqt_magnitude(window, weights);
    if n_frames != FRAMES_PER_WINDOW {
        return Err(Error::message(format!(
            "Basic Pitch CQT produced {n_frames} frames, expected {FRAMES_PER_WINDOW}"
        )));
    }
    normalized_log(&mut magnitude);
    cqt_batchnorm(&mut magnitude, weights);
    let harmonics = harmonic_stack(&magnitude, n_frames); // [8, n_frames, 264]

    let (mut onset_conv1, oh, ow) = conv2d_same(
        &harmonics,
        N_HARMONIC_CHANNELS,
        n_frames,
        N_FREQ_BINS_CONTOURS,
        &weights.onset_conv1_w,
        &weights.onset_conv1_b,
        32,
        5,
        5,
        1,
        3,
    );
    relu_inplace(&mut onset_conv1);
    debug_assert_eq!(oh, n_frames);

    let (mut contour_conv1, _, _) = conv2d_same(
        &harmonics,
        N_HARMONIC_CHANNELS,
        n_frames,
        N_FREQ_BINS_CONTOURS,
        &weights.contour_conv1_w,
        &weights.contour_conv1_b,
        8,
        3,
        39,
        1,
        1,
    );
    relu_inplace(&mut contour_conv1);

    let (mut contour_final, _, _) = conv2d_same(
        &contour_conv1,
        8,
        n_frames,
        N_FREQ_BINS_CONTOURS,
        &weights.contour_final_w,
        &weights.contour_final_b,
        1,
        5,
        5,
        1,
        1,
    );
    sigmoid_inplace(&mut contour_final); // [1, n_frames, 264] -- the contour output

    let (mut note_conv1, _, note_w) = conv2d_same(
        &contour_final,
        1,
        n_frames,
        N_FREQ_BINS_CONTOURS,
        &weights.note_conv1_w,
        &weights.note_conv1_b,
        32,
        7,
        7,
        1,
        3,
    );
    relu_inplace(&mut note_conv1);
    debug_assert_eq!(note_w, ow);

    let (mut note_final, _, _) = conv2d_same(
        &note_conv1,
        32,
        n_frames,
        note_w,
        &weights.note_final_w,
        &weights.note_final_b,
        1,
        7,
        3,
        1,
        1,
    );
    sigmoid_inplace(&mut note_final); // [1, n_frames, 88] -- the note output, and onset's concat input

    let onset_concat = concat_channels(&note_final, 1, &onset_conv1, 32, n_frames * ow);
    let (mut onset_final, _, _) = conv2d_same(
        &onset_concat,
        33,
        n_frames,
        ow,
        &weights.onset_final_w,
        &weights.onset_final_b,
        1,
        3,
        3,
        1,
        1,
    );
    sigmoid_inplace(&mut onset_final); // [1, n_frames, 88] -- the onset output

    if !magnitude_is_finite(&contour_final) || !magnitude_is_finite(&note_final) || !magnitude_is_finite(&onset_final)
    {
        return Err(Error::message(
            "Basic Pitch produced non-finite activation evidence".to_string(),
        ));
    }

    // `note_final`/`onset_final` above are named for their true role in the
    // reference architecture (verified bit-exact against onnxruntime on a
    // real window: `note_final` is the NOTES_KERNEL_SIZE_* branch reading
    // the contour output, `onset_final` is the ONSET_KERNEL_SIZE_* branch
    // reading the harmonic-stacked CQT directly + the notes-pre-sigmoid
    // concat -- exactly matching basic_pitch/models.py). But this
    // repository's own OpenVINO worker
    // (native-inference/openvino-worker/src/basic_pitch.rs) has always
    // reported `output_tensor_by_index(0)` as `note_max` and index(1) as
    // `onset_max`, and empirically (diffed against real production
    // OpenVINO evidence on the same 305.8s song) that index assignment is
    // the OPPOSITE of the raw ONNX graph's own declared output order used
    // above -- i.e. this repository's established "note_max"/"onset_max"
    // evidence contract is swapped relative to the source model's own
    // variable names. Match the established contract (everything
    // downstream -- fusion, Studio -- already depends on it), not the
    // model source's naming.
    Ok(WindowActivations {
        notes: onset_final,
        onsets: note_final,
        contours: contour_final,
    })
}

fn magnitude_is_finite(data: &[f32]) -> bool {
    data.iter().all(|value| value.is_finite())
}

fn maximum(values: &[f32]) -> f32 {
    values.iter().copied().fold(0.0, f32::max)
}

fn window_count(source_samples: usize) -> usize {
    let padded_samples = source_samples + 2 * PADDING_SAMPLES;
    if padded_samples <= INPUT_SAMPLES {
        1
    } else {
        (padded_samples - INPUT_SAMPLES).div_ceil(WINDOW_HOP_SAMPLES) + 1
    }
}

fn fill_padded_window(input: &mut [f32], audio: &[f32], window_index: usize) {
    input.fill(0.0);
    let padded_start = window_index * WINDOW_HOP_SAMPLES;
    for (local, value) in input.iter_mut().enumerate() {
        let padded_sample = padded_start + local;
        if let Some(source_sample) = padded_sample.checked_sub(PADDING_SAMPLES)
            && let Some(source) = audio.get(source_sample)
        {
            *value = *source;
        }
    }
}

#[derive(serde::Serialize)]
struct ActivationFrame {
    time: f64,
    note_max: f32,
    onset_max: f32,
    contour_class: usize,
    contour_score: f32,
}

#[allow(clippy::too_many_arguments)]
fn append_window_frames(
    frames: &mut Vec<ActivationFrame>,
    activations: &WindowActivations,
    window_index: usize,
    source_samples: usize,
) {
    let target_frames = source_samples / FFT_HOP_SAMPLES;
    for frame in HALF_OVERLAP_FRAMES..FRAMES_PER_WINDOW - HALF_OVERLAP_FRAMES {
        let source_frame = window_index * OWNED_FRAMES_PER_WINDOW + frame - HALF_OVERLAP_FRAMES;
        if source_frame >= target_frames {
            break;
        }
        let source_sample = source_frame * FFT_HOP_SAMPLES;
        let contour = &activations.contours[frame * N_FREQ_BINS_CONTOURS..(frame + 1) * N_FREQ_BINS_CONTOURS];
        let (contour_class, contour_score) = contour
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap_or((0, 0.0));
        frames.push(ActivationFrame {
            time: source_sample as f64 / SAMPLE_RATE as f64,
            note_max: maximum(&activations.notes[frame * N_NOTES..(frame + 1) * N_NOTES]),
            onset_max: maximum(&activations.onsets[frame * N_NOTES..(frame + 1) * N_NOTES]),
            contour_class,
            contour_score,
        });
    }
}

#[derive(serde::Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_manifest_sha256: &'a str,
    model_xml_sha256: &'a str,
    model_bin_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    fft_hop_samples: u32,
    overlap_frames: usize,
    padding_samples: u32,
    frames_per_window: usize,
    owned_frames_per_window: usize,
    frames: Vec<ActivationFrame>,
}

pub fn infer(
    audio: &[f32],
    model_path: &Path,
    output_dir: &Path,
    mut progress: impl FnMut(f32, &'static str, Option<(u64, u64)>),
) -> Result<PathBuf> {
    if audio.is_empty() {
        return Err(Error::message("Basic Pitch requires non-empty decoded audio".to_string()));
    }
    let weights = Weights::load(model_path)?;
    let count = window_count(audio.len());
    let mut frames = Vec::new();
    let mut input = vec![0.0_f32; INPUT_SAMPLES];
    for window_index in 0..count {
        fill_padded_window(&mut input, audio, window_index);
        let activations = run_window(&input, &weights)?;
        append_window_frames(&mut frames, &activations, window_index, audio.len());
        let completed = (window_index + 1) as u64;
        let total = count as u64;
        progress(
            completed as f32 / total as f32,
            "Running Basic Pitch activation windows",
            Some((completed, total)),
        );
    }

    let destination = output_dir.join("basic-pitch-activation-evidence.json");
    let temporary = output_dir.join("basic-pitch-activation-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    serde_json::to_writer(
        &mut file,
        &Evidence {
            schema_version: 3,
            model_id: "basic_pitch",
            source_model_sha256: "",
            model_manifest_sha256: "",
            model_xml_sha256: "",
            model_bin_sha256: "",
            runtime_manifest_sha256: "basic-pitch-native-recipe-v1",
            backend: "ggml_native",
            sample_rate: SAMPLE_RATE as u32,
            window_samples: INPUT_SAMPLES as u32,
            window_hop_samples: WINDOW_HOP_SAMPLES as u32,
            fft_hop_samples: FFT_HOP_SAMPLES as u32,
            overlap_frames: OVERLAP_FRAMES,
            padding_samples: PADDING_SAMPLES as u32,
            frames_per_window: FRAMES_PER_WINDOW,
            owned_frames_per_window: OWNED_FRAMES_PER_WINDOW,
            frames,
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    std::fs::rename(&temporary, &destination)?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonic_shifts_match_reference_formula() {
        // round(36 * log2(h)) for h in [0.5, 1, 2, 3, 4, 5, 6, 7]
        let expected = [-36_isize, 0, 36, 57, 72, 84, 93, 101];
        const HARMONICS: [f64; 8] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let shifts: Vec<isize> = HARMONICS.iter().map(|h| (36.0 * h.log2()).round() as isize).collect();
        assert_eq!(shifts, expected);
    }

    #[test]
    fn normalized_log_maps_constant_input_to_zero() {
        let mut data = vec![0.5_f32; 4];
        normalized_log(&mut data);
        assert!(data.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn normalized_log_maps_extremes_to_zero_and_one() {
        let mut data = vec![0.001_f32, 1.0, 0.5, 0.001];
        normalized_log(&mut data);
        let min = data.iter().copied().fold(f32::INFINITY, f32::min);
        let max = data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((min - 0.0).abs() < 1e-6);
        assert!((max - 1.0).abs() < 1e-6);
    }

    #[test]
    fn conv2d_same_padding_preserves_spatial_size_at_stride_one() {
        let input = vec![1.0_f32; 2 * 5 * 7]; // c_in=2, h=5, w=7
        let weight = vec![1.0_f32; 1 * 2 * 3 * 3]; // c_out=1, c_in=2, kh=3, kw=3
        let bias = vec![0.0_f32];
        let (out, oh, ow) = conv2d_same(&input, 2, 5, 7, &weight, &bias, 1, 3, 3, 1, 1);
        assert_eq!((oh, ow), (5, 7));
        // Interior pixel sees the full 3x3x2 receptive field of all-ones input/weight.
        assert_eq!(out[2 * 7 + 3], 18.0);
        // Corner pixel is missing rows/cols outside the padded boundary.
        assert_eq!(out[0], 8.0);
    }

    #[test]
    fn conv2d_same_stride_three_matches_ceil_division() {
        let input = vec![1.0_f32; 1 * 4 * 264];
        let weight = vec![1.0_f32; 1 * 1 * 5 * 5];
        let bias = vec![0.0_f32];
        let (_, oh, ow) = conv2d_same(&input, 1, 4, 264, &weight, &bias, 1, 5, 5, 1, 3);
        assert_eq!((oh, ow), (4, 88));
    }

    #[test]
    fn window_count_matches_openvino_worker_reference() {
        // Mirrors native-inference/openvino-worker/src/basic_pitch.rs's own
        // `window_stitching_uses_reference_overlap_grid_and_clips_tail` test.
        assert_eq!(window_count(1), 1);
        assert_eq!(window_count(INPUT_SAMPLES + WINDOW_HOP_SAMPLES / 2), 2);
    }

    fn read_f32le(path: &str) -> Vec<f32> {
        let bytes = std::fs::read(path).unwrap();
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn matches_onnxruntime_on_a_real_window() {
        let Ok(dir) = std::env::var("UTA_STUDIO_TEST_BASIC_PITCH_PROBE_DIR") else {
            eprintln!("skipping: set UTA_STUDIO_TEST_BASIC_PITCH_PROBE_DIR to run this check");
            return;
        };
        let Ok(gguf) = std::env::var("UTA_STUDIO_TEST_BASIC_PITCH_GGUF") else {
            eprintln!("skipping: set UTA_STUDIO_TEST_BASIC_PITCH_GGUF to run this check");
            return;
        };
        let weights = Weights::load(std::path::Path::new(&gguf)).unwrap();
        let window = read_f32le(&format!("{dir}/window0.f32le"));
        assert_eq!(window.len(), INPUT_SAMPLES);
        let expected_notes = read_f32le(&format!("{dir}/onnx_notes.f32le"));
        let expected_onsets = read_f32le(&format!("{dir}/onnx_onsets.f32le"));
        let expected_contours = read_f32le(&format!("{dir}/onnx_contours.f32le"));

        let activations = run_window(&window, &weights).unwrap();

        let max_diff = |a: &[f32], b: &[f32]| -> f32 {
            a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
        };
        eprintln!(
            "note max_diff={} onset max_diff={} contour max_diff={}",
            max_diff(&activations.notes, &expected_notes),
            max_diff(&activations.onsets, &expected_onsets),
            max_diff(&activations.contours, &expected_contours),
        );
        assert!(max_diff(&activations.notes, &expected_notes) < 1e-3, "note mismatch");
        assert!(max_diff(&activations.onsets, &expected_onsets) < 1e-3, "onset mismatch");
        assert!(
            max_diff(&activations.contours, &expected_contours) < 1e-3,
            "contour mismatch"
        );
    }

    #[test]
    fn cqt_pure_tone_peaks_at_the_expected_bin() {
        let weights_path = std::env::var("UTA_STUDIO_TEST_BASIC_PITCH_GGUF");
        let Ok(path) = weights_path else {
            eprintln!("skipping: set UTA_STUDIO_TEST_BASIC_PITCH_GGUF to run this check");
            return;
        };
        let weights = Weights::load(std::path::Path::new(&path)).unwrap();
        // A4 = 440 Hz is exactly 4 octaves above ANNOTATIONS_BASE_FREQUENCY
        // (27.5 Hz = A0), so the expected bin is an exact integer:
        // round(36 * log2(440/27.5)) = 36 * 4 = 144, no rounding ambiguity.
        let freq = 440.0_f64;
        let mut window = vec![0.0_f32; INPUT_SAMPLES];
        for (i, sample) in window.iter_mut().enumerate() {
            *sample = (2.0 * std::f64::consts::PI * freq * i as f64 / SAMPLE_RATE as f64).sin() as f32;
        }
        let (magnitude, n_frames) = cqt_magnitude(&window, &weights);
        // Use a frame comfortably inside the window, away from edge effects.
        let frame = n_frames / 2;
        let row = &magnitude[frame * CQT_N_BINS..(frame + 1) * CQT_N_BINS];
        let (peak_bin, peak_value) = row
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        eprintln!("440Hz peak bin = {peak_bin} (expected 144), value = {peak_value}");
        assert!(
            (peak_bin as isize - 144).abs() <= 1,
            "peak bin {peak_bin} is not near the expected bin 144"
        );
    }

    #[test]
    fn cqt_top_octave_frame_count_matches_reference() {
        let weights_path = std::env::var("UTA_STUDIO_TEST_BASIC_PITCH_GGUF");
        let Ok(path) = weights_path else {
            eprintln!("skipping: set UTA_STUDIO_TEST_BASIC_PITCH_GGUF to run this check");
            return;
        };
        let weights = Weights::load(std::path::Path::new(&path)).unwrap();
        let window = vec![0.0_f32; INPUT_SAMPLES];
        let (_, n_frames) = cqt_magnitude(&window, &weights);
        assert_eq!(n_frames, FRAMES_PER_WINDOW);
    }
}
