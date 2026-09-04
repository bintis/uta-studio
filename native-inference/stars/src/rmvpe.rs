//! Native CPU port of RMVPE (E2E0 DeepUnet + bidirectional GRU pitch
//! estimator), ported term-for-term from the validated GGML/Vulkan graph at
//! `native-inference/rmvpe/src/graph.cpp` (every tensor name, kernel size,
//! and op ordering below mirrors that file's comments, which were themselves
//! confirmed node-by-node against the pinned `rmvpe.onnx` graph). This runs
//! the *same* pinned RMVPE checkpoint (loaded from the already-installed
//! `rmvpe-f32.gguf`, architecture `rmvpe`) that the standalone `rmvpe` model
//! uses, just single-shot over a fixed `T=256` window instead of chunked --
//! chunking in `graph.cpp` exists only to keep the unrolled GGML graph within
//! a safe node-count ceiling; a plain Rust loop has no such ceiling, so this
//! always uses the simpler one-shot path (`graph.cpp`'s `Build()`, not its
//! three-stage chunked split).
//!
//! Conv2d/BatchNorm2d/ConvTranspose2d weights are read as raw row-major
//! bytes in their native PyTorch/ONNX shapes: `(out_ch, in_ch, kH, kW)` for
//! Conv2d, `(in_ch, out_ch, kH, kW)` for ConvTranspose2d (PyTorch's actual,
//! non-obvious convention for that op), `(C,)` for BatchNorm affine/running
//! stats. Channel counts are never hardcoded from a table: each layer's
//! `out_ch` is derived from its own weight tensor's byte length divided by
//! `in_ch * kH * kW`, where `in_ch` is simply the previous layer's `out_ch`
//! -- exactly mirroring how `graph.cpp` never hardcodes the
//! 16/32/64/128/256 channel progression either (it reads shapes off the
//! loaded `ggml_tensor`s).

use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;

pub const PITCH_CLASSES: usize = 360;
const GRU_HIDDEN: usize = 256;
const GRU_INPUT: usize = 384;
const ENCODER_STAGES: usize = 5;
const BOTTLENECK_STAGES: usize = 4;
const DECODER_STAGES: usize = 5;
const BLOCKS_PER_STAGE: usize = 4;

/// A 2D feature map, `[channels, height, width]` row-major (width fastest).
#[derive(Clone)]
struct Feature {
    data: Vec<f32>,
    c: usize,
    h: usize,
    w: usize,
}

impl Feature {
    fn zeros(c: usize, h: usize, w: usize) -> Self {
        Self {
            data: vec![0.0; c * h * w],
            c,
            h,
            w,
        }
    }

    fn channel(&self, ch: usize) -> &[f32] {
        &self.data[ch * self.h * self.w..(ch + 1) * self.h * self.w]
    }

    fn channel_mut(&mut self, ch: usize) -> &mut [f32] {
        let stride = self.h * self.w;
        &mut self.data[ch * stride..(ch + 1) * stride]
    }
}

struct Conv2dWeights {
    weight: Vec<f32>, // native (out_ch, in_ch, kh, kw) row-major
    bias: Option<Vec<f32>>,
    out_ch: usize,
    in_ch: usize,
    kh: usize,
    kw: usize,
}

struct BatchNorm2dWeights {
    weight: Vec<f32>,
    bias: Vec<f32>,
    running_mean: Vec<f32>,
    running_var: Vec<f32>,
}

struct ResBlockWeights {
    conv0: Conv2dWeights,
    conv3: Conv2dWeights,
    shortcut: Option<Conv2dWeights>,
}

struct DecoderStageWeights {
    up_weight: Vec<f32>, // native ConvTranspose2d (in_ch, out_ch, 3, 3)
    up_in_ch: usize,
    up_out_ch: usize,
    up_bn: BatchNorm2dWeights,
    blocks: Vec<ResBlockWeights>,
}

struct GruWeights {
    weight_ih: Vec<f32>, // [2, 3*hidden, input]
    weight_hh: Vec<f32>, // [2, 3*hidden, hidden]
    bias: Vec<f32>,      // [2, 6*hidden]
}

pub struct RmvpeWeights {
    encoder_bn: BatchNorm2dWeights,
    encoder_stages: Vec<Vec<ResBlockWeights>>,
    bottleneck_blocks: Vec<ResBlockWeights>,
    decoder_stages: Vec<DecoderStageWeights>,
    cnn_head: Conv2dWeights,
    gru: GruWeights,
    fc_weight: Vec<f32>, // [360, 512]
    fc_bias: Vec<f32>,   // [360]
}

fn take(file: &GGUFFile, name: &str) -> Result<Vec<f32>> {
    file.tensor_data_f32_owned(name)
}

fn take_optional(file: &GGUFFile, name: &str) -> Option<Vec<f32>> {
    file.tensor_data_f32_owned(name).ok()
}

fn take_conv2d(
    file: &GGUFFile,
    prefix: &str,
    in_ch: usize,
    kh: usize,
    kw: usize,
) -> Result<Conv2dWeights> {
    let weight = take(file, &format!("{prefix}.weight"))?;
    let per_out = in_ch * kh * kw;
    if per_out == 0 || !weight.len().is_multiple_of(per_out) {
        return Err(Error::message(format!(
            "{prefix}.weight has {} elements, not a multiple of in_ch*kh*kw={per_out}",
            weight.len()
        )));
    }
    let out_ch = weight.len() / per_out;
    let bias = take_optional(file, &format!("{prefix}.bias"));
    Ok(Conv2dWeights {
        weight,
        bias,
        out_ch,
        in_ch,
        kh,
        kw,
    })
}

fn take_batch_norm2d(file: &GGUFFile, prefix: &str) -> Result<BatchNorm2dWeights> {
    Ok(BatchNorm2dWeights {
        weight: take(file, &format!("{prefix}.weight"))?,
        bias: take(file, &format!("{prefix}.bias"))?,
        running_mean: take(file, &format!("{prefix}.running_mean"))?,
        running_var: take(file, &format!("{prefix}.running_var"))?,
    })
}

fn take_res_block(file: &GGUFFile, prefix: &str, in_ch: usize) -> Result<ResBlockWeights> {
    let conv0 = take_conv2d(file, &format!("{prefix}.conv.conv.0"), in_ch, 3, 3)?;
    let conv3 = take_conv2d(file, &format!("{prefix}.conv.conv.3"), conv0.out_ch, 3, 3)?;
    let shortcut = if take_optional(file, &format!("{prefix}.shortcut.weight")).is_some() {
        Some(take_conv2d(
            file,
            &format!("{prefix}.shortcut"),
            in_ch,
            1,
            1,
        )?)
    } else {
        None
    };
    Ok(ResBlockWeights {
        conv0,
        conv3,
        shortcut,
    })
}

impl RmvpeWeights {
    pub fn load(path: &Path) -> Result<Self> {
        let file = GGUFFile::open(path)?;
        if file.architecture() != "rmvpe" {
            return Err(Error::UnsupportedArchitecture {
                found: file.architecture().to_string(),
            });
        }
        let encoder_bn = take_batch_norm2d(&file, "unet.encoder.bn")?;

        let mut encoder_stages = Vec::with_capacity(ENCODER_STAGES);
        let mut channels = 1usize; // single-channel mel "image"
        for stage in 0..ENCODER_STAGES {
            let mut blocks = Vec::with_capacity(BLOCKS_PER_STAGE);
            for block in 0..BLOCKS_PER_STAGE {
                let prefix = format!("unet.encoder.layers.{stage}.conv.{block}");
                let res = take_res_block(&file, &prefix, channels)?;
                channels = res.conv3.out_ch;
                blocks.push(res);
            }
            encoder_stages.push(blocks);
        }

        let mut bottleneck_blocks = Vec::with_capacity(BOTTLENECK_STAGES * BLOCKS_PER_STAGE);
        for stage in 0..BOTTLENECK_STAGES {
            for block in 0..BLOCKS_PER_STAGE {
                let prefix = format!("unet.intermediate.layers.{stage}.conv.{block}");
                let res = take_res_block(&file, &prefix, channels)?;
                channels = res.conv3.out_ch;
                bottleneck_blocks.push(res);
            }
        }

        let mut decoder_stages = Vec::with_capacity(DECODER_STAGES);
        for stage in 0..DECODER_STAGES {
            let up_prefix = format!("unet.decoder.layers.{stage}.conv1.conv1.0");
            let up_weight = take(&file, &format!("{up_prefix}.weight"))?;
            // ConvTranspose2d native shape (in_ch, out_ch, 3, 3); in_ch is
            // the running channel count coming into this decoder stage.
            let up_in_ch = channels;
            if up_in_ch == 0 || !up_weight.len().is_multiple_of(up_in_ch * 9) {
                return Err(Error::message(format!(
                    "{up_prefix}.weight has {} elements, not a multiple of in_ch*9={}",
                    up_weight.len(),
                    up_in_ch * 9
                )));
            }
            let up_out_ch = up_weight.len() / (up_in_ch * 9);
            let up_bn =
                take_batch_norm2d(&file, &format!("unet.decoder.layers.{stage}.conv1.conv1.1"))?;
            // Skip connection concatenation doubles the channel count before
            // the stage's own ResConvBlocks.
            let mut block_in = up_out_ch * 2;
            let mut blocks = Vec::with_capacity(BLOCKS_PER_STAGE);
            for block in 0..BLOCKS_PER_STAGE {
                let prefix = format!("unet.decoder.layers.{stage}.conv2.{block}");
                let res = take_res_block(&file, &prefix, block_in)?;
                block_in = res.conv3.out_ch;
                blocks.push(res);
            }
            channels = block_in;
            decoder_stages.push(DecoderStageWeights {
                up_weight,
                up_in_ch,
                up_out_ch,
                up_bn,
                blocks,
            });
        }

        let cnn_head = take_conv2d(&file, "cnn", channels, 3, 3)?;
        if cnn_head.out_ch != 3 {
            return Err(Error::message(format!(
                "RMVPE cnn head has {} output channels, expected 3",
                cnn_head.out_ch
            )));
        }

        let gru = GruWeights {
            weight_ih: take(&file, "gru.weight_ih")?,
            weight_hh: take(&file, "gru.weight_hh")?,
            bias: take(&file, "gru.bias")?,
        };
        if gru.weight_ih.len() != 2 * 3 * GRU_HIDDEN * GRU_INPUT {
            return Err(Error::message("RMVPE gru.weight_ih has an unexpected size"));
        }
        if gru.weight_hh.len() != 2 * 3 * GRU_HIDDEN * GRU_HIDDEN {
            return Err(Error::message("RMVPE gru.weight_hh has an unexpected size"));
        }
        if gru.bias.len() != 2 * 6 * GRU_HIDDEN {
            return Err(Error::message("RMVPE gru.bias has an unexpected size"));
        }

        let fc_weight = take(&file, "fc.1.weight")?;
        let fc_bias = take(&file, "fc.1.bias")?;
        if fc_weight.len() != PITCH_CLASSES * 2 * GRU_HIDDEN || fc_bias.len() != PITCH_CLASSES {
            return Err(Error::message("RMVPE fc.1 has an unexpected size"));
        }

        Ok(Self {
            encoder_bn,
            encoder_stages,
            bottleneck_blocks,
            decoder_stages,
            cnn_head,
            gru,
            fc_weight,
            fc_bias,
        })
    }
}

fn conv2d_same(x: &Feature, w: &Conv2dWeights) -> Feature {
    debug_assert_eq!(x.c, w.in_ch);
    let (h, wd) = (x.h, x.w);
    let pad_h = w.kh / 2;
    let pad_w = w.kw / 2;
    let mut out = Feature::zeros(w.out_ch, h, wd);
    out.data
        .par_chunks_mut(h * wd)
        .enumerate()
        .for_each(|(oc, out_channel)| {
            let bias = w.bias.as_ref().map_or(0.0, |b| b[oc]);
            for value in out_channel.iter_mut() {
                *value = bias;
            }
            for ic in 0..w.in_ch {
                let input_channel = x.channel(ic);
                for ky in 0..w.kh {
                    for kx in 0..w.kw {
                        let weight = w.weight[((oc * w.in_ch + ic) * w.kh + ky) * w.kw + kx];
                        if weight == 0.0 {
                            continue;
                        }
                        let dy = ky as isize - pad_h as isize;
                        let dx = kx as isize - pad_w as isize;
                        for oy in 0..h {
                            let iy = oy as isize + dy;
                            if iy < 0 || iy as usize >= h {
                                continue;
                            }
                            let iy = iy as usize;
                            let in_row = &input_channel[iy * wd..(iy + 1) * wd];
                            let out_row = &mut out_channel[oy * wd..(oy + 1) * wd];
                            // ix = ox + dx must land in [0, wd): ox >= -dx
                            // and ox < wd - dx.
                            let ox_start = if dx < 0 { (-dx) as usize } else { 0 };
                            let ox_end = if dx < 0 {
                                wd
                            } else {
                                wd.saturating_sub(dx as usize)
                            };
                            for ox in ox_start..ox_end {
                                let ix = (ox as isize + dx) as usize;
                                out_row[ox] += weight * in_row[ix];
                            }
                        }
                    }
                }
            }
        });
    out
}

fn batch_norm2d(x: &mut Feature, bn: &BatchNorm2dWeights) {
    let stride = x.h * x.w;
    for c in 0..x.c {
        let scale = bn.weight[c];
        let shift = bn.bias[c];
        let mean = bn.running_mean[c];
        let std_dev = (bn.running_var[c] + 1.0e-5).sqrt();
        for value in &mut x.data[c * stride..(c + 1) * stride] {
            *value = (*value - mean) / std_dev * scale + shift;
        }
    }
}

fn relu_inplace(x: &mut Feature) {
    for value in &mut x.data {
        *value = value.max(0.0);
    }
}

fn res_conv_block(x: &Feature, weights: &ResBlockWeights) -> Feature {
    let mut main = conv2d_same(x, &weights.conv0);
    relu_inplace(&mut main);
    let mut main = conv2d_same(&main, &weights.conv3);
    relu_inplace(&mut main);
    let residual = match &weights.shortcut {
        Some(shortcut) => conv2d_same(x, shortcut),
        None => x.clone(),
    };
    debug_assert_eq!(main.data.len(), residual.data.len());
    for (m, r) in main.data.iter_mut().zip(&residual.data) {
        *m += r;
    }
    main
}

fn avg_pool_2x2(x: &Feature) -> Feature {
    let (h, w) = (x.h / 2, x.w / 2);
    let mut out = Feature::zeros(x.c, h, w);
    for c in 0..x.c {
        let input = x.channel(c);
        let output = out.channel_mut(c);
        for oy in 0..h {
            for ox in 0..w {
                let sum = input[(2 * oy) * x.w + 2 * ox]
                    + input[(2 * oy) * x.w + 2 * ox + 1]
                    + input[(2 * oy + 1) * x.w + 2 * ox]
                    + input[(2 * oy + 1) * x.w + 2 * ox + 1];
                output[oy * w + ox] = sum * 0.25;
            }
        }
    }
    out
}

/// `ConvTranspose2d(kernel=3, stride=2, padding=1, output_padding=1)`,
/// implemented as the full unpadded (`(2*in+1)`-sized) transpose convolution
/// followed by dropping row/col index 0 -- the exact crop recipe documented
/// in `graph.cpp::ConvTransposeUpsample`, producing a clean `2x` spatial
/// upsample (`2*in_h x 2*in_w`).
fn conv_transpose2d_upsample(x: &Feature, weight: &[f32], in_ch: usize, out_ch: usize) -> Feature {
    debug_assert_eq!(x.c, in_ch);
    let full_h = 2 * x.h + 1;
    let full_w = 2 * x.w + 1;
    let mut full = vec![0.0_f32; out_ch * full_h * full_w];
    // weight native shape (in_ch, out_ch, 3, 3).
    for ic in 0..in_ch {
        let input_channel = x.channel(ic);
        for iy in 0..x.h {
            for ix in 0..x.w {
                let value = input_channel[iy * x.w + ix];
                if value == 0.0 {
                    continue;
                }
                for ky in 0..3 {
                    let oy = iy * 2 + ky;
                    for kx in 0..3 {
                        let ox = ix * 2 + kx;
                        for oc in 0..out_ch {
                            let w = weight[((ic * out_ch + oc) * 3 + ky) * 3 + kx];
                            full[(oc * full_h + oy) * full_w + ox] += value * w;
                        }
                    }
                }
            }
        }
    }
    let out_h = 2 * x.h;
    let out_w = 2 * x.w;
    let mut out = Feature::zeros(out_ch, out_h, out_w);
    for oc in 0..out_ch {
        for oy in 0..out_h {
            let src = &full[(oc * full_h + (oy + 1)) * full_w + 1
                ..(oc * full_h + (oy + 1)) * full_w + 1 + out_w];
            out.channel_mut(oc)[oy * out_w..(oy + 1) * out_w].copy_from_slice(src);
        }
    }
    out
}

fn concat_channels(a: &Feature, b: &Feature) -> Feature {
    debug_assert_eq!((a.h, a.w), (b.h, b.w));
    let mut out = Feature::zeros(a.c + b.c, a.h, a.w);
    out.data[..a.data.len()].copy_from_slice(&a.data);
    out.data[a.data.len()..].copy_from_slice(&b.data);
    out
}

fn gru_gate_view(
    weight: &[f32],
    direction: usize,
    gate: usize,
    rows: usize,
    cols: usize,
) -> &[f32] {
    let stage = 3 * rows * cols;
    let base = direction * stage + gate * rows * cols;
    &weight[base..base + rows * cols]
}

fn gru_bias_view(bias: &[f32], direction: usize, part: usize, hidden: usize) -> &[f32] {
    let base = direction * 6 * hidden + part * hidden;
    &bias[base..base + hidden]
}

fn mat_vec(matrix: &[f32], rows: usize, cols: usize, vector: &[f32], out: &mut [f32]) {
    debug_assert_eq!(vector.len(), cols);
    debug_assert_eq!(out.len(), rows);
    for (row_index, row) in matrix.chunks_exact(cols).enumerate().take(rows) {
        out[row_index] = row.iter().zip(vector).map(|(w, v)| w * v).sum();
    }
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn gru_direction(
    gru: &GruWeights,
    x: &[f32], // [T, GRU_INPUT] row-major
    t: usize,
    direction: usize,
) -> Vec<f32> {
    let h = GRU_HIDDEN;
    let wz = gru_gate_view(&gru.weight_ih, direction, 0, h, GRU_INPUT);
    let wr = gru_gate_view(&gru.weight_ih, direction, 1, h, GRU_INPUT);
    let wh = gru_gate_view(&gru.weight_ih, direction, 2, h, GRU_INPUT);
    let rz = gru_gate_view(&gru.weight_hh, direction, 0, h, h);
    let rr = gru_gate_view(&gru.weight_hh, direction, 1, h, h);
    let rh = gru_gate_view(&gru.weight_hh, direction, 2, h, h);
    let wbz = gru_bias_view(&gru.bias, direction, 0, h);
    let wbr = gru_bias_view(&gru.bias, direction, 1, h);
    let wbh = gru_bias_view(&gru.bias, direction, 2, h);
    let rbz = gru_bias_view(&gru.bias, direction, 3, h);
    let rbr = gru_bias_view(&gru.bias, direction, 4, h);
    let rbh = gru_bias_view(&gru.bias, direction, 5, h);

    let mut out = vec![0.0_f32; t * h];
    let mut h_prev = vec![0.0_f32; h];
    let mut wz_x = vec![0.0_f32; h];
    let mut wr_x = vec![0.0_f32; h];
    let mut wh_x = vec![0.0_f32; h];
    let mut rz_h = vec![0.0_f32; h];
    let mut rr_h = vec![0.0_f32; h];
    let mut rh_h = vec![0.0_f32; h];

    let order: Box<dyn Iterator<Item = usize>> = if direction == 0 {
        Box::new(0..t)
    } else {
        Box::new((0..t).rev())
    };
    for time in order {
        let x_t = &x[time * GRU_INPUT..(time + 1) * GRU_INPUT];
        mat_vec(wz, h, GRU_INPUT, x_t, &mut wz_x);
        mat_vec(wr, h, GRU_INPUT, x_t, &mut wr_x);
        mat_vec(wh, h, GRU_INPUT, x_t, &mut wh_x);
        mat_vec(rz, h, h, &h_prev, &mut rz_h);
        mat_vec(rr, h, h, &h_prev, &mut rr_h);
        mat_vec(rh, h, h, &h_prev, &mut rh_h);
        let mut h_t = vec![0.0_f32; h];
        for i in 0..h {
            let z = sigmoid(wz_x[i] + rz_h[i] + wbz[i] + rbz[i]);
            let r = sigmoid(wr_x[i] + rr_h[i] + wbr[i] + rbr[i]);
            let h_tilde = (wh_x[i] + r * (rh_h[i] + rbh[i]) + wbh[i]).tanh();
            h_t[i] = h_tilde + z * (h_prev[i] - h_tilde);
        }
        out[time * h..(time + 1) * h].copy_from_slice(&h_t);
        h_prev.copy_from_slice(&h_t);
    }
    out
}

/// Runs the full RMVPE forward pass on a fixed `[T,128]` frame-major log-mel
/// window, returning `[T,360]` sigmoid pitch-class salience, row-major.
pub fn forward(weights: &RmvpeWeights, mel_channel_major: &[f32], t: usize) -> Vec<f32> {
    debug_assert_eq!(mel_channel_major.len(), crate::mel16::MEL_BINS * t);
    // mel_channel_major is [128 mel-bins, T] (mel-major, T fastest) --
    // transpose to our own [1, T, 128] (time-major, mel fastest) CHW image.
    let mut image = Feature::zeros(1, t, crate::mel16::MEL_BINS);
    for mel in 0..crate::mel16::MEL_BINS {
        for time in 0..t {
            image.data[time * crate::mel16::MEL_BINS + mel] = mel_channel_major[mel * t + time];
        }
    }
    batch_norm2d(&mut image, &weights.encoder_bn);

    let mut x = image;
    let mut skips: Vec<Feature> = Vec::with_capacity(ENCODER_STAGES);
    for stage_blocks in &weights.encoder_stages {
        for block in stage_blocks {
            x = res_conv_block(&x, block);
        }
        skips.push(x.clone());
        x = avg_pool_2x2(&x);
    }

    for block in &weights.bottleneck_blocks {
        x = res_conv_block(&x, block);
    }

    for (stage, decoder) in weights.decoder_stages.iter().enumerate() {
        let mut up =
            conv_transpose2d_upsample(&x, &decoder.up_weight, decoder.up_in_ch, decoder.up_out_ch);
        batch_norm2d(&mut up, &decoder.up_bn);
        relu_inplace(&mut up);
        let skip = &skips[ENCODER_STAGES - 1 - stage];
        x = concat_channels(&up, skip);
        for block in &decoder.blocks {
            x = res_conv_block(&x, block);
        }
    }

    let head = conv2d_same(&x, &weights.cnn_head);
    debug_assert_eq!(head.c, 3);
    // Rearranges [3, T, 128] -> [T, 384] (channel,mel folded together per
    // timestep, matching graph.cpp's Transpose(perm=[0,2,1,3])+reshape).
    let mut gru_input = vec![0.0_f32; t * GRU_INPUT];
    for time in 0..t {
        for channel in 0..3 {
            let row = &head.channel(channel)
                [time * crate::mel16::MEL_BINS..(time + 1) * crate::mel16::MEL_BINS];
            gru_input[time * GRU_INPUT + channel * crate::mel16::MEL_BINS
                ..time * GRU_INPUT + (channel + 1) * crate::mel16::MEL_BINS]
                .copy_from_slice(row);
        }
    }

    let fwd = gru_direction(&weights.gru, &gru_input, t, 0);
    let bwd = gru_direction(&weights.gru, &gru_input, t, 1);
    let mut gru_out = vec![0.0_f32; t * 2 * GRU_HIDDEN];
    for time in 0..t {
        gru_out[time * 2 * GRU_HIDDEN..time * 2 * GRU_HIDDEN + GRU_HIDDEN]
            .copy_from_slice(&fwd[time * GRU_HIDDEN..(time + 1) * GRU_HIDDEN]);
        gru_out[time * 2 * GRU_HIDDEN + GRU_HIDDEN..time * 2 * GRU_HIDDEN + 2 * GRU_HIDDEN]
            .copy_from_slice(&bwd[time * GRU_HIDDEN..(time + 1) * GRU_HIDDEN]);
    }

    let mut output = vec![0.0_f32; t * PITCH_CLASSES];
    output
        .par_chunks_mut(PITCH_CLASSES)
        .enumerate()
        .for_each(|(time, row)| {
            let x_t = &gru_out[time * 2 * GRU_HIDDEN..(time + 1) * 2 * GRU_HIDDEN];
            for (cls, value) in row.iter_mut().enumerate() {
                let logit: f32 = weights.fc_weight
                    [cls * 2 * GRU_HIDDEN..(cls + 1) * 2 * GRU_HIDDEN]
                    .iter()
                    .zip(x_t)
                    .map(|(w, v)| w * v)
                    .sum::<f32>()
                    + weights.fc_bias[cls];
                *value = sigmoid(logit);
            }
        });
    output
}
