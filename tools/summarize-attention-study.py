#!/usr/bin/env python3
"""Summarize recorded attention probes without launching inference or dropping runs."""

import argparse
import json
from pathlib import Path
import re
import statistics


def other_gpu_clients(case: Path, target_pid: int) -> list:
    path = case / "host-samples.ndjson"
    if not path.exists():
        return []
    clients = {}
    for line in path.read_text().splitlines():
        interval = json.loads(line).get("interval_summary", {})
        for item in interval.get("drm_engine_activity", []):
            owners = item["owners"]
            if any(owner["pid"] == target_pid for owner in owners):
                continue
            percent = item["percent_of_one_engine"]
            if percent < 1:
                continue
            key = (item["device"], item["engine"],
                   tuple((owner["pid"], owner["start_ticks"]) for owner in owners))
            previous = clients.get(key, {})
            if percent > previous.get("peak_percent_of_one_engine", 0):
                clients[key] = {
                    "device": item["device"], "driver": item["driver"],
                    "engine": item["engine"], "owners": owners,
                    "peak_percent_of_one_engine": percent,
                }
    return sorted(clients.values(), key=lambda item: item["peak_percent_of_one_engine"], reverse=True)


def summarize_case(case: Path) -> dict:
    result = json.loads((case / "result.json").read_text())
    text = (case / "stderr.txt").read_text(errors="replace")
    records = [
        json.loads(line.split("ATTENTION_RESULT ", 1)[1])
        for line in text.splitlines()
        if line.startswith("ATTENTION_RESULT ")
    ]
    entry = {
        "exit_code": result["exit_code"], "numerical_cases": len(records),
        "timing_eligible": result["exit_code"] == 0 and not case.name.startswith("compiler-"),
        "other_gpu_clients": other_gpu_clients(case, result["pid"]),
    }
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
        for client in result["other_gpu_clients"]:
            if client["engine"].endswith("ccs"):
                print("  other compute client:", client["owners"],
                      "peak engine percent:", round(client["peak_percent_of_one_engine"], 2))


if __name__ == "__main__":
    main()
