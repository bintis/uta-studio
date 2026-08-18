"""BS-RoFormer / MelBand-RoFormer runner. Architecture comes from the catalog."""

from __future__ import annotations

from pathlib import Path

from audio_models.catalog import ModelSpec
from audio_models.errors import ModelConfigurationError
from audio_models.parameters import ResolvedParameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.contracts import (
    ProcessorResult,
    ProgressSink,
    requested_backend_for,
)
from audio_processors.outputs import descriptor_from_spec, map_named_outputs, match_named_file, path_for_stem
from audio_processors.runners.base import (
    emit,
    resolve_installed_file,
    run_with_whole_model_fallback,
)


def _torch_device(backend: str):
    import torch

    if backend == "torch_cuda":
        return torch.device("cuda")
    if backend == "torch_xpu":
        return torch.device("xpu")
    return torch.device("cpu")


class MdxcTorchRunner:
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
        step_id: str = "mdxc",
    ) -> ProcessorResult:
        if not model_spec.architecture.startswith("mdxc_"):
            raise ModelConfigurationError(
                f"{model_spec.id} is not an MDXC model",
                model_id=model_spec.id,
            )
        models_dir = installed_dir.parent.parent if installed_dir is not None else Path()
        if installed_dir is None:
            raise ModelConfigurationError("installed_dir is required", model_id=model_spec.id)
        models_root = installed_dir.parent.parent

        def execute(backend: str) -> ProcessorResult:
            checkpoint = resolve_installed_file(
                models_root, model_spec, model_spec.file("checkpoint")
            )
            config_path = resolve_installed_file(
                models_root, model_spec, model_spec.file("model_config")
            )
            emit(
                progress_sink,
                8,
                f"Loading {model_spec.display_name}",
                model_id=model_spec.id,
                architecture=model_spec.architecture,
                requested_backend=requested_backend_for(model_spec, runtime_request),
                actual_backend=backend,
            )
            descriptor = descriptor_from_spec(model_spec, step_id)
            named = _separate_offline(
                model_spec=model_spec,
                checkpoint=checkpoint,
                config_path=config_path,
                input_path=input_path,
                work_dir=work_dir,
                parameters=parameters,
                backend=backend,
                descriptor_names=dict(descriptor.output_names),
            )
            artifacts = map_named_outputs(
                model_spec,
                {stem: path_for_stem(work_dir, descriptor, stem) if stem not in named else named[stem]
                 for stem in model_spec.expected_stems},
                sample_rate=44100,
                channels=2,
                required_roles=tuple(model_spec.output_contract[stem] for stem in model_spec.expected_stems),
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


def _separate_offline(
    *,
    model_spec: ModelSpec,
    checkpoint: Path,
    config_path: Path,
    input_path: Path,
    work_dir: Path,
    parameters: ResolvedParameters,
    backend: str,
    descriptor_names: dict[str, str],
) -> dict[str, Path]:
    from audio_separator_adapter import OfflineSeparator, apply_torch_device

    overlap_policy = parameters.get("mdxc.overlapPolicy", "model_default")
    overlap_count = int(parameters.get("mdxc.overlapCount", 8))
    mdxc_params = {
        "segment_size": 256,
        "override_model_segment_size": False,
        "batch_size": 1,
        "overlap": overlap_count if overlap_policy == "overlap_count" else 8,
        "pitch_shift": int(parameters.get("mdxc.pitchShiftSemitones", 0)),
    }
    separator = OfflineSeparator(
        model_file_dir=str(checkpoint.parent),
        output_dir=str(work_dir),
        normalization_threshold=float(parameters.get("common.normalizationThreshold", 0.9)),
        output_format="WAV",
        mdxc_params=mdxc_params,
        torch_backend=backend,
    )
    separator.load_model_from_spec(
        model_path=str(checkpoint),
        architecture=model_spec.architecture,
        config_path=str(config_path),
    )
    apply_torch_device(separator, backend)
    custom_names = descriptor_names
    if backend == "torch_xpu":
        output_files = _separate_on_xpu(separator, input_path, custom_names)
    else:
        output_files = separator.separate(str(input_path), custom_output_names=custom_names)
    named: dict[str, Path] = {}
    for stem, token in custom_names.items():
        matched = match_named_file(work_dir, token, output_files)
        if matched is not None:
            named[stem] = matched
    if len(named) < len(model_spec.expected_stems):
        raise RuntimeError(
            f"{model_spec.id} did not write deterministic stem names {custom_names}"
        )
    return named


def _separate_on_xpu(separator, input_path, custom_names):
    import torch
    from audio_processors.runners.demucs_torch import _matmul_sdpa

    original_sdpa = torch.nn.functional.scaled_dot_product_attention
    original_stft = torch.stft
    original_istft = torch.istft

    def cpu_stft(input_tensor, *args, **kwargs):
        if getattr(input_tensor, "device", None) is not None and input_tensor.device.type == "xpu":
            window = kwargs.get("window")
            if window is not None and getattr(window, "device", None) is not None:
                kwargs = {**kwargs, "window": window.cpu()}
            return original_stft(input_tensor.cpu(), *args, **kwargs).to(input_tensor.device)
        return original_stft(input_tensor, *args, **kwargs)

    def cpu_istft(input_tensor, *args, **kwargs):
        if getattr(input_tensor, "device", None) is not None and input_tensor.device.type == "xpu":
            window = kwargs.get("window")
            if window is not None and getattr(window, "device", None) is not None:
                kwargs = {**kwargs, "window": window.cpu()}
            return original_istft(input_tensor.cpu(), *args, **kwargs).to(input_tensor.device)
        return original_istft(input_tensor, *args, **kwargs)

    torch.nn.functional.scaled_dot_product_attention = _matmul_sdpa
    torch.stft = cpu_stft
    torch.istft = cpu_istft
    try:
        return separator.separate(str(input_path), custom_output_names=custom_names)
    finally:
        torch.nn.functional.scaled_dot_product_attention = original_sdpa
        torch.stft = original_stft
        torch.istft = original_istft
