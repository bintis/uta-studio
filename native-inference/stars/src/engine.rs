//! Native CPU re-implementation of STARS (Singing Transcription with
//! Alignment, Rhythm and Style), matching
//! `native-inference/openvino-worker/src/advanced_notes.rs`'s validated
//! stage split, segment framing, and evidence contract exactly, but running
//! the model itself on hand-written CPU kernels (`crate::layers`) against a
//! native GGUF built directly from the official checkpoint
//! (`verstar/STARS@744a7ad02e1d788452293cd903ea6a933f7862c4`,
//! `model_ckpt_steps_200000.ckpt`) instead of the pinned OpenVINO IR export.
//!
//! Architecture and every tensor name/shape here were confirmed directly
//! against the real checkpoint's `state_dict` (1,354 tensors) and the
//! pinned reference source
//! (`gwx314/STARS@f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167`,
//! `modules/stars/{stars.py,unet.py,utils.py}`,
//! `modules/commons/{layers.py,conv.py,transformer.py,conformer/*}`,
//! `configs/{stars_chinese.yaml,base.yaml}`). Stage boundaries mirror
//! `advanced_notes.rs::run_stars` exactly (each stage break sits at a point
//! where CPU-side, non-differentiable work -- Viterbi decoding, boundary
//! regulation, phoneme-interval aggregation -- must run between neural
//! stages):
//! - Stage A: `mel_proj` + `mel_encoder` + pitch/uv embed -> mel_embed;
//!   `get_prosody_utter` -> feat; `ph_frame_predictor` -> phoneme
//!   boundary/logits (consumed by `stars_viterbi::align`, already ported
//!   and validated in this crate).
//! - Stage B: `get_prosody_ph` + `get_prosody_word` (using the Viterbi
//!   alignment's `mel2ph`/`mel2word`) -> enhanced feat;
//!   `note_frame_predictor` -> note boundary logits (consumed by
//!   `stars_viterbi::regulate_boundaries`).
//! - Stage C: `get_prosody_note` (using the regulated `mel2note`) ->
//!   `pitch_decoder` -> per-note pitch classification.
//! - Stage D (technique only): `get_prosody_sentence` (sentence-level
//!   prosody mean-pool + `align_sentence` cross-attention + `style_predict`)
//!   plus `tech_predictor`'s frame-level attention pre-computation.
//! - Stage E (technique only): `tech_predictor`'s phoneme-interval
//!   aggregation (done here, in Rust, using the real Viterbi intervals) +
//!   `tech_post` + `binary_tech_out`.
//!
//! `pitch`/`uv` are produced by this crate's own native RMVPE port
//! (`crate::rmvpe`), run over the shared 24 kHz singing frontend
//! (`crate::singing_frontend`) exactly as `advanced_notes.rs::shared_inputs`
//! does, using the *same* pinned `rmvpe-f32.gguf` the standalone `rmvpe`
//! model already uses in production (loaded via a second GGUF path so this
//! worker never re-derives RMVPE's own architecture from scratch).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::gguf::GGUFFile;
use crate::layers::*;
use crate::mel16;
use crate::rmvpe::{self, RmvpeWeights};
use crate::singing_frontend;
use crate::stars_g2p::ChineseG2pAsset;
use crate::stars_viterbi;

pub const FRAME_BUCKET: usize = 256;
pub const NOTE_BUCKET: usize = 32;
pub const PHONEME_BUCKET: usize = 256;
pub const PITCH_CLASSES: usize = 89;
pub const TECHNIQUE_CLASSES: usize = 9;
pub const NOTE_NUM: usize = 85;
pub const NOTE_START: usize = 30;
pub const NUM_CLS_TOKENS: usize = 16;
const NVQ: usize = 48;

pub const TECHNIQUE_TAXONOMY: [&str; TECHNIQUE_CLASSES] = [
    "bubble",
    "breathe",
    "pharyngeal",
    "vibrato",
    "glissando",
    "mixed",
    "falsetto",
    "weak",
    "strong",
];
pub const STYLE_TECHNIQUE_GROUP: [&str; 10] = [
    "control", "mixed", "falsetto", "pharyngeal", "glissando", "vibrato", "breathy", "weak", "strong", "bubble",
];
pub const STYLE_LANGUAGE: [&str; 9] = [
    "Chinese", "English", "Italian", "French", "Japanese", "Spanish", "German", "Korean", "Russian",
];
pub const STYLE_GENDER: [&str; 2] = ["female", "male"];
pub const STYLE_EMOTION: [&str; 4] = ["neutral", "happy", "sad", "angry"];
pub const STYLE_METHOD: [&str; 2] = ["pop", "bel_canto"];
pub const STYLE_PACE: [&str; 3] = ["slow", "moderate", "fast"];
pub const STYLE_RANGE: [&str; 3] = ["low", "medium", "high"];

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

fn take_conformer_layer(file: &GGUFFile, prefix: &str, heads: usize) -> Result<ConformerEncoderLayerWeights> {
    let take_moe = |sub: &str| -> Result<FeedForwardMoeWeights> {
        let mut experts = Vec::with_capacity(4);
        // `FeedForwardMOE(input_dim=HIDDEN, hidden_dim=HIDDEN*4, num_freq_experts=4)`
        // builds each expert as `MultiLayeredConv1d(input_dim/4, hidden_dim/4,
        // kernel=1)`: the per-expert channel chunk (HIDDEN/4=64) is expanded
        // to hidden_dim/4=HIDDEN (256), NOT to another 64-wide hidden -- real
        // checkpoint shapes confirm `w_1: [256,64,1]`, `w_2: [64,256,1]`.
        let chunk_dim = HIDDEN / 4;
        for e in 0..4 {
            let ep = format!("{prefix}.{sub}.freq_experts.{e}");
            experts.push((
                take_linear(file, &format!("{ep}.w_1"), HIDDEN, chunk_dim, true)?,
                take_linear(file, &format!("{ep}.w_2"), chunk_dim, HIDDEN, true)?,
            ));
        }
        Ok(FeedForwardMoeWeights { experts })
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
        feed_forward: take_moe("feed_forward")?,
        feed_forward_macaron: take_moe("feed_forward_macaron")?,
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

fn take_conformer_layers_moe(file: &GGUFFile, prefix: &str, num_layers: usize) -> Result<ConformerLayersMoeWeights> {
    let mut layers = Vec::with_capacity(num_layers);
    for i in 0..num_layers {
        layers.push(take_conformer_layer(file, &format!("{prefix}.encoder_layers.{i}"), 4)?);
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
            net: take_conformer_layers_moe(file, &format!("{prefix}.mid.net"), mid_layers)?,
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

fn take_local_style_adaptor(file: &GGUFFile, prefix: &str, mid_layers: usize) -> Result<LocalStyleAdaptorWeights> {
    Ok(LocalStyleAdaptorWeights {
        cmuencoder: take_unet(file, &format!("{prefix}.cmuencoder.net"), mid_layers)?,
        encoder: take_conv_blocks(file, &format!("{prefix}.encoder"), 1, 3, 3, false)?,
        vq_codebook: take(file, &format!("{prefix}.vqvae.embedding"))?,
        num_codes: NVQ,
    })
}

pub struct StarsWeights {
    mel_proj_weight: Vec<f32>,
    mel_proj_bias: Vec<f32>,
    mel_encoder: ConvBlocksWeights,
    pitch_embed: Vec<f32>, // [300, HIDDEN]
    uv_embed: Vec<f32>,    // [3, HIDDEN]

    prosody_utter: LocalStyleAdaptorWeights,
    l1_utter: LinearWeights,
    ph_frame_head: LinearWeights, // 256 -> 62

    prosody_word: LocalStyleAdaptorWeights,
    l1_word: LinearWeights,
    prosody_ph: LocalStyleAdaptorWeights,
    l1_ph: LinearWeights,
    note_frame_head: LinearWeights, // 256 -> 90

    prosody_note: LocalStyleAdaptorWeights,
    l1_note: LinearWeights,
    pitch_decoder_attn: LinearWeights, // 256 -> 4
    pitch_decoder_post: ConvBlocksWeights,
    pitch_decoder_out: LinearWeights, // 256 -> 89

    prosody_sentence: LocalStyleAdaptorWeights,
    cls_tokens: Vec<f32>, // [16, 256]
    align_sentence: ProsodyAlignerWeights,
    style_predict: StylePredictWeights,

    tech_attn: LinearWeights, // 256 -> 4
    tech_post: ConvBlocksWeights,
    tech_out: LinearWeights, // 256 -> 9
}

struct StylePredictWeights {
    tech_norm: LayerNormWeights,
    lan_norm: LayerNormWeights,
    gen_norm: LayerNormWeights,
    emo_norm: LayerNormWeights,
    meth_norm: LayerNormWeights,
    pace_norm: LayerNormWeights,
    range_norm: LayerNormWeights,
    tech_head: LinearWeights,
    lan_head: LinearWeights,
    gen_head: LinearWeights,
    emo_head: LinearWeights,
    meth_head: LinearWeights,
    pace_head: LinearWeights,
    range_head: LinearWeights,
}

impl StarsWeights {
    fn load(path: &Path) -> Result<Self> {
        let file = GGUFFile::open(path)?;
        if file.architecture() != "stars" {
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

            prosody_utter: take_local_style_adaptor(&file, "prosody_extractor_utter", 2)?,
            l1_utter: take_linear(&file, "l1_utter", HIDDEN, 2 * HIDDEN, true)?,
            ph_frame_head: take_linear(&file, "ph_frame_predictor.ph_head", 62, HIDDEN, true)?,

            prosody_word: take_local_style_adaptor(&file, "prosody_extractor_word", 2)?,
            l1_word: take_linear(&file, "l1_word", HIDDEN, 2 * HIDDEN, true)?,
            prosody_ph: take_local_style_adaptor(&file, "prosody_extractor_ph", 2)?,
            l1_ph: take_linear(&file, "l1_ph", HIDDEN, 2 * HIDDEN, true)?,
            note_frame_head: take_linear(&file, "note_frame_predictor.note_head", 90, HIDDEN, true)?,

            prosody_note: take_local_style_adaptor(&file, "prosody_extractor_note", 2)?,
            l1_note: take_linear(&file, "l1_note", HIDDEN, 2 * HIDDEN, true)?,
            pitch_decoder_attn: take_linear(&file, "pitch_decoder.multihead_dot_attn", 4, HIDDEN, true)?,
            pitch_decoder_post: take_conv_blocks(&file, "pitch_decoder.post", 1, 3, 3, false)?,
            pitch_decoder_out: take_linear(&file, "pitch_decoder.pitch_out", PITCH_CLASSES, HIDDEN, true)?,

            prosody_sentence: take_local_style_adaptor(&file, "prosody_extractor_sentence", 1)?,
            cls_tokens: take(&file, "cls_tokens")?,
            align_sentence: ProsodyAlignerWeights {
                layers: (0..2)
                    .map(|i| take_cross_attn_layer(&file, &format!("align_sentence.layers.{i}")))
                    .collect::<Result<Vec<_>>>()?,
            },
            style_predict: StylePredictWeights {
                tech_norm: take_layer_norm(&file, "style_predict.tech_norm")?,
                lan_norm: take_layer_norm(&file, "style_predict.lan_norm")?,
                gen_norm: take_layer_norm(&file, "style_predict.gen_norm")?,
                emo_norm: take_layer_norm(&file, "style_predict.emo_norm")?,
                meth_norm: take_layer_norm(&file, "style_predict.meth_norm")?,
                pace_norm: take_layer_norm(&file, "style_predict.pace_norm")?,
                range_norm: take_layer_norm(&file, "style_predict.range_norm")?,
                tech_head: take_linear(&file, "style_predict.tech_head", STYLE_TECHNIQUE_GROUP.len(), HIDDEN, true)?,
                lan_head: take_linear(&file, "style_predict.lan_head", STYLE_LANGUAGE.len(), HIDDEN, true)?,
                gen_head: take_linear(&file, "style_predict.gen_head", STYLE_GENDER.len(), HIDDEN, true)?,
                emo_head: take_linear(&file, "style_predict.emo_head", STYLE_EMOTION.len(), HIDDEN, true)?,
                meth_head: take_linear(&file, "style_predict.meth_head", STYLE_METHOD.len(), HIDDEN, true)?,
                pace_head: take_linear(&file, "style_predict.pace_head", STYLE_PACE.len(), HIDDEN, true)?,
                range_head: take_linear(&file, "style_predict.range_head", STYLE_RANGE.len(), HIDDEN, true)?,
            },

            tech_attn: take_linear(&file, "tech_predictor.multihead_tech_attn", 4, HIDDEN, true)?,
            tech_post: take_conv_blocks(&file, "tech_predictor.tech_post", 1, 3, 3, true)?,
            tech_out: take_linear(&file, "tech_predictor.binary_tech_out", TECHNIQUE_CLASSES, HIDDEN, true)?,
        })
    }
}

fn take_cross_attn_layer(file: &GGUFFile, prefix: &str) -> Result<CrossAttnLayerWeights> {
    Ok(CrossAttnLayerWeights {
        in_proj_weight: take(file, &format!("{prefix}.multihead_attn.in_proj_weight"))?,
        in_proj_bias: take(file, &format!("{prefix}.multihead_attn.in_proj_bias"))?,
        out_proj: take_linear(file, &format!("{prefix}.multihead_attn.out_proj"), HIDDEN, HIDDEN, true)?,
        linear1: take_linear(file, &format!("{prefix}.linear1"), 2048, HIDDEN, true)?,
        linear2: take_linear(file, &format!("{prefix}.linear2"), HIDDEN, 2048, true)?,
        norm1: take_layer_norm(file, &format!("{prefix}.norm1"))?,
        norm2: take_layer_norm(file, &format!("{prefix}.norm2"))?,
        heads: 2,
    })
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

fn mul_mask_inplace(x: &mut [f32], t: usize, c: usize, nonpadding: &[bool]) {
    apply_nonpadding(x, t, c, nonpadding);
}

/// `get_prosody_{ph,word,note}`: VQ-quantized, segment-grouped prosody,
/// projected with absolute positions, then expanded back to per-frame
/// resolution via `expand_states`.
fn get_prosody_grouped(
    mel_embed: &[f32],
    t: usize,
    seg_ids: &[i64],
    num_segments: usize,
    adaptor: &LocalStyleAdaptorWeights,
    l1: &LinearWeights,
) -> Vec<f32> {
    let (pooled, rows) = local_style_adaptor(mel_embed, t, Some((seg_ids, num_segments)), false, adaptor);
    debug_assert_eq!(rows, num_segments);
    let positions = sinusoidal_position_embedding(rows, rows);
    let mut cat = vec![0.0_f32; rows * 2 * HIDDEN];
    for row in 0..rows {
        cat[row * 2 * HIDDEN..row * 2 * HIDDEN + HIDDEN].copy_from_slice(&pooled[row * HIDDEN..(row + 1) * HIDDEN]);
        cat[row * 2 * HIDDEN + HIDDEN..row * 2 * HIDDEN + 2 * HIDDEN]
            .copy_from_slice(&positions[row * HIDDEN..(row + 1) * HIDDEN]);
    }
    let projected = linear(&cat, rows, l1);
    expand_states(&projected, rows, seg_ids, t)
}

// ---------------------------------------------------------------------
// Stage A
// ---------------------------------------------------------------------

pub struct StageAOutput {
    pub mel_embed: Vec<f32>,      // a[0], [T,HIDDEN]
    pub feat: Vec<f32>,           // a[1], [T,HIDDEN]
    pub ph_bd_sigmoid: Vec<f32>,  // a[2], [T]  (single sigmoid, see stars_viterbi doc)
    pub ph_frame_logits: Vec<f32>, // a[3], [T,61]
}

pub fn stage_a(w: &StarsWeights, mel: &[f32], pitch: &[i64], uv: &[i64], nonpadding: &[bool], t: usize) -> StageAOutput {
    let mut mel_embed = conv1d_same(mel, t, singing_frontend::MEL_BINS, &w.mel_proj_weight, Some(&w.mel_proj_bias), HIDDEN, 3, 1, 1);
    mel_embed = conv_blocks(&mel_embed, t, &w.mel_encoder);
    mul_mask_inplace(&mut mel_embed, t, HIDDEN, nonpadding);

    let mut pitch_embed = embedding_lookup(&w.pitch_embed, pitch);
    let uv_embed = embedding_lookup(&w.uv_embed, uv);
    add_inplace(&mut pitch_embed, &uv_embed);
    mul_mask_inplace(&mut pitch_embed, t, HIDDEN, nonpadding);

    add_inplace(&mut mel_embed, &pitch_embed);

    let (utter_prosody, rows) = local_style_adaptor(&mel_embed, t, None, true, &w.prosody_utter);
    debug_assert_eq!(rows, t);
    let positions = sinusoidal_position_embedding(nonpadding.iter().filter(|v| **v).count(), t);
    let mut cat = vec![0.0_f32; t * 2 * HIDDEN];
    for row in 0..t {
        cat[row * 2 * HIDDEN..row * 2 * HIDDEN + HIDDEN].copy_from_slice(&utter_prosody[row * HIDDEN..(row + 1) * HIDDEN]);
        cat[row * 2 * HIDDEN + HIDDEN..row * 2 * HIDDEN + 2 * HIDDEN]
            .copy_from_slice(&positions[row * HIDDEN..(row + 1) * HIDDEN]);
    }
    let feat = linear(&cat, t, &w.l1_utter);

    let frame_logits = linear(&feat, t, &w.ph_frame_head); // [T,62]
    let mut ph_bd_sigmoid = vec![0.0_f32; t];
    let mut ph_frame_logits = vec![0.0_f32; t * 61];
    for time in 0..t {
        let bd = frame_logits[time * 62].clamp(-16.0, 16.0);
        ph_bd_sigmoid[time] = sigmoid_scalar(bd);
        ph_frame_logits[time * 61..(time + 1) * 61].copy_from_slice(&frame_logits[time * 62 + 1..(time + 1) * 62]);
    }

    StageAOutput {
        mel_embed,
        feat,
        ph_bd_sigmoid,
        ph_frame_logits,
    }
}

// ---------------------------------------------------------------------
// Stage B
// ---------------------------------------------------------------------

pub struct StageBOutput {
    pub feat: Vec<f32>,          // b[0], [T,HIDDEN]
    pub note_bd_logits: Vec<f32>, // b[1], [T] (raw, clamped, NOT sigmoided)
}

pub fn stage_b(
    w: &StarsWeights,
    mel_embed: &[f32],
    feat_a: &[f32],
    t: usize,
    mel2ph: &[i64],
    num_ph: usize,
    mel2word: &[i64],
    num_word: usize,
) -> StageBOutput {
    let prosody_ph = get_prosody_grouped(mel_embed, t, mel2ph, num_ph, &w.prosody_ph, &w.l1_ph);
    let prosody_word = get_prosody_grouped(mel_embed, t, mel2word, num_word, &w.prosody_word, &w.l1_word);
    let mut feat = feat_a.to_vec();
    add_inplace(&mut feat, &prosody_ph);
    add_inplace(&mut feat, &prosody_word);

    let frame_logits = linear(&feat, t, &w.note_frame_head); // [T,90]
    const NOTE_BD_TEMPERATURE: f32 = 0.2;
    let mut note_bd_logits = vec![0.0_f32; t];
    for time in 0..t {
        note_bd_logits[time] = (frame_logits[time * 90] / NOTE_BD_TEMPERATURE).clamp(-16.0, 16.0);
    }

    StageBOutput { feat, note_bd_logits }
}

// ---------------------------------------------------------------------
// Stage C
// ---------------------------------------------------------------------

pub struct StageCOutput {
    pub feat: Vec<f32>,       // c[0], [T,HIDDEN]
    pub note_logits: Vec<f32>, // c[3], [num_notes, PITCH_CLASSES]
}

pub fn stage_c(
    w: &StarsWeights,
    mel_embed: &[f32],
    feat_b: &[f32],
    t: usize,
    mel2note: &[i64],
    num_notes: usize,
    note_bd: &[i64], // regulated 0/1 boundary indicator, length t
) -> StageCOutput {
    // `get_prosody_note` reuses `group_hidden_by_segs`'s own 1-indexed
    // convention (bucket 0 = padding, dropped) on this *same* raw
    // `mel2note = cumsum(note_bd)` array -- so the very first note (frames
    // before the first internal boundary, `mel2note == 0`) is silently
    // excluded from the VQ-quantized prosody contribution. That is not a
    // bug here: it is the real reference's own behavior (confirmed against
    // a genuine PyTorch forward pass), since `STARS.forward` passes this
    // identical 0-indexed array straight into `get_prosody_note` with no
    // reindexing.
    let prosody_note = get_prosody_grouped(mel_embed, t, mel2note, num_notes, &w.prosody_note, &w.l1_note);
    let mut feat = feat_b.to_vec();
    add_inplace(&mut feat, &prosody_note);

    // `PitchDecoder.forward` performs its own, *separate* re-derivation
    // (`mel2note = cumsum(note_bd)` again, inline) and scatters directly
    // into `note_length = max(sum(note_bd)) + 1` buckets with no
    // bucket-0-drop -- unlike `group_hidden_by_segs` above, note 0 *is*
    // included here (0-indexed, not 1-indexed with a padding sentinel).
    // Reusing the same `mel2note` array is correct precisely because it
    // already equals that raw cumsum; only the indexing convention differs
    // between the two call sites.
    let attn_logits = linear(&feat, t, &w.pitch_decoder_attn); // [T,4]
    let mut note_aggregate = vec![0.0_f32; num_notes * HIDDEN];
    let mut denom = vec![0.0_f32; num_notes];
    for time in 0..t {
        let seg = mel2note[time];
        if seg < 0 {
            continue;
        }
        let index = seg as usize;
        if index >= num_notes {
            continue;
        }
        let mut attn_mean = 0.0_f32;
        for head in 0..4 {
            attn_mean += sigmoid_scalar(attn_logits[time * 4 + head]);
        }
        attn_mean /= 4.0;
        denom[index] += attn_mean;
        for ch in 0..HIDDEN {
            note_aggregate[index * HIDDEN + ch] += feat[time * HIDDEN + ch] * attn_mean;
        }
    }
    for index in 0..num_notes {
        let d = denom[index] + 1.0e-5;
        for ch in 0..HIDDEN {
            note_aggregate[index * HIDDEN + ch] /= d;
        }
    }
    let post = conv_blocks(&note_aggregate, num_notes, &w.pitch_decoder_post);
    let mut note_logits = linear(&post, num_notes, &w.pitch_decoder_out);
    const PITCH_TEMPERATURE: f32 = 0.01;
    for v in &mut note_logits {
        *v /= PITCH_TEMPERATURE;
    }

    let _ = note_bd;
    StageCOutput { feat, note_logits }
}

// ---------------------------------------------------------------------
// Stage D (technique + style only)
// ---------------------------------------------------------------------

pub struct StageDOutput {
    pub output: Vec<f32>,   // d[0], [T,HIDDEN]
    pub weighted: Vec<f32>, // d[1], [T,HIDDEN]
    pub attention: Vec<f32>, // d[2], [T]
    pub styles: [Vec<f32>; 7], // technique_group, language, gender, emotion, method, pace, range
}

pub fn stage_d(w: &StarsWeights, mel_embed: &[f32], feat_c: &[f32], t: usize, nonpadding: &[bool]) -> StageDOutput {
    let (sentence_prosody, rows) = local_style_adaptor(mel_embed, t, None, true, &w.prosody_sentence);
    debug_assert_eq!(rows, t);
    // `prosody_embedding.mean(dim=1)`: a *plain* mean over all T rows,
    // including zero-padded ones (not a masked mean) -- replicated exactly.
    let mut mean = vec![0.0_f32; HIDDEN];
    for row in 0..t {
        for ch in 0..HIDDEN {
            mean[ch] += sentence_prosody[row * HIDDEN + ch];
        }
    }
    for v in &mut mean {
        *v /= t as f32;
    }
    let mut output = feat_c.to_vec();
    for row in 0..t {
        for ch in 0..HIDDEN {
            output[row * HIDDEN + ch] += mean[ch];
        }
    }

    let style_features = prosody_aligner(&w.cls_tokens, NUM_CLS_TOKENS, &output, t, nonpadding, &w.align_sentence);
    let styles = style_predict(&w.style_predict, &style_features);

    let attn_logits = linear(&output, t, &w.tech_attn); // [T,4]
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
            weighted[time * HIDDEN + ch] = output[time * HIDDEN + ch] * attn_mean;
        }
    }

    StageDOutput {
        output,
        weighted,
        attention,
        styles,
    }
}

fn style_predict(w: &StylePredictWeights, features: &[f32]) -> [Vec<f32>; 7] {
    let row = |index: usize| -> Vec<f32> { features[index * HIDDEN..(index + 1) * HIDDEN].to_vec() };
    let head = |mut token: Vec<f32>, norm: &LayerNormWeights, linear_w: &LinearWeights| -> Vec<f32> {
        layer_norm(&mut token, 1, HIDDEN, norm, 1.0e-5);
        linear(&token, 1, linear_w)
    };
    [
        head(row(0), &w.tech_norm, &w.tech_head),
        head(row(1), &w.lan_norm, &w.lan_head),
        head(row(2), &w.gen_norm, &w.gen_head),
        head(row(3), &w.emo_norm, &w.emo_head),
        head(row(4), &w.meth_norm, &w.meth_head),
        head(row(5), &w.pace_norm, &w.pace_head),
        head(row(6), &w.range_norm, &w.range_head),
    ]
}

// ---------------------------------------------------------------------
// Stage E (technique only)
// ---------------------------------------------------------------------

/// `TechniquePredictor`'s scatter-mean-by-phoneme-segment aggregation,
/// mirroring `advanced_notes.rs::aggregate_phoneme_technique` exactly
/// (weighted-sum divided by attention-sum per interval).
pub fn aggregate_phoneme_technique(weighted: &[f32], attention: &[f32], intervals: &[stars_viterbi::Interval]) -> Vec<f32> {
    let mut result = vec![0.0_f32; intervals.len() * HIDDEN];
    for (phoneme, interval) in intervals.iter().enumerate() {
        let denom = attention[interval.start..interval.end].iter().sum::<f32>() + 1.0e-5;
        for frame in interval.start..interval.end {
            for ch in 0..HIDDEN {
                result[phoneme * HIDDEN + ch] += weighted[frame * HIDDEN + ch];
            }
        }
        for v in &mut result[phoneme * HIDDEN..(phoneme + 1) * HIDDEN] {
            *v /= denom;
        }
    }
    result
}

pub fn stage_e(w: &StarsWeights, aggregated: &[f32], num_phonemes: usize) -> Vec<f32> {
    let post = conv_blocks(aggregated, num_phonemes, &w.tech_post);
    let mut logits = linear(&post, num_phonemes, &w.tech_out);
    const TECH_TEMPERATURE: f32 = 1.0;
    for v in &mut logits {
        *v /= TECH_TEMPERATURE;
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
    start: usize, // frame offset into the shared generation
    valid: usize,
    words: Vec<ConfigWord>,
}

fn frame_to_micros(frame: usize) -> u64 {
    (frame as u128 * singing_frontend::HOP_SIZE as u128 * 1_000_000 / singing_frontend::SAMPLE_RATE as u128) as u64
}

fn conditioned_segments(words: &[ConfigWord], source_start: u64, frames: usize) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < frames {
        let valid = (frames - start).min(FRAME_BUCKET);
        // `word.start`/`word.duration` are canonical (whole-song) timeline
        // timestamps, but `start`/`valid` are local frame offsets into this
        // clip's own audio -- add `source_start` before comparing.
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

fn mapping_from_boundaries(values: &[i64], valid: usize) -> Vec<i64> {
    let mut mapping = vec![0_i64; FRAME_BUCKET];
    let mut note = 0_i64;
    for frame in 0..valid {
        note += values[frame];
        mapping[frame] = note.min((NOTE_BUCKET - 1) as i64);
    }
    mapping[valid..].fill(note.min((NOTE_BUCKET - 1) as i64));
    mapping
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
// Evidence types
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RawTechnique {
    pub start_frame: usize,
    pub end_frame: usize,
    pub phoneme_id: i64,
    pub raw_logits: Vec<f32>,
    pub source_local_scores: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawStyleHead {
    pub taxonomy: Vec<&'static str>,
    pub raw_logits: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawGlobalStyle {
    pub start_frame: usize,
    pub end_frame: usize,
    pub heads: std::collections::BTreeMap<&'static str, RawStyleHead>,
}

/// Same upstream checkpoint identity the OpenVINO route pins (this crate's
/// GGUF is a from-scratch re-derivation of the exact same
/// `model_ckpt_steps_200000.ckpt`, confirmed byte-identical by `sha256sum`
/// against `STARS_CHECKPOINT` at conversion time) -- so the native route
/// reports the *same* `upstream_commit`/`checkpoint_sha256`/`config_sha256`
/// identity as `native-inference/openvino-worker/src/advanced_notes.rs`,
/// distinguished only by `backend`.
const STARS_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
const STARS_CHECKPOINT: &str = "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c";
const STARS_CONFIG: &str = "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91";
/// Same pinned shared-frontend identity `advanced_notes.rs` uses for both
/// its `shared_frontend` and `annotation_rmvpe` dependency entries -- this
/// crate's `singing_frontend`/`mel16`/RMVPE-annotation code is a direct,
/// unmodified port of that exact profile.
const SHARED_MANIFEST_SHA256: &str = "986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c";
/// This worker's own identity for the `runtime_manifest_sha256` evidence
/// field (unlike the OpenVINO route, there is no separately-packaged
/// runtime to hash -- the worker binary itself IS the runtime).
const RUNTIME_MANIFEST_IDENTITY: &str = "stars-native-recipe-v1";

#[derive(Debug, Clone, Serialize)]
pub struct DependencyIdentity {
    pub kind: &'static str,
    pub generation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StarsEvidence {
    pub schema_version: u32,
    pub model_id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<&'static str>,
    pub capabilities: Vec<&'static str>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g2p_profile: Option<&'static str>,
    pub frame_step_num: u32,
    pub frame_step_den: u32,
    pub valid_frames: usize,
    pub note_boundary_logits: Vec<f32>,
    pub regulated_note_boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technique_taxonomy: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technique_calibration: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub techniques: Option<Vec<RawTechnique>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<Vec<RawGlobalStyle>>,
    pub dependencies: Vec<DependencyIdentity>,
}

// ---------------------------------------------------------------------
// Top-level orchestration
// ---------------------------------------------------------------------

pub fn resolve_model_files(config: &serde_json::Value) -> Result<(PathBuf, PathBuf)> {
    let stars_path = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).and_then(|home| {
                let candidate = home.join(".local/share/uta-studio/runtime/ggml-models/stars/stars-f32.gguf");
                candidate.is_file().then_some(candidate)
            })
        })
        .ok_or_else(|| Error::message("STARS GGUF model path not found in config or runtime store"))?;
    let rmvpe_path = config
        .get("rmvpe_model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).and_then(|home| {
                let candidate = home.join(".local/share/uta-studio/runtime/ggml-models/rmvpe/rmvpe-f32.gguf");
                candidate.is_file().then_some(candidate)
            })
        })
        .ok_or_else(|| Error::message("RMVPE GGUF model path not found in config or runtime store"))?;
    Ok((stars_path, rmvpe_path))
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
    let (mel, frames) = singing_frontend::mel_80(audio_24k).map_err(Error::message)?;
    let raw_f0 = run_annotation_rmvpe(rmvpe_weights, audio_16k)?;
    let pitch = singing_frontend::annotation_pitch(&raw_f0, frames).map_err(Error::message)?;
    Ok(SharedInputs {
        mel,
        frames,
        pitch_coarse: pitch.pitch_coarse,
        uv: pitch.uv,
    })
}

pub struct RunStarsResult {
    pub boundary_logits: Vec<f32>,
    pub boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
    pub techniques: Option<Vec<RawTechnique>>,
    pub styles: Option<Vec<RawGlobalStyle>>,
}

pub fn run_stars(
    weights: &StarsWeights,
    shared: &SharedInputs,
    words: &[ConfigWord],
    source_start: u64,
    include_technique: bool,
    mut progress: impl FnMut(f32, &str),
) -> Result<RunStarsResult> {
    let g2p = ChineseG2pAsset::load_embedded().map_err(Error::message)?;
    let segments = conditioned_segments(words, source_start, shared.frames);
    if segments.is_empty() {
        return Err(Error::message("STARS has no TimedTranscript-conditioned frames"));
    }
    let mut all_logits = vec![0.0; shared.frames];
    let mut all_boundaries = Vec::new();
    let mut all_notes: Vec<RawNote> = Vec::new();
    let mut all_techniques = include_technique.then(Vec::<RawTechnique>::new);
    let mut all_styles = include_technique.then(Vec::<RawGlobalStyle>::new);

    let total = segments.len().max(1);
    for (index, segment) in segments.iter().enumerate() {
        let message = if include_technique {
            "Running STARS Stage A/B/C/D/E segments"
        } else {
            "Running STARS Stage A/B/C segments"
        };
        progress(index as f32 / total as f32, message);

        let mel = padded_rows(&shared.mel, shared.frames, singing_frontend::MEL_BINS, segment.start);
        let pitch = padded_i64(&shared.pitch_coarse, segment.start);
        let uv = padded_i64(&shared.uv, segment.start);
        let mut nonpadding = vec![false; FRAME_BUCKET];
        nonpadding[..segment.valid].fill(true);

        let a = stage_a(weights, &mel, &pitch, &uv, &nonpadding, FRAME_BUCKET);

        let phonemes = g2p
            .phonemize_words(&segment.words.iter().map(|w| w.text.clone()).collect::<Vec<_>>())
            .map_err(Error::message)?;
        let alignment = stars_viterbi::align(
            &a.ph_frame_logits[..segment.valid * 61],
            61,
            &a.ph_bd_sigmoid[..segment.valid],
            &phonemes.phone_ids,
            &phonemes.phone_to_word,
        )
        .map_err(Error::message)?;
        let mut mel2ph = alignment.mel_to_phoneme.clone();
        let mut mel2word = alignment.mel_to_word.clone();
        mel2ph.resize(FRAME_BUCKET, 0);
        mel2word.resize(FRAME_BUCKET, 0);
        let num_ph = alignment.phoneme_intervals.len();
        let num_word = alignment.word_intervals.len();

        let b = stage_b(weights, &a.mel_embed, &a.feat, FRAME_BUCKET, &mel2ph, num_ph, &mel2word, num_word);
        all_logits[segment.start..segment.start + segment.valid].copy_from_slice(&b.note_bd_logits[..segment.valid]);
        let regulated = stars_viterbi::regulate_boundaries(&b.note_bd_logits, 0.8, 17, segment.valid).map_err(Error::message)?;
        let local_boundaries = boundary_indices(&regulated, segment.valid);
        let ranges = note_ranges(&local_boundaries, segment.valid);
        if ranges.len() > NOTE_BUCKET {
            return Err(Error::message("STARS segment exceeds the pinned note bucket"));
        }
        let mel2note = mapping_from_boundaries(&regulated, segment.valid);

        let c = stage_c(weights, &a.mel_embed, &b.feat, FRAME_BUCKET, &mel2note, ranges.len().max(1), &regulated);
        append_notes(&mut all_notes, segment.start, &ranges, &c.note_logits);

        if include_technique {
            let d = stage_d(weights, &a.mel_embed, &c.feat, FRAME_BUCKET, &nonpadding);
            if alignment.phoneme_intervals.is_empty() || alignment.phoneme_intervals.len() > PHONEME_BUCKET {
                return Err(Error::message("STARS phoneme intervals exceed the technique bucket"));
            }
            let aggregated = aggregate_phoneme_technique(&d.weighted, &d.attention, &alignment.phoneme_intervals);
            let e = stage_e(weights, &aggregated, alignment.phoneme_intervals.len());
            append_techniques(
                all_techniques.as_mut().expect("technique collection is available"),
                segment.start,
                &alignment.phoneme_intervals,
                &e,
            );
            all_styles.as_mut().expect("style collection is available").push(RawGlobalStyle {
                start_frame: segment.start,
                end_frame: segment.start + segment.valid,
                heads: std::collections::BTreeMap::from([
                    ("technique_group", style_head(&STYLE_TECHNIQUE_GROUP, &d.styles[0])),
                    ("language", style_head(&STYLE_LANGUAGE, &d.styles[1])),
                    ("gender", style_head(&STYLE_GENDER, &d.styles[2])),
                    ("emotion", style_head(&STYLE_EMOTION, &d.styles[3])),
                    ("method", style_head(&STYLE_METHOD, &d.styles[4])),
                    ("pace", style_head(&STYLE_PACE, &d.styles[5])),
                    ("range", style_head(&STYLE_RANGE, &d.styles[6])),
                ]),
            });
        }

        for boundary in ranges.iter().skip(1).map(|range| segment.start + range.0) {
            all_boundaries.push(boundary);
        }
    }
    stitch_notes(&mut all_notes);
    progress(1.0, "STARS inference complete");
    Ok(RunStarsResult {
        boundary_logits: all_logits,
        boundaries: all_boundaries,
        notes: all_notes,
        techniques: all_techniques,
        styles: all_styles,
    })
}

fn append_techniques(target: &mut Vec<RawTechnique>, segment_start: usize, intervals: &[stars_viterbi::Interval], logits: &[f32]) {
    for (phoneme, interval) in intervals.iter().enumerate() {
        let raw_logits = logits[phoneme * TECHNIQUE_CLASSES..(phoneme + 1) * TECHNIQUE_CLASSES].to_vec();
        let source_local_scores = raw_logits.iter().map(|v| sigmoid_scalar(*v)).collect();
        target.push(RawTechnique {
            start_frame: segment_start + interval.start,
            end_frame: segment_start + interval.end,
            phoneme_id: interval.label,
            raw_logits,
            source_local_scores,
        });
    }
}

fn style_head(taxonomy: &[&'static str], logits: &[f32]) -> RawStyleHead {
    RawStyleHead {
        taxonomy: taxonomy.to_vec(),
        raw_logits: logits.to_vec(),
    }
}

/// Full worker entry point: decode audio at both required sample rates,
/// load both GGUFs, run the shared frontend + RMVPE annotation, then STARS
/// itself, and write the evidence JSON.
#[allow(clippy::too_many_arguments)]
pub fn infer(
    audio_24k: &[f32],
    audio_16k: &[f32],
    words: &[ConfigWord],
    source_start: u64,
    timed_transcript_generation: &str,
    model_generation: &str,
    include_technique: bool,
    stars_model_path: &Path,
    rmvpe_model_path: &Path,
    output_dir: &Path,
    mut progress: impl FnMut(f32, &str, Option<(u64, u64)>),
) -> Result<PathBuf> {
    progress(0.0, "Loading STARS and RMVPE weights", None);
    let stars_weights = StarsWeights::load(stars_model_path)?;
    let rmvpe_weights = RmvpeWeights::load(rmvpe_model_path)?;

    progress(0.05, "Computing shared mel and pitch annotation", None);
    let shared = shared_inputs(audio_24k, audio_16k, &rmvpe_weights)?;

    let result = run_stars(&stars_weights, &shared, words, source_start, include_technique, |fraction, message| {
        progress(0.1 + fraction * 0.85, message, None);
    })?;

    let mut capabilities = vec!["notes.stars"];
    if include_technique {
        capabilities.push("technique.analyze");
    }
    let mut dependencies = vec![
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
        DependencyIdentity {
            kind: "chinese_g2p",
            generation: crate::stars_g2p::ASSET_SHA256.to_string(),
        },
    ];
    dependencies.sort_by(|a, b| a.kind.cmp(b.kind));

    let evidence = StarsEvidence {
        schema_version: 2,
        model_id: "stars",
        capability: None,
        capabilities,
        upstream_commit: STARS_COMMIT,
        checkpoint_sha256: STARS_CHECKPOINT,
        config_sha256: STARS_CONFIG,
        model_generation: model_generation.to_string(),
        runtime_manifest_sha256: RUNTIME_MANIFEST_IDENTITY,
        backend: "ggml_native",
        shared_frontend_profile: singing_frontend::PROFILE,
        shared_frontend_generation: SHARED_MANIFEST_SHA256,
        annotation_rmvpe_sha256: singing_frontend::ANNOTATION_RMVPE_SHA256,
        word_boundary_source: "timed_transcript",
        g2p_profile: Some(crate::stars_g2p::PROFILE),
        frame_step_num: singing_frontend::HOP_SIZE as u32,
        frame_step_den: singing_frontend::SAMPLE_RATE as u32,
        valid_frames: shared.frames,
        note_boundary_logits: result.boundary_logits,
        regulated_note_boundaries: result.boundaries,
        notes: result.notes,
        technique_taxonomy: result.techniques.as_ref().map(|_| TECHNIQUE_TAXONOMY.to_vec()),
        technique_calibration: result.techniques.as_ref().map(|_| "source_local_sigmoid_uncalibrated"),
        techniques: result.techniques,
        style_scope: result.styles.as_ref().map(|_| "segment_global"),
        styles: result.styles,
        dependencies,
    };
    let path = output_dir.join("advanced-note-evidence.json");
    let file = std::fs::File::create(&path)?;
    serde_json::to_writer(std::io::BufWriter::new(file), &evidence)?;
    progress(1.0, "STARS evidence written", None);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_the_real_pinned_checkpoint_gguf_end_to_end() {
        let Ok(path) = std::env::var("UTA_STUDIO_TEST_STARS_GGUF") else {
            return;
        };
        let weights = StarsWeights::load(std::path::Path::new(&path)).unwrap();
        // Touch a representative sample from every submodule to prove the
        // GGUF tensor names line up with what the real checkpoint contains
        // (not just that the file opens).
        assert_eq!(weights.mel_proj_bias.len(), HIDDEN);
        assert_eq!(weights.pitch_embed.len(), 300 * HIDDEN);
        assert_eq!(weights.uv_embed.len(), 3 * HIDDEN);
        assert_eq!(weights.cls_tokens.len(), NUM_CLS_TOKENS * HIDDEN);
        assert_eq!(weights.ph_frame_head.out_dim, 62);
        assert_eq!(weights.note_frame_head.out_dim, 90);
        assert_eq!(weights.pitch_decoder_out.out_dim, PITCH_CLASSES);
        assert_eq!(weights.tech_out.out_dim, TECHNIQUE_CLASSES);
        assert_eq!(weights.prosody_utter.vq_codebook.len(), NVQ * HIDDEN);
        assert_eq!(weights.prosody_ph.cmuencoder.mid.net.layers.len(), 2);
        assert_eq!(weights.prosody_sentence.cmuencoder.mid.net.layers.len(), 1);
        assert_eq!(weights.align_sentence.layers.len(), 2);
    }

    #[test]
    fn stage_a_through_e_run_without_panicking_on_synthetic_input() {
        let Ok(stars_path) = std::env::var("UTA_STUDIO_TEST_STARS_GGUF") else {
            return;
        };
        let Ok(rmvpe_path) = std::env::var("UTA_STUDIO_TEST_RMVPE_GGUF") else {
            return;
        };
        let stars_weights = StarsWeights::load(std::path::Path::new(&stars_path)).unwrap();
        let rmvpe_weights = RmvpeWeights::load(std::path::Path::new(&rmvpe_path)).unwrap();

        // 3 seconds of a synthetic tone at both sample rates -- enough to
        // exercise every stage without needing a real audio fixture yet.
        let make_tone = |sample_rate: usize| -> Vec<f32> {
            (0..sample_rate * 3)
                .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / sample_rate as f32).sin() * 0.2)
                .collect()
        };
        let audio_24k = make_tone(singing_frontend::SAMPLE_RATE);
        let audio_16k = make_tone(mel16::SAMPLE_RATE);
        let words = vec![ConfigWord {
            id: "w0".to_string(),
            text: "你好".to_string(),
            start: 0,
            duration: 1_500_000,
        }];

        let shared = shared_inputs(&audio_24k, &audio_16k, &rmvpe_weights).unwrap();
        let result = run_stars(&stars_weights, &shared, &words, 0, true, |_, _| {}).unwrap();
        assert!(result.techniques.is_some());
        assert!(result.styles.is_some());
        for value in &result.boundary_logits {
            assert!(value.is_finite());
        }
        for note in &result.notes {
            for value in &note.pitch_logits {
                assert!(value.is_finite());
            }
        }
    }
}

#[cfg(test)]
mod pytorch_reference {
    use super::*;

    #[derive(serde::Deserialize)]
    struct FrontendFixture {
        mel_frames: usize,
        mel: Vec<Vec<f32>>,
        annotation_pitch_coarse: Vec<i64>,
        annotation_uv: Vec<i64>,
    }

    #[derive(serde::Deserialize)]
    struct StageADebug {
        mel_proj_out: Vec<Vec<f32>>,
        mel_encoder_out: Vec<Vec<f32>>,
        mel_embed_a0: Vec<Vec<f32>>,
        feat_a1: Vec<Vec<f32>>,
        a2_ph_bd_sigmoid: Vec<f32>,
        a3_ph_frame_logits: Vec<Vec<f32>>,
        prosody_ph_mel: Vec<Vec<f32>>,
        prosody_word_mel: Vec<Vec<f32>>,
        feat_b0: Vec<Vec<f32>>,
        b1_note_bd_logits: Vec<f32>,
        prosody_note_mel: Vec<Vec<f32>>,
        feat_c0: Vec<Vec<f32>>,
        c3_note_logits: Vec<Vec<f32>>,
        note_bd_arr: Vec<i64>,
        mel2ph: Vec<i64>,
        mel2word: Vec<i64>,
        mel2note: Vec<i64>,
        num_ph: usize,
        num_word: usize,
        num_note: usize,
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

    /// Cross-checks Stage A of the native Rust engine against a genuine
    /// PyTorch reference forward pass (using the real checkpoint and the
    /// same padded 256-frame input built from
    /// `fixtures/shared-singing-frontend-upstream.json`'s real upstream mel
    /// + annotation-pitch values). This exercises the shared building
    /// blocks `native-inference/rosvot` already validated this way
    /// (`mel_proj`/`mel_encoder`/`ConvBlocks`, the U-Net+Conformer
    /// backbone) plus STARS-specific ones ROSVOT never exercises: VQ-free
    /// `LocalStyleAdaptor` (`get_prosody_utter`) and
    /// `SinusoidalPositionalEmbedding`.
    #[test]
    fn stage_a_matches_a_genuine_pytorch_reference_forward_pass() {
        let Ok(stars_path) = std::env::var("UTA_STUDIO_TEST_STARS_GGUF") else {
            return;
        };
        let weights = StarsWeights::load(std::path::Path::new(&stars_path)).unwrap();

        let frontend: FrontendFixture =
            serde_json::from_str(include_str!("../fixtures/shared-singing-frontend-upstream.json")).unwrap();
        let reference: StageADebug =
            serde_json::from_str(include_str!("../fixtures/pytorch-reference-stars-stage-a-debug.json")).unwrap();
        let valid = frontend.mel_frames;
        const T: usize = 256;

        let mut mel = vec![0.0_f32; T * singing_frontend::MEL_BINS];
        for (row, frame) in frontend.mel.iter().enumerate().take(valid) {
            mel[row * singing_frontend::MEL_BINS..(row + 1) * singing_frontend::MEL_BINS]
                .copy_from_slice(&frame[..singing_frontend::MEL_BINS]);
        }
        let mut pitch = vec![0_i64; T];
        pitch[..valid].copy_from_slice(&frontend.annotation_pitch_coarse[..valid]);
        let mut uv = vec![0_i64; T];
        uv[..valid].copy_from_slice(&frontend.annotation_uv[..valid]);
        let mut nonpadding = vec![false; T];
        nonpadding[..valid].fill(true);

        let mel_proj_out = conv1d_same(
            &mel,
            T,
            singing_frontend::MEL_BINS,
            &weights.mel_proj_weight,
            Some(&weights.mel_proj_bias),
            HIDDEN,
            3,
            1,
            1,
        );
        let d = max_diff_2d(&mel_proj_out, &reference.mel_proj_out, valid, HIDDEN);
        println!("mel_proj_out diff: {d}");
        assert!(d < 1.0e-3, "mel_proj_out diverged: {d}");

        let mel_encoder_out = conv_blocks(&mel_proj_out, T, &weights.mel_encoder);
        let d = max_diff_2d(&mel_encoder_out, &reference.mel_encoder_out, valid, HIDDEN);
        println!("mel_encoder_out diff: {d}");
        assert!(d < 5.0e-3, "mel_encoder_out diverged: {d}");

        let a = stage_a(&weights, &mel, &pitch, &uv, &nonpadding, T);
        let d = max_diff_2d(&a.mel_embed, &reference.mel_embed_a0, valid, HIDDEN);
        println!("mel_embed (a0) diff: {d}");
        assert!(d < 5.0e-3, "a[0] mel_embed diverged: {d}");

        let d = max_diff_2d(&a.feat, &reference.feat_a1, valid, HIDDEN);
        println!("feat (a1) diff: {d}");
        assert!(d < 1.0e-2, "a[1] feat diverged: {d}");

        let bd_diff = a.ph_bd_sigmoid[..valid]
            .iter()
            .zip(&reference.a2_ph_bd_sigmoid[..valid])
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f32, f32::max);
        println!("ph_bd_sigmoid (a2) diff: {bd_diff}");
        assert!(bd_diff < 1.0e-2, "a[2] ph_bd_sigmoid diverged: {bd_diff}");

        let logits_diff = max_diff_2d(&a.ph_frame_logits, &reference.a3_ph_frame_logits, valid, 61);
        println!("ph_frame_logits (a3) diff: {logits_diff}");
        assert!(logits_diff < 5.0e-2, "a[3] ph_frame_logits diverged: {logits_diff}");
    }
}
