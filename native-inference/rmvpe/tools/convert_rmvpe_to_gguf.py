#!/usr/bin/env python3
"""Convert the pinned RMVPE ONNX artifact (HuggingFace lj1995/VoiceConversionWebUI
revision e6d0c1a17da07c33557852f9dfa2bd44cc75737d, rmvpe.onnx; catalog
content identity 5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd)
into a GGUF
file for native-inference/rmvpe's GGML/Vulkan graph (architecture "rmvpe").

Usage:
    python convert_rmvpe_to_gguf.py <rmvpe.onnx> <output.gguf>

Requires: onnx, numpy, gguf (pip install onnx numpy gguf).

Ground truth for every tensor's identity and shape was read directly from the
ONNX graph (not a separate .pt checkpoint -- there is none pinned in this
repo, and the model's recurrence is a native ONNX GRU op with unambiguous
W/R/B layout, so the ONNX export is a clean, direct source; see the RMVPE
native graph's GRU implementation). Encoder/decoder Conv/BatchNormalization
tensors mostly have auto-generated ONNX initializer names (`onnx::Conv_1234`)
rather than meaningful ones, but every node's OWN `.name` field already
encodes its full hierarchical path (e.g.
`/unet/encoder/layers.0/conv.0/conv/conv.0/Conv`) -- so tensor names below are
derived from the producing node's name, not the initializer's name, giving
clean/traceable names without hand-mapping 124 conv layers individually.

Linear/GRU weights are written in their native (out, ..., in) numpy shape;
GGUF's convention (ggml ne[i] = numpy shape[-(i+1)]) then gives ne=[in, out],
matching ggml_mul_mat(weight, x) with no transpose -- same convention as
native-inference/roformer/tools/convert_polarformer_to_gguf.py. The ONE
exception is `fc.1`'s weight: it is exported as a raw ONNX MatMul operand
with shape [512, 360] = [in, out] (not PyTorch nn.Linear's native [out, in]
layout -- this is an artifact of how the exporter represented that specific
op, confirmed by reading the initializer's actual shape, not assumed from
the general Linear convention). Left as-is, the standard axis-reversal would
produce ne=[360, 512] = [out, in], which is backwards for ggml_mul_mat; it is
explicitly transposed to [360, 512] = [out, in] in numpy terms before writing
so GGUF's reversal yields the correct ne=[512, 360] = [in, out].

Conv2d/ConvTranspose2d weights need no transpose: PyTorch/ONNX shape
(out_ch, in_ch, kH, kW) reverses to ggml ne=[kW, kH, in_ch, out_ch], exactly
what ggml_conv_2d/ggml_conv_transpose_2d expect.

All values are written F32 (the ONNX initializers are already F32; ggml-cpu's
binary ops reject mixed f32/f16 operands, matching every other production
GGUF in this repo).
"""
import atexit
import os
import sys
from pathlib import Path

import numpy as np
import onnx
from onnx import numpy_helper
import gguf

if len(sys.argv) != 3:
    raise SystemExit(f"usage: {sys.argv[0]} <rmvpe.onnx> <output.gguf>")

ONNX_PATH = sys.argv[1]
OUT_PATH = Path(sys.argv[2])
TEMP_PATH = OUT_PATH.with_name(f".{OUT_PATH.name}.tmp-{os.getpid()}")
if OUT_PATH.exists() or OUT_PATH.is_symlink():
    raise SystemExit(f"refusing to overwrite existing output: {OUT_PATH}")
if TEMP_PATH.exists() or TEMP_PATH.is_symlink():
    raise SystemExit(f"temporary output already exists: {TEMP_PATH}")


def cleanup_temporary():
    try:
        TEMP_PATH.unlink()
    except FileNotFoundError:
        pass


atexit.register(cleanup_temporary)
model = onnx.load(ONNX_PATH, load_external_data=True)
graph = model.graph

init_by_name = {t.name: t for t in graph.initializer}


def arr32(name):
    """F32 numpy array, C-contiguous, for the named initializer."""
    return np.ascontiguousarray(numpy_helper.to_array(init_by_name[name]).astype(np.float32))


def canonical_name(node):
    """Derive a clean GGUF tensor-name prefix from the producing node's own
    name, e.g. '/unet/encoder/layers.0/conv.0/conv/conv.0/Conv' ->
    'unet.encoder.layers.0.conv.0.conv.conv.0'."""
    name = node.name
    if name.startswith("/"):
        name = name[1:]
    suffix = "/" + node.op_type
    if name.endswith(suffix):
        name = name[: -len(suffix)]
    return name.replace("/", ".")


writer = gguf.GGUFWriter(str(TEMP_PATH), "rmvpe")
written = set()


def write_once(name, array):
    if name in written:
        raise ValueError(f"duplicate tensor name {name}")
    written.add(name)
    writer.add_tensor(name, array)


conv_count = 0
conv_transpose_count = 0
bn_count = 0

for node in graph.node:
    if node.op_type == "Conv":
        base = canonical_name(node)
        write_once(f"{base}.weight", arr32(node.input[1]))
        if len(node.input) > 2 and node.input[2]:
            write_once(f"{base}.bias", arr32(node.input[2]))
        conv_count += 1
    elif node.op_type == "ConvTranspose":
        base = canonical_name(node)
        write_once(f"{base}.weight", arr32(node.input[1]))
        if len(node.input) > 2 and node.input[2]:
            write_once(f"{base}.bias", arr32(node.input[2]))
        conv_transpose_count += 1
    elif node.op_type == "BatchNormalization":
        base = canonical_name(node)
        # inputs: [x, scale, bias, running_mean, running_var]
        write_once(f"{base}.weight", arr32(node.input[1]))
        write_once(f"{base}.bias", arr32(node.input[2]))
        write_once(f"{base}.running_mean", arr32(node.input[3]))
        write_once(f"{base}.running_var", arr32(node.input[4]))
        bn_count += 1
    elif node.op_type == "GRU":
        write_once("gru.weight_ih", arr32(node.input[1]))  # W [2,768,384]
        write_once("gru.weight_hh", arr32(node.input[2]))  # R [2,768,256]
        write_once("gru.bias", arr32(node.input[3]))       # B [2,1536]
    elif node.op_type == "MatMul" and node.name == "/fc/fc.1/MatMul":
        w = arr32(node.input[1])  # ONNX shape [512,360] = [in,out]
        assert w.shape == (512, 360), w.shape
        write_once("fc.1.weight", np.ascontiguousarray(w.T))  # -> [360,512]=[out,in] numpy

# The cnn head (final 16->3 channel conv, node name '/cnn/Conv') is already
# written by the generic Conv loop above (canonical name 'cnn'). Only fc.1's
# bias needs adding explicitly -- it comes from a separate Add node, not the
# MatMul node whose canonical name gave us fc.1.weight above.
write_once("fc.1.bias", arr32("fc.1.bias"))

if len(written) != 282:
    raise ValueError(
        "unexpected RMVPE graph identity: "
        f"Conv={conv_count}, ConvTranspose={conv_transpose_count}, "
        f"BatchNormalization={bn_count}, tensors={len(written)}"
    )
print(
    f"wrote {conv_count} Conv, {conv_transpose_count} ConvTranspose, "
    f"{bn_count} BatchNormalization, GRU, fc.1 ({len(written)} tensors total)",
    file=sys.stderr,
)

# ---- architecture metadata --------------------------------------------
kp = "rmvpe."
writer.add_uint32(kp + "sample_rate", 16_000)
writer.add_uint32(kp + "n_fft", 1_024)
writer.add_uint32(kp + "hop_length", 160)
writer.add_uint32(kp + "mel_bins", 128)
writer.add_uint32(kp + "pitch_classes", 360)
writer.add_uint32(kp + "gru_input_size", 384)
writer.add_uint32(kp + "gru_hidden_size", 256)
writer.add_bool(kp + "gru_bidirectional", True)
writer.add_bool(kp + "gru_linear_before_reset", True)
writer.add_uint32(kp + "cnn_head_out_channels", 3)
writer.add_uint32(kp + "encoder_stages", 5)
writer.add_array(kp + "encoder_channels", [16, 32, 64, 128, 256])
writer.add_uint32(kp + "bottleneck_stages", 4)
writer.add_uint32(kp + "bottleneck_channels", 512)
writer.add_uint32(kp + "decoder_stages", 5)
writer.add_array(kp + "decoder_channels", [256, 128, 64, 32, 16])
writer.add_uint32(kp + "blocks_per_stage", 4)

writer.write_header_to_file()
writer.write_kv_data_to_file()
writer.write_tensors_to_file()
writer.close()
# A same-directory hard-link publishes atomically and fails rather than
# replacing a target created after the initial check.
os.link(TEMP_PATH, OUT_PATH)
TEMP_PATH.unlink()
print("wrote", OUT_PATH, file=sys.stderr)
