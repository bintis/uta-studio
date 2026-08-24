#!/usr/bin/env python3
"""Export and validate exact-context Harmony as bounded CPU/GPU IR islands.

The product contract remains T=801. Time attention runs in six independent
10-band microbatches and frequency attention in independent 64-frame batches;
neither operation splits its attention sequence. Band split and eight bounded
mask-estimator groups run on CPU. Transformer islands use rolling GPU
residency. ONNX and the pinned checkpoint are offline conversion evidence only.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import importlib.util
import json
import os
import resource
import time
from pathlib import Path
from typing import Any

import numpy as np

MODEL_ID = "melband_roformer_harmony"
SOURCE_FILENAME = "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt"
FRAMES = 801
CHUNK_SAMPLES = 352_800
OVERLAP = 4
BANDS = 60
DIM = 384
GATHERED_WIDTH = 7916
DEPTH = 6
TIME_BATCH = 10
FREQUENCY_BATCH = 64
MASK_GROUPS = ((0, 8), (8, 16), (16, 24), (24, 32), (32, 40), (40, 48), (48, 56), (56, 60))
SOURCE_SHA256 = "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813"
CONFIG_SHA256 = "b35077d94861f068097cce1a5e54633c055e7dcc2613eade4e4dc7c7c9c3f48b"
MONOLITHIC_RECIPE_SHA256 = "4d428de5af93bb5163e19cbc0aea197b233c254d60d7e318347cdefe402c52ce"
LAYOUT_INDICES_SHA256 = "c087bfc8e1a110a16a7aa998de5fe43b025ea08de0e4606c7b80e258b1ed5ecc"
LAYOUT_COUNTS_SHA256 = "41947c540f2511f98bb2530176d9a9a3576e5a954135dbf5ef207247e0933683"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_hash(path: Path, expected: str) -> None:
    if not path.is_file():
        raise SystemExit(f"required file is unavailable: {path}")
    actual = sha256(path)
    if actual != expected:
        raise SystemExit(f"identity mismatch for {path}: {actual}")


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    with temporary.open("rb") as file:
        os.fsync(file.fileno())
    temporary.replace(path)


def peak_rss_bytes() -> int:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024


def base_converter():
    path = Path(__file__).with_name("convert-melband-roformer-harmony-to-ir.py")
    require_hash(path, MONOLITHIC_RECIPE_SHA256)
    spec = importlib.util.spec_from_file_location("inst_v2_monolithic_recipe", path)
    if spec is None or spec.loader is None:
        raise SystemExit("could not load the audited Harmony source recipe")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_model(checkpoint: Path, config_path: Path):
    require_hash(checkpoint, SOURCE_SHA256)
    require_hash(config_path, CONFIG_SHA256)
    base = base_converter()
    config = base.load_yaml(config_path)
    base.verify_config(config)
    return base.build_model(checkpoint, config)


def specs() -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = [{"name": "band-split", "kind": "band", "device": "CPU"}]
    for layer in range(DEPTH):
        result.append({"name": f"layer-{layer:02}-time", "kind": "time", "layer": layer, "device": "GPU"})
        result.append({"name": f"layer-{layer:02}-freq", "kind": "freq", "layer": layer, "device": "GPU"})
    for start, end in MASK_GROUPS:
        result.append({"name": f"mask-{start:02}-{end - 1:02}", "kind": "mask", "start": start, "end": end, "device": "CPU"})
    return result


def onnx_path(directory: Path, name: str) -> Path:
    return directory / f"harmony-{name}.onnx"


def xml_path(directory: Path, name: str) -> Path:
    return directory / f"harmony-{name}.xml"


def module_and_fixture(model, spec: dict[str, Any]):
    import torch

    class MaskGroup(torch.nn.Module):
        def __init__(self, modules):
            super().__init__()
            self.mlps = torch.nn.ModuleList(modules)

        def forward(self, features):
            return torch.cat([mlp(band) for band, mlp in zip(features.unbind(dim=-2), self.mlps)], dim=-1)

    generator = torch.Generator(device="cpu").manual_seed(0x04A40017)
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
    start, end = spec["start"], spec["end"]
    value = torch.randn(1, FRAMES, end - start, DIM, generator=generator) * 0.05
    module = MaskGroup(list(model.mask_estimators[0].to_freqs[start:end])).eval()
    return module, value, "features", "gathered_mask_part"


def export(arguments: argparse.Namespace) -> None:
    import torch

    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    model = load_model(arguments.checkpoint, arguments.config)
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
        pieces = [invoke(np.ascontiguousarray(source[start : start + TIME_BATCH])) for start in range(0, BANDS, TIME_BATCH)]
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
    value = np.asarray(np.load(arguments.input, mmap_mode="r"))
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
        if kind == "band":
            value = invoke(np.ascontiguousarray(value))
        elif kind in ("time", "freq"):
            value = run_transform_stage(value, kind, invoke)
        else:
            part = invoke(np.ascontiguousarray(value[:, :, spec["start"] : spec["end"], :]))
            mask_parts.append(part)
        records.append({**spec, "elapsed_seconds": time.monotonic() - stage_started})
        del invoke, owner
        gc.collect()
    candidate = np.concatenate(mask_parts, axis=-1)
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


def package(arguments: argparse.Namespace) -> None:
    require_hash(arguments.config, CONFIG_SHA256)
    require_hash(Path(__file__).resolve(), arguments.recipe_sha256)
    conversion = json.loads(arguments.conversion_result.read_text(encoding="utf-8"))
    if conversion.get("phase") != "split-openvino-conversion":
        raise SystemExit("Harmony split conversion evidence is invalid")
    declared = conversion.get("islands")
    expected = specs()
    if not isinstance(declared, list) or len(declared) != len(expected):
        raise SystemExit("Harmony split conversion island count mismatch")
    islands = []
    for record, spec in zip(declared, expected):
        if any(record.get(key) != value for key, value in spec.items()):
            raise SystemExit(f"Harmony split conversion order mismatch: {record}")
        xml = xml_path(arguments.artifact_dir, spec["name"])
        binary = xml.with_suffix(".bin")
        for path, identity in ((xml, record["xml"]), (binary, record["bin"])):
            if (
                not path.is_file()
                or path.stat().st_size != identity["bytes"]
                or sha256(path) != identity["sha256"]
            ):
                raise SystemExit(f"Harmony split file identity mismatch: {path}")
        islands.append(
            {
                **spec,
                "xml": {"filename": xml.name, "bytes": xml.stat().st_size, "sha256": sha256(xml)},
                "bin": {"filename": binary.name, "bytes": binary.stat().st_size, "sha256": sha256(binary)},
            }
        )
    parity = {}
    for name, path in (
        ("ort", arguments.ort_result),
        ("openvino_cpu", arguments.cpu_result),
        ("openvino_product", arguments.product_result),
    ):
        value = json.loads(path.read_text(encoding="utf-8"))
        metrics_value = value.get("metrics")
        if not isinstance(metrics_value, dict):
            raise SystemExit(f"Harmony split parity evidence is invalid: {path}")
        parity[name] = metrics_value
    manifest = {
        "schema_version": 2,
        "resource": f"model:{MODEL_ID}",
        "capability": "audio.lead_isolate",
        "semantic_output": "lead_vocal+backing_vocal_residual",
        "semantic_contract": {
            "input": "all_vocals",
            "primary": "lead_vocal",
            "residual": "vocal_residual",
            "residual_formula": "all_vocals_minus_lead_vocal",
        },
        "backend": "OpenVINO FP32 explicit CPU/GPU split IR",
        "source": {
            "filename": SOURCE_FILENAME,
            "checkpoint_sha256": SOURCE_SHA256,
            "config_sha256": CONFIG_SHA256,
            "checkpoint_license": "unresolved",
        },
        "conversion_recipe": {
            "filename": Path(__file__).name,
            "sha256": arguments.recipe_sha256,
            "monolithic_source_recipe_sha256": MONOLITHIC_RECIPE_SHA256,
        },
        "exact_contract": {
            "sample_rate": 44_100,
            "channels": 2,
            "chunk_samples": CHUNK_SAMPLES,
            "frames": FRAMES,
            "hop_length": 441,
            "overlap": OVERLAP,
            "bands": BANDS,
            "feature_dim": DIM,
            "gathered_width": GATHERED_WIDTH,
            "time_microbatch": TIME_BATCH,
            "frequency_microbatch": FREQUENCY_BATCH,
            "full_time_context_preserved": True,
            "rolling_gpu_residency": True,
        },
        "layout": {
            "frequency_indices_sha256": LAYOUT_INDICES_SHA256,
            "bands_per_frequency_sha256": LAYOUT_COUNTS_SHA256,
        },
        "exact_parity": parity,
        "islands": islands,
    }
    if arguments.manifest.exists():
        raise SystemExit("refusing to replace Harmony split manifest")
    atomic_json(arguments.manifest, manifest)
    print(json.dumps(manifest, indent=2, sort_keys=True))


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    export_command = commands.add_parser("export")
    export_command.add_argument("--checkpoint", required=True, type=Path)
    export_command.add_argument("--config", required=True, type=Path)
    export_command.add_argument("--output-dir", required=True, type=Path)
    export_command.add_argument("--result", required=True, type=Path)
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
    package_command = commands.add_parser("package")
    package_command.add_argument("--artifact-dir", required=True, type=Path)
    package_command.add_argument("--config", required=True, type=Path)
    package_command.add_argument("--conversion-result", required=True, type=Path)
    package_command.add_argument("--ort-result", required=True, type=Path)
    package_command.add_argument("--cpu-result", required=True, type=Path)
    package_command.add_argument("--product-result", required=True, type=Path)
    package_command.add_argument("--recipe-sha256", required=True)
    package_command.add_argument("--manifest", required=True, type=Path)
    package_command.set_defaults(function=package)
    return root


if __name__ == "__main__":
    arguments = parser().parse_args()
    arguments.function(arguments)
