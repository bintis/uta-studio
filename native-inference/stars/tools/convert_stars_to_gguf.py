#!/usr/bin/env python3
"""Convert the official STARS PyTorch checkpoint to GGUF (F32).

Source: https://huggingface.co/verstar/STARS
(revision 744a7ad02e1d788452293cd903ea6a933f7862c4,
model_ckpt_steps_200000.ckpt). Algorithm reference:
https://github.com/gwx314/STARS (commit
f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167).

Every float32 tensor in `state_dict["state_dict"]["model"]` is written
verbatim under its exact PyTorch key (no renaming, no shape transpose --
tensor shapes are written in native PyTorch row-major order, matching every
other hand-rolled GGUF converter in this repo, e.g.
native-inference/firered/tools/convert_firered_to_gguf.py). This includes a
few tensors that `native-inference/stars/src/engine.rs` never reads
(`*.vqvae.ema_count`, `*.vqvae.ema_weight`, `*.vqvae.data_initialized`, and
`l1_sentence.*` -- the last is genuinely dead code in the reference's own
`STARS.get_prosody_sentence`, confirmed by reading `modules/stars/stars.py`
directly); leaving them in costs a little disk space but keeps this
converter a simple, low-risk "write everything float" pass rather than a
hand-maintained include list that could silently drop something real.
`*.num_batches_tracked` (int64 BatchNorm bookkeeping, unused at inference)
is the only kind of tensor explicitly skipped.
"""
import struct
import sys

import numpy as np
import torch

GGUF_TYPE_STRING = 8
GGML_TYPE_F32 = 0
ALIGN = 32
EXPECTED_GLOBAL_STEP = 200_000
EXPECTED_TENSOR_COUNT = 1_345  # 1,354 total minus 9 int64 `*.num_batches_tracked` (one per ConformerEncoderLayer: 4 extractors x 2 layers + 1 extractor x 1 layer)


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
        print(f"Usage: {sys.argv[0]} <model_ckpt_steps_200000.ckpt> <output.gguf>")
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
        raise ValueError(f"unexpected STARS checkpoint identity: {len(tensors)} float32 tensors, expected {EXPECTED_TENSOR_COUNT}")
    print(f"Writing {len(tensors)} tensors (F32) to {out_path}...")

    header = [b"GGUF", struct.pack("<I", 3), struct.pack("<Q", len(tensors))]
    kv = []
    kv_str(kv, "general.architecture", "stars")
    kv_str(kv, "general.name", "STARS")
    kv_str(kv, "general.description", "Singing Transcription with Alignment, Rhythm and Style")
    n_kv = 3
    header.append(struct.pack("<Q", n_kv))

    tensor_info = []
    data_blobs = []
    offset = 0
    for name, tensor in tensors:
        arr = tensor.detach().to(torch.float32).numpy().astype(np.float32)
        dims = list(arr.shape) or [1]  # scalars (e.g. vqvae.data_initialized) get one dim
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
