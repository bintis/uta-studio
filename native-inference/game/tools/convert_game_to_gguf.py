#!/usr/bin/env python3
"""Convert the official GAME 1.0.3 medium ONNX directory to one F32 GGUF.

Usage:
    python convert_game_to_gguf.py EXTRACTED_ONNX_DIRECTORY OUTPUT.gguf

The source directory must contain the official encoder.onnx, segmenter.onnx,
estimator.onnx, and config.json files from GAME-1.0.3-medium-onnx.zip.
Requires: onnx, numpy, gguf.

The three ONNX graphs were exported from one PyTorch state dictionary. Named
initializers retain their ``model.model.`` state-dict names. PyTorch Linear
weights exported as anonymous ONNX MatMul operands are recovered from the
consumer node's hierarchical name and transposed from [in, out] back to
[out, in]. Conv1d, embeddings, biases, RMSNorm scales, and the learned pool
token already use their native state-dict layout. All tensors remain F32; this
first native route intentionally performs no quality-affecting quantization.

This tool validates the model's structural conversion contract rather than
using content hashes as an acceptance gate. Runtime Manager records source and
converted provenance independently when the resulting artifact is imported.
"""

import atexit
import json
import os
import sys
from collections import defaultdict
from pathlib import Path

import gguf
import numpy as np
import onnx
from onnx import numpy_helper

SOURCE_FILES = ("encoder.onnx", "segmenter.onnx", "estimator.onnx", "config.json")
EXPECTED_TENSORS = {"encoder.onnx": 135, "segmenter.onnx": 267, "estimator.onnx": 242}
EXPECTED_GRAPH_INPUTS = {
    "encoder.onnx": ("waveform", "duration"),
    "segmenter.onnx": (
        "x_seg",
        "language",
        "known_boundaries",
        "prev_boundaries",
        "t",
        "maskT",
        "threshold",
        "radius",
    ),
    "estimator.onnx": ("x_est", "boundaries", "maskT", "maskN", "threshold"),
}
EXPECTED_CONFIG = {
    "samplerate": 44_100,
    "timestep": 0.01,
    "languages": {"en": 1, "ja": 2, "yue": 3, "zh": 4},
    "loop": True,
    "embedding_dim": 256,
}

if len(sys.argv) != 3:
    raise SystemExit(f"usage: {sys.argv[0]} EXTRACTED_ONNX_DIRECTORY OUTPUT.gguf")

source_dir = Path(sys.argv[1])
output_path = Path(sys.argv[2])
temporary_path = output_path.with_name(f".{output_path.name}.tmp-{os.getpid()}")

if not source_dir.is_dir():
    raise SystemExit(f"GAME ONNX source directory is unavailable: {source_dir}")
for name in SOURCE_FILES:
    if not (source_dir / name).is_file():
        raise SystemExit(f"GAME ONNX source file is unavailable: {name}")
if output_path.exists() or output_path.is_symlink():
    raise SystemExit(f"refusing to overwrite existing output: {output_path}")
if temporary_path.exists() or temporary_path.is_symlink():
    raise SystemExit(f"temporary output already exists: {temporary_path}")


def cleanup_temporary():
    try:
        temporary_path.unlink()
    except FileNotFoundError:
        pass


atexit.register(cleanup_temporary)

config = json.loads((source_dir / "config.json").read_text(encoding="utf-8"))
if config != EXPECTED_CONFIG:
    raise ValueError("GAME config does not match the 1.0.3 medium conversion contract")

writer = gguf.GGUFWriter(str(temporary_path), "game-me")
written = set()
converted_arrays = {}


def write_tensor(name, array):
    if name in written:
        raise ValueError(f"duplicate converted tensor name: {name}")
    array = np.ascontiguousarray(array, dtype=np.float32)
    if not array.size or not np.isfinite(array).all():
        raise ValueError(f"GAME tensor is empty or non-finite: {name}")
    written.add(name)
    converted_arrays[name] = array
    writer.add_tensor(name, array)


def canonical_node_prefix(node_name, operation):
    suffix = f"/{operation}"
    if not node_name.startswith("/") or not node_name.endswith(suffix):
        raise ValueError(f"unexpected GAME {operation} node name: {node_name}")
    prefix = node_name[1 : -len(suffix)].replace("/", ".")
    # The exporter nests the Sequential module's own state-dict name below a
    # wrapper with the same name; GGUF uses the underlying state-dict identity.
    if prefix.startswith("time_embedding.time_embedding."):
        prefix = "time_embedding." + prefix.removeprefix("time_embedding.time_embedding.")
    return prefix


def convert_graph(filename):
    model = onnx.load(source_dir / filename, load_external_data=True)
    graph_inputs = tuple(value.name for value in model.graph.input)
    if graph_inputs != EXPECTED_GRAPH_INPUTS[filename]:
        raise ValueError(f"unexpected GAME graph inputs for {filename}: {graph_inputs}")

    uses = defaultdict(list)
    for node in model.graph.node:
        for index, value_name in enumerate(node.input):
            uses[value_name].append((node, index))

    converted = 0
    for initializer in model.graph.initializer:
        if initializer.data_type != onnx.TensorProto.FLOAT:
            continue
        source_name = initializer.name
        destination_name = None
        array = None

        if source_name.startswith("model.model."):
            destination_name = source_name.removeprefix("model.model.")
            array = numpy_helper.to_array(initializer)
        else:
            consumers = uses[source_name]
            if len(consumers) == 1:
                node, input_index = consumers[0]
                if node.op_type == "MatMul" and input_index == 1:
                    if node.name != "/to_spectrogram/MatMul":
                        destination_name = canonical_node_prefix(node.name, "MatMul") + ".weight"
                        array = numpy_helper.to_array(initializer).T
                elif node.op_type == "Mul" and "/lay_scale" in node.name:
                    destination_name = canonical_node_prefix(node.name, "Mul") + ".scale"
                    array = numpy_helper.to_array(initializer).reshape(-1)
                elif node.op_type == "Expand" and node.name == "/estimator/pool_token_gen/Expand":
                    destination_name = "estimator.pool_token_gen.emb"
                    array = numpy_helper.to_array(initializer).reshape(1, -1)

        if destination_name is not None:
            write_tensor(destination_name, array)
            converted += 1

    if converted != EXPECTED_TENSORS[filename]:
        raise ValueError(
            f"unexpected GAME graph identity for {filename}: "
            f"converted {converted}, expected {EXPECTED_TENSORS[filename]} tensors"
        )


for graph_filename in ("encoder.onnx", "segmenter.onnx", "estimator.onnx"):
    convert_graph(graph_filename)

# ONNX graph simplification prunes the final estimator layer's frame-stream
# output projection and subsequent per-frame post-attention / FFN operations
# because only the pool stream reaches the deployed score outputs.
# The reference native loader still binds those shapes even though the
# computed frame output is discarded after the final layer. Reusing corresponding
# layer 2 / pool weights supplies the structurally required, semantically dead tensors;
# they cannot influence presence or pitch scores.
PRUNED_L3_SUFFIXES = (
    "attn.jattn.x_out.weight",
    "attn.jattn.x_out.bias",
    "attn.c_norm_x.weight",
    "attn.c_x.dw.bias",
    "attn.c_x.dw.weight",
    "attn.c_x.norm.weight",
    "attn.c_x.pw1.bias",
    "attn.c_x.pw1.weight",
    "attn.c_x.pw2.bias",
    "attn.c_x.pw2.weight",
    "attn.merge_dw_conv_x.bias",
    "attn.merge_dw_conv_x.weight",
    "attn.merge_linear_x.bias",
    "attn.merge_linear_x.weight",
    "ffn2_x.ln1.bias",
    "ffn2_x.ln1.weight",
    "ffn2_x.ln2.bias",
    "ffn2_x.ln2.weight",
    "lay_scale_ffn2_x.scale",
    "lay_scale_jpac_x.scale",
    "norm_ffn2_x.weight",
)

for suffix in PRUNED_L3_SUFFIXES:
    target_name = f"estimator.layers.3.{suffix}"
    if target_name not in written:
        if suffix == "attn.jattn.x_out.weight":
            source_array = converted_arrays["estimator.layers.3.attn.jattn.pool_out.weight"]
        elif suffix == "attn.jattn.x_out.bias":
            source_array = converted_arrays["estimator.layers.3.attn.jattn.pool_out.bias"]
        else:
            source_array = converted_arrays[f"estimator.layers.2.{suffix}"]
        write_tensor(target_name, source_array)

PRUNED_OUTPUT_TENSORS = (
    ("estimator.output_norm_x.weight", "estimator.output_norm_pool.weight"),
    ("estimator.output_proj_x.weight", "estimator.output_proj_pool.weight"),
    ("estimator.output_proj_x.bias", "estimator.output_proj_pool.bias"),
)

for target_name, source_name in PRUNED_OUTPUT_TENSORS:
    if target_name not in written:
        write_tensor(target_name, converted_arrays[source_name])

expected_total = (
    sum(EXPECTED_TENSORS.values()) + len(PRUNED_L3_SUFFIXES) + len(PRUNED_OUTPUT_TENSORS)
)
if len(written) != expected_total:
    raise ValueError(f"unexpected GAME tensor count: {len(written)}")

# Metadata contract consumed by the pinned GAME native implementation.
writer.add_string("general.name", "GAME 1.0.3 medium")
writer.add_string("general.version", "1.0.3")
writer.add_string("game.source.repository", "https://github.com/openvpi/GAME.git")
writer.add_string("game.source.commit", "475a8ee781fe8cca980b3b12fbe6c80c768a813a")
writer.add_string("game.source.asset", "GAME-1.0.3-medium-onnx.zip")
writer.add_string("game.source.license", "CC-BY-NC-SA-4.0")

writer.add_string("game.model.mode", "d3pm")
writer.add_uint32("game.model.embedding_dim", 256)
writer.add_uint32("game.model.in_dim", 80)
writer.add_uint32("game.model.estimator_out_dim", 257)
writer.add_uint32("game.model.region_cycle_len", 3)
writer.add_bool("game.model.use_languages", True)
writer.add_uint32("game.model.num_languages", 127)


def add_common_backbone(section, cls, layers):
    prefix = f"game.{section}."
    writer.add_string(prefix + "cls", cls)
    writer.add_uint32(prefix + "dim", 256)
    writer.add_uint32(prefix + "num_layers", layers)
    writer.add_uint32(prefix + "num_heads", 8)
    writer.add_uint32(prefix + "head_dim", 64)
    writer.add_string(prefix + "ffn_type", "glu")
    writer.add_bool(prefix + "use_ls", True)
    writer.add_bool(prefix + "use_out_norm", True)
    writer.add_bool(prefix + "skip_first_ffn", False)
    writer.add_bool(prefix + "skip_out_ffn", False)


for section, layers in (("encoder", 4), ("segmenter", 8)):
    add_common_backbone(section, "modules.backbones.EBF.EBFBackbone", layers)
    writer.add_uint32(f"game.{section}.c_kernel_size", 31)
    writer.add_uint32(f"game.{section}.m_kernel_size", 31)
# The deployment ONNX graph does not export the training-only latent head;
# omitting these optional keys keeps the native inference graph identical.

add_common_backbone(
    "estimator", "modules.backbones.ebf_with_joint_attention.JEBFBackbone", 4
)
writer.add_uint32("game.estimator.region_token_num", 1)
writer.add_string("game.estimator.pool_merge_mode", "mean")
writer.add_string("game.estimator.attn_type", "joint")
writer.add_string("game.estimator.rope_mode", "mixed")
writer.add_bool("game.estimator.qk_norm", True)
writer.add_bool("game.estimator.use_region_bias", False)
writer.add_uint32("game.estimator.c_kernel_size_pool", 7)
writer.add_uint32("game.estimator.m_kernel_size_pool", 5)
writer.add_uint32("game.estimator.c_kernel_size_x", 31)
writer.add_uint32("game.estimator.m_kernel_size_x", 31)
writer.add_bool("game.estimator.use_rope", True)
writer.add_bool("game.estimator.use_pool_offset", False)
writer.add_float32("game.estimator.theta", 10_000.0)

writer.add_uint32("game.inference.audio_sample_rate", 44_100)
writer.add_uint32("game.inference.hop_size", 441)
writer.add_uint32("game.inference.fft_size", 2_048)
writer.add_uint32("game.inference.win_size", 2_048)
writer.add_string("game.inference.spectrogram.type", "mel")
writer.add_uint32("game.inference.spectrogram.num_bins", 80)
writer.add_float32("game.inference.spectrogram.fmin", 0.0)
writer.add_float32("game.inference.spectrogram.fmax", 8_000.0)
writer.add_float32("game.inference.midi_min", 0.0)
writer.add_float32("game.inference.midi_max", 128.0)
writer.add_uint32("game.inference.midi_num_bins", 257)
writer.add_float32("game.inference.midi_std", 0.5)
writer.add_string(
    "game.inference.lang_map",
    json.dumps(config["languages"], ensure_ascii=True, separators=(",", ":")),
)

writer.write_header_to_file()
writer.write_kv_data_to_file()
writer.write_tensors_to_file()
writer.close()
# Same-directory hard-link publication is atomic and cannot replace a target
# created after the initial existence check.
os.link(temporary_path, output_path)
temporary_path.unlink()
print(f"wrote {output_path} ({len(written)} F32 tensors)", file=sys.stderr)
