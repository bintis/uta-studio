#!/usr/bin/env python3
"""Export and validate exact-context BS-RoFormer as explicit CPU/GPU IR islands.

The product contract remains T=801. Time attention retains all 801 frames and
microbatches only independent bands; frequency attention retains all 62 bands
and microbatches only independent frames. Band split, final norm, and bounded
mask groups are explicit CPU islands. Transformer islands are rolling GPU
residents. ONNX is offline evidence only.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import sys
import json
import os
import resource
import time
from pathlib import Path
from typing import Any

import numpy as np

FRAMES = 801
BANDS = 62
DIM = 512
GATHERED_WIDTH = 4100
DEPTH = 12
TIME_BATCH = 8
FREQUENCY_BATCH = 64
MASK_GROUPS = ((0, 8), (8, 16), (16, 24), (24, 32), (32, 40), (40, 48), (48, 56), (56, 62))
SOURCE_SHA256 = "5b84f37e8d444c8cb30c79d77f613a41c05868ff9c9ac6c7049c00aefae115aa"
CONFIG_SHA256 = "2bfdd16c656bd9519aba757cc4f8834b7ede675eb1e00ec4772d74ae1c41af7f"
MODEL_MODULE = "audio_separator.separator.uvr_lib_v5.roformer.bs_roformer"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_hash(path: Path, _expected: str) -> None:
    if not path.is_file():
        raise SystemExit(f"required file is unavailable: {path}")


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    with temporary.open("rb") as file:
        os.fsync(file.fileno())
    temporary.replace(path)


def peak_rss_bytes() -> int:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024


def load_model(checkpoint: Path, config_path: Path, vendor_site: Path):
    import torch
    import yaml

    require_hash(checkpoint, SOURCE_SHA256)
    require_hash(config_path, CONFIG_SHA256)
    if not vendor_site.is_dir():
        raise SystemExit(f"audio-separator vendor site is unavailable: {vendor_site}")
    sys.path.insert(0, str(vendor_site))
    from audio_separator.separator.uvr_lib_v5.roformer.bs_roformer import BSRoformer

    config = yaml.load(config_path.read_text(encoding="utf-8"), Loader=yaml.FullLoader)
    expected = {"dim": 512, "depth": 12, "stereo": True, "num_stems": 1,
                "time_transformer_depth": 1, "freq_transformer_depth": 1,
                "dim_freqs_in": 1025, "stft_n_fft": 2048,
                "stft_hop_length": 441, "mask_estimator_depth": 2}
    if not isinstance(config, dict) or any(config.get("model", {}).get(key) != value for key, value in expected.items()):
        raise SystemExit("BS-RoFormer source config topology mismatch")
    model = BSRoformer(**config["model"]).eval()
    state = torch.load(checkpoint, map_location="cpu", weights_only=True)
    model.load_state_dict(state, strict=True)
    return model


def specs() -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = [{"name": "band-split", "kind": "band", "device": "CPU"}]
    for layer in range(DEPTH):
        result.append({"name": f"layer-{layer:02}-time", "kind": "time", "layer": layer, "device": "GPU"})
        result.append({"name": f"layer-{layer:02}-freq", "kind": "freq", "layer": layer, "device": "GPU"})
    result.append({"name": "final-norm", "kind": "norm", "device": "CPU"})
    for start, end in MASK_GROUPS:
        result.append({"name": f"mask-{start:02}-{end - 1:02}", "kind": "mask", "start": start, "end": end, "device": "CPU"})
    return result


def onnx_path(directory: Path, name: str) -> Path:
    return directory / f"bs-roformer-{name}.onnx"


def xml_path(directory: Path, name: str) -> Path:
    return directory / f"bs-roformer-{name}.xml"


def module_and_fixture(model, spec: dict[str, Any]):
    import torch

    class MaskGroup(torch.nn.Module):
        def __init__(self, modules):
            super().__init__()
            self.mlps = torch.nn.ModuleList(modules)

        def forward(self, features):
            return torch.cat([mlp(band) for band, mlp in zip(features.unbind(dim=-2), self.mlps)], dim=-1)

    generator = torch.Generator(device="cpu").manual_seed(0x55106)
    kind = spec["kind"]
    if kind == "band":
        value = torch.randn(1, FRAMES, GATHERED_WIDTH, generator=generator) * 0.05
        return model.band_split, value, "gathered_stft", "features"
    if kind == "time":
        value = torch.randn(TIME_BATCH, FRAMES, DIM, generator=generator) * 0.05
        return model.layers[spec["layer"]][0], value, "time_features", "time_output"
    if kind == "freq":
        value = torch.randn(FREQUENCY_BATCH, BANDS, DIM, generator=generator) * 0.05
        return model.layers[spec["layer"]][1], value, "frequency_features", "frequency_output"
    if kind == "norm":
        value = torch.randn(1, FRAMES, BANDS, DIM, generator=generator) * 0.05
        return model.final_norm, value, "features", "normalized_features"
    start, end = spec["start"], spec["end"]
    value = torch.randn(1, FRAMES, end - start, DIM, generator=generator) * 0.05
    module = MaskGroup(list(model.mask_estimators[0].to_freqs[start:end])).eval()
    return module, value, "features", "gathered_mask_part"


def export(arguments: argparse.Namespace) -> None:
    import torch

    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    model = load_model(arguments.checkpoint, arguments.config, arguments.vendor_site)
    records = []
    for spec in specs():
        name = spec["name"]
        destination = onnx_path(arguments.output_dir, name)
        if any(destination.parent.glob(destination.name + "*")):
            raise SystemExit(f"refusing to replace split artifact: {destination}")
        module, value, input_name, output_name = module_and_fixture(model, spec)
        module.eval()
        started = time.monotonic()
        with torch.inference_mode():
            torch.onnx.export(
                module,
                (value,),
                destination,
                input_names=[input_name],
                output_names=[output_name],
                opset_version=18,
                do_constant_folding=True,
                external_data=True,
                dynamo=False,
            )
        files = [
            {"path": str(path), "bytes": path.stat().st_size, "sha256": sha256(path)}
            for path in sorted(destination.parent.glob(destination.name + "*"))
        ]
        records.append({**spec, "input_shape": list(value.shape), "files": files, "elapsed_seconds": time.monotonic() - started})
        del value, module
        gc.collect()
    result = {
        "phase": "split-onnx-export",
        "exact_frames": FRAMES,
        "time_batch": TIME_BATCH,
        "frequency_batch": FREQUENCY_BATCH,
        "source_sha256": SOURCE_SHA256,
        "config_sha256": CONFIG_SHA256,
        "islands": records,
        "process_peak_rss_bytes": peak_rss_bytes(),
        "torch": torch.__version__,
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, indent=2, sort_keys=True))


def convert(arguments: argparse.Namespace) -> None:
    import openvino as ov

    records = []
    for spec in specs():
        name = spec["name"]
        source = onnx_path(arguments.artifact_dir, name)
        if not source.is_file():
            raise SystemExit(f"split ONNX is unavailable: {source}")
        destination = xml_path(arguments.artifact_dir, name)
        binary = destination.with_suffix(".bin")
        if destination.exists() or binary.exists():
            raise SystemExit(f"refusing to replace split IR: {destination}")
        started = time.monotonic()
        model = ov.convert_model(source)
        ov.save_model(model, destination, compress_to_fp16=False)
        records.append(
            {
                **spec,
                "xml": {"path": str(destination), "bytes": destination.stat().st_size, "sha256": sha256(destination)},
                "bin": {"path": str(binary), "bytes": binary.stat().st_size, "sha256": sha256(binary)},
                "elapsed_seconds": time.monotonic() - started,
            }
        )
        del model
        gc.collect()
    result = {
        "phase": "split-openvino-conversion",
        "exact_frames": FRAMES,
        "islands": records,
        "openvino": ov.get_version(),
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, indent=2, sort_keys=True))


def metrics(reference: np.ndarray, candidate: np.ndarray) -> dict[str, float]:
    if reference.shape != candidate.shape or not np.isfinite(candidate).all():
        raise SystemExit(f"malformed split output: {candidate.shape}")
    difference = candidate.astype(np.float64) - reference.astype(np.float64)
    reference64 = reference.astype(np.float64)
    candidate64 = candidate.astype(np.float64)
    return {
        "max_abs": float(np.max(np.abs(difference))),
        "mean_abs": float(np.mean(np.abs(difference))),
        "relative_l2": float(np.linalg.norm(difference) / max(np.linalg.norm(reference64), 1e-12)),
        "cosine": float(np.vdot(reference64.ravel(), candidate64.ravel()) / max(np.linalg.norm(reference64) * np.linalg.norm(candidate64), 1e-12)),
    }


def run_transform_stage(value: np.ndarray, kind: str, invoke) -> np.ndarray:
    if kind == "time":
        source = value.transpose(0, 2, 1, 3).reshape(BANDS, FRAMES, DIM)
        pieces = []
        for start in range(0, BANDS, TIME_BATCH):
            valid = min(TIME_BATCH, BANDS - start)
            batch = np.zeros((TIME_BATCH, FRAMES, DIM), dtype=np.float32)
            batch[:valid] = source[start : start + valid]
            pieces.append(invoke(batch)[:valid])
        merged = np.concatenate(pieces, axis=0)
        return merged.reshape(1, BANDS, FRAMES, DIM).transpose(0, 2, 1, 3)
    source = value.reshape(FRAMES, BANDS, DIM)
    pieces = []
    for start in range(0, FRAMES, FREQUENCY_BATCH):
        valid = min(FREQUENCY_BATCH, FRAMES - start)
        batch = np.zeros((FREQUENCY_BATCH, BANDS, DIM), dtype=np.float32)
        batch[:valid] = source[start : start + valid]
        pieces.append(invoke(batch)[:valid])
    return np.concatenate(pieces, axis=0).reshape(1, FRAMES, BANDS, DIM)


def run_pipeline(arguments: argparse.Namespace, backend: str) -> dict[str, Any]:
    model_input = np.asarray(np.load(arguments.input, mmap_mode="r"))
    if model_input.shape != (1, 2050, FRAMES, 2):
        raise SystemExit(f"unexpected exact BS-RoFormer fixture shape: {model_input.shape}")
    value = np.ascontiguousarray(model_input.transpose(0, 2, 1, 3).reshape(1, FRAMES, GATHERED_WIDTH))
    reference = np.asarray(np.load(arguments.reference, mmap_mode="r"))
    records = []
    mask_parts = []
    if backend == "ort":
        import onnxruntime as ort

        def make_invoker(spec):
            options = ort.SessionOptions()
            options.intra_op_num_threads = 1
            options.inter_op_num_threads = 1
            options.enable_cpu_mem_arena = False
            session = ort.InferenceSession(str(onnx_path(arguments.artifact_dir, spec["name"])), sess_options=options, providers=["CPUExecutionProvider"])
            input_name, output_name = session.get_inputs()[0].name, session.get_outputs()[0].name
            return session, lambda data: session.run([output_name], {input_name: data})[0]

        version = ort.__version__
    else:
        import openvino as ov

        core = ov.Core()

        def make_invoker(spec):
            device = spec["device"] if arguments.devices == "product" else "CPU"
            compiled = core.compile_model(
                xml_path(arguments.artifact_dir, spec["name"]),
                device,
                {"INFERENCE_PRECISION_HINT": "f32", "EXECUTION_MODE_HINT": "ACCURACY"},
            )
            return compiled, lambda data: np.asarray(compiled([data])[0]).copy()

        version = ov.get_version()
    started = time.monotonic()
    for spec in specs():
        stage_started = time.monotonic()
        owner, invoke = make_invoker(spec)
        kind = spec["kind"]
        if kind in ("band", "norm"):
            value = invoke(np.ascontiguousarray(value))
        elif kind in ("time", "freq"):
            value = run_transform_stage(value, kind, invoke)
        else:
            part = invoke(np.ascontiguousarray(value[:, :, spec["start"] : spec["end"], :]))
            mask_parts.append(part)
        records.append({**spec, "elapsed_seconds": time.monotonic() - stage_started})
        del invoke, owner
        gc.collect()
    gathered_mask = np.concatenate(mask_parts, axis=-1)
    candidate = gathered_mask.reshape(1, FRAMES, 2050, 2).transpose(0, 2, 1, 3)[:, None]
    return {
        "phase": f"split-{backend}-{getattr(arguments, 'devices', 'cpu')}-exact-parity",
        "frames": FRAMES,
        "metrics": metrics(reference, candidate),
        "stages": records,
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
        "version": version,
    }


def parity(arguments: argparse.Namespace) -> None:
    result = run_pipeline(arguments, arguments.backend)
    atomic_json(arguments.result, result)
    print(json.dumps(result, indent=2, sort_keys=True))


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    export_command = commands.add_parser("export")
    export_command.add_argument("--checkpoint", required=True, type=Path)
    export_command.add_argument("--config", required=True, type=Path)
    export_command.add_argument("--output-dir", required=True, type=Path)
    export_command.add_argument("--result", required=True, type=Path)
    export_command.add_argument("--vendor-site", required=True, type=Path)
    export_command.set_defaults(function=export)
    convert_command = commands.add_parser("convert")
    convert_command.add_argument("--artifact-dir", required=True, type=Path)
    convert_command.add_argument("--result", required=True, type=Path)
    convert_command.set_defaults(function=convert)
    parity_command = commands.add_parser("parity")
    parity_command.add_argument("--backend", choices=("ort", "openvino"), required=True)
    parity_command.add_argument("--devices", choices=("cpu", "product"), default="cpu")
    parity_command.add_argument("--artifact-dir", required=True, type=Path)
    parity_command.add_argument("--input", required=True, type=Path)
    parity_command.add_argument("--reference", required=True, type=Path)
    parity_command.add_argument("--result", required=True, type=Path)
    parity_command.set_defaults(function=parity)
    return root


if __name__ == "__main__":
    arguments = parser().parse_args()
    arguments.function(arguments)
