#!/usr/bin/env python3
"""Download only the analysis models explicitly selected in Settings."""

from __future__ import annotations

import argparse
import gc
from pathlib import Path


KARAOKE_MODEL = "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt"
MMS_KARAOKE_MODEL = "NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn"


def whisper_repository(model: str) -> str:
    if model == "large-v3-turbo":
        return "mobiuslabsgmbh/faster-whisper-large-v3-turbo"
    return f"Systran/faster-whisper-{model}"


def download_huggingface(repository: str) -> None:
    from huggingface_hub import snapshot_download

    try:
        snapshot_download(repo_id=repository, local_files_only=True)
        print(f"Using existing {repository}", flush=True)
        return
    except Exception:
        pass
    print(f"Preparing {repository}...", flush=True)
    snapshot_download(repo_id=repository)


def download_selected_models(
    models_dir: Path,
    backend: str,
    engine: str,
    whisper_model: str,
    separator: str,
    align_backend: str,
    target: str = "all",
) -> None:
    if target in ("all", "parakeet") and engine == "parakeet":
        if backend == "cuda":
            import nemo.collections.asr as nemo_asr

            print("Preparing NVIDIA Parakeet v3...", flush=True)
            model = nemo_asr.models.ASRModel.from_pretrained(
                model_name="nvidia/parakeet-tdt-0.6b-v3"
            )
            del model
            gc.collect()
            marker = models_dir / "selected" / "parakeet-cuda.ready"
            marker.parent.mkdir(parents=True, exist_ok=True)
            marker.write_text("nvidia/parakeet-tdt-0.6b-v3\n", encoding="utf-8")
        else:
            download_huggingface("istupakov/parakeet-tdt-0.6b-v3-onnx")

    if target in ("all", "whisper"):
        download_huggingface(whisper_repository(whisper_model))

    if target == "language_detection" or (
        target == "all" and (engine == "parakeet" or backend == "intel")
    ):
        download_huggingface(whisper_repository("tiny"))

    if target in ("all", "separator") and separator == "karaoke":
        from audio_models.catalog import DEFAULT_LEGACY_KARAOKE_MODEL_ID
        from audio_models.install import install_audio_model

        print("Preparing catalog karaoke separator...", flush=True)
        install_audio_model(
            models_dir,
            DEFAULT_LEGACY_KARAOKE_MODEL_ID,
            allow_network=True,
            on_progress=lambda message: print(message, flush=True),
        )
    elif target in ("all", "separator") and separator == "demucs":
        from demucs.pretrained import get_model

        # The analyzer loads Demucs through its official pretrained registry,
        # which caches the checkpoint under TORCH_HOME. Merely mirroring a
        # similarly named Hugging Face repository would still make first use
        # download the real checkpoint silently.
        print("Preparing Demucs separator...", flush=True)
        model = get_model("htdemucs")
        del model
        gc.collect()

    if target in ("all", "alignment") and align_backend == "qwen":
        download_huggingface("Qwen/Qwen3-ForcedAligner-0.6B-hf")
    elif target in ("all", "alignment") and align_backend == "mms_karaoke":
        download_huggingface(MMS_KARAOKE_MODEL)


def main() -> None:
    parser = argparse.ArgumentParser(description="Prepare selected Uta Studio models")
    parser.add_argument("--models-dir", required=True)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--engine", required=True)
    parser.add_argument("--whisper-model", required=True)
    parser.add_argument("--separator", required=True)
    parser.add_argument("--align-backend", required=True)
    parser.add_argument(
        "--target",
        choices=(
            "all",
            "whisper",
            "language_detection",
            "parakeet",
            "separator",
            "alignment",
            "audio_model",
        ),
        default="all",
        help="Download one selected model family instead of the complete set",
    )
    parser.add_argument("--audio-model-id", default="")
    parser.add_argument("--audio-model-action", choices=("install", "remove"), default="install")
    args = parser.parse_args()
    if args.target == "audio_model":
        from audio_models.install import install_audio_model, remove_audio_model

        models_dir = Path(args.models_dir).expanduser().resolve()
        if args.audio_model_action == "remove":
            remove_audio_model(models_dir, args.audio_model_id)
        else:
            install_audio_model(
                models_dir,
                args.audio_model_id,
                allow_network=True,
            )
        return
    download_selected_models(
        Path(args.models_dir).expanduser().resolve(),
        args.backend,
        args.engine,
        args.whisper_model,
        args.separator,
        args.align_backend,
        args.target,
    )


if __name__ == "__main__":
    main()
