#!/usr/bin/env python3
"""Convert the official ROSVOT PyTorch checkpoint to GGUF (F32).

Source: https://github.com/RickyL-2000/ROSVOT
(commit 3c8332bf43adae35f6e4d64971862f2f6139b310), checkpoint from the
project's published `checkpoints.zip`
(https://drive.google.com/file/d/1JNtNT37KiLq9uFQqHk7JFs-3trxd3bRh),
`checkpoints/rosvot/model.pt`.

Every float32 tensor in `state_dict["state_dict"]["model"]` is written
verbatim under its exact PyTorch key (no renaming, no shape transpose --
tensor shapes are written in native PyTorch row-major order, matching every
other hand-rolled GGUF converter in this repo, e.g.
native-inference/stars/tools/convert_stars_to_gguf.py). `*.num_batches_tracked`
(int64 BatchNorm bookkeeping, unused at inference) is the only kind of
tensor skipped.
"""
import struct
import sys

import numpy as np
import torch

GGUF_TYPE_STRING = 8
GGML_TYPE_F32 = 0
ALIGN = 32
EXPECTED_GLOBAL_STEP = 50_000
EXPECTED_TENSOR_COUNT = 245  # 247 total minus 2 int64 `*.num_batches_tracked` (one per Conformer layer)


def w_str(buf, s):
    b = s.encode("utf-8")
    buf.append(struct.pack("<Q", len(b)))
    buf.append(b)


def w_u32(buf, v):
    buf.append(struct.pack("<I", v))


def w_u64(buf, v):
    buf.append(struct.pack("<Q", v))


def kv_str(buf, key, value):
    w_str(buf, key)
    w_u32(buf, GGUF_TYPE_STRING)
    w_str(buf, value)


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <rosvot/model.pt> <output.gguf>")
        sys.exit(1)
    checkpoint_path, out_path = sys.argv[1], sys.argv[2]

    print(f"Loading checkpoint from {checkpoint_path}...")
    checkpoint = torch.load(checkpoint_path, map_location="cpu", weights_only=False)
    if checkpoint.get("global_step") != EXPECTED_GLOBAL_STEP:
        raise ValueError(f"unexpected global_step {checkpoint.get('global_step')!r}, expected {EXPECTED_GLOBAL_STEP}")
    state_dict = checkpoint["state_dict"]["model"]

    tensors = [
        (name, tensor)
        for name, tensor in state_dict.items()
        if tensor.dtype == torch.float32
    ]
    tensors.sort(key=lambda item: item[0])
    if len(tensors) != EXPECTED_TENSOR_COUNT:
        raise ValueError(f"unexpected ROSVOT checkpoint identity: {len(tensors)} float32 tensors, expected {EXPECTED_TENSOR_COUNT}")
    print(f"Writing {len(tensors)} tensors (F32) to {out_path}...")

    header = [b"GGUF", struct.pack("<I", 3), struct.pack("<Q", len(tensors))]
    kv = []
    kv_str(kv, "general.architecture", "rosvot")
    kv_str(kv, "general.name", "ROSVOT")
    kv_str(kv, "general.description", "Robust Singing Voice Transcription and MIDI Extraction")
    n_kv = 3
    header.append(struct.pack("<Q", n_kv))

    tensor_info = []
    data_blobs = []
    offset = 0
    for name, tensor in tensors:
        arr = tensor.detach().to(torch.float32).numpy().astype(np.float32)
        dims = list(arr.shape) or [1]
        w_str(tensor_info, name)
        w_u32(tensor_info, len(dims))
        for d in dims:
            w_u64(tensor_info, d)
        w_u32(tensor_info, GGML_TYPE_F32)
        w_u64(tensor_info, offset)
        raw = np.ascontiguousarray(arr).tobytes()
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
