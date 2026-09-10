#!/usr/bin/env python3
"""Summarize existing native probe logs, including failed or incomplete cases."""
import importlib.util
import json
from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
module_path = Path(__file__).resolve().parents[1] / "summarize-attention-study.py"
spec = importlib.util.spec_from_file_location("attention_observation", module_path)
observation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observation)
records = []
for directory in sorted(root.iterdir()):
    command = directory / "command.json"
    result_file = directory / "result.json"
    stdout = directory / "stdout.txt"
    if not command.exists():
        continue
    completion = json.loads(result_file.read_text()) if result_file.exists() else {}
    entry = {"run": directory.name, "completion": completion,
             "command": json.loads(command.read_text()), "results": [], "gpu_timings": [], "environment": []}
    if stdout.exists():
        for line in stdout.read_text(errors="replace").splitlines():
            for marker, target in [("PROBE_RESULT ", "results"), ("PROBE_GPU_TIMING ", "gpu_timings"), ("PROBE_ENV ", "environment")]:
                if line.startswith(marker):
                    entry[target].append(json.loads(line[len(marker):]))
    if "pid" in completion:
        entry["other_gpu_clients"] = observation.other_gpu_clients(directory, completion["pid"])
    stderr = directory / "stderr.txt"
    if stderr.exists():
        entry["errors"] = [line for line in stderr.read_text(errors="replace").splitlines() if "PROBE_ERROR" in line or "error while loading" in line]
    records.append(entry)
(root / "measurements.json").write_text(json.dumps({
    "scope": "synthetic production-sized operators, not XE90 model inference or audio qualification",
    "comparison": "same fixture generator and logical shapes; inspect storage and output rounding notes",
    "primary_timing": "synchronized host compute with eight warmups/eight samples for full cases",
    "secondary_timing": "XPU stream profiling events when present; not whole-model timing",
    "reference": "128 deterministic full-contraction f64 output samples and a full finite-output scan",
    "runs": records,
}, indent=2) + "\n")
for entry in records:
    if not entry["results"]:
        print(entry["run"], "exit", entry["completion"].get("exit_code"), "no successful result", entry.get("errors", []))
    for item in entry["results"]:
        print(entry["run"], item["backend"], item["case"], item["precision"],
              f"{item['mean_ms']:.4f} ms", f"{item['effective_tflops']:.3f} TFLOPS", "NMSE", item["nmse_original_float_input_reference"])
    for client in entry.get("other_gpu_clients", []):
        if client["engine"].endswith("ccs"):
            print("other compute client", client["owners"], client["peak_percent_of_one_engine"])
