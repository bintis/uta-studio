"""Direct Demucs runner. Stem names come from the model, not array order."""

from __future__ import annotations

from pathlib import Path

from audio_models.catalog import ModelSpec
from audio_models.errors import ModelConfigurationError, OutputContractError
from audio_models.parameters import ResolvedParameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.contracts import ProcessorResult, ProgressSink, requested_backend_for
from audio_processors.outputs import map_named_outputs
from audio_processors.runners.base import (
    emit,
    resolve_installed_file,
    run_with_whole_model_fallback,
)

_ISOLATED_XPU_RESOURCES: list[object] = []


class DemucsTorchRunner:
    def run(
        self,
        *,
        model_spec: ModelSpec,
        input_path: Path,
        work_dir: Path,
        parameters: ResolvedParameters,
        runtime_request: AudioRuntimeRequest,
        progress_sink: ProgressSink | None = None,
        installed_dir: Path | None = None,
        step_id: str = "demucs",
    ) -> ProcessorResult:
        if model_spec.architecture != "demucs":
            raise ModelConfigurationError(
                f"{model_spec.id} is not a Demucs model",
                model_id=model_spec.id,
            )
        if installed_dir is None:
            raise ModelConfigurationError("installed_dir is required", model_id=model_spec.id)
        models_root = installed_dir.parent.parent

        def execute(backend: str) -> ProcessorResult:
            attempt_dir = work_dir / backend
            attempt_dir.mkdir(parents=True, exist_ok=True)
            yaml_path = resolve_installed_file(
                models_root, model_spec, model_spec.file("model_config")
            )
            weight_path = resolve_installed_file(
                models_root, model_spec, model_spec.file("checkpoint")
            )
            emit(
                progress_sink,
                8,
                f"Loading {model_spec.display_name}",
                model_id=model_spec.id,
                architecture=model_spec.architecture,
                actual_backend=backend,
            )
            if backend == "torch_xpu":
                from audio_processors.xpu_worker import run_isolated_xpu

                payload = run_isolated_xpu(
                    {
                        "runner": "demucs_torch",
                        "model_id": model_spec.id,
                        "yaml_path": str(yaml_path),
                        "weight_path": str(weight_path),
                        "input_path": str(input_path),
                        "work_dir": str(attempt_dir),
                        "parameters": parameters.as_map(),
                    }
                )
                named = {
                    stem: Path(path) for stem, path in payload["stems"].items()
                }
                sample_rate = int(payload["sample_rate"])
                channels = int(payload["channels"])
            else:
                named, sample_rate, channels = _separate_demucs(
                    yaml_path=yaml_path,
                    weight_path=weight_path,
                    input_path=input_path,
                    work_dir=attempt_dir,
                    parameters=parameters,
                    backend=backend,
                    expected=model_spec.expected_stems,
                )
            artifacts = map_named_outputs(
                model_spec,
                named,
                sample_rate=sample_rate,
                channels=channels,
            )
            return ProcessorResult(
                model_id=model_spec.id,
                architecture=model_spec.architecture,
                artifacts=artifacts,
                requested_backend=requested_backend_for(model_spec, runtime_request),
                actual_backend=backend,
                precision=str(parameters.get("runtime.precisionPolicy", "fp32")),
                effective_parameters=parameters.as_map(),
            )

        return run_with_whole_model_fallback(
            model_spec=model_spec,
            runtime_request=runtime_request,
            parameters=parameters,
            execute=execute,
            progress_sink=progress_sink,
        )


def _separate_demucs(
    *,
    yaml_path: Path,
    weight_path: Path,
    input_path: Path,
    work_dir: Path,
    parameters: ResolvedParameters,
    backend: str,
    expected: tuple[str, ...],
    process_isolated: bool = False,
) -> tuple[dict[str, Path], int, int]:
    import numpy as np
    import soundfile as sf
    import torch
    from demucs.apply import apply_model
    from demucs.states import load_model

    device_name = {"torch_cuda": "cuda", "torch_xpu": "xpu"}.get(backend, "cpu")
    device = torch.device(device_name)
    model = None
    wav = ref = wav_centered = wav_scaled = sources = peak = mean = None
    try:
        # Architecture lives inside the Demucs checkpoint package. The YAML is
        # only the catalog pairing (models: ['5c90dfd2']); never call get_model().
        _ = yaml_path
        model = load_model(str(weight_path))
        if set(model.sources) < set(expected):
            raise OutputContractError(
                f"Demucs model sources {model.sources} do not cover {expected}"
            )
        model.to(device)
        work_dir.mkdir(parents=True, exist_ok=True)
        data, sample_rate = sf.read(str(input_path), dtype="float32", always_2d=True)
        wav = torch.from_numpy(np.ascontiguousarray(data.T)).to(device)
        ref = wav.mean(0)
        wav_centered = wav - ref.mean()
        wav_scaled = wav_centered / ref.abs().max().clamp(min=1e-8)
        shifts = int(parameters.get("demucs.shifts", 0))
        overlap = float(parameters.get("demucs.overlapRatio", 0.25))
        # XPU oneDNN cannot create SDPA primitives on this driver. Keep the
        # whole HTDemucs model on XPU by using a matmul attention fallback.
        split = False if backend == "torch_xpu" else bool(parameters.get("demucs.splitEnabled", True))
        if backend == "torch_xpu":
            sources = _apply_demucs_on_xpu(
                model,
                wav_scaled[None],
                device=device,
                shifts=max(shifts, 0),
                overlap=overlap,
                split=split,
            )[0]
        else:
            sources = apply_model(
                model,
                wav_scaled[None],
                device=device,
                shifts=max(shifts, 0),
                overlap=overlap,
                split=split,
            )[0]
        named: dict[str, Path] = {}
        peak = ref.abs().max()
        mean = ref.mean()
        source_names = tuple(model.sources)
        for name, tensor in zip(source_names, sources):
            audio = (tensor * peak + mean).detach().cpu().to(torch.float32)
            path = work_dir / f"step_demucs__{name}.wav"
            peak_value = float(audio.abs().max().item())
            if peak_value > 1.0:
                audio = audio / (1.01 * peak_value)
            sf.write(str(path), audio.numpy().T, sample_rate, subtype="PCM_16")
            named[name] = path
        return named, int(sample_rate), 2
    finally:
        if process_isolated:
            # xpu_worker intentionally owns these objects until os._exit(),
            # which destroys the complete Level Zero context at once.
            _ISOLATED_XPU_RESOURCES.append(
                (model, wav, ref, wav_centered, wav_scaled, sources, peak, mean)
            )
        else:
            from gpu import hard_free_gpu, move_to_cpu

            move_to_cpu(model)
            model = wav = ref = wav_centered = wav_scaled = sources = peak = mean = None
            hard_free_gpu(f"audio-model:demucs:{backend}")


def _matmul_sdpa(query, key, value, attn_mask=None, dropout_p=0.0, is_causal=False, scale=None, **_kwargs):
    import torch

    del dropout_p
    scale_value = scale if scale is not None else query.shape[-1] ** -0.5
    scores = query.matmul(key.transpose(-2, -1)) * scale_value
    if is_causal:
        causal = torch.ones(scores.shape[-2], scores.shape[-1], dtype=torch.bool, device=scores.device)
        scores = scores.masked_fill(causal.triu(diagonal=1), float("-inf"))
    if attn_mask is not None:
        scores = scores + attn_mask
    weights = torch.softmax(scores, dim=-1)
    return weights.matmul(value)


def _apply_demucs_on_xpu(model, mix, *, device, shifts, overlap, split):
    import torch
    from demucs.apply import apply_model

    original = torch.nn.functional.scaled_dot_product_attention
    torch.nn.functional.scaled_dot_product_attention = _matmul_sdpa
    try:
        return apply_model(
            model,
            mix,
            device=device,
            shifts=shifts,
            overlap=overlap,
            split=split,
        )
    finally:
        torch.nn.functional.scaled_dot_product_attention = original
