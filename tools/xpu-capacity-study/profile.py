#!/usr/bin/env python3
"""Sequential counter experiments; only the read-only collector runs with existing sudo permission."""
import argparse
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import threading
import time

CASES = {
    "time": ("attention-time", "f16", 1, "interleaved", 12, 30, 8),
    "frequency": ("attention-frequency", "f16", 1, "interleaved", 12, 30, 16),
    "bridge": ("attention-time", "f16", 1, "bridge", 12, 30, 8),
    "gemm-half": ("gemm", "f16", 8192, "interleaved", 12, 30, 8),
    "gemm-brain": ("gemm", "bf16", 8192, "interleaved", 12, 30, 8),
    "gemm-float": ("gemm", "f32", 4096, "interleaved", 12, 30, 16),
    "copy": ("copy", "f32", 134217728, "interleaved", 12, 30, 32),
    "exp": ("exp", "f32", 134217728, "interleaved", 12, 30, 32),
}


def save(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n")


def environment(root):
    result = os.environ.copy()
    paths = [
        root / "test-artifacts/xpu-capacity-study/metrics-library/dump/linux64/release/metrics_library",
        root / "test-artifacts/xpu-capacity-study/metrics-discovery/dump/linux64/release/metrics_discovery",
        root / "test-artifacts/libtorch-xpu-isolated/native/torch/lib",
        root / "test-artifacts/libtorch-xpu-isolated/venv/lib",
        Path("/run/opengl-driver/lib"),
        Path("/nix/store/yfvm0a8avc10lw18ps7xp7ym5smh8kn0-level-zero-1.32.0/lib"),
        Path("/nix/store/3h9gdk910cwj8c1r8290nrx37pbxwpqs-ocl-icd-2.3.5/lib"),
    ]
    result["LD_LIBRARY_PATH"] = ":".join(map(str, paths)) + ":" + result.get("LD_LIBRARY_PATH", "")
    result["ONEAPI_DEVICE_SELECTOR"] = "level_zero:gpu"
    result["ZE_FLAT_DEVICE_HIERARCHY"] = "FLAT"
    for key in ("ZE_ENABLE_METRICS", "ZET_ENABLE_METRICS", "ONEDNN_VERBOSE", "DNNL_VERBOSE"):
        result.pop(key, None)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--groups", nargs="+", default=["ComputeBasic", "VectorEngineProfile", "VectorEngineStalls"])
    parser.add_argument("--cases", nargs="+", choices=tuple(CASES), default=["time", "gemm-half", "gemm-float", "frequency", "bridge"])
    args = parser.parse_args()
    root = Path.cwd()
    directory = Path(tempfile.mkdtemp(prefix="profile-", dir=root / "test-artifacts/xpu-capacity-study"))
    env = environment(root)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "status", "--porcelain"], text=True)
    boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    print("PROFILE", directory, flush=True)
    done = threading.Event()

    def telemetry():
        with (directory / "nvtop.ndjson").open("x") as output:
            while not done.is_set():
                row = {"epoch_ns": time.time_ns()}
                try:
                    result = subprocess.run(["nvtop", "--snapshot"], capture_output=True, text=True, timeout=5)
                    row.update(returncode=result.returncode, stderr=result.stderr)
                    row["devices"] = json.loads(result.stdout) if result.returncode == 0 else []
                except Exception as error:
                    row["error"] = str(error)
                output.write(json.dumps(row) + "\n")
                output.flush()
                done.wait(0.2)
    watcher = threading.Thread(target=telemetry)
    watcher.start()
    records = []
    try:
        for group in args.groups:
            for name in args.cases:
                case = directory / (group + "-" + name)
                case.mkdir()
                command = ["sudo", "-n", "env", "LD_LIBRARY_PATH=" + env["LD_LIBRARY_PATH"],
                           "ZE_ENABLE_METRICS=1", "ZET_ENABLE_METRICS=1", "ZE_FLAT_DEVICE_HIERARCHY=FLAT",
                           str(root / "test-artifacts/xpu-capacity-study/build/sample"), group, str(case / "raw.bin")]
                launched = {"command": command, "cwd": str(root), "commit": commit, "dirty": dirty,
                            "boot_id": boot, "epoch_ns": time.time_ns(), "ordinary_user_uid": os.getuid(),
                            "scope": "sudo is confined to counter reads; benchmark remains ordinary user"}
                save(case / "collector-launch.json", launched)
                print("BEGIN", group, name, flush=True)
                collector = None
                record = {"group": group, "name": name, "case_dir": str(case)}
                with (case / "metrics.ndjson").open("x") as output, (case / "collector-stderr.txt").open("x") as errors:
                    try:
                        collector = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=output,
                                                     stderr=subprocess.PIPE, text=True, env=env)
                        ready, _, _ = select.select([collector.stderr], [], [], 30)
                        line = collector.stderr.readline() if ready else "collector readiness wait expired\n"
                        errors.write(line)
                        errors.flush()
                        if line.strip() != "READY":
                            raise RuntimeError(line.strip())
                        benchmark = [str(root / "test-artifacts/xpu-capacity-study/build/benchmark"),
                                     *map(str, CASES[name])]
                        invocation = [sys.executable, "tools/record-operation.py", "xpu-counter-" + group + "-" + name,
                                      "--output", str(case), "--", sys.executable, "tools/observe-roformer-run.py",
                                      str(case), "--", *benchmark]
                        record["benchmark_uid"] = os.getuid()
                        record["benchmark_command"] = benchmark
                        record["begin_ns"] = time.time_ns()
                        result = subprocess.run(invocation, env=env)
                        record["end_ns"] = time.time_ns()
                        record["benchmark_returncode"] = result.returncode
                    except Exception as error:
                        record["error"] = str(error)
                    finally:
                        if collector is not None:
                            if collector.stdin:
                                collector.stdin.close()
                            tail = collector.stderr.read()
                            errors.write(tail)
                            record["collector_returncode"] = collector.wait()
                            record["collector_pid"] = collector.pid
                        record["completed_epoch_ns"] = time.time_ns()
                        record["boot_after"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
                        save(case / "collector-completion.json", record)
                stdout = case / "stdout.txt"
                if stdout.exists():
                    for line in stdout.read_text().splitlines():
                        if line.startswith("CAPACITY_RESULT "):
                            record["measurement"] = json.loads(line.split(" ", 1)[1])
                        elif line.startswith("CAPACITY_PHASE "):
                            record.setdefault("phases", []).append(json.loads(line.split(" ", 1)[1]))
                records.append(record)
                save(directory / "series.json", records)
                print("END", group, name, record.get("benchmark_returncode"), record.get("collector_returncode"), flush=True)
    finally:
        done.set()
        watcher.join()
    print("COMPLETE", directory, flush=True)
    return int(any(row.get("collector_returncode") != 0 or row.get("benchmark_returncode") != 0 for row in records))


if __name__ == "__main__":
    raise SystemExit(main())
