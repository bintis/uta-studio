#!/usr/bin/env python3
"""Convert the official FireRedASR2-AED PyTorch checkpoint to GGUF (F32).

Source: https://huggingface.co/FireRedTeam/FireRedASR2-AED (Apache-2.0,
official FireRedTeam release -- NOT the third-party INT8 ONNX export used by
the OpenVINO route). Using the original checkpoint avoids needing to
reimplement ONNX Runtime's dynamic INT8 quantization (DynamicQuantizeLinear +
MatMulInteger) to get a faithful port.

F32, not FP16: an FP16 conversion was tried first (half the GGUF size), but
real-audio validation against the canonical `hello_zh.wav` ("你好世界")
fixture showed FP16 rounding compounding across the 16-layer Conformer
encoder was enough to flip a close greedy-decoding decision at the 3rd
token, producing a real wrong transcript ("你好师姐") -- confirmed by
diffing against a genuine PyTorch F32 reference forward pass, which matched
exactly. Disk space is not actually constrained here (~4.7GB, same as the
source checkpoint), so there is no real tradeoff for keeping F32.

Only the `encoder.*` and `decoder.*` state dict is needed: CTC
(`ctc.ctc_lo.*`) is present in the checkpoint but is not used by this
model's integration (see native-inference/openvino-worker/src/firered.rs --
its own CTC call is a shape/finite-ness validation only, never consumed for
the transcript; greedy transformer-decoder output is the sole source of
text), so it is intentionally excluded here.
"""
import struct
import sys

import numpy as np
import torch

EXPECTED_ARGS = {
    "d_model": 1280,
    "n_head": 20,
    "n_layers_enc": 16,
    "n_layers_dec": 16,
    "kernel_size": 33,
    "idim": 80,
    "odim": 8667,
    "sos_id": 3,
    "eos_id": 4,
    "pad_id": 2,
    "blank_id": 0,
    "subsample": 4,
    "pe_maxlen": 5000,
}

GGUF_TYPE_UINT32 = 4
GGUF_TYPE_STRING = 8
GGML_TYPE_F32 = 0
ALIGN = 32


def w_str(buf, s):
    b = s.encode("utf-8")
    buf.append(struct.pack("<Q", len(b)))
    buf.append(b)


def w_u32(buf, v):
    buf.append(struct.pack("<I", v))


def w_u64(buf, v):
    buf.append(struct.pack("<Q", v))


def kv_u32(buf, key, value):
    w_str(buf, key)
    w_u32(buf, GGUF_TYPE_UINT32)
    w_u32(buf, value)


def kv_str(buf, key, value):
    w_str(buf, key)
    w_u32(buf, GGUF_TYPE_STRING)
    w_str(buf, value)


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <model.pth.tar> <output.gguf>")
        sys.exit(1)
    checkpoint_path, out_path = sys.argv[1], sys.argv[2]

    print(f"Loading checkpoint from {checkpoint_path}...")
    package = torch.load(checkpoint_path, map_location="cpu", weights_only=False)
    args = vars(package["args"])
    for key, expected in EXPECTED_ARGS.items():
        actual = args.get(key)
        if actual != expected:
            raise ValueError(f"unexpected args.{key} = {actual!r}, expected {expected!r}")

    state_dict = package["model_state_dict"]
    tensors = [
        (name, tensor)
        for name, tensor in state_dict.items()
        if not name.startswith("ctc.")
    ]
    tensors.sort(key=lambda item: item[0])
    print(f"Writing {len(tensors)} tensors (F32) to {out_path}...")

    header = [b"GGUF", struct.pack("<I", 3), struct.pack("<Q", len(tensors))]
    kv = []
    kv_str(kv, "general.architecture", "firered_asr2_aed")
    kv_str(kv, "general.name", "FireRedASR2-AED")
    kv_str(kv, "general.description", "Conformer encoder + Transformer decoder AED speech recognizer")
    for key in EXPECTED_ARGS:
        kv_u32(kv, key, EXPECTED_ARGS[key])
    n_kv = 3 + len(EXPECTED_ARGS)  # 3 general.* string entries + all args
    header.append(struct.pack("<Q", n_kv))

    tensor_info = []
    data_blobs = []
    offset = 0
    for name, tensor in tensors:
        arr = tensor.detach().to(torch.float32).numpy().astype(np.float32)
        dims = list(arr.shape)
        w_str(tensor_info, name)
        w_u32(tensor_info, len(dims))
        for d in dims:
            w_u64(tensor_info, d)
        w_u32(tensor_info, GGML_TYPE_F32)
        w_u64(tensor_info, offset)
        raw = arr.tobytes()
        pad = (ALIGN - len(raw) % ALIGN) % ALIGN
        data_blobs.append(raw + b"\x00" * pad)
        offset += len(raw) + pad

    pre_data = b"".join(header) + b"".join(kv) + b"".join(tensor_info)
    pad = (ALIGN - len(pre_data) % ALIGN) % ALIGN
    with open(out_path, "wb") as f:
        f.write(pre_data)
        f.write(b"\x00" * pad)
        for blob in data_blobs:
            f.write(blob)

    import os

    size = os.path.getsize(out_path)
    print(f"Successfully generated {out_path} ({size} bytes, {len(tensors)} tensors)")


if __name__ == "__main__":
    main()
