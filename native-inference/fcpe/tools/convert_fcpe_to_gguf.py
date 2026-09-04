#!/usr/bin/env python3
"""Convert FCPE ONNX model to GGUF format."""
import os
import sys
from pathlib import Path

import numpy as np
import onnx
from onnx import numpy_helper
import gguf

def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <fcpe.onnx> <output.gguf>")
        sys.exit(1)

    onnx_path = sys.argv[1]
    out_path = sys.argv[2]

    print(f"Loading ONNX from {onnx_path}...")
    model = onnx.load(onnx_path)

    initializers = {}
    for init in model.graph.initializer:
        initializers[init.name] = numpy_helper.to_array(init).astype(np.float32)

    writer = gguf.GGUFWriter(out_path, "fcpe")
    writer.add_name("FCPE")
    writer.add_description("Fast Context-base Pitch Estimation")
    writer.add_uint32("sample_rate", 16000)
    writer.add_uint32("hop_size", 160)
    writer.add_uint32("window_size", 32000)
    writer.add_uint32("n_frames", 201)
    writer.add_uint32("n_mel_bins", 128)

    # Standard tensor naming
    tensor_map = {
        "model.model.input_stack.0.weight": "input_stack.0.weight",
        "model.model.input_stack.0.bias": "input_stack.0.bias",
        "model.model.input_stack.3.weight": "input_stack.1.weight",
        "model.model.input_stack.3.bias": "input_stack.1.bias",
        "model.model.norm.weight": "norm.weight",
        "model.model.norm.bias": "norm.bias",
        "onnx::MatMul_334": "output_proj.weight",
        "model.model.output_proj.bias": "output_proj.bias",
        "onnx::Expand_336": "cents_mapping",
        "onnx::Mul_329": "mel_scale",
        "onnx::Add_330": "mel_bias",
    }

    # Add 6 conformer layers
    for i in range(6):
        prefix = f"model.model.net.encoder_layers.{i}.conformer.net"
        target = f"encoder_layers.{i}"
        tensor_map[f"{prefix}.0.weight"] = f"{target}.norm.weight"
        tensor_map[f"{prefix}.0.bias"] = f"{target}.norm.bias"
        tensor_map[f"{prefix}.2.weight"] = f"{target}.fc1.weight"
        tensor_map[f"{prefix}.2.bias"] = f"{target}.fc1.bias"
        tensor_map[f"{prefix}.4.conv.weight"] = f"{target}.conv.weight"
        tensor_map[f"{prefix}.4.conv.bias"] = f"{target}.conv.bias"
        tensor_map[f"{prefix}.6.weight"] = f"{target}.fc2.weight"
        tensor_map[f"{prefix}.6.bias"] = f"{target}.fc2.bias"

    print(f"Writing {len(tensor_map)} tensors to {out_path}...")
    for onnx_name, gguf_name in tensor_map.items():
        if onnx_name not in initializers:
            raise ValueError(f"Missing initializer: {onnx_name}")
        arr = initializers[onnx_name]
        # Clean 1D squeezes if needed
        if gguf_name in ("cents_mapping", "mel_scale", "mel_bias"):
            arr = arr.squeeze()
        writer.add_tensor(gguf_name, arr)
        print(f"  {gguf_name}: shape={arr.shape}")

    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()
    size = os.path.getsize(out_path)
    print(f"Successfully generated {out_path} ({size} bytes, {len(tensor_map)} tensors)")

if __name__ == "__main__":
    main()
