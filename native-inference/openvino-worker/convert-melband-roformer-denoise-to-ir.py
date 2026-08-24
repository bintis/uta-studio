#!/usr/bin/env python3
"""Memory-bounded phased conversion for the exact card-R03 Denoise RoFormer.

Each heavy subcommand is intentionally a separate process.  The commands never
co-reside PyTorch, ONNX Runtime, and OpenVINO model objects.  The tensor-only
neural graph excludes deterministic STFT, mask scatter/application, iSTFT,
chunking, and overlap-add host operations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import sys
import time
from typing import Any

import numpy as np

SOURCE_FILENAME = "denoise_mel_band_roformer_aufr33_sdr_27.9959.ckpt"
SOURCE_SHA256 = "7c1c39191edc34e942ca7f2346ce6b6c0e1208a5f76349ffce6f696bd12910de"
SOURCE_SIZE = 913_097_300
SOURCE_REPOSITORY = "poiqazwsx/melband-roformer-denoise"
SOURCE_REVISION = "4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9"
CONFIG_SHA256 = "5d7d83b2e9d232da60941b717b0abdc345155d45cff3f79715cdb2790ba18c36"
MODEL_ID = "melband_roformer_denoise_aufr33"
GRAPH_NAME = "melband-roformer-denoise-neural"
GATHERED_WIDTH = 7_916
EXACT_FRAMES = 801
SEED = 0x03D3_0015


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_identity(path: Path, expected_size: int | None, expected_sha256: str) -> None:
    if not path.is_file():
        raise SystemExit(f"required source is unavailable: {path}")
    if expected_size is not None and path.stat().st_size != expected_size:
        raise SystemExit(f"source size mismatch for {path}")
    actual = sha256(path)
    if actual != expected_sha256:
        raise SystemExit(f"source SHA-256 mismatch for {path}: {actual}")


def load_yaml(path: Path) -> dict[str, Any]:
    import yaml

    class Loader(yaml.SafeLoader):
        pass

    Loader.add_constructor(
        "tag:yaml.org,2002:python/tuple",
        lambda loader, node: tuple(loader.construct_sequence(node)),
    )
    with path.open("r", encoding="utf-8") as file:
        value = yaml.load(file, Loader=Loader)
    if not isinstance(value, dict):
        raise SystemExit("RoFormer YAML did not decode to an object")
    return value


def verify_config(config: dict[str, Any]) -> None:
    expected = {
        "sample_rate": 44_100,
        "chunk_size": 352_800,
        "dim_t": EXACT_FRAMES,
        "hop_length": 441,
        "n_fft": 2_048,
        "num_channels": 2,
        "dim": 384,
        "depth": 6,
        "num_bands": 60,
        "target_instrument": "dry",
        "num_overlap": 4,
    }
    actual = {
        "sample_rate": config["audio"]["sample_rate"],
        "chunk_size": config["audio"]["chunk_size"],
        "dim_t": config["audio"]["dim_t"],
        "hop_length": config["audio"]["hop_length"],
        "n_fft": config["audio"]["n_fft"],
        "num_channels": config["audio"]["num_channels"],
        "dim": config["model"]["dim"],
        "depth": config["model"]["depth"],
        "num_bands": config["model"]["num_bands"],
        "target_instrument": config["training"]["target_instrument"],
        "num_overlap": config["inference"]["num_overlap"],
    }
    if actual != expected:
        raise SystemExit(f"exact Denoise config contract mismatch: {actual}")


def load_inputs(arguments: argparse.Namespace) -> dict[str, Any]:
    require_identity(arguments.checkpoint, SOURCE_SIZE, SOURCE_SHA256)
    require_identity(arguments.config, None, CONFIG_SHA256)
    config = load_yaml(arguments.config)
    verify_config(config)
    return config


def build_model(checkpoint: Path, config: dict[str, Any]):
    import torch
    from audio_separator.separator.uvr_lib_v5.roformer.mel_band_roformer import (
        MelBandRoformer,
    )

    model_config = dict(config["model"])
    # Eval-mode manual attention is source-equivalent and exports as portable
    # ONNX primitives instead of a fused PyTorch-only operator.
    model_config["flash_attn"] = False
    model = MelBandRoformer(**model_config)
    state = torch.load(checkpoint, map_location="cpu", mmap=True, weights_only=False)
    model.load_state_dict(state, strict=True, assign=True)
    model.eval()
    return model


def neural_island(model):
    import torch

    class NeuralIsland(torch.nn.Module):
        def __init__(self, source):
            super().__init__()
            self.band_split = source.band_split
            self.layers = source.layers
            self.mask_estimator = source.mask_estimators[0]

        def forward(self, gathered_stft):
            x = self.band_split(gathered_stft)
            for time_transformer, freq_transformer in self.layers:
                batch, frames, bands, width = x.shape
                x = x.permute(0, 2, 1, 3).reshape(batch * bands, frames, width)
                x = time_transformer(x)
                x = x.reshape(batch, bands, frames, width).permute(0, 2, 1, 3)
                x = x.reshape(batch * frames, bands, width)
                x = freq_transformer(x)
                x = x.reshape(batch, frames, bands, width)
            return self.mask_estimator(x)

    return NeuralIsland(model).eval()


def fixture(frames: int):
    import torch

    generator = torch.Generator(device="cpu").manual_seed(SEED)
    return torch.randn(
        1, frames, GATHERED_WIDTH, generator=generator, dtype=torch.float32
    ) * 0.05


def rss_bytes() -> int:
    with Path("/proc/self/status").open("r", encoding="utf-8") as file:
        for line in file:
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) * 1024
    raise RuntimeError("VmRSS is unavailable")


def peak_rss_bytes() -> int:
    # Linux reports ru_maxrss in KiB.
    return int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss) * 1024


def sync_file(path: Path) -> None:
    with path.open("rb") as file:
        os.fsync(file.fileno())


def sync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def atomic_json(path: Path, value: Any) -> None:
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    sync_file(temporary)
    temporary.replace(path)
    sync_directory(path.parent)


def relative_metrics(reference: np.ndarray, candidate: np.ndarray) -> dict[str, float]:
    difference = candidate.astype(np.float64) - reference.astype(np.float64)
    reference64 = reference.astype(np.float64)
    candidate64 = candidate.astype(np.float64)
    denominator = max(float(np.linalg.norm(reference64.ravel())), 1e-12)
    cosine_denominator = max(
        float(np.linalg.norm(reference64.ravel()) * np.linalg.norm(candidate64.ravel())),
        1e-12,
    )
    return {
        "max_abs": float(np.max(np.abs(difference))),
        "mean_abs": float(np.mean(np.abs(difference))),
        "relative_l2": float(np.linalg.norm(difference.ravel()) / denominator),
        "cosine": float(
            np.dot(reference64.ravel(), candidate64.ravel()) / cosine_denominator
        ),
    }


def require_graph_width(model) -> None:
    gathered_width = sum(model.band_split.dim_inputs)
    if gathered_width != len(model.freq_indices) * 2 or gathered_width != GATHERED_WIDTH:
        raise SystemExit(f"unexpected Denoise mel-band gather width: {gathered_width}")


def probe(arguments: argparse.Namespace) -> None:
    import torch

    config = load_inputs(arguments)
    model = build_model(arguments.checkpoint, config)
    graph = neural_island(model)
    require_graph_width(model)
    baseline = rss_bytes()
    measurements = []
    previous = 0
    for frames in arguments.frames:
        if frames <= previous or frames >= EXACT_FRAMES:
            raise SystemExit("probe frames must increase strictly and remain below 801")
        previous = frames
        started = time.monotonic()
        value = fixture(frames)
        with torch.inference_mode():
            output = graph(value)
        if output.shape != value.shape or not torch.isfinite(output).all().item():
            raise SystemExit(f"malformed probe output at T={frames}")
        del value, output
        measurements.append(
            {
                "frames": frames,
                "elapsed_seconds": time.monotonic() - started,
                "rss_bytes": rss_bytes(),
                "peak_rss_bytes": peak_rss_bytes(),
            }
        )
        if peak_rss_bytes() >= arguments.soft_stop_bytes:
            raise SystemExit("probe reached the configured soft memory stop")

    largest = measurements[-1]
    activation_peak = max(largest["peak_rss_bytes"] - baseline, 0)
    # Attention is quadratic in T.  Apply an additional 25% safety factor to
    # the observed activation high-water increase instead of linear scaling.
    projection = int(
        baseline
        + activation_peak * (EXACT_FRAMES / largest["frames"]) ** 2 * 1.25
    )
    result = {
        "phase": "memory-probe",
        "frames": [entry["frames"] for entry in measurements],
        "baseline_rss_bytes": baseline,
        "measurements": measurements,
        "projection_method": "baseline + observed_delta*(801/max_T)^2*1.25",
        "projected_exact_peak_bytes": projection,
        "soft_stop_bytes": arguments.soft_stop_bytes,
        "exact_reference_allowed": projection < arguments.soft_stop_bytes,
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))
    if not result["exact_reference_allowed"]:
        raise SystemExit("conservative exact-shape projection exceeds the soft stop")


def export_onnx(arguments: argparse.Namespace) -> None:
    import onnx
    import torch
    import audio_separator.separator.uvr_lib_v5.roformer.mel_band_roformer as source_module

    config = load_inputs(arguments)
    model = build_model(arguments.checkpoint, config)
    graph = neural_island(model)
    require_graph_width(model)
    value = fixture(arguments.export_frames)
    output = arguments.output
    data_name = f"{output.name}.data"
    if output.exists() or output.with_name(data_name).exists():
        raise SystemExit("refusing to replace an existing ONNX artifact")
    started = time.monotonic()
    with torch.inference_mode():
        torch.onnx.export(
            graph,
            (value,),
            output,
            input_names=["gathered_stft"],
            output_names=["gathered_mask"],
            opset_version=18,
            do_constant_folding=True,
            external_data=True,
            dynamo=False,
            dynamic_axes={
                "gathered_stft": {1: "frames"},
                "gathered_mask": {1: "frames"},
            },
        )
    # Force one auditable external-data file even though this graph is below
    # ONNX's mandatory 2 GiB externalization threshold.
    exported = onnx.load(output, load_external_data=True)
    onnx.external_data_helper.convert_model_to_external_data(
        exported,
        all_tensors_to_one_file=True,
        location=data_name,
        size_threshold=1024,
        convert_attribute=False,
    )
    onnx.save_model(exported, output)
    data_path = output.with_name(data_name)
    if not data_path.is_file():
        raise SystemExit("ONNX export did not produce required external data")
    sync_file(output)
    sync_file(data_path)
    sync_directory(output.parent)
    metadata = {
        "phase": "dynamic-export",
        "export_frames": arguments.export_frames,
        "dynamic_time_axis": True,
        "semantic_time_chunking": False,
        "graph_boundary": "band_split+transformers+mask_estimator",
        "onnx": {"path": str(output), "bytes": output.stat().st_size, "sha256": sha256(output)},
        "external_data": {
            "path": str(data_path),
            "bytes": data_path.stat().st_size,
            "sha256": sha256(data_path),
        },
        "source_implementation": {
            "path": str(Path(source_module.__file__).resolve()),
            "sha256": sha256(Path(source_module.__file__).resolve()),
        },
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, metadata)
    print(json.dumps(metadata, sort_keys=True, indent=2))


def check_onnx(arguments: argparse.Namespace) -> None:
    import onnx

    started = time.monotonic()
    onnx.checker.check_model(str(arguments.onnx), full_check=True)
    model = onnx.load(arguments.onnx, load_external_data=False)
    graph_input = model.graph.input[0]
    graph_output = model.graph.output[0]
    input_dims = graph_input.type.tensor_type.shape.dim
    output_dims = graph_output.type.tensor_type.shape.dim
    if input_dims[1].dim_param != "frames" or output_dims[1].dim_param != "frames":
        raise SystemExit("ONNX graph does not retain the required dynamic time axis")
    result = {
        "phase": "onnx-check",
        "input": [dimension.dim_param or dimension.dim_value for dimension in input_dims],
        "output": [dimension.dim_param or dimension.dim_value for dimension in output_dims],
        "node_count": len(model.graph.node),
        "initializer_count": len(model.graph.initializer),
        "full_check": True,
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def convert_openvino(arguments: argparse.Namespace) -> None:
    import openvino as ov

    xml = arguments.xml
    binary = xml.with_suffix(".bin")
    if xml.exists() or binary.exists():
        raise SystemExit("refusing to replace an existing OpenVINO artifact")
    started = time.monotonic()
    model = ov.convert_model(str(arguments.onnx))
    ov.save_model(model, xml, compress_to_fp16=False)
    sync_file(xml)
    sync_file(binary)
    sync_directory(xml.parent)
    result = {
        "phase": "openvino-convert",
        "precision": "fp32",
        "openvino": ov.get_version(),
        "xml": {"path": str(xml), "bytes": xml.stat().st_size, "sha256": sha256(xml)},
        "bin": {"path": str(binary), "bytes": binary.stat().st_size, "sha256": sha256(binary)},
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def exact_reference(arguments: argparse.Namespace) -> None:
    import torch

    probe_result = json.loads(arguments.probe_result.read_text(encoding="utf-8"))
    if not probe_result.get("exact_reference_allowed"):
        raise SystemExit("memory probe did not authorize an exact reference")
    config = load_inputs(arguments)
    model = build_model(arguments.checkpoint, config)
    graph = neural_island(model)
    require_graph_width(model)
    value = fixture(EXACT_FRAMES)
    started = time.monotonic()
    with torch.inference_mode():
        output = graph(value)
    if output.shape != value.shape or not torch.isfinite(output).all().item():
        raise SystemExit("exact PyTorch reference output is malformed")
    for path in (arguments.input, arguments.output):
        if path.exists():
            raise SystemExit(f"refusing to replace exact tensor: {path}")
    np.save(arguments.input, value.numpy())
    np.save(arguments.output, output.numpy())
    sync_file(arguments.input)
    sync_file(arguments.output)
    sync_directory(arguments.input.parent)
    result = {
        "phase": "exact-pytorch-reference",
        "frames": EXACT_FRAMES,
        "input": {"path": str(arguments.input), "shape": list(value.shape), "sha256": sha256(arguments.input)},
        "output": {"path": str(arguments.output), "shape": list(output.shape), "sha256": sha256(arguments.output)},
        "finite": True,
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def ort_parity(arguments: argparse.Namespace) -> None:
    import onnxruntime

    reference_input = np.load(arguments.input, mmap_mode="r")
    reference_output = np.load(arguments.reference, mmap_mode="r")
    started = time.monotonic()
    session = onnxruntime.InferenceSession(
        str(arguments.onnx), providers=["CPUExecutionProvider"]
    )
    candidate = session.run(["gathered_mask"], {"gathered_stft": reference_input})[0]
    metrics = relative_metrics(reference_output, candidate)
    if metrics["relative_l2"] > 2e-4 or metrics["cosine"] < 0.99999:
        raise SystemExit(f"ONNX Runtime exact parity failed: {metrics}")
    result = {
        "phase": "exact-onnxruntime-parity",
        "frames": EXACT_FRAMES,
        "metrics": metrics,
        "finite": bool(np.isfinite(candidate).all()),
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
        "onnxruntime": onnxruntime.__version__,
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def openvino_parity(arguments: argparse.Namespace) -> None:
    import openvino as ov

    reference_input = np.load(arguments.input, mmap_mode="r")
    reference_output = np.load(arguments.reference, mmap_mode="r")
    started = time.monotonic()
    core = ov.Core()
    compiled = core.compile_model(
        str(arguments.xml), "CPU", {"INFERENCE_PRECISION_HINT": "f32"}
    )
    candidate = compiled([reference_input])[0]
    metrics = relative_metrics(reference_output, candidate)
    if metrics["relative_l2"] > 3e-4 or metrics["cosine"] < 0.99998:
        raise SystemExit(f"OpenVINO CPU exact parity failed: {metrics}")
    result = {
        "phase": "exact-openvino-cpu-parity",
        "frames": EXACT_FRAMES,
        "metrics": metrics,
        "finite": bool(np.isfinite(candidate).all()),
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
        "openvino": ov.get_version(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def gpu_smoke(arguments: argparse.Namespace) -> None:
    import openvino as ov

    value = fixture(arguments.frames).numpy()
    started = time.monotonic()
    core = ov.Core()
    available = core.available_devices
    gpu_devices = [device for device in available if device == "GPU" or device.startswith("GPU.")]
    if not gpu_devices:
        raise SystemExit(f"Intel OpenVINO GPU is unavailable: {available}")
    compiled = core.compile_model(str(arguments.xml), gpu_devices[0])
    output = compiled([value])[0]
    if output.shape != value.shape or not np.isfinite(output).all():
        raise SystemExit("OpenVINO GPU smoke output is malformed")
    result = {
        "phase": "bounded-openvino-gpu-smoke",
        "device": gpu_devices[0],
        "frames": arguments.frames,
        "shape": list(output.shape),
        "finite": True,
        "elapsed_seconds": time.monotonic() - started,
        "process_peak_rss_bytes": peak_rss_bytes(),
        "openvino": ov.get_version(),
    }
    atomic_json(arguments.result, result)
    print(json.dumps(result, sort_keys=True, indent=2))


def add_sources(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--checkpoint", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)


def main() -> None:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)

    probe_parser = commands.add_parser("probe")
    add_sources(probe_parser)
    probe_parser.add_argument("--frames", required=True, nargs="+", type=int)
    probe_parser.add_argument("--soft-stop-bytes", required=True, type=int)
    probe_parser.add_argument("--result", required=True, type=Path)
    probe_parser.set_defaults(function=probe)

    export_parser = commands.add_parser("export")
    add_sources(export_parser)
    export_parser.add_argument("--export-frames", type=int, default=9)
    export_parser.add_argument("--output", required=True, type=Path)
    export_parser.add_argument("--result", required=True, type=Path)
    export_parser.set_defaults(function=export_onnx)

    check_parser = commands.add_parser("check-onnx")
    check_parser.add_argument("--onnx", required=True, type=Path)
    check_parser.add_argument("--result", required=True, type=Path)
    check_parser.set_defaults(function=check_onnx)

    convert_parser = commands.add_parser("convert")
    convert_parser.add_argument("--onnx", required=True, type=Path)
    convert_parser.add_argument("--xml", required=True, type=Path)
    convert_parser.add_argument("--result", required=True, type=Path)
    convert_parser.set_defaults(function=convert_openvino)

    reference_parser = commands.add_parser("reference")
    add_sources(reference_parser)
    reference_parser.add_argument("--probe-result", required=True, type=Path)
    reference_parser.add_argument("--input", required=True, type=Path)
    reference_parser.add_argument("--output", required=True, type=Path)
    reference_parser.add_argument("--result", required=True, type=Path)
    reference_parser.set_defaults(function=exact_reference)

    ort_parser = commands.add_parser("ort-parity")
    ort_parser.add_argument("--onnx", required=True, type=Path)
    ort_parser.add_argument("--input", required=True, type=Path)
    ort_parser.add_argument("--reference", required=True, type=Path)
    ort_parser.add_argument("--result", required=True, type=Path)
    ort_parser.set_defaults(function=ort_parity)

    ov_parser = commands.add_parser("openvino-parity")
    ov_parser.add_argument("--xml", required=True, type=Path)
    ov_parser.add_argument("--input", required=True, type=Path)
    ov_parser.add_argument("--reference", required=True, type=Path)
    ov_parser.add_argument("--result", required=True, type=Path)
    ov_parser.set_defaults(function=openvino_parity)

    gpu_parser = commands.add_parser("gpu-smoke")
    gpu_parser.add_argument("--xml", required=True, type=Path)
    gpu_parser.add_argument("--frames", type=int, default=9)
    gpu_parser.add_argument("--result", required=True, type=Path)
    gpu_parser.set_defaults(function=gpu_smoke)

    arguments = parser.parse_args()
    arguments.function(arguments)


if __name__ == "__main__":
    main()
