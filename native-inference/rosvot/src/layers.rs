//! Shared building blocks for the STARS native engine, ported term-for-term
//! from the pinned reference (`gwx314/STARS@f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167`):
//! `modules/commons/{layers.py,conv.py,transformer.py,conformer/*}` and
//! `modules/stars/{stars.py,unet.py,utils.py}`. Every function here operates
//! on a single (batch=1) sequence, row-major `[T, C]` (time-major, channel
//! fastest) -- the PyTorch reference keeps the analogous math in `[B,T,C]`
//! (or transposes to `[B,C,T]` around `nn.Conv1d` calls, which this module
//! absorbs internally so callers never see a channel-first layout).

use rayon::prelude::*;

pub const HIDDEN: usize = 256;

// ---------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------

pub struct LinearWeights {
    pub weight: Vec<f32>, // [out, in], PyTorch nn.Linear layout
    pub bias: Option<Vec<f32>>,
    pub out_dim: usize,
    pub in_dim: usize,
}

/// `y = x @ W^T [+ b]`. `x` is `[rows, in_dim]` row-major.
pub fn linear(x: &[f32], rows: usize, w: &LinearWeights) -> Vec<f32> {
    debug_assert_eq!(x.len(), rows * w.in_dim);
    let mut out = vec![0.0_f32; rows * w.out_dim];
    unsafe {
        gemm::gemm(
            rows,
            w.out_dim,
            w.in_dim,
            out.as_mut_ptr(),
            1,
            w.out_dim as isize,
            false,
            x.as_ptr(),
            1,
            w.in_dim as isize,
            w.weight.as_ptr(),
            w.in_dim as isize,
            1,
            0.0,
            1.0,
            false,
            false,
            false,
            gemm::Parallelism::Rayon(0),
        );
    }
    if let Some(bias) = &w.bias {
        out.par_chunks_mut(w.out_dim).for_each(|row| {
            for (v, b) in row.iter_mut().zip(bias) {
                *v += b;
            }
        });
    }
    out
}

pub struct LayerNormWeights {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

/// In-place LayerNorm over the channel axis of each `[T,C]` row.
pub fn layer_norm(x: &mut [f32], t: usize, c: usize, w: &LayerNormWeights, eps: f32) {
    debug_assert_eq!(x.len(), t * c);
    x.par_chunks_mut(c).for_each(|row| {
        let mean = row.iter().sum::<f32>() / c as f32;
        let var = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / c as f32;
        let inv_std = 1.0 / (var + eps).sqrt();
        for (v, (weight, bias)) in row.iter_mut().zip(w.weight.iter().zip(&w.bias)) {
            *v = (*v - mean) * inv_std * weight + bias;
        }
    });
}

pub fn layer_norm_copy(x: &[f32], t: usize, c: usize, w: &LayerNormWeights, eps: f32) -> Vec<f32> {
    let mut out = x.to_vec();
    layer_norm(&mut out, t, c, w, eps);
    out
}

/// General 1D "same"-padded convolution, `[T,Cin] -> [T,Cout]`.
/// `weight` is native PyTorch `nn.Conv1d` layout `[Cout, Cin/groups, K]`.
pub fn conv1d_same(
    x: &[f32],
    t: usize,
    cin: usize,
    weight: &[f32],
    bias: Option<&[f32]>,
    cout: usize,
    k: usize,
    groups: usize,
    dilation: usize,
) -> Vec<f32> {
    debug_assert_eq!(x.len(), t * cin);
    let cin_per_group = cin / groups;
    let cout_per_group = cout / groups;
    let pad = (dilation * (k - 1)) / 2;
    let mut out = vec![0.0_f32; t * cout];
    out.par_chunks_mut(cout).enumerate().for_each(|(time, row)| {
        for oc in 0..cout {
            let group = oc / cout_per_group;
            let mut acc = bias.map_or(0.0, |b| b[oc]);
            for ic_local in 0..cin_per_group {
                let ic = group * cin_per_group + ic_local;
                for kk in 0..k {
                    let offset = kk as isize * dilation as isize - pad as isize;
                    let src_t = time as isize + offset;
                    if src_t < 0 || src_t as usize >= t {
                        continue;
                    }
                    let w = weight[(oc * cin_per_group + ic_local) * k + kk];
                    acc += w * x[src_t as usize * cin + ic];
                }
            }
            row[oc] = acc;
        }
    });
    out
}

pub fn leaky_relu(x: &mut [f32]) {
    for v in x {
        if *v < 0.0 {
            *v *= 0.01;
        }
    }
}

pub fn swish(x: &mut [f32]) {
    for v in x {
        *v *= 1.0 / (1.0 + (-*v).exp());
    }
}

pub fn relu(x: &mut [f32]) {
    for v in x {
        *v = v.max(0.0);
    }
}

pub fn sigmoid_scalar(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}

/// Per-timestep zero/nonzero mask (a timestep is "nonpadding" when at least
/// one channel is nonzero), matching the reference's ubiquitous
/// `x.abs().sum(-1) > 0` check.
pub fn nonpadding_from_zero_rows(x: &[f32], t: usize, c: usize) -> Vec<bool> {
    (0..t)
        .map(|time| x[time * c..(time + 1) * c].iter().any(|v| *v != 0.0))
        .collect()
}

pub fn apply_nonpadding(x: &mut [f32], t: usize, c: usize, nonpadding: &[bool]) {
    for time in 0..t {
        if !nonpadding[time] {
            x[time * c..(time + 1) * c].fill(0.0);
        }
    }
}

// ---------------------------------------------------------------------
// ResidualBlock / ConvBlocks (modules/commons/conv.py)
// ---------------------------------------------------------------------

pub struct ResidualSubBlock {
    pub norm: LayerNormWeights,
    pub expand_weight: Vec<f32>, // [c_multiple*channels, channels, kernel]
    pub expand_bias: Vec<f32>,
    pub project_weight: Vec<f32>, // [channels, c_multiple*channels, 1]
    pub project_bias: Vec<f32>,
}

pub struct ResidualBlockWeights {
    pub blocks: Vec<ResidualSubBlock>,
    pub kernel_size: usize,
    pub dilation: usize,
    pub channels: usize,
    pub c_multiple: usize,
    pub act_swish: bool, // false = leakyrelu
}

/// Matches `ResidualBlock.forward(x)`: the reference takes no external
/// padding mask at all -- it always self-derives `nonpadding =
/// (x.abs().sum(1) > 0)` fresh from its own current input.
pub fn residual_block(x: &[f32], t: usize, w: &ResidualBlockWeights) -> Vec<f32> {
    let nonpadding = nonpadding_from_zero_rows(x, t, w.channels);
    let mut current = x.to_vec();
    for sub in &w.blocks {
        let normed = layer_norm_copy(&current, t, w.channels, &sub.norm, 1.0e-5);
        let expanded_dim = w.channels * w.c_multiple;
        let mut expanded = conv1d_same(
            &normed,
            t,
            w.channels,
            &sub.expand_weight,
            Some(&sub.expand_bias),
            expanded_dim,
            w.kernel_size,
            1,
            w.dilation,
        );
        let scale = (w.kernel_size as f32).powf(-0.5);
        for v in &mut expanded {
            *v *= scale;
        }
        if w.act_swish {
            swish(&mut expanded);
        } else {
            leaky_relu(&mut expanded);
        }
        let projected = conv1d_same(
            &expanded,
            t,
            expanded_dim,
            &sub.project_weight,
            Some(&sub.project_bias),
            w.channels,
            1,
            1,
            w.dilation,
        );
        for i in 0..current.len() {
            current[i] += projected[i];
        }
        apply_nonpadding(&mut current, t, w.channels, &nonpadding);
    }
    current
}

pub struct ConvBlocksWeights {
    pub res_blocks: Vec<ResidualBlockWeights>, // one per "layer" (dilation slot)
    pub last_norm: LayerNormWeights,
    pub post_net_weight: Vec<f32>, // [out_dims, channels, post_net_kernel]
    pub post_net_bias: Vec<f32>,
    pub channels: usize,
    pub out_dims: usize,
    pub post_net_kernel: usize,
}

/// Matches `ConvBlocks.forward(x, nonpadding=None)` as called everywhere in
/// this codebase (always with `nonpadding=None`): a padding mask is derived
/// once from the raw input `x` and applied after `res_blocks`, `last_norm`,
/// and `post_net1` (each internal `ResidualBlock` also self-derives its own
/// mask from whatever it receives -- with `num_layers=1` everywhere in this
/// model there is exactly one such block, whose input is this same raw `x`,
/// so both derivations agree).
pub fn conv_blocks(x: &[f32], t: usize, w: &ConvBlocksWeights) -> Vec<f32> {
    let nonpadding = nonpadding_from_zero_rows(x, t, w.channels);
    let mut current = x.to_vec();
    for block in &w.res_blocks {
        current = residual_block(&current, t, block);
    }
    apply_nonpadding(&mut current, t, w.channels, &nonpadding);
    layer_norm(&mut current, t, w.channels, &w.last_norm, 1.0e-5);
    apply_nonpadding(&mut current, t, w.channels, &nonpadding);
    let mut out = conv1d_same(
        &current,
        t,
        w.channels,
        &w.post_net_weight,
        Some(&w.post_net_bias),
        w.out_dims,
        w.post_net_kernel,
        1,
        1,
    );
    apply_nonpadding(&mut out, t, w.out_dims, &nonpadding);
    out
}

// ---------------------------------------------------------------------
// Sinusoidal absolute positional embedding (modules/commons/transformer.py)
// ---------------------------------------------------------------------

/// `SinusoidalPositionalEmbedding` + `make_positions`, specialized to a
/// contiguous nonpadding prefix `[0, valid)` (always true for both STARS's
/// own zero-padded frame buckets and its segment-pooled sequences), which
/// makes `make_positions` reduce to `t+1` for `t < valid`, else `0`
/// (`padding_idx=0`).
pub fn sinusoidal_position_embedding(valid: usize, total: usize) -> Vec<f32> {
    let half_dim = HIDDEN / 2;
    let emb_scale = 10_000.0_f32.ln() / (half_dim as f32 - 1.0);
    let freqs = (0..half_dim)
        .map(|i| (-(i as f32) * emb_scale).exp())
        .collect::<Vec<_>>();
    let mut out = vec![0.0_f32; total * HIDDEN];
    for t in 0..valid {
        let position = (t + 1) as f32;
        for (i, freq) in freqs.iter().enumerate() {
            let angle = position * freq;
            out[t * HIDDEN + i] = angle.sin();
            out[t * HIDDEN + half_dim + i] = angle.cos();
        }
    }
    out
}

// ---------------------------------------------------------------------
// Relative positional encoding + attention (ESPnet-style Conformer)
// ---------------------------------------------------------------------

/// `PositionalEncoding.extend_pe`'s lazily-built table is created exactly
/// once, at `__init__` time, for a fixed `max_len=5000` (`RelPositionalEncoding`'s
/// default) -- every later `forward(x)` call finds `self.pe.size(1) >= x.size(1)`
/// and returns immediately without recomputing, then slices `self.pe[:, :T]`.
/// With `reverse=True`, that table's row `r` encodes position `(MAX_LEN-1-r)`,
/// so a forward call at length `T < MAX_LEN` sees positions counting down from
/// `MAX_LEN-1`, *not* from `T-1` -- confirmed empirically against a genuine
/// PyTorch reference forward pass (a `T`-scaled reversed range was tried
/// first and diverged by ~2.0 in `pos_emb`, i.e. essentially uncorrelated).
const REL_POS_MAX_LEN: usize = 5_000;

/// `RelPositionalEncoding` (`reverse=True`): scales `x` by `sqrt(d_model)`
/// and returns a same-length `pos_emb[row]` table whose row `r` encodes
/// position `(REL_POS_MAX_LEN-1-r)` -- see `REL_POS_MAX_LEN`'s doc comment.
pub fn rel_positional_encoding(x: &[f32], t: usize) -> (Vec<f32>, Vec<f32>) {
    let xscale = (HIDDEN as f32).sqrt();
    let scaled = x.iter().map(|v| v * xscale).collect::<Vec<_>>();
    let half_dim = HIDDEN / 2;
    let mut pe = vec![0.0_f32; t * HIDDEN];
    for row in 0..t {
        let position = (REL_POS_MAX_LEN - 1 - row) as f32;
        for i in 0..half_dim {
            let div = (-((2 * i) as f32) * 10_000.0_f32.ln() / HIDDEN as f32).exp();
            let angle = position * div;
            pe[row * HIDDEN + 2 * i] = angle.sin();
            pe[row * HIDDEN + 2 * i + 1] = angle.cos();
        }
    }
    (scaled, pe)
}

/// ESPnet `RelPositionMultiHeadedAttention.rel_shift`, mirrored via the
/// literal pad -> reshape -> drop-first-row -> reshape sequence (not a
/// derived closed form) to avoid off-by-one risk. Operates on one head's
/// `[T1,T2]` score matrix.
fn rel_shift(x: &[f32], t1: usize, t2: usize) -> Vec<f32> {
    let mut padded = vec![0.0_f32; t1 * (t2 + 1)];
    for row in 0..t1 {
        padded[row * (t2 + 1)] = 0.0;
        padded[row * (t2 + 1) + 1..row * (t2 + 1) + 1 + t2].copy_from_slice(&x[row * t2..(row + 1) * t2]);
    }
    // Reinterpret the same flat buffer as [(t2+1), t1] row-major, drop the
    // first row, then reinterpret the remainder as [t1, t2] row-major.
    let dropped = &padded[t1..];
    debug_assert_eq!(dropped.len(), t1 * t2);
    dropped.to_vec()
}

pub struct RelPosMhsaWeights {
    pub w_q: LinearWeights,
    pub w_k: LinearWeights,
    pub w_v: LinearWeights,
    pub w_out: LinearWeights,
    pub linear_pos: LinearWeights, // bias = None
    pub pos_bias_u: Vec<f32>, // [heads, d_k]
    pub pos_bias_v: Vec<f32>, // [heads, d_k]
    pub heads: usize,
}

pub fn rel_position_mhsa(
    x: &[f32],
    pos_emb: &[f32],
    t: usize,
    nonpadding: &[bool],
    w: &RelPosMhsaWeights,
) -> Vec<f32> {
    let d_k = HIDDEN / w.heads;
    let q = linear(x, t, &w.w_q);
    let k = linear(x, t, &w.w_k);
    let v = linear(x, t, &w.w_v);
    let p = linear(pos_emb, t, &w.linear_pos);
    let scale = 1.0 / (d_k as f32).sqrt();

    let mut context = vec![0.0_f32; t * HIDDEN];
    for h in 0..w.heads {
        let mut q_u = vec![0.0_f32; t * d_k];
        let mut q_v = vec![0.0_f32; t * d_k];
        for time in 0..t {
            for d in 0..d_k {
                let base = q[time * HIDDEN + h * d_k + d];
                q_u[time * d_k + d] = base + w.pos_bias_u[h * d_k + d];
                q_v[time * d_k + d] = base + w.pos_bias_v[h * d_k + d];
            }
        }
        let mut matrix_ac = vec![0.0_f32; t * t];
        let mut matrix_bd_raw = vec![0.0_f32; t * t];
        for t1 in 0..t {
            for t2 in 0..t {
                let mut ac = 0.0_f32;
                let mut bd = 0.0_f32;
                for d in 0..d_k {
                    ac += q_u[t1 * d_k + d] * k[t2 * HIDDEN + h * d_k + d];
                    bd += q_v[t1 * d_k + d] * p[t2 * HIDDEN + h * d_k + d];
                }
                matrix_ac[t1 * t + t2] = ac;
                matrix_bd_raw[t1 * t + t2] = bd;
            }
        }
        let matrix_bd = rel_shift(&matrix_bd_raw, t, t);

        for t1 in 0..t {
            let row_ac = &matrix_ac[t1 * t..(t1 + 1) * t];
            let row_bd = &matrix_bd[t1 * t..(t1 + 1) * t];
            let mut scores = vec![f32::NEG_INFINITY; t];
            let mut max_score = f32::NEG_INFINITY;
            for t2 in 0..t {
                if !nonpadding[t2] {
                    continue;
                }
                let s = (row_ac[t2] + row_bd[t2]) * scale;
                scores[t2] = s;
                if s > max_score {
                    max_score = s;
                }
            }
            let mut weights = vec![0.0_f32; t];
            let mut sum = 0.0_f32;
            for t2 in 0..t {
                if nonpadding[t2] {
                    let e = (scores[t2] - max_score).exp();
                    weights[t2] = e;
                    sum += e;
                }
            }
            if sum > 0.0 {
                for w_ in &mut weights {
                    *w_ /= sum;
                }
            }
            for d in 0..d_k {
                let mut acc = 0.0_f32;
                for t2 in 0..t {
                    if weights[t2] != 0.0 {
                        acc += weights[t2] * v[t2 * HIDDEN + h * d_k + d];
                    }
                }
                context[t1 * HIDDEN + h * d_k + d] = acc;
            }
        }
    }
    linear(&context, t, &w.w_out)
}

// ---------------------------------------------------------------------
// FeedForwardMOE (4-way channel-blocked MultiLayeredConv1d, kernel=1)
// ---------------------------------------------------------------------

pub struct FeedForwardMoeWeights {
    pub experts: Vec<(LinearWeights, LinearWeights)>, // (w_1, w_2) per expert, kernel=1 so plain Linear
}

pub fn feed_forward_moe(x: &[f32], t: usize, w: &FeedForwardMoeWeights) -> Vec<f32> {
    let num_experts = w.experts.len();
    let chunk = HIDDEN / num_experts;
    let mut out = vec![0.0_f32; t * HIDDEN];
    for (expert_index, (w1, w2)) in w.experts.iter().enumerate() {
        let mut chunk_in = vec![0.0_f32; t * chunk];
        for time in 0..t {
            chunk_in[time * chunk..(time + 1) * chunk]
                .copy_from_slice(&x[time * HIDDEN + expert_index * chunk..time * HIDDEN + (expert_index + 1) * chunk]);
        }
        let mut hidden = linear(&chunk_in, t, w1);
        relu(&mut hidden);
        let projected = linear(&hidden, t, w2);
        for time in 0..t {
            out[time * HIDDEN + expert_index * chunk..time * HIDDEN + (expert_index + 1) * chunk]
                .copy_from_slice(&projected[time * chunk..(time + 1) * chunk]);
        }
    }
    out
}

// ---------------------------------------------------------------------
// ConvolutionModule (Conformer conv sandwich, real BatchNorm1d, Swish)
// ---------------------------------------------------------------------

pub struct BatchNorm1dWeights {
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
    pub running_mean: Vec<f32>,
    pub running_var: Vec<f32>,
}

fn batch_norm_1d(x: &mut [f32], t: usize, c: usize, w: &BatchNorm1dWeights) {
    for time in 0..t {
        for ch in 0..c {
            let v = x[time * c + ch];
            let std_dev = (w.running_var[ch] + 1.0e-5).sqrt();
            x[time * c + ch] = (v - w.running_mean[ch]) / std_dev * w.weight[ch] + w.bias[ch];
        }
    }
}

pub struct ConvolutionModuleWeights {
    pub pointwise1_weight: Vec<f32>, // [2*C, C, 1]
    pub pointwise1_bias: Vec<f32>,
    pub depthwise_weight: Vec<f32>, // [C, 1, kernel]
    pub depthwise_bias: Vec<f32>,
    pub norm: BatchNorm1dWeights,
    pub pointwise2_weight: Vec<f32>, // [C, C, 1]
    pub pointwise2_bias: Vec<f32>,
    pub kernel_size: usize,
}

pub fn convolution_module(x: &[f32], t: usize, w: &ConvolutionModuleWeights) -> Vec<f32> {
    let expanded = conv1d_same(
        x,
        t,
        HIDDEN,
        &w.pointwise1_weight,
        Some(&w.pointwise1_bias),
        2 * HIDDEN,
        1,
        1,
        1,
    );
    // GLU along the channel axis: first half * sigmoid(second half).
    let mut glu = vec![0.0_f32; t * HIDDEN];
    for time in 0..t {
        for ch in 0..HIDDEN {
            let a = expanded[time * 2 * HIDDEN + ch];
            let b = expanded[time * 2 * HIDDEN + HIDDEN + ch];
            glu[time * HIDDEN + ch] = a * sigmoid_scalar(b);
        }
    }
    let mut depthwise = conv1d_same(
        &glu,
        t,
        HIDDEN,
        &w.depthwise_weight,
        Some(&w.depthwise_bias),
        HIDDEN,
        w.kernel_size,
        HIDDEN,
        1,
    );
    batch_norm_1d(&mut depthwise, t, HIDDEN, &w.norm);
    swish(&mut depthwise);
    conv1d_same(
        &depthwise,
        t,
        HIDDEN,
        &w.pointwise2_weight,
        Some(&w.pointwise2_bias),
        HIDDEN,
        1,
        1,
        1,
    )
}

// ---------------------------------------------------------------------
// Conformer EncoderLayer (pre-norm, macaron FFN, ff_scale=0.5)
// ---------------------------------------------------------------------

pub struct ConformerEncoderLayerWeights {
    pub self_attn: RelPosMhsaWeights,
    pub feed_forward: FeedForwardMoeWeights,
    pub feed_forward_macaron: FeedForwardMoeWeights,
    pub conv_module: ConvolutionModuleWeights,
    pub norm_ff: LayerNormWeights,
    pub norm_mha: LayerNormWeights,
    pub norm_ff_macaron: LayerNormWeights,
    pub norm_conv: LayerNormWeights,
    pub norm_final: LayerNormWeights,
}

fn add_scaled(base: &mut [f32], delta: &[f32], scale: f32) {
    for (b, d) in base.iter_mut().zip(delta) {
        *b += d * scale;
    }
}

pub fn conformer_encoder_layer(
    x: &[f32],
    pos_emb: &[f32],
    t: usize,
    nonpadding: &[bool],
    w: &ConformerEncoderLayerWeights,
) -> Vec<f32> {
    let mut x = x.to_vec();

    let normed = layer_norm_copy(&x, t, HIDDEN, &w.norm_ff_macaron, 1.0e-5);
    let ff_macaron = feed_forward_moe(&normed, t, &w.feed_forward_macaron);
    add_scaled(&mut x, &ff_macaron, 0.5);

    let normed = layer_norm_copy(&x, t, HIDDEN, &w.norm_mha, 1.0e-5);
    let attn = rel_position_mhsa(&normed, pos_emb, t, nonpadding, &w.self_attn);
    add_scaled(&mut x, &attn, 1.0);

    let normed = layer_norm_copy(&x, t, HIDDEN, &w.norm_conv, 1.0e-5);
    let conv_out = convolution_module(&normed, t, &w.conv_module);
    add_scaled(&mut x, &conv_out, 1.0);

    let normed = layer_norm_copy(&x, t, HIDDEN, &w.norm_ff, 1.0e-5);
    let ff = feed_forward_moe(&normed, t, &w.feed_forward);
    add_scaled(&mut x, &ff, 0.5);

    layer_norm(&mut x, t, HIDDEN, &w.norm_final, 1.0e-5);
    x
}

pub struct ConformerLayersMoeWeights {
    pub layers: Vec<ConformerEncoderLayerWeights>,
    pub final_layer_norm: LayerNormWeights, // plain nn.LayerNorm, eps=1e-5
}

/// `ConformerLayersMOE.forward(x, padding_mask=None)`: the reference always
/// self-derives `nonpadding_mask = x.abs().sum(-1) > 0` from its own raw
/// input (the `padding_mask` parameter is accepted but never read), then
/// threads that single mask into every inner encoder layer's attention.
pub fn conformer_layers_moe(x: &[f32], t: usize, w: &ConformerLayersMoeWeights) -> Vec<f32> {
    let nonpadding = nonpadding_from_zero_rows(x, t, HIDDEN);
    let (mut current, pos_emb) = rel_positional_encoding(x, t);
    for layer in &w.layers {
        current = conformer_encoder_layer(&current, &pos_emb, t, &nonpadding, layer);
    }
    layer_norm(&mut current, t, HIDDEN, &w.final_layer_norm, 1.0e-5);
    apply_nonpadding(&mut current, t, HIDDEN, &nonpadding);
    current
}

// ---------------------------------------------------------------------
// Unet (modules/stars/unet.py) -- constant HIDDEN channels throughout,
// since STARS pins channel_multiples="1-1-1-1".
// ---------------------------------------------------------------------

pub struct UnetDownStageWeights {
    pub block0: ResidualBlockWeights,
    pub mid_conv_weight: Vec<f32>, // [HIDDEN, HIDDEN, kernel]
    pub mid_conv_bias: Vec<f32>,
    pub block2: ResidualBlockWeights,
    pub kernel_size: usize,
}

pub struct UnetDownWeights {
    pub stages: Vec<UnetDownStageWeights>, // 4 stages, each downsamples by 2
    pub last_norm: LayerNormWeights,
    pub post_net_weight: Vec<f32>,
    pub post_net_bias: Vec<f32>,
    pub kernel_size: usize,
}

fn avg_pool_1d_by_2(x: &[f32], t: usize, c: usize) -> Vec<f32> {
    let out_t = t / 2;
    let mut out = vec![0.0_f32; out_t * c];
    for ot in 0..out_t {
        for ch in 0..c {
            out[ot * c + ch] = 0.5 * (x[(2 * ot) * c + ch] + x[(2 * ot + 1) * c + ch]);
        }
    }
    out
}

/// Returns `(bottleneck_input, skips)`, `skips[i]` being stage `i`'s
/// pre-pool output (post channel-mixing conv + second residual block). No
/// nonpadding mask is threaded in: `UnetDown.forward` has no explicit
/// masking of its own -- only the internal (self-deriving) `ResidualBlock`s
/// mask anything, and `last_norm`/`post_net` are applied unmasked.
pub fn unet_down(x: &[f32], t: usize, w: &UnetDownWeights) -> (Vec<f32>, Vec<(Vec<f32>, usize)>) {
    let mut current = x.to_vec();
    let mut current_t = t;
    let mut skips = Vec::with_capacity(w.stages.len());
    for stage in &w.stages {
        let after0 = residual_block(&current, current_t, &stage.block0);
        let mixed = conv1d_same(
            &after0,
            current_t,
            HIDDEN,
            &stage.mid_conv_weight,
            Some(&stage.mid_conv_bias),
            HIDDEN,
            stage.kernel_size,
            1,
            1,
        );
        let skip = residual_block(&mixed, current_t, &stage.block2);
        skips.push((skip.clone(), current_t));
        current = avg_pool_1d_by_2(&skip, current_t, HIDDEN);
        current_t /= 2;
    }
    layer_norm(&mut current, current_t, HIDDEN, &w.last_norm, 1.0e-5);
    let out = conv1d_same(
        &current,
        current_t,
        HIDDEN,
        &w.post_net_weight,
        Some(&w.post_net_bias),
        HIDDEN,
        w.kernel_size,
        1,
        1,
    );
    (out, skips)
}

pub struct UnetMidWeights {
    pub pre_weight: Vec<f32>,
    pub pre_bias: Vec<f32>,
    pub net: ConformerLayersMoeWeights,
    pub post_weight: Vec<f32>,
    pub post_bias: Vec<f32>,
    pub kernel_size: usize,
}

pub fn unet_mid(x: &[f32], t: usize, w: &UnetMidWeights) -> Vec<f32> {
    let pre = conv1d_same(x, t, HIDDEN, &w.pre_weight, Some(&w.pre_bias), HIDDEN, w.kernel_size, 1, 1);
    let net_out = conformer_layers_moe(&pre, t, &w.net);
    conv1d_same(&net_out, t, HIDDEN, &w.post_weight, Some(&w.post_bias), HIDDEN, w.kernel_size, 1, 1)
}

pub struct UnetUpStageWeights {
    pub up_transpose_weight: Vec<f32>, // native ConvTranspose1d [in=HIDDEN,out=HIDDEN,kernel]
    pub up_transpose_bias: Vec<f32>,
    pub up_norm: LayerNormWeights,
    pub merge_weight: Vec<f32>, // [HIDDEN, 2*HIDDEN, kernel]
    pub merge_bias: Vec<f32>,
    pub merge_block: ResidualBlockWeights,
    pub kernel_size: usize,
}

pub struct UnetUpWeights {
    pub stages: Vec<UnetUpStageWeights>,
    pub last_norm: LayerNormWeights,
    pub post_net_weight: Vec<f32>,
    pub post_net_bias: Vec<f32>,
    pub kernel_size: usize,
}

/// `ConvTranspose1d(kernel, stride=2, padding=1, output_padding=1)`, native
/// weight `[in_ch, out_ch, kernel]`. Output length is exactly `2*t`.
fn conv_transpose1d_upsample2x(x: &[f32], t: usize, c: usize, weight: &[f32], bias: &[f32], kernel: usize) -> Vec<f32> {
    let out_t = 2 * t;
    let mut out = vec![0.0_f32; out_t * c];
    for ot in 0..out_t {
        for oc in 0..c {
            out[ot * c + oc] = bias[oc];
        }
    }
    for it in 0..t {
        for k in 0..kernel {
            let ot = it as isize * 2 + k as isize - 1;
            if ot < 0 || ot as usize >= out_t {
                continue;
            }
            let ot = ot as usize;
            for ic in 0..c {
                let value = x[it * c + ic];
                if value == 0.0 {
                    continue;
                }
                for oc in 0..c {
                    let w = weight[(ic * c + oc) * kernel + k];
                    out[ot * c + oc] += value * w;
                }
            }
        }
    }
    out
}

pub fn unet_up(x: &[f32], t: usize, skips: &[(Vec<f32>, usize)], w: &UnetUpWeights) -> Vec<f32> {
    let mut current = x.to_vec();
    let mut current_t = t;
    for (index, stage) in w.stages.iter().enumerate() {
        let mut up = conv_transpose1d_upsample2x(
            &current,
            current_t,
            HIDDEN,
            &stage.up_transpose_weight,
            &stage.up_transpose_bias,
            stage.kernel_size,
        );
        current_t *= 2;
        layer_norm(&mut up, current_t, HIDDEN, &stage.up_norm, 1.0e-5);
        leaky_relu(&mut up);
        let (skip, skip_t) = &skips[skips.len() - 1 - index];
        debug_assert_eq!(*skip_t, current_t);
        let mut merged_input = vec![0.0_f32; current_t * 2 * HIDDEN];
        for time in 0..current_t {
            merged_input[time * 2 * HIDDEN..time * 2 * HIDDEN + HIDDEN]
                .copy_from_slice(&up[time * HIDDEN..(time + 1) * HIDDEN]);
            merged_input[time * 2 * HIDDEN + HIDDEN..time * 2 * HIDDEN + 2 * HIDDEN]
                .copy_from_slice(&skip[time * HIDDEN..(time + 1) * HIDDEN]);
        }
        let merged = conv1d_same(
            &merged_input,
            current_t,
            2 * HIDDEN,
            &stage.merge_weight,
            Some(&stage.merge_bias),
            HIDDEN,
            stage.kernel_size,
            1,
            1,
        );
        current = residual_block(&merged, current_t, &stage.merge_block);
    }
    layer_norm(&mut current, current_t, HIDDEN, &w.last_norm, 1.0e-5);
    conv1d_same(
        &current,
        current_t,
        HIDDEN,
        &w.post_net_weight,
        Some(&w.post_net_bias),
        HIDDEN,
        w.kernel_size,
        1,
        1,
    )
}

pub struct UnetWeights {
    pub down: UnetDownWeights,
    pub mid: UnetMidWeights,
    pub up: UnetUpWeights,
}

pub fn unet_forward(x: &[f32], t: usize, w: &UnetWeights) -> Vec<f32> {
    let (bottleneck_in, skips) = unet_down(x, t, &w.down);
    let bottleneck_t = t / 2_usize.pow(w.down.stages.len() as u32);
    let bottleneck_out = unet_mid(&bottleneck_in, bottleneck_t, &w.mid);
    unet_up(&bottleneck_out, bottleneck_t, &skips, &w.up)
}

// ---------------------------------------------------------------------
// VQEmbeddingEMA (encode-only: nearest-neighbour codebook lookup)
// ---------------------------------------------------------------------

pub fn vq_encode(x: &[f32], rows: usize, codebook: &[f32], num_codes: usize) -> Vec<f32> {
    let dim = HIDDEN;
    let mut out = vec![0.0_f32; rows * dim];
    for row in 0..rows {
        let v = &x[row * dim..(row + 1) * dim];
        let mut best = 0usize;
        let mut best_dist = f32::INFINITY;
        for code in 0..num_codes {
            let c = &codebook[code * dim..(code + 1) * dim];
            let dist = v.iter().zip(c).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
            if dist < best_dist {
                best_dist = dist;
                best = code;
            }
        }
        out[row * dim..(row + 1) * dim].copy_from_slice(&codebook[best * dim..(best + 1) * dim]);
    }
    out
}

// ---------------------------------------------------------------------
// LocalStyleAdaptor (CMUEncoder = Unet, then a small ConvBlocks encoder,
// optional VQ)
// ---------------------------------------------------------------------

pub struct LocalStyleAdaptorWeights {
    pub cmuencoder: UnetWeights,
    pub encoder: ConvBlocksWeights,
    pub vq_codebook: Vec<f32>, // [num_codes, HIDDEN]
    pub num_codes: usize,
}

/// `ref_mels: [T,HIDDEN]`. `group: Some((seg_ids, num_segments))` mirrors
/// `group_hidden_by_segs` (1-indexed `seg_ids`, `0` = unassigned/padding).
/// Returns `(prosody, rows)`, `rows = T` when `group` is `None`, else
/// `num_segments`.
pub fn local_style_adaptor(
    ref_mels: &[f32],
    t: usize,
    group: Option<(&[i64], usize)>,
    no_vq: bool,
    w: &LocalStyleAdaptorWeights,
) -> (Vec<f32>, usize) {
    let cmu_out = unet_forward(ref_mels, t, &w.cmuencoder);
    let (ref_ph, rows);
    match group {
        Some((seg_ids, num_segments)) => {
            let mut summed = vec![0.0_f32; num_segments * HIDDEN];
            let mut counts = vec![0.0_f32; num_segments];
            for time in 0..t {
                let seg = seg_ids[time];
                if seg <= 0 {
                    continue;
                }
                let index = (seg - 1) as usize;
                if index >= num_segments {
                    continue;
                }
                counts[index] += 1.0;
                for ch in 0..HIDDEN {
                    summed[index * HIDDEN + ch] += cmu_out[time * HIDDEN + ch];
                }
            }
            for index in 0..num_segments {
                let denom = counts[index].max(1.0);
                for ch in 0..HIDDEN {
                    summed[index * HIDDEN + ch] /= denom;
                }
            }
            ref_ph = summed;
            rows = num_segments;
        }
        None => {
            ref_ph = cmu_out;
            rows = t;
        }
    }
    let prosody = conv_blocks(&ref_ph, rows, &w.encoder);
    if no_vq {
        (prosody, rows)
    } else {
        (vq_encode(&prosody, rows, &w.vq_codebook, w.num_codes), rows)
    }
}

// ---------------------------------------------------------------------
// expand_states: gather segment-pooled features back to per-frame
// resolution via a 1-indexed (0=padding) segment map.
// ---------------------------------------------------------------------

pub fn expand_states(pooled: &[f32], rows: usize, seg_ids: &[i64], t: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; t * HIDDEN];
    for time in 0..t {
        let seg = seg_ids[time];
        if seg <= 0 {
            continue;
        }
        let index = (seg - 1) as usize;
        if index >= rows {
            continue;
        }
        out[time * HIDDEN..(time + 1) * HIDDEN].copy_from_slice(&pooled[index * HIDDEN..(index + 1) * HIDDEN]);
    }
    out
}

// ---------------------------------------------------------------------
// CrossAttenLayer / ProsodyAligner (standard nn.MultiheadAttention + FFN)
// ---------------------------------------------------------------------

pub struct CrossAttnLayerWeights {
    pub in_proj_weight: Vec<f32>, // [3*HIDDEN, HIDDEN] (q;k;v stacked)
    pub in_proj_bias: Vec<f32>,   // [3*HIDDEN]
    pub out_proj: LinearWeights,
    pub linear1: LinearWeights, // HIDDEN -> ff_dim
    pub linear2: LinearWeights, // ff_dim -> HIDDEN
    pub norm1: LayerNormWeights,
    pub norm2: LayerNormWeights,
    pub heads: usize,
}

/// `nn.MultiheadAttention(src, local_emotion, local_emotion)` (query != kv),
/// standard (non-relative) scaled dot-product attention, no causal mask;
/// `emotion_key_padding_mask` marks padded key positions.
fn cross_multihead_attention(
    src: &[f32],
    tsrc: usize,
    kv: &[f32],
    tkv: usize,
    kv_nonpadding: &[bool],
    w: &CrossAttnLayerWeights,
) -> Vec<f32> {
    let d_k = HIDDEN / w.heads;
    let q_w = LinearWeights {
        weight: w.in_proj_weight[0..HIDDEN * HIDDEN].to_vec(),
        bias: Some(w.in_proj_bias[0..HIDDEN].to_vec()),
        out_dim: HIDDEN,
        in_dim: HIDDEN,
    };
    let k_w = LinearWeights {
        weight: w.in_proj_weight[HIDDEN * HIDDEN..2 * HIDDEN * HIDDEN].to_vec(),
        bias: Some(w.in_proj_bias[HIDDEN..2 * HIDDEN].to_vec()),
        out_dim: HIDDEN,
        in_dim: HIDDEN,
    };
    let v_w = LinearWeights {
        weight: w.in_proj_weight[2 * HIDDEN * HIDDEN..3 * HIDDEN * HIDDEN].to_vec(),
        bias: Some(w.in_proj_bias[2 * HIDDEN..3 * HIDDEN].to_vec()),
        out_dim: HIDDEN,
        in_dim: HIDDEN,
    };
    let q = linear(src, tsrc, &q_w);
    let k = linear(kv, tkv, &k_w);
    let v = linear(kv, tkv, &v_w);
    let scale = 1.0 / (d_k as f32).sqrt();

    let mut context = vec![0.0_f32; tsrc * HIDDEN];
    for h in 0..w.heads {
        for t1 in 0..tsrc {
            let mut scores = vec![f32::NEG_INFINITY; tkv];
            let mut max_score = f32::NEG_INFINITY;
            for t2 in 0..tkv {
                if !kv_nonpadding[t2] {
                    continue;
                }
                let mut s = 0.0_f32;
                for d in 0..d_k {
                    s += q[t1 * HIDDEN + h * d_k + d] * k[t2 * HIDDEN + h * d_k + d];
                }
                s *= scale;
                scores[t2] = s;
                if s > max_score {
                    max_score = s;
                }
            }
            let mut weights = vec![0.0_f32; tkv];
            let mut sum = 0.0_f32;
            for t2 in 0..tkv {
                if kv_nonpadding[t2] {
                    let e = (scores[t2] - max_score).exp();
                    weights[t2] = e;
                    sum += e;
                }
            }
            if sum > 0.0 {
                for w_ in &mut weights {
                    *w_ /= sum;
                }
            }
            for d in 0..d_k {
                let mut acc = 0.0_f32;
                for t2 in 0..tkv {
                    if weights[t2] != 0.0 {
                        acc += weights[t2] * v[t2 * HIDDEN + h * d_k + d];
                    }
                }
                context[t1 * HIDDEN + h * d_k + d] = acc;
            }
        }
    }
    linear(&context, tsrc, &w.out_proj)
}

/// One `CrossAttenLayer`: cross-attention + residual + LayerNorm, then a
/// 2-layer ReLU FFN + residual + LayerNorm (standard post-norm Transformer
/// block, matching the reference's `nn.MultiheadAttention`-based module).
pub fn cross_attn_layer(
    src: &[f32],
    tsrc: usize,
    kv: &[f32],
    tkv: usize,
    kv_nonpadding: &[bool],
    w: &CrossAttnLayerWeights,
) -> Vec<f32> {
    let attn = cross_multihead_attention(src, tsrc, kv, tkv, kv_nonpadding, w);
    let mut x = src.to_vec();
    for i in 0..x.len() {
        x[i] += attn[i];
    }
    layer_norm(&mut x, tsrc, HIDDEN, &w.norm1, 1.0e-5);
    let mut ff = linear(&x, tsrc, &w.linear1);
    relu(&mut ff);
    let ff = linear(&ff, tsrc, &w.linear2);
    for i in 0..x.len() {
        x[i] += ff[i];
    }
    layer_norm(&mut x, tsrc, HIDDEN, &w.norm2, 1.0e-5);
    x
}

pub struct ProsodyAlignerWeights {
    pub layers: Vec<CrossAttnLayerWeights>,
}

pub fn prosody_aligner(
    src: &[f32],
    tsrc: usize,
    kv: &[f32],
    tkv: usize,
    kv_nonpadding: &[bool],
    w: &ProsodyAlignerWeights,
) -> Vec<f32> {
    let mut current = src.to_vec();
    for layer in &w.layers {
        current = cross_attn_layer(&current, tsrc, kv, tkv, kv_nonpadding, layer);
    }
    current
}
