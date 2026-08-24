#!/usr/bin/env python3
"""Convert pinned Chinese STARS into P0 Stage A/B/C OpenVINO islands.

This recipe never exports monolithic STARS.forward(train=False). Native code
must consume the fixed shared frontend/annotation-RMVPE generation and own
pitch adaptation, phoneme/word Viterbi, boundary regulation and variable-length
note cropping. Technique/style heads are intentionally outside
P0. Export fixtures are conversion evidence, not a product semantic golden.
"""
from __future__ import annotations
import argparse, hashlib, json, os, resource, sys, time
from pathlib import Path
from typing import Any
import numpy as np

CHECKPOINT_SHA256 = "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c"
ANNOTATION_RMVPE_SHA256 = "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2"
SHARED_FRONTEND_PROFILE = "shared-singing-frontend-24k-v1"
SHARED_SOURCE_REVISION = "3c8332bf43adae35f6e4d64971862f2f6139b310"
SHARED_SOURCE_MANIFEST_SHA256 = "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8"
G2P_PROFILE = "stars-chinese-g2p-pypinyin-0.55.0-v1"
G2P_ASSET_SHA256 = "289fcbcddfa8e5a1a911419af48ef36ddc08736aef7818e2c9321bdb331a94cc"
PHONE_SET_SHA256 = "8767ab69222297499de3c109598fcfcabaf9585211a2ed4f5797dc944dca82a7"
SOURCE_HASHES = {
    "configs/base.yaml": "1f0ce90cd81efc2d8cc27a38e89481a0e1096a647f01a9f4c6db392e01813b78",
    "configs/stars_chinese.yaml": "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91",
    "modules/stars/stars.py": "3dc87492b08b66063e48333618bd4b7bec03f6277c45ebe080ec42d173355ac5",
    "modules/stars/utils.py": "c346cb82a5c56bd8bd5f21a08fe7e5c6fccd780cec4d99317709ece6ddae5230",
    "chinese_phone_set.json": "8767ab69222297499de3c109598fcfcabaf9585211a2ed4f5797dc944dca82a7",
}
SOURCE_REVISION = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require(path: Path, _expected: str) -> None:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"required file is unavailable: {path}")


def atomic_json(path: Path, value: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    with temporary.open("rb") as file:
        os.fsync(file.fileno())
    temporary.replace(path)


def peak_rss() -> int:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024


def validate_g2p_asset(path: Path) -> str:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"native Chinese G2P asset is missing: {path}")
    value = json.loads(path.read_text())
    if (value.get("schema_version") != 1 or value.get("profile") != G2P_PROFILE
            or value.get("source_revision") != SOURCE_REVISION
            or value.get("generator") != {"pypinyin": "0.55.0", "jieba": "0.42.1"}
            or value.get("runtime") != "native_json_asset_only"
            or not value.get("characters")):
        raise SystemExit("native Chinese G2P asset identity mismatch")
    return sha256(path)


def validate_shared_frontend(path: Path) -> str:
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"shared singing frontend manifest is missing: {path}")
    value = json.loads(path.read_text())
    files = value.get("files", {})
    names = [name for name in files if isinstance(name, str)] if isinstance(files, dict) else []
    stems = {Path(name).stem for name in names}
    file_contract_valid = (
        len(names) == 3 and len(stems) == 1
        and {Path(name).suffix for name in names} == {".onnx", ".xml", ".bin"}
        and next(iter(stems)).startswith("annotation-rmvpe-t")
        and next(iter(stems)).removeprefix("annotation-rmvpe-t").isdigit()
        and int(next(iter(stems)).removeprefix("annotation-rmvpe-t")) > 0
        and int(next(iter(stems)).removeprefix("annotation-rmvpe-t")) % 32 == 0
    )
    if (value.get("schema_version") != 1
            or value.get("profile") != SHARED_FRONTEND_PROFILE
            or value.get("source_revision") != SHARED_SOURCE_REVISION
            or value.get("native_mel") != {"sample_rate": 24000, "fft_size": 512,
                                           "hop_size": 128, "mel_bins": 80,
                                           "rosvot_prefix_bins": 40}
            or not file_contract_valid):
        raise SystemExit("shared singing frontend generation identity mismatch")
    return sha256(path)


def load_model(source: Path, checkpoint: Path):
    import torch
    require(checkpoint, CHECKPOINT_SHA256)
    for relative, digest in SOURCE_HASHES.items():
        require(source / relative, digest)
    sys.path.insert(0, str(source))
    from utils.commons.hparams import hparams, set_hparams
    set_hparams(config=str(source / "configs/stars_chinese.yaml"), print_hparams=False,
                global_hparams=True, root_dir=str(source))
    from modules.stars.stars import STARS
    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    if saved.get("global_step") != 200_000:
        raise SystemExit("STARS checkpoint step mismatch")
    model = STARS(hparams)
    model.load_state_dict(saved["state_dict"]["model"], strict=True)
    return model.eval()


def wrappers(model, frames: int, note_bucket: int):
    import torch

    class StageA(torch.nn.Module):
        def __init__(self, owner): super().__init__(); self.owner = owner
        def forward(self, mel, pitch, uv, mask):
            m = self.owner
            mel_embed = m.mel_proj(mel.transpose(1, 2)).transpose(1, 2)
            mel_embed = m.mel_encoder(mel_embed) * mask.unsqueeze(-1)
            mel_embed = mel_embed + (m.pitch_embed(pitch) + m.uv_embed(uv)) * mask.unsqueeze(-1)
            prosody = m.prosody_extractor_utter(mel_embed, no_vq=True)
            feat = m.l1_utter(torch.cat([prosody, m.embed_positions(prosody[:, :, 0])], -1))
            ret = {}; m.ph_frame_predictor(feat, ret, train=False)
            raw = m.note_frame_predictor.note_head(feat)
            note_bd = torch.clamp(raw[:, :, 0] / m.note_frame_predictor.note_bd_temperature,
                                  min=-16.0, max=16.0)
            note_frame = raw[:, :, 1:] / m.note_frame_predictor.note_temperature
            return (mel_embed, feat, ret["ph_bd_logits"], ret["ph_frame_logits"],
                    note_bd, note_frame)

    class StageB(torch.nn.Module):
        def __init__(self, owner): super().__init__(); self.owner = owner
        @staticmethod
        def expand(value, mapping):
            value = torch.nn.functional.pad(value, [0, 0, 1, 0])
            return torch.gather(value, 1, mapping[..., None].repeat(1, 1, value.shape[-1]))
        @staticmethod
        def quantized_prosody(extractor, mel, mapping):
            value = extractor.cmuencoder(mel)
            maximum = torch.max(mapping)
            grouped = value.new_zeros(1, maximum + 1, value.shape[-1]).scatter_add(
                1, mapping[..., None].repeat(1, 1, value.shape[-1]), value)
            counts = value.new_zeros(1, maximum + 1).scatter_add(
                1, mapping, value.new_ones(value.shape[:2]))
            value = grouped[:, 1:] / torch.clamp(counts[:, 1:, None], min=1)
            value = extractor.encoder(value)
            _, quantized, _ = extractor.vqvae.encode(value)
            return quantized
        def prosody(self, extractor, linear, mel, mapping):
            value = self.quantized_prosody(extractor, mel, mapping)
            value = linear(torch.cat([value, self.owner.embed_positions(value[:, :, 0])], -1))
            return self.expand(value, mapping)
        def forward(self, mel_embed, frame_features, mel2ph, mel2word):
            m = self.owner
            feat = frame_features + self.prosody(m.prosody_extractor_ph, m.l1_ph, mel_embed, mel2ph)
            feat = feat + self.prosody(m.prosody_extractor_word, m.l1_word, mel_embed, mel2word)
            raw = m.note_frame_predictor.note_head(feat)
            boundary = torch.clamp(raw[:, :, 0] / m.note_frame_predictor.note_bd_temperature,
                                   min=-16.0, max=16.0)
            return feat, boundary, raw[:, :, 1:] / m.note_frame_predictor.note_temperature

    class StageC(torch.nn.Module):
        def __init__(self, owner): super().__init__(); self.owner = owner
        @staticmethod
        def quantized_prosody(extractor, mel, mapping):
            value = extractor.cmuencoder(mel)
            maximum = torch.max(mapping)
            grouped = value.new_zeros(1, maximum + 1, value.shape[-1]).scatter_add(
                1, mapping[..., None].repeat(1, 1, value.shape[-1]), value)
            counts = value.new_zeros(1, maximum + 1).scatter_add(
                1, mapping, value.new_ones(value.shape[:2]))
            value = grouped[:, 1:] / torch.clamp(counts[:, 1:, None], min=1)
            value = extractor.encoder(value)
            _, quantized, _ = extractor.vqvae.encode(value)
            return quantized
        def forward(self, mel_embed, enhanced_features, mel2note):
            m = self.owner
            value = self.quantized_prosody(m.prosody_extractor_note, mel_embed, mel2note)
            value = m.l1_note(torch.cat([value, m.embed_positions(value[:, :, 0])], -1))
            value = torch.gather(torch.nn.functional.pad(value, [0, 0, 1, 0]), 1,
                                 mel2note[..., None].repeat(1, 1, value.shape[-1]))
            feat = enhanced_features + value
            decoder = m.pitch_decoder
            attention = torch.sigmoid(decoder.multihead_dot_attn(feat))
            weighted = (feat.unsqueeze(3) * attention.unsqueeze(2)).mean(-1)
            attention = attention.mean(-1)
            denominator = attention.new_zeros(1, note_bucket).scatter_add(1, mel2note, attention)
            indices = mel2note[..., None].repeat(1, 1, decoder.hidden_size)
            aggregate = weighted.new_zeros(1, note_bucket, decoder.hidden_size).scatter_add(1, indices, weighted)
            aggregate = aggregate / (denominator[..., None] + 1e-5)
            logits = decoder.pitch_out(decoder.post(aggregate)) / decoder.pitch_temperature
            return feat, attention, denominator, logits

    generator = torch.Generator().manual_seed(0x57A25)
    stage_a = (StageA(model).eval(), [torch.randn(1, frames, 80, generator=generator) * 0.1,
              torch.randint(1, 299, (1, frames), generator=generator),
              torch.zeros(1, frames, dtype=torch.long), torch.ones(1, frames)],
              ["mel", "pitch_coarse", "uv", "mel_nonpadding"],
              ["mel_embed", "frame_features", "ph_boundary_logits", "phoneme_frame_logits",
               "note_boundary_logits", "note_frame_logits"])
    mel2ph = (torch.arange(frames) // 8 + 1)[None]
    mel2word = (torch.arange(frames) // 16 + 1)[None]
    stage_b = (StageB(model).eval(), [torch.randn(1, frames, 256, generator=generator) * 0.1,
              torch.randn(1, frames, 256, generator=generator) * 0.1, mel2ph, mel2word],
              ["mel_embed", "frame_features", "mel2ph", "mel2word"],
              ["enhanced_features", "note_boundary_logits", "note_frame_logits"])
    mel2note = (torch.arange(frames) // 16)[None].clamp(max=note_bucket - 1)
    stage_c = (StageC(model).eval(), [torch.randn(1, frames, 256, generator=generator) * 0.1,
              torch.randn(1, frames, 256, generator=generator) * 0.1, mel2note],
              ["mel_embed", "enhanced_features", "mel2note"],
              ["note_context_features", "frame_note_attention", "note_attention_denominator",
               "note_pitch_logits"])
    return {"stage-a": stage_a, "stage-b": stage_b, "stage-c": stage_c}


def export(arguments):
    import torch
    if arguments.frames <= 0 or arguments.frames % 16 or arguments.note_bucket <= 1:
        raise SystemExit("frames must be positive/divisible by 16 and note bucket > 1")
    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    shared_frontend_manifest_sha256 = validate_shared_frontend(arguments.shared_frontend_manifest)
    g2p_asset_sha256 = validate_g2p_asset(arguments.g2p_asset)
    model = load_model(arguments.source_dir, arguments.checkpoint)
    records = []
    for name, (module, inputs, input_names, output_names) in wrappers(model, arguments.frames, arguments.note_bucket).items():
        onnx = arguments.output_dir / f"stars-{name}-t{arguments.frames}-n{arguments.note_bucket}.onnx"
        if onnx.exists(): raise SystemExit(f"refusing to replace {onnx}")
        started = time.monotonic()
        with torch.inference_mode():
            outputs = module(*inputs)
            torch.onnx.export(module, tuple(inputs), onnx, input_names=input_names,
                              output_names=output_names, opset_version=18,
                              do_constant_folding=True, external_data=True, dynamo=False)
        for index, value in enumerate(inputs): np.save(arguments.output_dir / f"{name}-input-{index}.npy", value.numpy())
        for index, value in enumerate(outputs): np.save(arguments.output_dir / f"{name}-reference-{index}.npy", value.numpy())
        records.append({"name": name, "onnx": {"filename": onnx.name, "bytes": onnx.stat().st_size,
                        "sha256": sha256(onnx)}, "inputs": input_names,
                        "outputs": output_names, "output_shapes": [list(value.shape) for value in outputs],
                        "elapsed_seconds": time.monotonic() - started})
    atomic_json(arguments.result, {"phase": "stars-p0-notes-split-export", "source_revision": SOURCE_REVISION,
                "checkpoint_sha256": CHECKPOINT_SHA256,
                "annotation_rmvpe_sha256": ANNOTATION_RMVPE_SHA256,
                "shared_frontend_profile": SHARED_FRONTEND_PROFILE,
                "shared_frontend_manifest_sha256": shared_frontend_manifest_sha256,
                "g2p_profile": G2P_PROFILE, "g2p_asset_sha256": g2p_asset_sha256,
                "capability": "notes.stars", "technique_analyze_enabled": False,
                "frames": arguments.frames, "note_bucket": arguments.note_bucket,
                "stages": records, "process_peak_rss_bytes": peak_rss(),
                "torch": torch.__version__})


def convert(arguments):
    import openvino as ov
    records = []
    for name in ("stage-a", "stage-b", "stage-c"):
        source = next(arguments.artifact_dir.glob(f"stars-{name}-*.onnx"), None)
        if source is None: raise SystemExit(f"missing {name} ONNX")
        xml = source.with_suffix(".xml")
        if xml.exists() or xml.with_suffix(".bin").exists(): raise SystemExit(f"refusing to replace {xml}")
        model = ov.convert_model(source); ov.save_model(model, xml, compress_to_fp16=False)
        records.append({"name": name, "xml": {"filename": xml.name, "bytes": xml.stat().st_size,
                        "sha256": sha256(xml)}, "bin": {"filename": xml.with_suffix('.bin').name,
                        "bytes": xml.with_suffix('.bin').stat().st_size,
                        "sha256": sha256(xml.with_suffix('.bin'))}})
    atomic_json(arguments.result, {"phase": "stars-p0-split-openvino-conversion", "stages": records,
                "openvino": ov.get_version(), "process_peak_rss_bytes": peak_rss()})


def metrics(reference, candidate):
    difference = candidate.astype(np.float64) - reference.astype(np.float64)
    return {"max_abs": float(np.max(np.abs(difference))), "mean_abs": float(np.mean(np.abs(difference))),
            "relative_l2": float(np.linalg.norm(difference) / max(np.linalg.norm(reference.astype(np.float64)), 1e-12))}


def parity(arguments):
    records = []
    if arguments.backend == "ort":
        if arguments.devices != "cpu":
            raise SystemExit("ORT parity is CPU-only")
        import onnxruntime as ort
    else:
        import openvino as ov
        core = ov.Core()
    for name in ("stage-a", "stage-b", "stage-c"):
        inputs = [np.load(path) for path in sorted(arguments.artifact_dir.glob(f"{name}-input-*.npy"))]
        references = [np.load(path) for path in sorted(arguments.artifact_dir.glob(f"{name}-reference-*.npy"))]
        if arguments.backend == "ort":
            source = next(arguments.artifact_dir.glob(f"stars-{name}-*.onnx")); session = ort.InferenceSession(str(source), providers=["CPUExecutionProvider"])
            outputs = session.run(None, {port.name: value for port, value in zip(session.get_inputs(), inputs)})
        else:
            source = next(arguments.artifact_dir.glob(f"stars-{name}-*.xml")); device = "GPU" if arguments.devices == "product" else "CPU"
            compiled = core.compile_model(source, device, {"INFERENCE_PRECISION_HINT": "f32", "EXECUTION_MODE_HINT": "ACCURACY"})
            outputs = [np.asarray(compiled(inputs)[index]).copy() for index in range(len(references))]
        observed = [metrics(a, b) for a, b in zip(references, outputs)]
        if any(not all(np.isfinite(value) for value in result.values())
               or result["relative_l2"] > 5e-5 or result["max_abs"] > 5e-3
               for result in observed):
            raise SystemExit(f"STARS {name} parity failed: {observed}")
        records.append({"name": name, "metrics": observed})
    atomic_json(arguments.result, {"phase": f"stars-p0-split-{arguments.backend}-{arguments.devices}-parity",
                "accepted": True, "stages": records, "process_peak_rss_bytes": peak_rss()})


def parser():
    root = argparse.ArgumentParser(); commands = root.add_subparsers(dest="command", required=True)
    command = commands.add_parser("export"); command.add_argument("--source-dir", type=Path, required=True); command.add_argument("--checkpoint", type=Path, required=True); command.add_argument("--shared-frontend-manifest", type=Path, required=True); command.add_argument("--g2p-asset", type=Path, required=True); command.add_argument("--output-dir", type=Path, required=True); command.add_argument("--frames", type=int, default=256); command.add_argument("--note-bucket", type=int, default=32); command.add_argument("--result", type=Path, required=True); command.set_defaults(function=export)
    command = commands.add_parser("convert"); command.add_argument("--artifact-dir", type=Path, required=True); command.add_argument("--result", type=Path, required=True); command.set_defaults(function=convert)
    command = commands.add_parser("parity"); command.add_argument("--backend", choices=("ort", "openvino"), required=True); command.add_argument("--devices", choices=("cpu", "product"), default="cpu", help="product selects OpenVINO GPU and requires repository-policy permission"); command.add_argument("--artifact-dir", type=Path, required=True); command.add_argument("--result", type=Path, required=True); command.set_defaults(function=parity)
    return root

if __name__ == "__main__":
    arguments = parser().parse_args(); arguments.function(arguments)
