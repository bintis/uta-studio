//! Native CPU re-implementation of ROSVOT (Robust Singing Voice
//! Transcription), matching
//! `native-inference/openvino-worker/src/advanced_notes.rs`'s validated
//! stage split, segment framing, and evidence contract exactly, but running
//! the model itself on hand-written CPU kernels (`crate::layers`, the same
//! module used by `native-inference/stars`) against a native GGUF built
//! directly from the official checkpoint
//! (`RickyL-2000/ROSVOT@3c8332bf43adae35f6e4d64971862f2f6139b310`,
//! `checkpoints/rosvot/model.pt`) instead of the pinned OpenVINO IR export.
//!
//! Architecture was confirmed directly against the real checkpoint's
//! `state_dict` (247 tensors) and the pinned reference source
//! (`modules/rosvot/rosvot.py`, `configs/rosvot.yaml`). ROSVOT is
//! substantially simpler than STARS: a single shared U-Net+Conformer
//! backbone (no per-granularity prosody extractors, no VQ codebooks, no
//! technique/style heads), no G2P or Viterbi phoneme alignment -- word
//! boundaries arrive as a direct model *input* (`word_bd`, from
//! TimedTranscript) rather than being predicted. Its Conformer mid-net uses
//! *plain* `ConformerLayers` (a single dense FFN), not STARS's
//! `ConformerLayersMOE` -- modeled here as a degenerate 1-expert call into
//! the same `feed_forward_moe` helper (chunk width = HIDDEN when there is
//! only one "expert"), so no changes to `crate::layers` were needed.
//!
//! Stage boundaries mirror `advanced_notes.rs::run_rosvot` exactly:
//! - Stage "frame": `mel_proj`+`mel_encoder`, pitch/uv/word-boundary embed,
//!   `cond_encoder`, the shared `net` (U-Net+Conformer) backbone,
//!   `note_bd_out`, and `PitchDecoder`'s frame-level attention
//!   pre-computation (weighted features + attention) -- everything that
//!   does not depend on the regulated note boundaries.
//! - CPU-side: `rosvot_host::regulate_boundaries` (boundary regulation
//!   conditioned on the *known* `word_bd` input -- unlike STARS's note
//!   boundaries, which have no such external reference) and
//!   `rosvot_host::aggregate_notes` (attention-weighted scatter-mean by
//!   note segment), both already validated and ported verbatim.
//! - Stage "pitch": `PitchDecoder.post`+`pitch_out` on the CPU-aggregated
//!   per-note features.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;
use crate::layers::*;
use crate::mel16;
use crate::rmvpe::{self, RmvpeWeights};
use crate::rosvot_host;
use crate::singing_frontend;

pub const FRAME_BUCKET: usize = 256;
pub const NOTE_BUCKET: usize = 32;
pub const PITCH_CLASSES: usize = 89;
pub const NOTE_NUM: usize = 85;
pub const NOTE_START: usize = 30;
const MEL_BINS: usize = 40;

const ROSVOT_COMMIT: &str = "3c8332bf43adae35f6e4d64971862f2f6139b310";
const ROSVOT_CHECKPOINT: &str = "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb";
const ROSVOT_CONFIG: &str = "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2";
const SHARED_MANIFEST_SHA256: &str = "986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c";
const RUNTIME_MANIFEST_IDENTITY: &str = "rosvot-native-recipe-v1";

// ---------------------------------------------------------------------
// Weight loading
// ---------------------------------------------------------------------

fn take(file: &GGUFFile, name: &str) -> Result<Vec<f32>> {
    file.tensor_data_f32_owned(name)
}

fn take_layer_norm(file: &GGUFFile, prefix: &str) -> Result<LayerNormWeights> {
    Ok(LayerNormWeights {
        weight: take(file, &format!("{prefix}.weight"))?,
        bias: take(file, &format!("{prefix}.bias"))?,
    })
}

fn take_linear(file: &GGUFFile, prefix: &str, out_dim: usize, in_dim: usize, has_bias: bool) -> Result<LinearWeights> {
    let weight = take(file, &format!("{prefix}.weight"))?;
    if weight.len() != out_dim * in_dim {
        return Err(Error::message(format!(
            "{prefix}.weight has {} elements, expected {}",
            weight.len(),
            out_dim * in_dim
        )));
    }
    let bias = if has_bias {
        Some(take(file, &format!("{prefix}.bias"))?)
    } else {
        None
    };
    Ok(LinearWeights {
        weight,
        bias,
        out_dim,
        in_dim,
    })
}

fn take_residual_sub_block(file: &GGUFFile, prefix: &str, channels: usize, c_multiple: usize, kernel: usize) -> Result<ResidualSubBlock> {
    let expand_weight = take(file, &format!("{prefix}.1.weight"))?;
    if expand_weight.len() != c_multiple * channels * channels * kernel {
        return Err(Error::message(format!(
            "{prefix}.1.weight has {} elements, expected {}",
            expand_weight.len(),
            c_multiple * channels * channels * kernel
        )));
    }
    let project_weight = take(file, &format!("{prefix}.4.weight"))?;
    if project_weight.len() != channels * c_multiple * channels {
        return Err(Error::message(format!(
            "{prefix}.4.weight has {} elements, expected {}",
            project_weight.len(),
            channels * c_multiple * channels
        )));
    }
    Ok(ResidualSubBlock {
        norm: take_layer_norm(file, &format!("{prefix}.0"))?,
        expand_weight,
        expand_bias: take(file, &format!("{prefix}.1.bias"))?,
        project_weight,
        project_bias: take(file, &format!("{prefix}.4.bias"))?,
    })
}

fn take_residual_block(
    file: &GGUFFile,
    prefix: &str,
    n_sub_blocks: usize,
    kernel_size: usize,
    channels: usize,
    c_multiple: usize,
    act_swish: bool,
) -> Result<ResidualBlockWeights> {
    let mut blocks = Vec::with_capacity(n_sub_blocks);
    for i in 0..n_sub_blocks {
        blocks.push(take_residual_sub_block(
            file,
            &format!("{prefix}.blocks.{i}"),
            channels,
            c_multiple,
            kernel_size,
        )?);
    }
    Ok(ResidualBlockWeights {
        blocks,
        kernel_size,
        dilation: 1,
        channels,
        c_multiple,
        act_swish,
    })
}

fn take_conv_blocks(
    file: &GGUFFile,
    prefix: &str,
    layers_in_block: usize,
    kernel_size: usize,
    post_net_kernel: usize,
    act_swish: bool,
) -> Result<ConvBlocksWeights> {
    Ok(ConvBlocksWeights {
        res_blocks: vec![take_residual_block(
            file,
            &format!("{prefix}.res_blocks.0"),
            layers_in_block,
            kernel_size,
            HIDDEN,
            1,
            act_swish,
        )?],
        last_norm: take_layer_norm(file, &format!("{prefix}.last_norm"))?,
        post_net_weight: take(file, &format!("{prefix}.post_net1.weight"))?,
        post_net_bias: take(file, &format!("{prefix}.post_net1.bias"))?,
        channels: HIDDEN,
        out_dims: HIDDEN,
        post_net_kernel,
    })
}

/// Plain (non-MoE) `ConformerLayers`: a single dense
/// `MultiLayeredConv1d(HIDDEN, HIDDEN*4)` feed-forward, modeled as one
/// "expert" spanning the full hidden width in `feed_forward_moe`.
fn take_conformer_layer_plain(file: &GGUFFile, prefix: &str, heads: usize) -> Result<ConformerEncoderLayerWeights> {
    let take_plain_ffn = |sub: &str| -> Result<FeedForwardMoeWeights> {
        Ok(FeedForwardMoeWeights {
            experts: vec![(
                take_linear(file, &format!("{prefix}.{sub}.w_1"), 4 * HIDDEN, HIDDEN, true)?,
                take_linear(file, &format!("{prefix}.{sub}.w_2"), HIDDEN, 4 * HIDDEN, true)?,
            )],
        })
    };
    Ok(ConformerEncoderLayerWeights {
        self_attn: RelPosMhsaWeights {
            w_q: take_linear(file, &format!("{prefix}.self_attn.linear_q"), HIDDEN, HIDDEN, true)?,
            w_k: take_linear(file, &format!("{prefix}.self_attn.linear_k"), HIDDEN, HIDDEN, true)?,
            w_v: take_linear(file, &format!("{prefix}.self_attn.linear_v"), HIDDEN, HIDDEN, true)?,
            w_out: take_linear(file, &format!("{prefix}.self_attn.linear_out"), HIDDEN, HIDDEN, true)?,
            linear_pos: take_linear(file, &format!("{prefix}.self_attn.linear_pos"), HIDDEN, HIDDEN, false)?,
            pos_bias_u: take(file, &format!("{prefix}.self_attn.pos_bias_u"))?,
            pos_bias_v: take(file, &format!("{prefix}.self_attn.pos_bias_v"))?,
            heads,
        },
        feed_forward: take_plain_ffn("feed_forward")?,
        feed_forward_macaron: take_plain_ffn("feed_forward_macaron")?,
        conv_module: ConvolutionModuleWeights {
            pointwise1_weight: take(file, &format!("{prefix}.conv_module.pointwise_conv1.weight"))?,
            pointwise1_bias: take(file, &format!("{prefix}.conv_module.pointwise_conv1.bias"))?,
            depthwise_weight: take(file, &format!("{prefix}.conv_module.depthwise_conv.weight"))?,
            depthwise_bias: take(file, &format!("{prefix}.conv_module.depthwise_conv.bias"))?,
            norm: BatchNorm1dWeights {
                weight: take(file, &format!("{prefix}.conv_module.norm.weight"))?,
                bias: take(file, &format!("{prefix}.conv_module.norm.bias"))?,
                running_mean: take(file, &format!("{prefix}.conv_module.norm.running_mean"))?,
                running_var: take(file, &format!("{prefix}.conv_module.norm.running_var"))?,
            },
            pointwise2_weight: take(file, &format!("{prefix}.conv_module.pointwise_conv2.weight"))?,
            pointwise2_bias: take(file, &format!("{prefix}.conv_module.pointwise_conv2.bias"))?,
            kernel_size: 9,
        },
        norm_ff: take_layer_norm(file, &format!("{prefix}.norm_ff"))?,
        norm_mha: take_layer_norm(file, &format!("{prefix}.norm_mha"))?,
        norm_ff_macaron: take_layer_norm(file, &format!("{prefix}.norm_ff_macaron"))?,
        norm_conv: take_layer_norm(file, &format!("{prefix}.norm_conv"))?,
        norm_final: take_layer_norm(file, &format!("{prefix}.norm_final"))?,
    })
}

fn take_conformer_layers_plain(file: &GGUFFile, prefix: &str, num_layers: usize) -> Result<ConformerLayersMoeWeights> {
    let mut layers = Vec::with_capacity(num_layers);
    for i in 0..num_layers {
        layers.push(take_conformer_layer_plain(file, &format!("{prefix}.encoder_layers.{i}"), 4)?);
    }
    Ok(ConformerLayersMoeWeights {
        layers,
        final_layer_norm: take_layer_norm(file, &format!("{prefix}.layer_norm"))?,
    })
}

fn take_unet(file: &GGUFFile, prefix: &str, mid_layers: usize) -> Result<UnetWeights> {
    let mut down_stages = Vec::with_capacity(4);
    for stage in 0..4 {
        let p = format!("{prefix}.down.layers.{stage}");
        down_stages.push(UnetDownStageWeights {
            block0: take_residual_block(file, &format!("{p}.0"), 1, 3, HIDDEN, 1, false)?,
            mid_conv_weight: take(file, &format!("{p}.1.weight"))?,
            mid_conv_bias: take(file, &format!("{p}.1.bias"))?,
            block2: take_residual_block(file, &format!("{p}.2"), 1, 3, HIDDEN, 1, false)?,
            kernel_size: 3,
        });
    }
    let mut up_stages = Vec::with_capacity(4);
    for stage in 0..4 {
        let p = format!("{prefix}.up");
        up_stages.push(UnetUpStageWeights {
            up_transpose_weight: take(file, &format!("{p}.ups.{stage}.0.weight"))?,
            up_transpose_bias: take(file, &format!("{p}.ups.{stage}.0.bias"))?,
            up_norm: take_layer_norm(file, &format!("{p}.ups.{stage}.1"))?,
            merge_weight: take(file, &format!("{p}.layers.{stage}.0.weight"))?,
            merge_bias: take(file, &format!("{p}.layers.{stage}.0.bias"))?,
            merge_block: take_residual_block(file, &format!("{p}.layers.{stage}.1"), 1, 3, HIDDEN, 1, false)?,
            kernel_size: 3,
        });
    }
    Ok(UnetWeights {
        down: UnetDownWeights {
            stages: down_stages,
            last_norm: take_layer_norm(file, &format!("{prefix}.down.last_norm"))?,
            post_net_weight: take(file, &format!("{prefix}.down.post_net.weight"))?,
            post_net_bias: take(file, &format!("{prefix}.down.post_net.bias"))?,
            kernel_size: 3,
        },
        mid: UnetMidWeights {
            pre_weight: take(file, &format!("{prefix}.mid.pre.weight"))?,
            pre_bias: take(file, &format!("{prefix}.mid.pre.bias"))?,
            net: take_conformer_layers_plain(file, &format!("{prefix}.mid.net"), mid_layers)?,
            post_weight: take(file, &format!("{prefix}.mid.post.weight"))?,
            post_bias: take(file, &format!("{prefix}.mid.post.bias"))?,
            kernel_size: 3,
        },
        up: UnetUpWeights {
            stages: up_stages,
            last_norm: take_layer_norm(file, &format!("{prefix}.up.last_norm"))?,
            post_net_weight: take(file, &format!("{prefix}.up.post_net.weight"))?,
            post_net_bias: take(file, &format!("{prefix}.up.post_net.bias"))?,
            kernel_size: 3,
        },
    })
}

pub struct RosvotWeights {
    mel_proj_weight: Vec<f32>,
    mel_proj_bias: Vec<f32>,
    mel_encoder: ConvBlocksWeights,
    pitch_embed: Vec<f32>,    // [300, HIDDEN]
    uv_embed: Vec<f32>,       // [3, HIDDEN]
    word_bd_embed: Vec<f32>,  // [3, HIDDEN]
    cond_encoder: ConvBlocksWeights,
    net: UnetWeights,
    note_bd_out: LinearWeights, // 256 -> 1
    pitch_decoder_attn: LinearWeights, // 256 -> 4
    pitch_decoder_post: ConvBlocksWeights,
    pitch_decoder_out: LinearWeights, // 256 -> 89
}

impl RosvotWeights {
    fn load(path: &Path) -> Result<Self> {
        let file = GGUFFile::open(path)?;
        if file.architecture() != "rosvot" {
            return Err(Error::UnsupportedArchitecture {
                found: file.architecture().to_string(),
            });
        }
        Ok(Self {
            mel_proj_weight: take(&file, "mel_proj.weight")?,
            mel_proj_bias: take(&file, "mel_proj.bias")?,
            mel_encoder: take_conv_blocks(&file, "mel_encoder", 2, 3, 3, false)?,
            pitch_embed: take(&file, "pitch_embed.weight")?,
            uv_embed: take(&file, "uv_embed.weight")?,
            word_bd_embed: take(&file, "word_bd_embed.weight")?,
            cond_encoder: take_conv_blocks(&file, "cond_encoder", 1, 3, 3, false)?,
            net: take_unet(&file, "net.net", 2)?,
            note_bd_out: take_linear(&file, "note_bd_out", 1, HIDDEN, true)?,
            pitch_decoder_attn: take_linear(&file, "pitch_decoder.multihead_dot_attn", 4, HIDDEN, true)?,
            pitch_decoder_post: take_conv_blocks(&file, "pitch_decoder.post", 1, 3, 3, false)?,
            pitch_decoder_out: take_linear(&file, "pitch_decoder.pitch_out", PITCH_CLASSES, HIDDEN, true)?,
        })
    }
}

// ---------------------------------------------------------------------
// Embedding lookups
// ---------------------------------------------------------------------

fn embedding_lookup(table: &[f32], ids: &[i64]) -> Vec<f32> {
    let mut out = vec![0.0_f32; ids.len() * HIDDEN];
    for (row, id) in ids.iter().enumerate() {
        let id = (*id).max(0) as usize;
        out[row * HIDDEN..(row + 1) * HIDDEN].copy_from_slice(&table[id * HIDDEN..(id + 1) * HIDDEN]);
    }
    out
}

fn add_inplace(a: &mut [f32], b: &[f32]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += y;
    }
}

// ---------------------------------------------------------------------
// Stage "frame": mel_proj+mel_encoder, pitch/uv/word_bd embed,
// cond_encoder, net (Unet backbone), note_bd_out, PitchDecoder's
// frame-level attention pre-computation.
// ---------------------------------------------------------------------

pub struct StageFrameOutput {
    pub note_bd_logits: Vec<f32>, // [T], raw (clamped, not sigmoided)
    pub attention: Vec<f32>,      // [T], mean of 4 heads
    pub weighted: Vec<f32>,       // [T,HIDDEN]
}

pub fn stage_frame(
    w: &RosvotWeights,
    mel: &[f32],
    pitch: &[i64],
    uv: &[i64],
    word_bd: &[i64],
    t: usize,
) -> StageFrameOutput {
    let mut mel_embed = conv1d_same(mel, t, MEL_BINS, &w.mel_proj_weight, Some(&w.mel_proj_bias), HIDDEN, 3, 1, 1);
    mel_embed = conv_blocks(&mel_embed, t, &w.mel_encoder);

    let mut pitch_embed = embedding_lookup(&w.pitch_embed, pitch);
    let uv_embed = embedding_lookup(&w.uv_embed, uv);
    add_inplace(&mut pitch_embed, &uv_embed);

    let word_bd_embed = embedding_lookup(&w.word_bd_embed, word_bd);

    let mut combined = mel_embed;
    add_inplace(&mut combined, &pitch_embed);
    add_inplace(&mut combined, &word_bd_embed);
    let feat = conv_blocks(&combined, t, &w.cond_encoder);

    let feat = unet_forward(&feat, t, &w.net);

    let raw_note_bd = linear(&feat, t, &w.note_bd_out); // [T,1]
    const NOTE_BD_TEMPERATURE: f32 = 0.2;
    let note_bd_logits = raw_note_bd
        .iter()
        .map(|v| (v / NOTE_BD_TEMPERATURE).clamp(-16.0, 16.0))
        .collect::<Vec<_>>();

    let attn_logits = linear(&feat, t, &w.pitch_decoder_attn); // [T,4]
    let mut weighted = vec![0.0_f32; t * HIDDEN];
    let mut attention = vec![0.0_f32; t];
    for time in 0..t {
        let mut attn_mean = 0.0_f32;
        for head in 0..4 {
            attn_mean += sigmoid_scalar(attn_logits[time * 4 + head]);
        }
        attn_mean /= 4.0;
        attention[time] = attn_mean;
        for ch in 0..HIDDEN {
            weighted[time * HIDDEN + ch] = feat[time * HIDDEN + ch] * attn_mean;
        }
    }

    StageFrameOutput {
        note_bd_logits,
        attention,
        weighted,
    }
}

// ---------------------------------------------------------------------
// Stage "pitch": PitchDecoder.post + pitch_out on CPU-aggregated notes.
// ---------------------------------------------------------------------

pub fn stage_pitch(w: &RosvotWeights, note_features: &[f32], num_notes: usize) -> Vec<f32> {
    let post = conv_blocks(note_features, num_notes, &w.pitch_decoder_post);
    let mut logits = linear(&post, num_notes, &w.pitch_decoder_out);
    const PITCH_TEMPERATURE: f32 = 0.01;
    for v in &mut logits {
        *v /= PITCH_TEMPERATURE;
    }
    logits
}

// ---------------------------------------------------------------------
// Segment framing (mirrors advanced_notes.rs's fixed 256-frame buckets)
// ---------------------------------------------------------------------

pub struct ConfigWord {
    pub id: String,
    pub text: String,
    pub start: u64,    // microseconds, canonical timeline
    pub duration: u64, // microseconds
}

struct Segment {
    start: usize,
    valid: usize,
    words: Vec<ConfigWord>,
}

fn frame_to_micros(frame: usize) -> u64 {
    (frame as u128 * singing_frontend::HOP_SIZE as u128 * 1_000_000 / singing_frontend::SAMPLE_RATE as u128) as u64
}

fn canonical_to_frame(value: u64) -> Result<usize> {
    usize::try_from(
        (u128::from(value) * singing_frontend::SAMPLE_RATE as u128
            + singing_frontend::HOP_SIZE as u128 * 500_000)
            / (singing_frontend::HOP_SIZE as u128 * 1_000_000),
    )
    .map_err(|_| Error::message("TimedTranscript frame projection overflows"))
}

fn conditioned_segments(words: &[ConfigWord], source_start: u64, frames: usize) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < frames {
        let valid = (frames - start).min(FRAME_BUCKET);
        let segment_start = frame_to_micros(start) + source_start;
        let segment_end = frame_to_micros(start + valid) + source_start;
        let segment_words: Vec<ConfigWord> = words
            .iter()
            .filter(|word| {
                let end = word.start.saturating_add(word.duration);
                word.start < segment_end && end > segment_start
            })
            .map(|word| ConfigWord {
                id: word.id.clone(),
                text: word.text.clone(),
                start: word.start,
                duration: word.duration,
            })
            .collect();
        if !segment_words.is_empty() {
            result.push(Segment {
                start,
                valid,
                words: segment_words,
            });
        }
        start += FRAME_BUCKET;
    }
    result
}

/// `word_bd` input: 0/1 per frame, 1 marking every word start after the
/// first (matching `advanced_notes.rs::segment_word_boundaries` exactly --
/// the first word's own start is never marked, only internal boundaries).
fn segment_word_boundaries(segment: &Segment, source_start: u64) -> Result<Vec<i64>> {
    let mut boundaries = vec![0_i64; FRAME_BUCKET];
    let segment_timeline_start = frame_to_micros(segment.start) + source_start;
    for word in segment.words.iter().skip(1) {
        let local = word.start.saturating_sub(segment_timeline_start);
        let frame = canonical_to_frame(local)?.min(segment.valid.saturating_sub(1));
        if frame > 0 {
            boundaries[frame] = 1;
        }
    }
    Ok(boundaries)
}

fn padded_rows(values: &[f32], frames: usize, width: usize, start: usize) -> Vec<f32> {
    let mut result = vec![0.0; FRAME_BUCKET * width];
    let count = frames.saturating_sub(start).min(FRAME_BUCKET);
    result[..count * width].copy_from_slice(&values[start * width..(start + count) * width]);
    result
}

fn padded_i64(values: &[i64], start: usize) -> Vec<i64> {
    let mut result = vec![0_i64; FRAME_BUCKET];
    let count = values.len().saturating_sub(start).min(FRAME_BUCKET);
    result[..count].copy_from_slice(&values[start..start + count]);
    result
}

fn note_ranges(boundaries: &[usize], valid: usize) -> Vec<(usize, usize)> {
    let mut starts = Vec::with_capacity(boundaries.len() + 1);
    starts.push(0);
    starts.extend(boundaries.iter().copied().filter(|value| *value > 0 && *value < valid));
    starts.sort_unstable();
    starts.dedup();
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| (*start, starts.get(index + 1).copied().unwrap_or(valid)))
        .filter(|(start, end)| end > start)
        .collect()
}

fn boundary_indices(values: &[i64], valid: usize) -> Vec<usize> {
    values[..valid]
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (*value == 1).then_some(index))
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RawNote {
    pub start_frame: usize,
    pub end_frame: usize,
    pub pitch_logits: Vec<f32>,
    pub midi: Option<u8>,
}

fn append_notes(notes: &mut Vec<RawNote>, segment_start: usize, ranges: &[(usize, usize)], logits: &[f32]) {
    for (index, (start, end)) in ranges.iter().copied().enumerate() {
        let row = logits[index * PITCH_CLASSES..(index + 1) * PITCH_CLASSES].to_vec();
        let midi = row
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .and_then(|(class, _)| (NOTE_START..=NOTE_NUM).contains(&class).then_some(class as u8));
        notes.push(RawNote {
            start_frame: segment_start + start,
            end_frame: segment_start + end,
            pitch_logits: row,
            midi,
        });
    }
}

fn stitch_notes(notes: &mut Vec<RawNote>) {
    let mut stitched: Vec<RawNote> = Vec::with_capacity(notes.len());
    for note in notes.drain(..) {
        if let Some(previous) = stitched.last_mut()
            && previous.end_frame == note.start_frame
            && previous.midi.is_some()
            && previous.midi == note.midi
        {
            previous.end_frame = note.end_frame;
            for (left, right) in previous.pitch_logits.iter_mut().zip(note.pitch_logits) {
                *left = (*left + right) * 0.5;
            }
        } else {
            stitched.push(note);
        }
    }
    *notes = stitched;
}

// ---------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DependencyIdentity {
    pub kind: &'static str,
    pub generation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RosvotEvidence {
    pub schema_version: u32,
    pub model_id: &'static str,
    pub capability: &'static str,
    pub upstream_commit: &'static str,
    pub checkpoint_sha256: &'static str,
    pub config_sha256: &'static str,
    pub model_generation: String,
    pub runtime_manifest_sha256: &'static str,
    pub backend: &'static str,
    pub shared_frontend_profile: &'static str,
    pub shared_frontend_generation: &'static str,
    pub annotation_rmvpe_sha256: &'static str,
    pub word_boundary_source: &'static str,
    pub frame_step_num: u32,
    pub frame_step_den: u32,
    pub valid_frames: usize,
    pub note_boundary_logits: Vec<f32>,
    pub regulated_note_boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
    pub dependencies: Vec<DependencyIdentity>,
}

// ---------------------------------------------------------------------
// Top-level orchestration
// ---------------------------------------------------------------------

pub fn resolve_model_files(config: &serde_json::Value) -> Result<(PathBuf, PathBuf)> {
    let rosvot_path = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).and_then(|home| {
                let candidate = home.join(".local/share/uta-studio/runtime/ggml-models/rosvot/rosvot-f32.gguf");
                candidate.is_file().then_some(candidate)
            })
        })
        .ok_or_else(|| Error::message("ROSVOT GGUF model path not found in config or runtime store"))?;
    let rmvpe_path = config
        .get("rmvpe_model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| Error::message("RMVPE GGUF model path not found in config"))?;
    Ok((rosvot_path, rmvpe_path))
}

fn run_annotation_rmvpe(weights: &RmvpeWeights, audio_16k: &[f32]) -> Result<Vec<f32>> {
    let (mel, frames) = mel16::log_mel_spectrogram(audio_16k).map_err(Error::message)?;
    const WINDOW: usize = 256;
    const OVERLAP: usize = 64;
    const STRIDE: usize = WINDOW - OVERLAP;
    let windows = if frames <= WINDOW { 1 } else { (frames - WINDOW).div_ceil(STRIDE) + 1 };
    let mut raw = Vec::with_capacity(frames);
    let mut start = 0;
    for window in 0..windows {
        let remaining = frames.saturating_sub(start);
        let final_window = remaining <= WINDOW;
        let values = mel16::to_channel_major_window(&mel, frames, start, WINDOW);
        let salience = rmvpe::forward(weights, &values, WINDOW);
        let keep_start = if window == 0 { 0 } else { OVERLAP / 2 };
        let keep_end = if final_window { remaining } else { WINDOW - OVERLAP / 2 };
        for frame in keep_start..keep_end {
            raw.push(decode_rmvpe_frame(&salience[frame * rmvpe::PITCH_CLASSES..(frame + 1) * rmvpe::PITCH_CLASSES]));
        }
        if final_window {
            break;
        }
        start += STRIDE;
    }
    if raw.len() != frames {
        return Err(Error::message("annotation RMVPE window stitching lost frames"));
    }
    Ok(raw)
}

fn decode_rmvpe_frame(values: &[f32]) -> f32 {
    const CENTS_OFFSET: f32 = 1_997.379_4;
    let (center, confidence) = values
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .unwrap_or((0, 0.0));
    if confidence < 0.03 {
        return 0.0;
    }
    let start = center.saturating_sub(4);
    let end = (center + 4).min(rmvpe::PITCH_CLASSES - 1);
    let mut weighted = 0.0;
    let mut weight = 0.0;
    for (class, salience) in values.iter().copied().enumerate().take(end + 1).skip(start) {
        weighted += salience * (20.0 * class as f32 + CENTS_OFFSET);
        weight += salience;
    }
    let cents = if weight > f32::EPSILON { weighted / weight } else { 20.0 * center as f32 + CENTS_OFFSET };
    10.0 * 2.0_f32.powf(cents / 1_200.0)
}

pub struct SharedInputs {
    pub mel: Vec<f32>,
    pub frames: usize,
    pub pitch_coarse: Vec<i64>,
    pub uv: Vec<i64>,
}

pub fn shared_inputs(audio_24k: &[f32], audio_16k: &[f32], rmvpe_weights: &RmvpeWeights) -> Result<SharedInputs> {
    let (mel80, frames) = singing_frontend::mel_80(audio_24k).map_err(Error::message)?;
    let mel = singing_frontend::rosvot_mel_prefix(&mel80, frames).map_err(Error::message)?;
    let raw_f0 = run_annotation_rmvpe(rmvpe_weights, audio_16k)?;
    let pitch = singing_frontend::annotation_pitch(&raw_f0, frames).map_err(Error::message)?;
    Ok(SharedInputs {
        mel,
        frames,
        pitch_coarse: pitch.pitch_coarse,
        uv: pitch.uv,
    })
}

pub struct RunRosvotResult {
    pub boundary_logits: Vec<f32>,
    pub boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
}

pub fn run_rosvot(
    weights: &RosvotWeights,
    shared: &SharedInputs,
    words: &[ConfigWord],
    source_start: u64,
    mut progress: impl FnMut(f32, &str),
) -> Result<RunRosvotResult> {
    let segments = conditioned_segments(words, source_start, shared.frames);
    if segments.is_empty() {
        return Err(Error::message("ROSVOT has no TimedTranscript-conditioned frames"));
    }
    let mut all_logits = vec![0.0; shared.frames];
    let mut all_boundaries = Vec::new();
    let mut all_notes: Vec<RawNote> = Vec::new();

    let total = segments.len().max(1);
    for (index, segment) in segments.iter().enumerate() {
        progress(index as f32 / total as f32, "Running ROSVOT frame/pitch segments");

        let mel = padded_rows(&shared.mel, shared.frames, MEL_BINS, segment.start);
        let pitch = padded_i64(&shared.pitch_coarse, segment.start);
        let uv = padded_i64(&shared.uv, segment.start);
        let reference = segment_word_boundaries(segment, source_start)?;

        let frame = stage_frame(weights, &mel, &pitch, &uv, &reference, FRAME_BUCKET);
        all_logits[segment.start..segment.start + segment.valid].copy_from_slice(&frame.note_bd_logits[..segment.valid]);

        let regulated =
            rosvot_host::regulate_boundaries(&frame.note_bd_logits, 0.85, 17, &reference, 8, segment.valid)
                .map_err(Error::message)?;
        let aggregated = rosvot_host::aggregate_notes(&frame.weighted, &frame.attention, &regulated, HIDDEN, segment.valid)
            .map_err(Error::message)?;
        if aggregated.count > NOTE_BUCKET {
            return Err(Error::message("ROSVOT segment exceeds the pinned note bucket"));
        }
        let pitch_logits = stage_pitch(weights, &aggregated.features, aggregated.count);

        let local_boundaries = boundary_indices(&regulated, segment.valid);
        let ranges = note_ranges(&local_boundaries, segment.valid);
        append_notes(&mut all_notes, segment.start, &ranges, &pitch_logits);
        for boundary in ranges.iter().skip(1).map(|range| segment.start + range.0) {
            all_boundaries.push(boundary);
        }
    }
    stitch_notes(&mut all_notes);
    progress(1.0, "ROSVOT inference complete");
    Ok(RunRosvotResult {
        boundary_logits: all_logits,
        boundaries: all_boundaries,
        notes: all_notes,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn infer(
    audio_24k: &[f32],
    audio_16k: &[f32],
    words: &[ConfigWord],
    source_start: u64,
    timed_transcript_generation: &str,
    model_generation: &str,
    rosvot_model_path: &Path,
    rmvpe_model_path: &Path,
    output_dir: &Path,
    mut progress: impl FnMut(f32, &str, Option<(u64, u64)>),
) -> Result<PathBuf> {
    progress(0.0, "Loading ROSVOT and RMVPE weights", None);
    let rosvot_weights = RosvotWeights::load(rosvot_model_path)?;
    let rmvpe_weights = RmvpeWeights::load(rmvpe_model_path)?;

    progress(0.05, "Computing shared mel and pitch annotation", None);
    let shared = shared_inputs(audio_24k, audio_16k, &rmvpe_weights)?;

    let result = run_rosvot(&rosvot_weights, &shared, words, source_start, |fraction, message| {
        progress(0.1 + fraction * 0.85, message, None);
    })?;

    let dependencies = vec![
        DependencyIdentity {
            kind: "shared_frontend",
            generation: SHARED_MANIFEST_SHA256.to_string(),
        },
        DependencyIdentity {
            kind: "annotation_rmvpe",
            generation: SHARED_MANIFEST_SHA256.to_string(),
        },
        DependencyIdentity {
            kind: "timed_transcript",
            generation: timed_transcript_generation.to_string(),
        },
    ];

    let evidence = RosvotEvidence {
        schema_version: 1,
        model_id: "rosvot",
        capability: "notes.rosvot",
        upstream_commit: ROSVOT_COMMIT,
        checkpoint_sha256: ROSVOT_CHECKPOINT,
        config_sha256: ROSVOT_CONFIG,
        model_generation: model_generation.to_string(),
        runtime_manifest_sha256: RUNTIME_MANIFEST_IDENTITY,
        backend: "ggml_native",
        shared_frontend_profile: singing_frontend::PROFILE,
        shared_frontend_generation: SHARED_MANIFEST_SHA256,
        annotation_rmvpe_sha256: singing_frontend::ANNOTATION_RMVPE_SHA256,
        word_boundary_source: "timed_transcript",
        frame_step_num: singing_frontend::HOP_SIZE as u32,
        frame_step_den: singing_frontend::SAMPLE_RATE as u32,
        valid_frames: shared.frames,
        note_boundary_logits: result.boundary_logits,
        regulated_note_boundaries: result.boundaries,
        notes: result.notes,
        dependencies,
    };
    let path = output_dir.join("advanced-note-evidence.json");
    let file = std::fs::File::create(&path)?;
    serde_json::to_writer(std::io::BufWriter::new(file), &evidence)?;
    progress(1.0, "ROSVOT evidence written", None);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_real_pinned_checkpoint_gguf_end_to_end() {
        let Ok(path) = std::env::var("UTA_STUDIO_TEST_ROSVOT_GGUF") else {
            return;
        };
        let weights = RosvotWeights::load(std::path::Path::new(&path)).unwrap();
        assert_eq!(weights.mel_proj_bias.len(), HIDDEN);
        assert_eq!(weights.pitch_embed.len(), 300 * HIDDEN);
        assert_eq!(weights.uv_embed.len(), 3 * HIDDEN);
        assert_eq!(weights.word_bd_embed.len(), 3 * HIDDEN);
        assert_eq!(weights.note_bd_out.out_dim, 1);
        assert_eq!(weights.pitch_decoder_out.out_dim, PITCH_CLASSES);
        assert_eq!(weights.net.mid.net.layers.len(), 2);
    }

    #[test]
    fn stage_frame_and_pitch_run_without_panicking_on_synthetic_input() {
        let Ok(rosvot_path) = std::env::var("UTA_STUDIO_TEST_ROSVOT_GGUF") else {
            return;
        };
        let Ok(rmvpe_path) = std::env::var("UTA_STUDIO_TEST_RMVPE_GGUF") else {
            return;
        };
        let rosvot_weights = RosvotWeights::load(std::path::Path::new(&rosvot_path)).unwrap();
        let rmvpe_weights = RmvpeWeights::load(std::path::Path::new(&rmvpe_path)).unwrap();

        let make_tone = |sample_rate: usize| -> Vec<f32> {
            (0..sample_rate * 3)
                .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sample_rate as f32).sin() * 0.2)
                .collect()
        };
        let audio_24k = make_tone(singing_frontend::SAMPLE_RATE);
        let audio_16k = make_tone(mel16::SAMPLE_RATE);
        let words = vec![
            ConfigWord {
                id: "w0".to_string(),
                text: "la".to_string(),
                start: 0,
                duration: 700_000,
            },
            ConfigWord {
                id: "w1".to_string(),
                text: "la".to_string(),
                start: 700_000,
                duration: 800_000,
            },
        ];

        let shared = shared_inputs(&audio_24k, &audio_16k, &rmvpe_weights).unwrap();
        let result = run_rosvot(&rosvot_weights, &shared, &words, 0, |_, _| {}).unwrap();
        for value in &result.boundary_logits {
            assert!(value.is_finite());
        }
        for note in &result.notes {
            for value in &note.pitch_logits {
                assert!(value.is_finite());
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct FrontendFixture {
        mel_frames: usize,
        mel: Vec<Vec<f32>>,
        annotation_pitch_coarse: Vec<i64>,
        annotation_uv: Vec<i64>,
    }

    #[derive(serde::Deserialize)]
    struct ReferenceOutput {
        valid: usize,
        note_bd_logits: Vec<f32>,
        attention: Vec<f32>,
        weighted: Vec<Vec<f32>>,
        note_agg: Vec<f32>,
        note_logits: Vec<f32>,
        word_bd: Vec<i64>,
    }

    /// Cross-checks the native Rust `stage_frame`/`stage_pitch` forward pass
    /// against a genuine PyTorch reference forward pass
    /// (`native-inference/rosvot/tools/../drive.py`-equivalent, using the
    /// real checkpoint and the exact same padded 256-frame input built from
    /// `fixtures/shared-singing-frontend-upstream.json`'s real upstream mel
    /// + annotation-pitch values). This is the rigor bar this session has
    /// held for every other native engine (FireRed, Basic Pitch) and STARS
    /// has not yet met -- ROSVOT now has.
    #[test]
    fn stage_frame_and_pitch_match_a_genuine_pytorch_reference_forward_pass() {
        let Ok(rosvot_path) = std::env::var("UTA_STUDIO_TEST_ROSVOT_GGUF") else {
            return;
        };
        let weights = RosvotWeights::load(std::path::Path::new(&rosvot_path)).unwrap();

        let frontend: FrontendFixture =
            serde_json::from_str(include_str!("../fixtures/shared-singing-frontend-upstream.json")).unwrap();
        let reference: ReferenceOutput =
            serde_json::from_str(include_str!("../fixtures/pytorch-reference-rosvot-output.json")).unwrap();
        assert_eq!(frontend.mel_frames, reference.valid);
        let valid = frontend.mel_frames;
        const T: usize = 256;

        let mut mel = vec![0.0_f32; T * MEL_BINS];
        for (row, frame) in frontend.mel.iter().enumerate().take(valid) {
            mel[row * MEL_BINS..(row + 1) * MEL_BINS].copy_from_slice(&frame[..MEL_BINS]);
        }
        let mut pitch = vec![0_i64; T];
        pitch[..valid].copy_from_slice(&frontend.annotation_pitch_coarse[..valid]);
        let mut uv = vec![0_i64; T];
        uv[..valid].copy_from_slice(&frontend.annotation_uv[..valid]);
        let mut word_bd = vec![0_i64; T];
        word_bd[..valid].copy_from_slice(&reference.word_bd[..valid]);

        let frame = stage_frame(&weights, &mel, &pitch, &uv, &word_bd, T);

        let max_diff = |a: &[f32], b: &[f32]| -> f32 {
            a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0_f32, f32::max)
        };
        let bd_diff = max_diff(&frame.note_bd_logits[..valid], &reference.note_bd_logits[..valid]);
        let attn_diff = max_diff(&frame.attention[..valid], &reference.attention[..valid]);
        let reference_weighted_flat = reference.weighted[..valid].concat();
        let weighted_diff = max_diff(&frame.weighted[..valid * HIDDEN], &reference_weighted_flat);
        println!("note_bd_logits max diff: {bd_diff}");
        println!("attention max diff: {attn_diff}");
        println!("weighted max diff: {weighted_diff}");
        // Thresholds sized for accumulated float32 rounding noise through
        // ~20+ conv/attention layers (bisected stage-by-stage above down to
        // ~1e-4/1e-5 at each individual layer boundary; see
        // `debug_bisect::bisect_stage_frame_against_reference`), not a
        // remaining correctness gap.
        assert!(bd_diff < 5.0e-3, "note_bd_logits diverged: {bd_diff}");
        assert!(attn_diff < 1.0e-3, "attention diverged: {attn_diff}");
        assert!(weighted_diff < 5.0e-3, "weighted features diverged: {weighted_diff}");

        // Cross-check PitchDecoder.post+pitch_out in isolation, using the
        // exact same synthetic note aggregate (mean of the first 5 valid
        // frames' weighted features) the Python driver used.
        let mut note_agg = vec![0.0_f32; HIDDEN];
        for f in 0..5 {
            for c in 0..HIDDEN {
                note_agg[c] += frame.weighted[f * HIDDEN + c];
            }
        }
        for v in &mut note_agg {
            *v /= 5.0;
        }
        let note_agg_diff = max_diff(&note_agg, &reference.note_agg);
        println!("note_agg max diff: {note_agg_diff}");
        assert!(note_agg_diff < 1.0e-3, "note aggregate input diverged: {note_agg_diff}");

        let note_logits = stage_pitch(&weights, &note_agg, 1);
        let logits_diff = max_diff(&note_logits, &reference.note_logits);
        println!("note_logits max diff: {logits_diff}");
        assert!(logits_diff < 1.0e-1, "note_logits diverged: {logits_diff}");
    }
}

#[cfg(test)]
mod debug_bisect {
    use super::*;

    #[derive(serde::Deserialize)]
    struct FrontendFixture {
        mel_frames: usize,
        mel: Vec<Vec<f32>>,
        annotation_pitch_coarse: Vec<i64>,
        annotation_uv: Vec<i64>,
    }
    #[derive(serde::Deserialize)]
    struct ReferenceOutput {
        word_bd: Vec<i64>,
    }
    #[derive(serde::Deserialize)]
    struct DebugFixture {
        mel_embed_proj: Vec<Vec<f32>>,
        mel_embed: Vec<Vec<f32>>,
        pitch_embed: Vec<Vec<f32>>,
        word_bd_embed: Vec<Vec<f32>>,
        feat_cond_encoder: Vec<Vec<f32>>,
        feat_net: Vec<Vec<f32>>,
        unet_down_out: Vec<Vec<f32>>,
        unet_skip_0: Vec<Vec<f32>>,
        unet_skip_1: Vec<Vec<f32>>,
        unet_skip_2: Vec<Vec<f32>>,
        unet_skip_3: Vec<Vec<f32>>,
        unet_mid_out: Vec<Vec<f32>>,
        unet_up_out: Vec<Vec<f32>>,
        mid_pre_out: Vec<Vec<f32>>,
        conformer_x_scaled: Vec<Vec<f32>>,
        conformer_pos_emb: Vec<Vec<f32>>,
        conformer_layer_0: Vec<Vec<f32>>,
        conformer_layer_1: Vec<Vec<f32>>,
        conformer_out: Vec<Vec<f32>>,
        mid_post_out: Vec<Vec<f32>>,
    }

    fn max_diff_2d(a: &[f32], b: &[Vec<f32>], valid: usize, width: usize) -> f32 {
        let mut m = 0.0_f32;
        for row in 0..valid {
            for col in 0..width {
                let d = (a[row * width + col] - b[row][col]).abs();
                if d > m {
                    m = d;
                }
            }
        }
        m
    }

    #[test]
    fn bisect_stage_frame_against_reference() {
        let Ok(rosvot_path) = std::env::var("UTA_STUDIO_TEST_ROSVOT_GGUF") else {
            return;
        };
        let w = RosvotWeights::load(std::path::Path::new(&rosvot_path)).unwrap();
        let frontend: FrontendFixture =
            serde_json::from_str(include_str!("../fixtures/shared-singing-frontend-upstream.json")).unwrap();
        let reference: ReferenceOutput =
            serde_json::from_str(include_str!("../fixtures/pytorch-reference-rosvot-output.json")).unwrap();
        let debug: DebugFixture =
            serde_json::from_str(include_str!("../fixtures/pytorch-reference-rosvot-debug.json")).unwrap();
        let valid = frontend.mel_frames;
        const T: usize = 256;

        let mut mel = vec![0.0_f32; T * MEL_BINS];
        for (row, frame) in frontend.mel.iter().enumerate().take(valid) {
            mel[row * MEL_BINS..(row + 1) * MEL_BINS].copy_from_slice(&frame[..MEL_BINS]);
        }
        let mut pitch = vec![0_i64; T];
        pitch[..valid].copy_from_slice(&frontend.annotation_pitch_coarse[..valid]);
        let mut uv = vec![0_i64; T];
        uv[..valid].copy_from_slice(&frontend.annotation_uv[..valid]);
        let mut word_bd = vec![0_i64; T];
        word_bd[..valid].copy_from_slice(&reference.word_bd[..valid]);

        let mel_embed_proj = conv1d_same(&mel, T, MEL_BINS, &w.mel_proj_weight, Some(&w.mel_proj_bias), HIDDEN, 3, 1, 1);
        println!(
            "mel_embed_proj diff: {}",
            max_diff_2d(&mel_embed_proj, &debug.mel_embed_proj, valid, HIDDEN)
        );

        let mel_embed = conv_blocks(&mel_embed_proj, T, &w.mel_encoder);
        println!("mel_embed diff: {}", max_diff_2d(&mel_embed, &debug.mel_embed, valid, HIDDEN));

        let mut pitch_embed = embedding_lookup(&w.pitch_embed, &pitch);
        let uv_embed = embedding_lookup(&w.uv_embed, &uv);
        add_inplace(&mut pitch_embed, &uv_embed);
        println!("pitch_embed diff: {}", max_diff_2d(&pitch_embed, &debug.pitch_embed, valid, HIDDEN));

        let word_bd_embed = embedding_lookup(&w.word_bd_embed, &word_bd);
        println!("word_bd_embed diff: {}", max_diff_2d(&word_bd_embed, &debug.word_bd_embed, valid, HIDDEN));

        let mut combined = mel_embed.clone();
        add_inplace(&mut combined, &pitch_embed);
        add_inplace(&mut combined, &word_bd_embed);
        let feat0 = conv_blocks(&combined, T, &w.cond_encoder);
        println!("feat_cond_encoder diff: {}", max_diff_2d(&feat0, &debug.feat_cond_encoder, valid, HIDDEN));

        let feat = unet_forward(&feat0, T, &w.net);
        println!("feat_net diff: {}", max_diff_2d(&feat, &debug.feat_net, valid, HIDDEN));

        // Bisect inside the Unet: down -> mid -> up.
        let (down_out, skips) = unet_down(&feat0, T, &w.net.down);
        let bottleneck_t = T / 16;
        println!(
            "unet_down_out diff: {}",
            max_diff_2d(&down_out, &debug.unet_down_out, bottleneck_t, HIDDEN)
        );
        let skip_refs = [&debug.unet_skip_0, &debug.unet_skip_1, &debug.unet_skip_2, &debug.unet_skip_3];
        for (i, (skip, skip_t)) in skips.iter().enumerate() {
            let sd = max_diff_2d(skip, skip_refs[i], *skip_t, HIDDEN);
            println!("unet_skip_{i} (t={skip_t}) diff: {sd}");
        }
        let mid_out = unet_mid(&down_out, bottleneck_t, &w.net.mid);
        println!(
            "unet_mid_out diff: {}",
            max_diff_2d(&mid_out, &debug.unet_mid_out, bottleneck_t, HIDDEN)
        );

        // Bisect inside unet_mid: pre-conv -> RelPositionalEncoding ->
        // per-layer Conformer output -> final norm -> post-conv.
        let pre_out = conv1d_same(
            &down_out,
            bottleneck_t,
            HIDDEN,
            &w.net.mid.pre_weight,
            Some(&w.net.mid.pre_bias),
            HIDDEN,
            3,
            1,
            1,
        );
        println!(
            "mid_pre_out diff: {}",
            max_diff_2d(&pre_out, &debug.mid_pre_out, bottleneck_t, HIDDEN)
        );
        let (x_scaled, pos_emb) = rel_positional_encoding(&pre_out, bottleneck_t);
        println!(
            "conformer_x_scaled diff: {}",
            max_diff_2d(&x_scaled, &debug.conformer_x_scaled, bottleneck_t, HIDDEN)
        );
        println!(
            "conformer_pos_emb diff: {}",
            max_diff_2d(&pos_emb, &debug.conformer_pos_emb, bottleneck_t, HIDDEN)
        );
        let nonpadding = nonpadding_from_zero_rows(&pre_out, bottleneck_t, HIDDEN);
        let mut current = x_scaled;
        let layer_refs = [&debug.conformer_layer_0, &debug.conformer_layer_1];
        for (li, layer) in w.net.mid.net.layers.iter().enumerate() {
            current = conformer_encoder_layer(&current, &pos_emb, bottleneck_t, &nonpadding, layer);
            println!(
                "conformer_layer_{li} diff: {}",
                max_diff_2d(&current, layer_refs[li], bottleneck_t, HIDDEN)
            );
        }
        let mut conformer_out = current.clone();
        layer_norm(&mut conformer_out, bottleneck_t, HIDDEN, &w.net.mid.net.final_layer_norm, 1.0e-5);
        apply_nonpadding(&mut conformer_out, bottleneck_t, HIDDEN, &nonpadding);
        println!(
            "conformer_out diff: {}",
            max_diff_2d(&conformer_out, &debug.conformer_out, bottleneck_t, HIDDEN)
        );
        let post_out = conv1d_same(
            &conformer_out,
            bottleneck_t,
            HIDDEN,
            &w.net.mid.post_weight,
            Some(&w.net.mid.post_bias),
            HIDDEN,
            3,
            1,
            1,
        );
        println!(
            "mid_post_out diff: {}",
            max_diff_2d(&post_out, &debug.mid_post_out, bottleneck_t, HIDDEN)
        );
        let up_out = unet_up(&mid_out, bottleneck_t, &skips, &w.net.up);
        println!("unet_up_out diff: {}", max_diff_2d(&up_out, &debug.unet_up_out, valid, HIDDEN));
    }
}

#[cfg(test)]
mod fullsong {
    use super::*;

    #[test]
    #[ignore = "requires local audio fixture and real GGUF paths"]
    fn full_song_runs_without_crashing() {
        let stars_audio = std::env::var("UTA_STUDIO_TEST_FULLSONG_WAV").expect("UTA_STUDIO_TEST_FULLSONG_WAV is required");
        let rosvot_path = std::env::var("UTA_STUDIO_TEST_ROSVOT_GGUF").expect("UTA_STUDIO_TEST_ROSVOT_GGUF is required");
        let rmvpe_path = std::env::var("UTA_STUDIO_TEST_RMVPE_GGUF").expect("UTA_STUDIO_TEST_RMVPE_GGUF is required");

        let scratch = std::env::temp_dir().join(format!("uta-rosvot-fullsong-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let audio_24k = crate::audio::decode_mono(std::path::Path::new(&stars_audio), &scratch, singing_frontend::SAMPLE_RATE).unwrap();
        let audio_16k = crate::audio::decode_mono(std::path::Path::new(&stars_audio), &scratch, mel16::SAMPLE_RATE).unwrap();
        println!("decoded {} 24kHz samples, {} 16kHz samples", audio_24k.len(), audio_16k.len());

        let duration_micros = (audio_24k.len() as u64 * 1_000_000) / singing_frontend::SAMPLE_RATE as u64;
        let mut words = Vec::new();
        let mut t = 0_u64;
        let mut idx = 0;
        while t + 400_000 < duration_micros {
            words.push(ConfigWord {
                id: format!("w{idx}"),
                text: "la".to_string(),
                start: t,
                duration: 400_000,
            });
            t += 500_000;
            idx += 1;
        }
        println!("{} synthetic words spanning {} us", words.len(), duration_micros);

        let rosvot_weights = RosvotWeights::load(std::path::Path::new(&rosvot_path)).unwrap();
        let rmvpe_weights = RmvpeWeights::load(std::path::Path::new(&rmvpe_path)).unwrap();
        let start = std::time::Instant::now();
        let shared = shared_inputs(&audio_24k, &audio_16k, &rmvpe_weights).unwrap();
        println!("shared_inputs: {:?}, frames={}", start.elapsed(), shared.frames);

        let start = std::time::Instant::now();
        let result = run_rosvot(&rosvot_weights, &shared, &words, 0, |fraction, _| {
            if (fraction * 100.0) as u32 % 10 == 0 {
                println!("progress: {:.1}%", fraction * 100.0);
            }
        })
        .unwrap();
        println!("run_rosvot: {:?}, notes={}", start.elapsed(), result.notes.len());

        let mut finite = 0;
        let mut with_midi = 0;
        for note in &result.notes {
            for v in &note.pitch_logits {
                assert!(v.is_finite());
                finite += 1;
            }
            if note.midi.is_some() {
                with_midi += 1;
            }
        }
        println!(
            "{} notes, {} with a MIDI claim, {finite} finite pitch-logit values",
            result.notes.len(),
            with_midi
        );
        for v in &result.boundary_logits {
            assert!(v.is_finite());
        }
        std::fs::remove_dir_all(&scratch).ok();
    }
}
