#!/usr/bin/env python3
"""Convert the fixed 24 kHz STARS/ROSVOT annotation frontend generation.

Native Rust owns the shared 80-bin mel and exact pitch adaptation. This recipe
exports only the exact annotation RMVPE neural graph. Model-specific converters
consume the resulting immutable manifest instead of exporting private copies.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import resource
import sys
from pathlib import Path
from typing import Any

sys.dont_write_bytecode = True

PROFILE = "shared-singing-frontend-24k-v1"
SOURCE_REVISION = "3c8332bf43adae35f6e4d64971862f2f6139b310"
SOURCE_MANIFEST_SHA256 = "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8"
ANNOTATION_RMVPE_SHA256 = "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require(path: Path, expected: str, label: str) -> None:
    if not path.is_file() or path.is_symlink() or sha256(path) != expected:
        raise SystemExit(f"{label} identity mismatch: {path}")


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    with temporary.open("rb") as handle:
        os.fsync(handle.fileno())
    temporary.replace(path)


def peak_rss() -> int:
    value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return value if sys.platform == "darwin" else value * 1024


def load_model(source: Path, audit: Path):
    import torch
    import types

    require(source.parent / "source-manifest.json", SOURCE_MANIFEST_SHA256, "ROSVOT source manifest")
    checkpoint = audit / "checkpoints/rmvpe/model.pt"
    require(checkpoint, ANNOTATION_RMVPE_SHA256, "annotation RMVPE checkpoint")
    sys.path.insert(0, str(source))
    sys.modules.setdefault("pyworld", types.ModuleType("pyworld"))
    pretty_midi = types.ModuleType("pretty_midi")
    pretty_midi.PrettyMIDI = type("PrettyMIDI", (), {})
    sys.modules.setdefault("pretty_midi", pretty_midi)
    from modules.pe.rmvpe.model import E2E0

    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    model = E2E0(4, 1, (2, 2)).eval()
    model.load_state_dict(saved["model"], strict=False)
    return model


def export(arguments: argparse.Namespace) -> None:
    import numpy as np
    import torch

    if arguments.frames <= 0 or arguments.frames % 32:
        raise SystemExit("annotation RMVPE frames must be divisible by 32")
    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    model = load_model(arguments.source_dir, arguments.audit_dir)
    generator = torch.Generator().manual_seed(0x19DC1809)
    value = torch.randn(1, 128, arguments.frames, generator=generator) * 0.1
    output = arguments.output_dir / f"annotation-rmvpe-t{arguments.frames}.onnx"
    if output.exists():
        raise SystemExit(f"refusing to replace {output}")
    with torch.inference_mode():
        reference = model(value)
        torch.onnx.export(
            model,
            (value,),
            output,
            input_names=["rmvpe_mel"],
            output_names=["rmvpe_salience"],
            opset_version=18,
            do_constant_folding=True,
            external_data=True,
            dynamo=False,
        )
    np.save(arguments.output_dir / "annotation-rmvpe-input-0.npy", value.numpy())
    np.save(arguments.output_dir / "annotation-rmvpe-reference-0.npy", reference.numpy())
    atomic_json(
        arguments.result,
        {
            "phase": "shared-singing-frontend-export",
            "profile": PROFILE,
            "source_revision": SOURCE_REVISION,
            "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
            "annotation_rmvpe_sha256": ANNOTATION_RMVPE_SHA256,
            "frames": arguments.frames,
            "onnx": {"filename": output.name, "bytes": output.stat().st_size, "sha256": sha256(output)},
            "native_frontend": {"sample_rate": 24000, "fft_size": 512, "hop_size": 128, "mel_bins": 80},
            "consumers": {"stars": {"mel_bins": 80}, "rosvot": {"mel_prefix_bins": 40}},
            "torch": torch.__version__,
            "process_peak_rss_bytes": peak_rss(),
        },
    )


def convert(arguments: argparse.Namespace) -> None:
    import openvino as ov

    onnx = next(arguments.artifact_dir.glob("annotation-rmvpe-t*.onnx"), None)
    if onnx is None:
        raise SystemExit("shared annotation RMVPE ONNX is missing")
    xml = onnx.with_suffix(".xml")
    if xml.exists() or xml.with_suffix(".bin").exists():
        raise SystemExit(f"refusing to replace {xml}")
    model = ov.convert_model(onnx)
    ov.save_model(model, xml, compress_to_fp16=False)
    atomic_json(
        arguments.result,
        {
            "phase": "shared-singing-frontend-openvino-conversion",
            "profile": PROFILE,
            "xml": {"filename": xml.name, "bytes": xml.stat().st_size, "sha256": sha256(xml)},
            "bin": {"filename": xml.with_suffix('.bin').name, "bytes": xml.with_suffix('.bin').stat().st_size, "sha256": sha256(xml.with_suffix('.bin'))},
            "openvino": ov.get_version(),
            "process_peak_rss_bytes": peak_rss(),
        },
    )


def metrics(reference, candidate) -> dict[str, float]:
    import numpy as np

    difference = candidate.astype(np.float64) - reference.astype(np.float64)
    return {
        "max_abs": float(np.max(np.abs(difference))),
        "mean_abs": float(np.mean(np.abs(difference))),
        "relative_l2": float(np.linalg.norm(difference) / max(np.linalg.norm(reference.astype(np.float64)), 1e-12)),
    }


def parity(arguments: argparse.Namespace) -> None:
    import numpy as np

    inputs = [np.load(arguments.artifact_dir / "annotation-rmvpe-input-0.npy")]
    reference = np.load(arguments.artifact_dir / "annotation-rmvpe-reference-0.npy")
    if arguments.backend == "ort":
        if arguments.devices != "cpu":
            raise SystemExit("ORT parity is CPU-only")
        import onnxruntime as ort
        source = next(arguments.artifact_dir.glob("annotation-rmvpe-t*.onnx"))
        session = ort.InferenceSession(str(source), providers=["CPUExecutionProvider"])
        candidate = session.run(None, {session.get_inputs()[0].name: inputs[0]})[0]
    else:
        import openvino as ov
        source = next(arguments.artifact_dir.glob("annotation-rmvpe-t*.xml"))
        device = "GPU" if arguments.devices == "product" else "CPU"
        compiled = ov.Core().compile_model(source, device, {"INFERENCE_PRECISION_HINT": "f32", "EXECUTION_MODE_HINT": "ACCURACY"})
        candidate = np.asarray(compiled(inputs)[0]).copy()
    observed = metrics(reference, candidate)
    if (not all(np.isfinite(value) for value in observed.values())
            or observed["relative_l2"] > 5e-4 or observed["max_abs"] > 1e-3):
        raise SystemExit(f"shared annotation RMVPE parity failed: {observed}")
    atomic_json(
        arguments.result,
        {"phase": f"shared-singing-frontend-{arguments.backend}-{arguments.devices}-parity", "profile": PROFILE,
         "accepted": True, "metrics": observed, "process_peak_rss_bytes": peak_rss()},
    )


def finalize(arguments: argparse.Namespace) -> None:
    files = {}
    for pattern in ("annotation-rmvpe-t*.onnx", "annotation-rmvpe-t*.xml", "annotation-rmvpe-t*.bin"):
        path = next(arguments.artifact_dir.glob(pattern), None)
        if path is None:
            raise SystemExit(f"shared generation file is missing: {pattern}")
        files[path.name] = sha256(path)
    atomic_json(
        arguments.output,
        {
            "schema_version": 1,
            "profile": PROFILE,
            "source_revision": SOURCE_REVISION,
            "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
            "annotation_rmvpe_sha256": ANNOTATION_RMVPE_SHA256,
            "native_mel": {"sample_rate": 24000, "fft_size": 512, "hop_size": 128, "mel_bins": 80, "rosvot_prefix_bins": 40},
            "files": files,
        },
    )


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    command = commands.add_parser("export")
    command.add_argument("--source-dir", type=Path, default=Path("third_party/rosvot/upstream"))
    command.add_argument("--audit-dir", type=Path, required=True)
    command.add_argument("--output-dir", type=Path, required=True)
    command.add_argument("--frames", type=int, default=256)
    command.add_argument("--result", type=Path, required=True)
    command.set_defaults(function=export)
    command = commands.add_parser("convert")
    command.add_argument("--artifact-dir", type=Path, required=True)
    command.add_argument("--result", type=Path, required=True)
    command.set_defaults(function=convert)
    command = commands.add_parser("parity")
    command.add_argument("--backend", choices=("ort", "openvino"), required=True)
    command.add_argument("--devices", choices=("cpu", "product"), default="cpu",
                         help="product selects OpenVINO GPU and requires repository-policy permission")
    command.add_argument("--artifact-dir", type=Path, required=True)
    command.add_argument("--result", type=Path, required=True)
    command.set_defaults(function=parity)
    command = commands.add_parser("finalize")
    command.add_argument("--artifact-dir", type=Path, required=True)
    command.add_argument("--output", type=Path, required=True)
    command.set_defaults(function=finalize)
    return root


if __name__ == "__main__":
    arguments = parser().parse_args()
    arguments.function(arguments)
