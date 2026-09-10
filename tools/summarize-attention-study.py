#!/usr/bin/env python3
"""Summarize recorded attention probes without launching inference or dropping runs."""

import argparse
import json
from pathlib import Path
import re
import statistics


def summarize_case(case: Path) -> dict:
    result = json.loads((case / "result.json").read_text())
    text = (case / "stderr.txt").read_text(errors="replace")
    records = [
        json.loads(line.split("ATTENTION_RESULT ", 1)[1])
        for line in text.splitlines()
        if line.startswith("ATTENTION_RESULT ")
    ]
    entry = {"exit_code": result["exit_code"], "numerical_cases": len(records)}
    if records:
        entry["maximum_nmse"] = max(row["nmse"] for row in records)
    for row in records:
        sequence = row["queries"]
        if sequence not in (1722, 90) or row["heads"] != 8:
            continue
        if row["batches"] not in (1722, 90):
            continue
        pattern = (
            r"FLASH_ATTN_EXT dst\(64,8," + str(sequence)
            + r",\d+\).*: 1 x ([\d.e+-]+) us"
        )
        all_times = [float(value) for value in re.findall(pattern, text)]
        times = all_times[row["warmup_calls"]:]
        if not times:
            continue
        mean = statistics.mean(times)
        entry[str(sequence)] = {
            "mean_ms": mean / 1000,
            "sd_ms": statistics.stdev(times) / 1000 if len(times) > 1 else 0,
            "tflops": row["matmul_flops"] / (mean * 1e6),
            "warmup_gpu_us": all_times[:row["warmup_calls"]],
            "measured_gpu_us": times,
        }
    return entry


def correct_loader_scope(root: Path) -> None:
    report_path = root / "installation-verification.json"
    worker_path = root / "worker-after/result.json"
    if not report_path.exists() or not worker_path.exists():
        return
    report = json.loads(report_path.read_text())
    worker = json.loads(worker_path.read_text())
    log = root / ("loader." + str(worker["pid"]))
    libraries = sorted({
        line.split("calling init:", 1)[1].strip()
        for line in log.read_text().splitlines()
        if "calling init:" in line and "libggml" in line
    })
    report.setdefault("all_process_libraries", report["loaded_libraries"])
    report["loader_worker_pid"] = worker["pid"]
    report["loaded_libraries"] = libraries
    report["auxiliary_process_libraries"] = sorted(
        set(report["all_process_libraries"]) - set(libraries)
    )
    report_path.write_text(json.dumps(report, indent=2) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    root = parser.parse_args().root
    summary = {}
    for case in sorted(root.iterdir()):
        if (case / "result.json").is_file() and (case / "stderr.txt").is_file():
            summary[case.name] = summarize_case(case)
    (root / "measurement-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    correct_loader_scope(root)
    for name, result in summary.items():
        shapes = [
            f"{sequence}: {result[sequence]['mean_ms']:.3f} ms, "
            f"{result[sequence]['tflops']:.3f} TFLOPS"
            for sequence in ("1722", "90") if sequence in result
        ]
        print(name, "exit=", result["exit_code"],
              "cases=", result["numerical_cases"], "; ".join(shapes))


if __name__ == "__main__":
    main()
