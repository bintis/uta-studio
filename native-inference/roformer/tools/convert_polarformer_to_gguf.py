#!/usr/bin/env python3
"""Convert the canonical ZFTurbo bs_polarformer_float16.ckpt checkpoint into
a GGUF file consumable by native-inference/roformer's public-schema loader
(UtaRoformerGraph::LoadWeights / BuildTransformersGraph, architecture
"bs_polarformer").

Usage:
    python convert_polarformer_to_gguf.py <checkpoint.ckpt> <config.yaml> <output.gguf>

Requires: torch, numpy, pyyaml, gguf (pip install torch numpy pyyaml gguf).

Ground truth for every tensor name/shape below was read directly from the
raw PyTorch state_dict (not the exported ONNX graph, which renames many
tensors during constant-folding). Linear weights are written in their
native PyTorch (out, in) numpy shape; GGUF's convention (last numpy axis ==
ggml ne[0]) then gives ne=[in, out], which is exactly what
ggml_mul_mat(weight, x) expects. No transposing.

The GGUF is written entirely in F32 (not the checkpoint's native FP16):
ggml-cpu's binary ops reject mixed f32/f16 operands (confirmed by an actual
CPU-backend crash: "unsupported types: dst: f32, src0: f32, src1: f16" out
of ggml_mul when RMSNorm gamma was stored as F16), matching the other
production RoFormer GGUFs, which are also F32.
"""
import sys
import torch
import numpy as np
import gguf
import yaml as pyyaml

CKPT_PATH = sys.argv[1]
YAML_PATH = sys.argv[2]
OUT_PATH = sys.argv[3]

with open(YAML_PATH) as f:
    cfg = pyyaml.unsafe_load(f)

model_cfg = cfg["model"]
audio_cfg = cfg["audio"]
infer_cfg = cfg["inference"]

DIM = model_cfg["dim"]
DEPTH = model_cfg["depth"]
HEADS = model_cfg["heads"]
DIM_HEAD = model_cfg["dim_head"]
MASK_DEPTH = model_cfg["mask_estimator_depth"]
MLP_EXPANSION = model_cfg["mlp_expansion_factor"]
FREQS_PER_BANDS = list(model_cfg["freqs_per_bands"])
N_BANDS = len(FREQS_PER_BANDS)
N_FFT = model_cfg["stft_n_fft"]
HOP = model_cfg["stft_hop_length"]
WIN = model_cfg["stft_win_length"]
SAMPLE_RATE = audio_cfg["sample_rate"]
NUM_STEMS = model_cfg["num_stems"]

assert sum(FREQS_PER_BANDS) == N_FFT // 2 + 1, (sum(FREQS_PER_BANDS), N_FFT // 2 + 1)
assert model_cfg["linear_transformer_depth"] == 0
assert model_cfg["use_pope"] is True

sd = torch.load(CKPT_PATH, map_location="cpu", weights_only=False)
print(f"loaded {len(sd)} tensors from checkpoint", file=sys.stderr)


def t(name):
    return sd[name]


def np32(name):
    """F32 numpy array, C-contiguous, upcast from the checkpoint's native fp16."""
    return np.ascontiguousarray(t(name).to(torch.float32).numpy())


writer = gguf.GGUFWriter(OUT_PATH, "bs_polarformer")

# ---- metadata --------------------------------------------------------
kp = "bs_polarformer."
writer.add_uint32(kp + "n_fft", N_FFT)
writer.add_uint32(kp + "hop_length", HOP)
writer.add_uint32(kp + "win_length", WIN)
writer.add_uint32(kp + "dim", DIM)
writer.add_uint32(kp + "n_bands", N_BANDS)
writer.add_uint32(kp + "depth", DEPTH)
writer.add_uint32(kp + "n_stems", NUM_STEMS)
writer.add_uint32(kp + "heads", HEADS)
writer.add_uint32(kp + "dim_head", DIM_HEAD)
writer.add_uint32(kp + "mask_layers", MASK_DEPTH)
writer.add_bool(kp + "has_final_norm", True)
writer.add_bool(kp + "skip_connection", False)
writer.add_bool(kp + "stft_normalized", bool(model_cfg.get("stft_normalized", False)))
writer.add_bool(kp + "zero_dc", False)
writer.add_uint32(kp + "mask_estimator_depth", MASK_DEPTH)
writer.add_uint32(kp + "mlp_expansion_factor", MLP_EXPANSION)
writer.add_uint32(kp + "sample_rate", SAMPLE_RATE)
writer.add_uint32(kp + "chunk_size", infer_cfg["chunk_size"])
writer.add_uint32(kp + "default_num_overlap", infer_cfg["num_overlap"])
writer.add_uint32(kp + "linear_transformer_depth", 0)
# LoadWeights divides band_widths by 4 to recover raw frequency-bin counts.
band_widths = np.array([w * 4 for w in FREQS_PER_BANDS], dtype=np.int32)
writer.add_array(kp + "band_widths", band_widths.tolist())

# ---- PoPE shared buffers ----------------------------------------------
# inv_freqs is tied across every layer and both time/freq transformers
# (verified: layers.0.0...inv_freqs == layers.5.1...inv_freqs, byte-exact).
writer.add_tensor("pope.inv_freqs", np32("layers.0.0.layers.0.0.pope_embed.inv_freqs"))
# k_phase_bias is tied across depth but differs between the time (index 0)
# and freq (index 1) transformer (verified: layers.0.0 == layers.1.0 but
# layers.0.0 != layers.0.1).
# The reference PoPE-pytorch module (lucidrains) applies
# `bias = self.bias.clamp(-2*pi, 0.)` in PoPE.forward() before adding it to
# the phase -- the raw checkpoint parameter is NOT used directly. 499 of
# this checkpoint's 512 (head, channel) values are small positive numbers
# that clamp to exactly 0; verified against the real ONNX Runtime cos_1/
# sin_1 tensors (max abs diff dropped from 0.0646 unclamped to 8e-8 clamped).
pope_clip = lambda name: np.clip(np32(name), -2.0 * np.pi, 0.0)
writer.add_tensor("pope.time_k_phase_bias", pope_clip("layers.0.0.layers.0.0.pope_embed.bias"))
writer.add_tensor("pope.freq_k_phase_bias", pope_clip("layers.0.1.layers.0.0.pope_embed.bias"))

# ---- band split --------------------------------------------------------
for i in range(N_BANDS):
    p = f"band_split.to_features.{i}"
    writer.add_tensor(f"band_split.{i}.norm", np32(f"{p}.0.gamma"))
    writer.add_tensor(f"band_split.{i}.w", np32(f"{p}.1.weight"))
    writer.add_tensor(f"band_split.{i}.b", np32(f"{p}.1.bias"))

# ---- transformer blocks -------------------------------------------------
for layer in range(DEPTH):
    for tf_idx, tf_name in ((0, "time"), (1, "freq")):
        src = f"layers.{layer}.{tf_idx}.layers"
        dst = f"blk.{layer}.{tf_name}"
        writer.add_tensor(f"{dst}.attn_norm", np32(f"{src}.0.0.norm.gamma"))
        writer.add_tensor(f"{dst}.qkv", np32(f"{src}.0.0.to_qkv.weight"))
        writer.add_tensor(f"{dst}.gates_w", np32(f"{src}.0.0.to_gates.weight"))
        writer.add_tensor(f"{dst}.gates_b", np32(f"{src}.0.0.to_gates.bias"))
        writer.add_tensor(f"{dst}.out", np32(f"{src}.0.0.to_out.0.weight"))
        writer.add_tensor(f"{dst}.ff_norm", np32(f"{src}.0.1.net.0.gamma"))
        writer.add_tensor(f"{dst}.ff1_w", np32(f"{src}.0.1.net.1.weight"))
        writer.add_tensor(f"{dst}.ff1_b", np32(f"{src}.0.1.net.1.bias"))
        writer.add_tensor(f"{dst}.ff2_w", np32(f"{src}.0.1.net.4.weight"))
        writer.add_tensor(f"{dst}.ff2_b", np32(f"{src}.0.1.net.4.bias"))

# ---- final norm ----------------------------------------------------------
writer.add_tensor("final_norm", np32("final_norm.gamma"))

# ---- mask estimators -------------------------------------------------
for s in range(NUM_STEMS):
    for b in range(N_BANDS):
        p = f"mask_estimators.{s}.to_freqs.{b}"
        writer.add_tensor(f"mask.{s}.{b}.w1", np32(f"{p}.0.0.weight"))
        writer.add_tensor(f"mask.{s}.{b}.b1", np32(f"{p}.0.0.bias"))
        writer.add_tensor(f"mask.{s}.{b}.w2", np32(f"{p}.0.2.weight"))
        writer.add_tensor(f"mask.{s}.{b}.b2", np32(f"{p}.0.2.bias"))

writer.write_header_to_file()
writer.write_kv_data_to_file()
writer.write_tensors_to_file()
writer.close()
print("wrote", OUT_PATH, file=sys.stderr)
