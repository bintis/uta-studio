#!/usr/bin/env python3
"""Audit and convert pinned ROSVOT P0 without RWBD.

P0 consumes caller-supplied TimedTranscript word timing and the separately
pinned shared frontend/annotation-RMVPE generation. This model-specific recipe
exports only the ROSVOT frame and note-pitch graphs. Boundary regulation and
variable note aggregation remain native host operations.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import resource
import shutil
import stat
import sys
import time
import zipfile

sys.dont_write_bytecode = True
from pathlib import Path, PurePosixPath
from typing import Any

SOURCE_REVISION = "3c8332bf43adae35f6e4d64971862f2f6139b310"
SOURCE_REPOSITORY = "https://github.com/RickyL-2000/ROSVOT"
SOURCE_MANIFEST_SHA256 = "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8"
ARCHIVE_SHA256 = "b6055e81315b93415c9bd7fc48e10a28a3da1bea960cab7385483bd7443ba852"
ROSVOT_CHECKPOINT_SHA256 = "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb"
ROSVOT_CONFIG_SHA256 = "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2"
ANNOTATION_RMVPE_SHA256 = "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2"
SHARED_FRONTEND_PROFILE = "shared-singing-frontend-24k-v1"
SELECTED_MEMBERS = {
    "checkpoints/rosvot/model.pt": ROSVOT_CHECKPOINT_SHA256,
    "checkpoints/rosvot/config.yaml": ROSVOT_CONFIG_SHA256,
    "checkpoints/rmvpe/model.pt": ANNOTATION_RMVPE_SHA256,
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require(path: Path, _expected: str, label: str) -> None:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"{label} is unavailable: {path}")


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


def validate_source(source_root: Path) -> dict[str, Any]:
    package = source_root.parent
    manifest_path = package / "source-manifest.json"
    require(manifest_path, SOURCE_MANIFEST_SHA256, "ROSVOT source manifest")
    manifest = json.loads(manifest_path.read_text())
    if (
        manifest.get("schema_version") != 1
        or manifest.get("repository") != SOURCE_REPOSITORY
        or manifest.get("commit") != SOURCE_REVISION
        or manifest.get("source_license") != "MIT"
        or not isinstance(manifest.get("files"), list)
    ):
        raise SystemExit("ROSVOT source manifest contract mismatch")
    declared: set[str] = set()
    for item in manifest["files"]:
        relative = PurePosixPath(item["path"])
        if relative.is_absolute() or ".." in relative.parts or relative.as_posix() in declared:
            raise SystemExit(f"unsafe or duplicate source path: {relative}")
        declared.add(relative.as_posix())
        path = source_root.joinpath(*relative.parts)
        require(path, item["sha256"], f"ROSVOT source {relative}")
        if path.stat().st_size != item["bytes"]:
            raise SystemExit(f"ROSVOT source size mismatch: {relative}")
    actual = {
        path.relative_to(source_root).as_posix()
        for path in source_root.rglob("*")
        if path.is_file() and not path.is_symlink()
    }
    if actual != declared:
        raise SystemExit("ROSVOT source tree contains undeclared or missing files")
    return manifest


def safe_members(archive: zipfile.ZipFile) -> dict[str, zipfile.ZipInfo]:
    result: dict[str, zipfile.ZipInfo] = {}
    for info in archive.infolist():
        path = PurePosixPath(info.filename)
        mode = info.external_attr >> 16
        unsafe_type = mode and not (stat.S_ISREG(mode) or stat.S_ISDIR(mode))
        if path.is_absolute() or ".." in path.parts or unsafe_type or info.filename in result:
            raise SystemExit(f"unsafe ZIP member: {info.filename}")
        result[info.filename] = info
    return result


def audit(arguments: argparse.Namespace) -> None:
    started = time.monotonic()
    source_manifest = validate_source(arguments.source_dir)
    require(arguments.checkpoints_zip, ARCHIVE_SHA256, "ROSVOT checkpoint archive")
    if arguments.output_dir.exists():
        raise SystemExit(f"refusing to replace audit workspace: {arguments.output_dir}")
    temporary = arguments.output_dir.with_name(arguments.output_dir.name + ".tmp")
    if temporary.exists():
        shutil.rmtree(temporary)
    temporary.mkdir(parents=True)
    consumed = []
    try:
        with zipfile.ZipFile(arguments.checkpoints_zip) as archive:
            members = safe_members(archive)
            if any(name not in members or members[name].is_dir() for name in SELECTED_MEMBERS):
                raise SystemExit("ROSVOT archive omits a loader-selected P0 member")
            for name in SELECTED_MEMBERS:
                destination = temporary.joinpath(*PurePosixPath(name).parts)
                destination.parent.mkdir(parents=True, exist_ok=True)
                digest = hashlib.sha256()
                with archive.open(members[name]) as source, destination.open("xb") as target:
                    for block in iter(lambda: source.read(1024 * 1024), b""):
                        digest.update(block)
                        target.write(block)
                    target.flush()
                    os.fsync(target.fileno())
                actual = digest.hexdigest()
                consumed.append({"path": name, "bytes": destination.stat().st_size, "sha256": actual})
        atomic_json(
            temporary / "audit-manifest.json",
            {
                "schema_version": 1,
                "profile": "rosvot-p0-timed-transcript-v1",
                "upstream": {"repository": SOURCE_REPOSITORY, "commit": SOURCE_REVISION},
                "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
                "source_license": "MIT",
                "checkpoint_archive_sha256": ARCHIVE_SHA256,
                "checkpoint_explicit_license": None,
                "word_boundary_source": "timed_transcript_required",
                "rwbd_included": False,
                "annotation_rmvpe_sha256": ANNOTATION_RMVPE_SHA256,
                "consumed_members": consumed,
            },
        )
        temporary.replace(arguments.output_dir)
    except BaseException:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    atomic_json(
        arguments.result,
        {
            "phase": "rosvot-p0-audit",
            "source_files": len(source_manifest["files"]),
            "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
            "checkpoint_archive_sha256": ARCHIVE_SHA256,
            "consumed_members": consumed,
            "rwbd_included": False,
            "elapsed_seconds": time.monotonic() - started,
        },
    )


def validate_shared_frontend(path: Path) -> str:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"shared singing frontend manifest is missing: {path}")
    value = json.loads(path.read_text())
    mel = value.get("native_mel", {})
    files = value.get("files", {})
    names = [name for name in files if isinstance(name, str)] if isinstance(files, dict) else []
    stems = {Path(name).stem for name in names}
    file_contract_valid = (
        len(names) == 3
        and len(stems) == 1
        and all(Path(name).suffix in {".onnx", ".xml", ".bin"} for name in names)
        and {Path(name).suffix for name in names} == {".onnx", ".xml", ".bin"}
        and next(iter(stems)).startswith("annotation-rmvpe-t")
        and next(iter(stems)).removeprefix("annotation-rmvpe-t").isdigit()
        and int(next(iter(stems)).removeprefix("annotation-rmvpe-t")) > 0
        and int(next(iter(stems)).removeprefix("annotation-rmvpe-t")) % 32 == 0
    )
    if (value.get("schema_version") != 1
            or value.get("profile") != SHARED_FRONTEND_PROFILE
            or value.get("source_revision") != SOURCE_REVISION
            or mel != {"sample_rate": 24000, "fft_size": 512, "hop_size": 128,
                       "mel_bins": 80, "rosvot_prefix_bins": 40}
            or not file_contract_valid):
        raise SystemExit("shared singing frontend generation identity mismatch")
    return sha256(path)


def load_model(source: Path, audit_root: Path):
    import torch

    validate_source(source)
    checkpoint = audit_root / "checkpoints/rosvot/model.pt"
    config = audit_root / "checkpoints/rosvot/config.yaml"
    require(checkpoint, ROSVOT_CHECKPOINT_SHA256, "ROSVOT checkpoint")
    require(config, ROSVOT_CONFIG_SHA256, "ROSVOT config")
    audit_manifest = json.loads((audit_root / "audit-manifest.json").read_text())
    if audit_manifest.get("profile") != "rosvot-p0-timed-transcript-v1" or audit_manifest.get("rwbd_included"):
        raise SystemExit("ROSVOT audit workspace does not select TimedTranscript P0")

    sys.path.insert(0, str(source))
    from modules.rosvot.rosvot import MidiExtractor
    from utils.commons.hparams import set_hparams

    hparams = set_hparams(
        config=str(config), print_hparams=False, global_hparams=False, root_dir=str(source)
    )
    expected = {
        "audio_sample_rate": 24000,
        "fft_size": 512,
        "win_size": 512,
        "hop_size": 128,
        "use_mel_bins": 40,
        "hidden_size": 256,
        "frames_multiple": 16,
        "use_pitch_embed": True,
        "updown_rates": "2-2-2-2",
        "note_num": 85,
        "note_start": 30,
    }
    if any(hparams.get(key) != value for key, value in expected.items()):
        raise SystemExit("ROSVOT P0 source config contract mismatch")
    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    model = MidiExtractor(hparams).eval()
    model.load_state_dict(saved["state_dict"]["model"], strict=True)
    return model


def wrappers(model, frames: int, note_bucket: int):
    import torch

    class FrameGraph(torch.nn.Module):
        def __init__(self, owner):
            super().__init__()
            self.owner = owner

        def forward(self, mel, pitch, uv, word_boundary):
            m = self.owner
            features = m.net(m.run_encoder(mel, word_boundary, pitch, uv, None))
            logits = torch.clamp(
                m.note_bd_out(features).squeeze(-1) / m.note_bd_temperature,
                min=-16.0,
                max=16.0,
            )
            attention = torch.sigmoid(m.pitch_decoder.multihead_dot_attn(features))
            weighted = (features.unsqueeze(-1) * attention.unsqueeze(-2)).mean(-1)
            return logits, attention.mean(-1), weighted

    class PitchGraph(torch.nn.Module):
        def __init__(self, owner):
            super().__init__()
            self.post = owner.pitch_decoder.post
            self.pitch_out = owner.pitch_decoder.pitch_out
            self.temperature = float(owner.pitch_decoder.pitch_temperature)

        def forward(self, note_features):
            return self.pitch_out(self.post(note_features)) / self.temperature

    generator = torch.Generator().manual_seed(0x20560)
    return {
        "frame": (
            FrameGraph(model).eval(),
            [
                torch.randn(1, frames, 40, generator=generator) * 0.1,
                torch.randint(1, 256, (1, frames), generator=generator),
                torch.zeros(1, frames, dtype=torch.long),
                torch.zeros(1, frames, dtype=torch.long),
            ],
            ["mel", "pitch_coarse", "uv", "timed_transcript_word_boundary"],
            ["note_boundary_logits", "frame_note_attention", "weighted_frame_features"],
        ),
        "pitch": (
            PitchGraph(model).eval(),
            [torch.randn(1, note_bucket, 256, generator=generator) * 0.1],
            ["note_features"],
            ["note_pitch_logits"],
        ),
    }


def export(arguments: argparse.Namespace) -> None:
    import numpy as np
    import torch

    if arguments.frames <= 0 or arguments.frames % 16:
        raise SystemExit("ROSVOT frames must be divisible by 16")
    if arguments.note_bucket <= 1:
        raise SystemExit("note bucket must be greater than one")
    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    shared_frontend_manifest_sha256 = validate_shared_frontend(arguments.shared_frontend_manifest)
    model = load_model(arguments.source_dir, arguments.audit_dir)
    records = []
    for name, (module, inputs, input_names, output_names) in wrappers(
        model, arguments.frames, arguments.note_bucket
    ).items():
        output = arguments.output_dir / f"rosvot-{name}-t{arguments.frames}-n{arguments.note_bucket}.onnx"
        if output.exists():
            raise SystemExit(f"refusing to replace {output}")
        started = time.monotonic()
        with torch.inference_mode():
            reference = module(*inputs)
            if not isinstance(reference, tuple):
                reference = (reference,)
            torch.onnx.export(
                module,
                tuple(inputs),
                output,
                input_names=input_names,
                output_names=output_names,
                opset_version=18,
                do_constant_folding=True,
                external_data=True,
                dynamo=False,
            )
        for index, value in enumerate(inputs):
            np.save(arguments.output_dir / f"{name}-input-{index}.npy", value.numpy())
        for index, value in enumerate(reference):
            np.save(arguments.output_dir / f"{name}-reference-{index}.npy", value.numpy())
        records.append(
            {
                "name": name,
                "onnx": {"filename": output.name, "bytes": output.stat().st_size, "sha256": sha256(output)},
                "inputs": input_names,
                "outputs": output_names,
                "output_shapes": [list(value.shape) for value in reference],
                "elapsed_seconds": time.monotonic() - started,
            }
        )
    atomic_json(
        arguments.result,
        {
            "phase": "rosvot-p0-timed-transcript-split-export",
            "source_revision": SOURCE_REVISION,
            "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
            "checkpoint_sha256": ROSVOT_CHECKPOINT_SHA256,
            "config_sha256": ROSVOT_CONFIG_SHA256,
            "annotation_rmvpe_sha256": ANNOTATION_RMVPE_SHA256,
            "frames": arguments.frames,
            "note_bucket": arguments.note_bucket,
            "shared_frontend_profile": SHARED_FRONTEND_PROFILE,
            "shared_frontend_manifest_sha256": shared_frontend_manifest_sha256,
            "word_boundary_source": "timed_transcript_required",
            "rwbd_included": False,
            "host_owned": ["timed_transcript_projection", "boundary_regulation", "variable_note_aggregation"],
            "graphs": records,
            "torch": torch.__version__,
            "process_peak_rss_bytes": peak_rss(),
        },
    )


def convert(arguments: argparse.Namespace) -> None:
    import openvino as ov

    records = []
    for name in ("frame", "pitch"):
        source = next(arguments.artifact_dir.glob(f"rosvot-{name}-*.onnx"), None)
        if source is None:
            raise SystemExit(f"missing {name} ONNX")
        xml = source.with_suffix(".xml")
        if xml.exists() or xml.with_suffix(".bin").exists():
            raise SystemExit(f"refusing to replace {xml}")
        model = ov.convert_model(source)
        ov.save_model(model, xml, compress_to_fp16=False)
        records.append(
            {
                "name": name,
                "xml": {"filename": xml.name, "bytes": xml.stat().st_size, "sha256": sha256(xml)},
                "bin": {
                    "filename": xml.with_suffix(".bin").name,
                    "bytes": xml.with_suffix(".bin").stat().st_size,
                    "sha256": sha256(xml.with_suffix(".bin")),
                },
            }
        )
    atomic_json(
        arguments.result,
        {
            "phase": "rosvot-p0-timed-transcript-openvino-conversion",
            "graphs": records,
            "rwbd_included": False,
            "openvino": ov.get_version(),
            "process_peak_rss_bytes": peak_rss(),
        },
    )


def metrics(reference: np.ndarray, candidate: np.ndarray) -> dict[str, float]:
    import numpy as np

    difference = candidate.astype(np.float64) - reference.astype(np.float64)
    denominator = max(float(np.linalg.norm(reference.astype(np.float64))), 1e-12)
    return {
        "max_abs": float(np.max(np.abs(difference))),
        "mean_abs": float(np.mean(np.abs(difference))),
        "relative_l2": float(np.linalg.norm(difference) / denominator),
    }


def parity(arguments: argparse.Namespace) -> None:
    import numpy as np

    records = []
    if arguments.backend == "ort":
        if arguments.devices != "cpu":
            raise SystemExit("ORT parity is CPU-only")
        import onnxruntime as ort
    else:
        import openvino as ov
        core = ov.Core()
    for name in ("frame", "pitch"):
        inputs = [np.load(path) for path in sorted(arguments.artifact_dir.glob(f"{name}-input-*.npy"))]
        references = [np.load(path) for path in sorted(arguments.artifact_dir.glob(f"{name}-reference-*.npy"))]
        if arguments.backend == "ort":
            source = next(arguments.artifact_dir.glob(f"rosvot-{name}-*.onnx"))
            session = ort.InferenceSession(str(source), providers=["CPUExecutionProvider"])
            outputs = session.run(None, {port.name: value for port, value in zip(session.get_inputs(), inputs)})
        else:
            source = next(arguments.artifact_dir.glob(f"rosvot-{name}-*.xml"))
            device = "GPU" if arguments.devices == "product" else "CPU"
            compiled = core.compile_model(
                source,
                device,
                {"INFERENCE_PRECISION_HINT": "f32", "EXECUTION_MODE_HINT": "ACCURACY"},
            )
            result = compiled(inputs)
            outputs = [np.asarray(result[index]).copy() for index in range(len(references))]
        observed = [metrics(a, b) for a, b in zip(references, outputs)]
        if any(not all(np.isfinite(value) for value in result.values())
               or result["relative_l2"] > 5e-5 or result["max_abs"] > 1e-3
               for result in observed):
            raise SystemExit(f"ROSVOT {name} parity failed: {observed}")
        records.append({"name": name, "metrics": observed})
    atomic_json(
        arguments.result,
        {
            "phase": f"rosvot-p0-timed-transcript-{arguments.backend}-{arguments.devices}-parity",
            "accepted": True,
            "graphs": records,
            "rwbd_included": False,
            "process_peak_rss_bytes": peak_rss(),
        },
    )


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    commands = root.add_subparsers(dest="command", required=True)
    command = commands.add_parser("audit")
    command.add_argument("--source-dir", type=Path, default=Path("third_party/rosvot/upstream"))
    command.add_argument("--checkpoints-zip", type=Path, default=Path("checkpoints.zip"))
    command.add_argument("--output-dir", type=Path, required=True)
    command.add_argument("--result", type=Path, required=True)
    command.set_defaults(function=audit)
    command = commands.add_parser("export")
    command.add_argument("--source-dir", type=Path, default=Path("third_party/rosvot/upstream"))
    command.add_argument("--audit-dir", type=Path, required=True)
    command.add_argument("--shared-frontend-manifest", type=Path, required=True)
    command.add_argument("--output-dir", type=Path, required=True)
    command.add_argument("--frames", type=int, default=256)
    command.add_argument("--note-bucket", type=int, default=32)
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
    return root


if __name__ == "__main__":
    args = parser().parse_args()
    args.function(args)
