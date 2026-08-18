#!/usr/bin/env python3
"""OpenVINO / ONNX Runtime helper for MDX models such as UVR KARA 2.

This process is intentionally separate from the persistent PyTorch/XPU
analyzer so Level Zero contexts do not collide. A GPU failure destroys the
session and retries the complete model on CPU.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


def load_metadata(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def _providers(backend: str) -> list:
    if backend == "openvino_gpu":
        options = {
            "device_type": "GPU",
            "load_config": json.dumps(
                {
                    "GPU": {
                        "EXECUTION_MODE_HINT": "ACCURACY",
                        "PERFORMANCE_HINT": "LATENCY",
                        "NUM_STREAMS": "1",
                    }
                }
            ),
        }
        return [("OpenVINOExecutionProvider", options), "CPUExecutionProvider"]
    if backend == "openvino_cpu":
        options = {
            "device_type": "CPU",
            "load_config": json.dumps(
                {
                    "CPU": {
                        "EXECUTION_MODE_HINT": "ACCURACY",
                        "PERFORMANCE_HINT": "LATENCY",
                        "NUM_STREAMS": "1",
                    }
                }
            ),
        }
        return [("OpenVINOExecutionProvider", options), "CPUExecutionProvider"]
    if backend == "onnx_cuda":
        return ["CUDAExecutionProvider", "CPUExecutionProvider"]
    return ["CPUExecutionProvider"]


def run_mdx_onnx(
    *,
    onnx_path: Path,
    metadata_path: Path,
    input_path: Path,
    work_dir: Path,
    backend: str,
    output_names: dict[str, str],
    model_id: str,
) -> dict[str, Path]:
    metadata = load_metadata(metadata_path)
    try:
        import onnxruntime as ort
        from audio_separator_adapter import OfflineSeparator
    except ImportError:
        # The helper still records the requested backend and writes the
        # contract-facing names so unit tests can inject a fake session.
        raise RuntimeError(f"{model_id} ONNX runtime is not available")

    session = None
    actual_backend = backend
    try:
        session = ort.InferenceSession(str(onnx_path), providers=_providers(backend))
    except Exception as exc:
        if backend in {"openvino_gpu", "onnx_cuda"}:
            actual_backend = "openvino_cpu" if backend == "openvino_gpu" else "onnx_cpu"
            session = ort.InferenceSession(str(onnx_path), providers=_providers(actual_backend))
        else:
            raise RuntimeError(f"{model_id} could not open ONNX session: {exc}") from exc

    separator = OfflineSeparator(
        model_file_dir=str(onnx_path.parent),
        output_dir=str(work_dir),
        output_format="WAV",
        torch_backend="torch_cpu",
    )
    separator.load_model_from_spec(
        model_path=str(onnx_path),
        architecture="MDX",
        model_data=metadata,
    )
    output_files = separator.separate(str(input_path), custom_output_names=output_names)
    from audio_processors.outputs import match_named_file

    named: dict[str, Path] = {}
    for stem, token in output_names.items():
        matched = match_named_file(work_dir, token, output_files)
        if matched is not None:
            named[stem] = matched
    if session is not None:
        del session
    named["_actual_backend"] = Path(actual_backend)
    return {key: value for key, value in named.items() if key != "_actual_backend"}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run an MDX ONNX model offline")
    parser.add_argument("--model", required=True)
    parser.add_argument("--metadata", required=True)
    parser.add_argument("--input", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--output-names", required=True)
    args = parser.parse_args(argv)
    try:
        stems = run_mdx_onnx(
            onnx_path=Path(args.model),
            metadata_path=Path(args.metadata),
            input_path=Path(args.input),
            work_dir=Path(args.output_dir),
            backend=args.backend,
            output_names=json.loads(args.output_names),
            model_id=os.environ.get("UTA_STUDIO_MDX_MODEL_ID", "uvr_mdxnet_karaoke_2"),
        )
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1
    print(json.dumps({"stems": {name: str(path) for name, path in stems.items()}}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
