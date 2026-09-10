#!/usr/bin/env python3
"""Recorded sequential synthetic XPU cases with concurrent nvtop telemetry; no retries."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time


def configurations(group):
    if group == "gemm":
        return [("gemm", precision, size, "interleaved", 8, 12, 8 if size >= 4096 else 32)
                for precision in ("f16", "bf16", "f32") for size in (1024, 2048, 4096, 8192)]
    if group == "attention":
        cases = [(axis, precision, 1, "interleaved", 8, 12, 4)
                 for precision in ("f16", "bf16", "f32")
                 for axis in ("attention-time", "attention-frequency")]
        cases += [(axis, "f16", 1, layout, 8, 12, 4)
                  for layout in ("contiguous", "bridge")
                  for axis in ("attention-time", "attention-frequency")]
        cases += [("smoke-attention", "f16", 1, "mixed", 2, 4, 1)]
        return cases
    if group == "memory":
        return [(name, "f32", 134217728, "interleaved", 8, 12, 16)
                for name in ("copy", "vector", "exp")] + [
                    ("softmax", precision, 64 * 1722 * 1722, "interleaved", 8, 12, 4)
                    for precision in ("f16", "f32")]
    if group == "confirm":
        return [("attention-time", "f16", 1, layout, 12, 20, 4)
                for layout in ("interleaved", "contiguous", "bridge", "interleaved")] + [
                    ("gemm", "f16", 8192, "interleaved", 12, 20, 8),
                    ("gemm", "bf16", 8192, "interleaved", 12, 20, 8),
                    ("copy", "f32", 134217728, "interleaved", 12, 20, 16)]
    raise ValueError(group)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("group", choices=("gemm", "attention", "memory", "confirm"))
    args = parser.parse_args()
    root = Path.cwd()
    study = root / "test-artifacts/xpu-capacity-study"
    directory = Path(tempfile.mkdtemp(prefix=args.group + "-", dir=study))
    environment = os.environ.copy()
    environment["LD_LIBRARY_PATH"] = ":".join((
        str(root / "test-artifacts/libtorch-xpu-isolated/native/torch/lib"),
        str(root / "test-artifacts/libtorch-xpu-isolated/venv/lib"),
        "/run/opengl-driver/lib", "/nix/store/yfvm0a8avc10lw18ps7xp7ym5smh8kn0-level-zero-1.32.0/lib",
        "/nix/store/3h9gdk910cwj8c1r8290nrx37pbxwpqs-ocl-icd-2.3.5/lib",
        environment.get("LD_LIBRARY_PATH", "")))
    environment["ONEAPI_DEVICE_SELECTOR"] = "level_zero:gpu"
    environment["ZE_FLAT_DEVICE_HIERARCHY"] = "FLAT"
    for key in ("ZE_ENABLE_METRICS", "ONEDNN_VERBOSE", "DNNL_VERBOSE", "MKL_VERBOSE", "SYCL_PI_TRACE"):
        environment.pop(key, None)
    done = threading.Event()
    def telemetry():
        with (directory / "nvtop.ndjson").open("x") as stream:
            while not done.is_set():
                entry = {"epoch_ns": time.time_ns()}
                try:
                    result = subprocess.run(["nvtop", "--snapshot"], capture_output=True, text=True, timeout=5)
                    entry.update(returncode=result.returncode, stderr=result.stderr)
                    entry["devices"] = json.loads(result.stdout) if result.returncode == 0 else []
                except Exception as error:
                    entry["error"] = str(error)
                stream.write(json.dumps(entry) + "\n")
                stream.flush()
                done.wait(0.2)
    thread = threading.Thread(target=telemetry)
    thread.start()
    results = []
    print("SERIES", directory, flush=True)
    try:
        for index, configuration in enumerate(configurations(args.group)):
            label = f"{index:02d}-" + "-".join(map(str, configuration[:4]))
            case = directory / label
            command = [str(root / "test-artifacts/xpu-capacity-study/build/benchmark"), *map(str, configuration)]
            invocation = [sys.executable, "tools/record-operation.py", "xpu-capacity-" + label,
                          "--output", str(case), "--", sys.executable, "tools/observe-roformer-run.py",
                          str(case), "--", *command]
            print("BEGIN", label, flush=True)
            started = time.time_ns()
            completed = subprocess.run(invocation, env=environment)
            record = {"label": label, "command": command, "case_dir": str(case),
                      "returncode": completed.returncode, "begin_ns": started, "end_ns": time.time_ns()}
            if (case / "stdout.txt").exists():
                for line in (case / "stdout.txt").read_text().splitlines():
                    if line.startswith("CAPACITY_RESULT "):
                        record["measurement"] = json.loads(line.split(" ", 1)[1])
                        print(line, flush=True)
                    elif line.startswith("CAPACITY_PHASE "):
                        record.setdefault("phases", []).append(json.loads(line.split(" ", 1)[1]))
            if (case / "stderr.txt").exists():
                record["stderr"] = (case / "stderr.txt").read_text()
            results.append(record)
            (directory / "series.json").write_text(json.dumps(results, indent=2) + "\n")
            print("END", label, completed.returncode, flush=True)
    finally:
        done.set()
        thread.join()
    print("COMPLETE", directory, flush=True)
    # Unsupported configurations remain explicit results, never rerun on another backend.

if __name__ == "__main__":
    main()
