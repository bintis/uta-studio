#!/usr/bin/env python3
"""Report measured AMD model workloads without converting failures into timings.

Reads record-operation evidence, not models. No inference, retries, installations
or device initialization are performed. Run against the postboot evidence root.
"""
import argparse
import csv
import io
import json
import math
import statistics
from pathlib import Path

MODELS = {
    "bs_roformer_leap_xe90_vocals": "LEAP XE90 vocals",
    "bs_roformer_leap_xe90_instrumental": "LEAP XE90 instrumental",
    "bs_polarformer_public_instrumental": "BS PolarFormer",
    "melband_roformer_harmony": "MelBand Harmony",
    "melband_roformer_denoise_aufr33": "MelBand Denoise",
    "melband_roformer_dereverb_anvuew": "MelBand Dereverb",
    "rmvpe": "RMVPE",
    "fcpe": "FCPE",
    "basic_pitch": "Basic Pitch",
    "game_1_0_3_small": "GAME small",
    "game_1_0_3_medium": "GAME medium",
    "game_1_0_3_large": "GAME large",
    "jbm555_cectc_80": "JBM555",
    "stars": "STARS",
    "rosvot": "ROSVOT",
    "qwen3_asr_1_7b": "Qwen3 ASR encoder",
    "qwen3_forced_aligner_0_6b": "Qwen3 Forced Aligner encoder",
    "firered_asr2_aed": "FireRed AED encoder",
}


def read_record(directory):
    """Return one complete successful measurement or an explicit exclusion."""
    evidence = {"record_directory": str(directory)}
    try:
        prepared = json.loads((directory / "prepared.json").read_text())
        evidence.update(label=prepared["label"], started=prepared["time"],
                        record_commit=prepared["commit"], boot_id=prepared["boot_id"])
        completion = directory / "result.json"
        if not completion.exists():
            return None, dict(evidence, reason="operation_not_completed")
        result = json.loads(completion.read_text())
        evidence["exit_code"] = result["exit_code"]
        if result.get("boot_id") != prepared["boot_id"]:
            return None, dict(evidence, reason="boot_changed_during_operation")
        if result["exit_code"] != 0:
            return None, dict(evidence, reason="process_failure")
        events = [json.loads(line) for line in (directory / "stdout.txt").read_text().splitlines() if line.strip()]
        reports = [event for event in events if event.get("event") == "result"]
        if len(reports) != 1 or reports[0].get("status") != "passed":
            return None, dict(evidence, reason="missing_successful_model_report")
        report = reports[0]
        backend = report.get("backend")
        device = report.get("device", "")
        if backend not in {"ggml", "libtorch"} or not (
            (backend == "ggml" and "AMD Radeon 780M" in device) or
            (backend == "libtorch" and device == "ROCm device 0")
        ):
            return None, dict(evidence, reason="not_the_explicit_amd_device")
        if report.get("build_profile") != "release":
            return None, dict(evidence, reason="not_release_profile")
        if not math.isclose(report.get("input_seconds", -1), 30.0, abs_tol=1e-6):
            return None, dict(evidence, reason="not_thirty_second_input")
        if report.get("resource") not in MODELS:
            return None, dict(evidence, reason="unlisted_model_resource")
        cold = report.get("cold_execution_seconds")
        warm = report.get("warm_execution_seconds", [])
        if not isinstance(cold, (int, float)) or not math.isfinite(cold) or cold <= 0:
            return None, dict(evidence, reason="invalid_first_inference_time")
        if any(not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0 for value in warm):
            return None, dict(evidence, reason="invalid_warm_inference_times")
        return dict(report, **evidence, warm_sample_count=len(warm),
                    warm_median_seconds=statistics.median(warm) if warm else None), None
    except (OSError, ValueError, KeyError, TypeError) as error:
        return None, dict(evidence, reason="invalid_evidence", detail=str(error))


def collect(root):
    accepted, excluded = [], []
    for directory in sorted((root / "runs").iterdir()):
        if not directory.is_dir():
            continue
        report, rejection = read_record(directory)
        (accepted if report is not None else excluded).append(report or rejection)
    # A precision change is a different candidate. Never choose the fastest run
    # across candidates or confuse mixed attention with strict F32 arithmetic.
    selected = {}
    for report in sorted(accepted, key=lambda value: value["started"]):
        selected[(report["resource"], report["backend"], report["precision"])] = report
    return list(selected.values()), excluded


def ratio(reference, candidate):
    if reference is None or candidate is None or candidate <= 0:
        return None
    return reference / candidate


def pairs(reports):
    output = []
    for resource, label in MODELS.items():
        ggml = next((row for row in reports if row["resource"] == resource and row["backend"] == "ggml"), {})
        native = next((row for row in reports if row["resource"] == resource and row["backend"] == "libtorch" and row["precision"] == "strict"), {})
        scopes = {row["scope"] for row in [ggml, native] if row}
        comparable = len(scopes) == 1 and bool(ggml) and bool(native) and ggml["model_path"] == native["model_path"] and ggml["input"] == native["input"]
        row = {"resource": resource, "model": label, "scope": next(iter(scopes)) if len(scopes) == 1 else "scope_mismatch_or_missing",
               "pair_status": "measured_same_workload" if comparable else "missing_or_not_comparable"}
        for name, measurement in [("ggml", ggml), ("libtorch", native)]:
            for field in ["cold_execution_seconds", "warm_median_seconds", "warm_sample_count", "runtime_load_seconds", "model_load_seconds", "record_directory", "boot_id", "record_commit"]:
                row[name + "_" + field] = measurement.get(field)
        row["first_inference_speedup"] = ratio(ggml.get("cold_execution_seconds"), native.get("cold_execution_seconds")) if comparable else None
        row["warm_speedup"] = ratio(ggml.get("warm_median_seconds"), native.get("warm_median_seconds")) if comparable else None
        output.append(row)
    return output


def display(value, suffix=""):
    return "—" if value is None else f"{value:.3f}{suffix}"


def markdown(rows, reports, exclusions):
    matched = sum(row["pair_status"] == "measured_same_workload" for row in rows)
    lines = ["# AMD 30-second GGML / LibTorch measurements", "",
             f"Completed comparable strict workload pairs: **{matched}/{len(rows)}**.", "",
             "The source is one real 30-second clip. Its canonical resamples are prepared outside inference timing. Both backends use the same GGUF per pair. Measurements are native GPU execution on AMD Radeon 780M, not CPU fallbacks.", "",
             "## First complete inference after model loading", "",
             "These are single first-inference observations, not cold-boot latency or statistical averages. Runtime loading and weight loading are separate. Lazy operator initialization and the canonical host frontend/postprocessing are included. Speedup = GGML / LibTorch; below one means GGML is faster.", "",
             "| Model | GGML (s) | LibTorch strict (s) | Speedup |", "|---|---:|---:|---:|"]
    for row in rows:
        lines.append(f"| {row['model']} | {display(row['ggml_cold_execution_seconds'])} | {display(row['libtorch_cold_execution_seconds'])} | {display(row['first_inference_speedup'], '×')} |")
    lines += ["", "## Warm inference (only where actually repeated)", "",
              "Medians are calculated within a single resident-model process. Counts are shown explicitly; a count of one is one observation, not a repeatability study. No warm time is inferred for single-pass separation runs.", "",
              "| Model | GGML (s; n) | LibTorch strict (s; n) | Speedup |", "|---|---:|---:|---:|"]
    for row in rows:
        if row["ggml_warm_median_seconds"] is not None or row["libtorch_warm_median_seconds"] is not None:
            left = display(row["ggml_warm_median_seconds"]) + f"; {row['ggml_warm_sample_count'] or 0}"
            right = display(row["libtorch_warm_median_seconds"]) + f"; {row['libtorch_warm_sample_count'] or 0}"
            lines.append(f"| {row['model']} | {left} | {right} | {display(row['warm_speedup'], '×')} |")
    lines += ["", "## Scope and interpretation", "",
              "Qwen3 ASR, Qwen3 Forced Aligner and FireRed rows measure frontend plus audio encoder only: no text generation, alignment decoder, or end-to-end Studio latency is claimed. FireRed covers every source sample using nonoverlapping encoder windows with a zero-padded final tail.", "",
              "STARS and ROSVOT use fixed synthetic pitch/transcript conditioning over the real audio; STARS also uses synthetic legal phoneme labels. JBM555 receives the same clip in both mix/vocal inputs. These are controlled computational workloads, not transcription accuracy tests.", "",
              "Output shapes/finite-value checks and selected complete operator comparisons do not establish whole-model output parity or production-dataset quality. GGML uses its current checkpoint-storage/kernel precision; LibTorch strict uses F32 model arithmetic. Experimental mixed-attention diagnostics are not substituted into the strict table.", "",
              "Results span recorded boot epochs. This was not an exclusively controlled laboratory host; parallel unrelated CPU/XPU activity is not ruled out. Boot IDs, source records, sample counts and raw timings are retained. Do not combine these numbers with earlier debug-binary measurements whose timer included weight loading and repeated a LibTorch load.", "",
              "## Evidence", "",
              "The sibling JSON contains accepted candidate measurements and explicit exclusions. The CSV includes runtime/weight loading, both inference timing definitions, boot IDs and the exact operation directories. A failed or incomplete run never becomes a zero-time or timeout-duration speed measurement.", "",
              f"Selected successful candidates: {len(reports)}. Excluded or incomplete operation records: {len(exclusions)}.", ""]
    for row in rows:
        lines.append(f"### {row['model']}")
        for backend in ["ggml", "libtorch"]:
            path = row[backend + "_record_directory"]
            lines.append(f"{backend}: `{path or 'no completed eligible record'}`")
        lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--destination", type=Path)
    options = parser.parse_args()
    root = options.root.resolve()
    destination = (options.destination or root / "summary").resolve()
    reports, exclusions = collect(root)
    rows = pairs(reports)
    destination.mkdir(parents=True, exist_ok=True)
    payload = {"source_root": str(root), "measurements": reports, "pairs": rows, "excluded": exclusions}
    (destination / "comparison.json").write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    buffer = io.StringIO()
    writer = csv.DictWriter(buffer, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)
    (destination / "comparison.csv").write_text(buffer.getvalue())
    (destination / "REPORT.md").write_text(markdown(rows, reports, exclusions))
    print(json.dumps({"report_directory": str(destination), "completed_pairs": sum(row["pair_status"] == "measured_same_workload" for row in rows), "resources": len(rows), "excluded_records": len(exclusions)}))


if __name__ == "__main__":
    main()
