#!/usr/bin/env python3
"""Convert the JBM555 CE-CTC 80 ONNX model to GGUF format."""
import os
import sys
from pathlib import Path

import numpy as np
import onnx
from onnx import numpy_helper
import gguf

def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <jbm555.onnx> <output.gguf>")
        sys.exit(1)

    onnx_path = sys.argv[1]
    out_path = sys.argv[2]

    print(f"Loading ONNX from {onnx_path}...")
    model = onnx.load(onnx_path)

    initializers = {}
    for init in model.graph.initializer:
        initializers[init.name] = numpy_helper.to_array(init).astype(np.float32)

    writer = gguf.GGUFWriter(out_path, "jbm555")
    writer.add_name("JBM555 CE-CTC 80")
    writer.add_description("JBM555 Japanese Note and Pitch-Class Estimator")
    writer.add_uint32("sample_rate", 44100)
    writer.add_uint32("hop_size", 1024)
    writer.add_uint32("bins", 384)
    writer.add_uint32("channels", 6)

    # Tensor mapping table
    # ONNX initializer -> GGUF tensor name
    tensor_map = {
        "model.onset_cnn.conv1.0.weight": "onset_cnn.conv1.weight",
        "model.onset_cnn.conv1.0.bias": "onset_cnn.conv1.bias",
        "model.onset_cnn.conv2.0.weight": "onset_cnn.conv2.weight",
        "model.onset_cnn.conv2.0.bias": "onset_cnn.conv2.bias",
        "model.onset_cnn.conv3.0.weight": "onset_cnn.conv3.weight",
        "model.onset_cnn.conv3.0.bias": "onset_cnn.conv3.bias",
        "model.onset_cnn.conv4.0.weight": "onset_cnn.conv4.weight",
        "model.onset_cnn.conv4.0.bias": "onset_cnn.conv4.bias",
        "model.onset_cnn.conv5.0.weight": "onset_cnn.conv5.weight",
        "model.onset_cnn.conv5.0.bias": "onset_cnn.conv5.bias",
        "onnx::MatMul_115": "onset_cnn.fc1.weight",
        "model.onset_cnn.fc1.0.bias": "onset_cnn.fc1.bias",
        "onnx::MatMul_116": "onset_cnn.fc2.weight",
        "model.onset_cnn.fc2.0.bias": "onset_cnn.fc2.bias",
        "onnx::MatMul_117": "onset_cnn.fc3.weight",
        "model.onset_cnn.fc3.0.bias": "onset_cnn.fc3.bias",

        "model.pitch_cnn.conv1.0.weight": "pitch_cnn.conv1.weight",
        "model.pitch_cnn.conv1.0.bias": "pitch_cnn.conv1.bias",
        "model.pitch_cnn.conv2.0.weight": "pitch_cnn.conv2.weight",
        "model.pitch_cnn.conv2.0.bias": "pitch_cnn.conv2.bias",
        "model.pitch_cnn.conv3.0.weight": "pitch_cnn.conv3.weight",
        "model.pitch_cnn.conv3.0.bias": "pitch_cnn.conv3.bias",
        "model.pitch_cnn.conv4.0.weight": "pitch_cnn.conv4.weight",
        "model.pitch_cnn.conv4.0.bias": "pitch_cnn.conv4.bias",
        "model.pitch_cnn.conv5.0.weight": "pitch_cnn.conv5.weight",
        "model.pitch_cnn.conv5.0.bias": "pitch_cnn.conv5.bias",
        "onnx::MatMul_118": "pitch_cnn.fc1.weight",
        "model.pitch_cnn.fc1.0.bias": "pitch_cnn.fc1.bias",
        "onnx::MatMul_119": "pitch_cnn.fc2.weight",
        "model.pitch_cnn.fc2.0.bias": "pitch_cnn.fc2.bias",
        "onnx::MatMul_120": "pitch_cnn.fc3.weight",
        "model.pitch_cnn.fc3.0.bias": "pitch_cnn.fc3.bias",
    }

    print(f"Writing {len(tensor_map)} tensors to {out_path}...")
    for onnx_name, gguf_name in tensor_map.items():
        if onnx_name not in initializers:
            raise ValueError(f"Missing expected initializer: {onnx_name}")
        arr = initializers[onnx_name]
        # In ONNX MatMul: input @ weight -> shape is [in_features, out_features]
        # For standard linear in GGML or PyTorch convention (out_features, in_features):
        if arr.ndim == 2:
            arr = arr.T
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
