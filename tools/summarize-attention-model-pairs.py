#!/usr/bin/env python3
"""Compare recorded model runs without rerunning inference or altering artifacts."""
import argparse
import array
import json
import math
import re
import struct
import sys
from pathlib import Path

ATTENTION = re.compile(r"FLASH_ATTN_EXT dst\(([^)]+)\).*: (\d+) x ([0-9.]+) us")
FLOAT_SUBFORMAT = bytes.fromhex("0300000000001000800000aa00389b71")


def read_wave(path):
    """Read IEEE-float or extensible IEEE-float RIFF WAV with strict bounds."""
    data = path.read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError(f"not a RIFF WAV: {path}")
    offset, metadata, samples = 12, None, None
    while offset + 8 <= len(data):
        name = data[offset:offset + 4]
        size = struct.unpack_from("<I", data, offset + 4)[0]
        if offset + 8 + size > len(data):
            raise ValueError(f"truncated WAV chunk: {path}")
        block = data[offset + 8:offset + 8 + size]
        if name == b"fmt ":
            metadata = struct.unpack_from("<HHIIHH", block)
            floating = metadata[0] == 3 or (
                metadata[0] == 65534 and len(block) >= 40
                and block[24:40] == FLOAT_SUBFORMAT
            )
            if not floating or metadata[5] != 32:
                raise ValueError(f"expected IEEE-float WAV: {path}, {metadata}")
        elif name == b"data":
            samples = array.array("f")
            samples.frombytes(block)
            if sys.byteorder != "little":
                samples.byteswap()
        offset += 8 + size + (size & 1)
    if metadata is None or not samples or not all(math.isfinite(v) for v in samples):
        raise ValueError(f"missing or nonfinite audio: {path}")
    return metadata, samples


def summarize(case):
    text = (case / "stderr.txt").read_text()
    passes = [json.loads(line.split("ATTENTION_MODEL_RESULT ", 1)[1])
              for line in text.splitlines() if "ATTENTION_MODEL_RESULT " in line]
    if len(passes) != 2 or [p["pass"] for p in passes] != [0, 1]:
        raise ValueError(f"expected two completed model passes: {case}")
    metrics = {}
    for shape, count, microseconds in ATTENTION.findall(text):
        metrics.setdefault(shape, []).append((int(count), float(microseconds)))
    gpu = {}
    for shape, rows in metrics.items():
        if len(rows) % 2:
            raise ValueError(f"unbalanced pass records for {shape}: {case}")
        steady = rows[len(rows) // 2:]
        total = sum(count * duration for count, duration in steady) / 1000
        gpu[shape] = {"second_pass_mean_ms": total / sum(count for count, _ in steady),
                      "second_pass_total_ms": total}
    result = {"process_seconds": [p["process_seconds"] for p in passes],
              "repeat_max_abs": passes[1]["repeat_max_abs"], "gpu": gpu}
    return result, read_wave(case / "audio/estimate-1.wav")


def compare(control, candidate):
    before, (before_format, left) = summarize(control)
    after, (after_format, right) = summarize(candidate)
    if before_format != after_format or len(left) != len(right):
        raise ValueError("incompatible audio outputs")
    error = sum((a - b) ** 2 for a, b in zip(left, right))
    energy = sum(a * a for a in left)
    return {"control": before, "candidate": after,
            "waveform": {"samples": len(left), "exact": error == 0,
                         "max_abs": max(abs(a - b) for a, b in zip(left, right)),
                         "snr_db": 10 * math.log10(energy / error) if error else None},
            "second_pass_process_reduction_percent":
                (1 - after["process_seconds"][1] / before["process_seconds"][1]) * 100}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("models", nargs="+")
    parser.add_argument("--control-suffix", default="control-checked")
    parser.add_argument("--candidate-suffix", default="candidate-checked")
    args = parser.parse_args()
    report = {name: compare(args.root / f"{name}-{args.control_suffix}",
                            args.root / f"{name}-{args.candidate_suffix}") for name in args.models}
    with args.output.open("x") as target:
        json.dump(report, target, indent=2, allow_nan=False)
        target.write("\n")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
