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

_ISOLATED_XPU_RESOURCES: list[object] = []


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
        if installed_dir is None:
            raise ModelConfigurationError("installed_dir is required", model_id=model_spec.id)
        models_root = installed_dir.parent.parent

        def execute(backend: str) -> ProcessorResult:
            attempt_dir = work_dir / backend
            attempt_dir.mkdir(parents=True, exist_ok=True)
            precision_policy = str(parameters.get("runtime.precisionPolicy", "fp32"))
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
            descriptor_names = dict(descriptor.output_names)
            if backend == "torch_xpu":
                from audio_processors.xpu_worker import run_isolated_xpu
                from audio_processors.xpu_segmented import run_segmented_mdxc_xpu

                request = {
                    "runner": "mdxc_torch",
                    "model_id": model_spec.id,
                    "checkpoint": str(checkpoint),
                    "config_path": str(config_path),
                    "input_path": str(input_path),
                    "work_dir": str(attempt_dir),
                    "parameters": parameters.as_map(),
                    "precision_policy": precision_policy,
                    "descriptor_names": descriptor_names,
                }
                named = run_segmented_mdxc_xpu(
                    request=request,
                    input_path=input_path,
                    attempt_dir=attempt_dir,
                    descriptor_names=descriptor_names,
                    expected_stems=model_spec.expected_stems,
                    run_worker=run_isolated_xpu,
                    progress_sink=progress_sink,
                )
            else:
                named = _separate_offline(
                    model_spec=model_spec,
                    checkpoint=checkpoint,
                    config_path=config_path,
                    input_path=input_path,
                    work_dir=attempt_dir,
                    parameters=parameters,
                    backend=backend,
                    precision_policy=precision_policy,
                    descriptor_names=descriptor_names,
                )
            artifacts = map_named_outputs(
                model_spec,
                {stem: path_for_stem(attempt_dir, descriptor, stem) if stem not in named else named[stem]
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
                precision=_actual_precision(backend, precision_policy),
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
    precision_policy: str,
    descriptor_names: dict[str, str],
    process_isolated: bool = False,
    require_all_outputs: bool = True,
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
    separator = None
    try:
        separator = OfflineSeparator(
            model_file_dir=str(checkpoint.parent),
            output_dir=str(work_dir),
            normalization_threshold=float(parameters.get("common.normalizationThreshold", 0.9)),
            output_format="WAV",
            mdxc_params=mdxc_params,
            torch_backend=backend,
            process_isolated=process_isolated,
        )
        separator.load_model_from_spec(
            model_path=str(checkpoint),
            architecture=model_spec.architecture,
            config_path=str(config_path),
        )
        apply_torch_device(separator, backend)
        custom_names = descriptor_names
        if backend == "torch_xpu":
            output_files = _separate_on_xpu(
                separator,
                input_path,
                custom_names,
                precision_policy=precision_policy,
            )
        else:
            output_files = separator.separate(str(input_path), custom_output_names=custom_names)
        named: dict[str, Path] = {}
        for stem, token in custom_names.items():
            matched = match_named_file(work_dir, token, output_files)
            if matched is not None:
                named[stem] = matched
        if require_all_outputs and len(named) < len(model_spec.expected_stems):
            raise RuntimeError(
                f"{model_spec.id} did not write deterministic stem names {custom_names}"
            )
        if not named:
            raise RuntimeError(f"{model_spec.id} did not write any deterministic stem")
        return named
    finally:
        if separator is not None:
            if process_isolated:
                # Keep every XPU-owned object alive until xpu_worker calls
                # os._exit(). Running Python or C++ destructors here can issue
                # more commands into an already fragile Level Zero context.
                _ISOLATED_XPU_RESOURCES.append(separator)
            else:
                from gpu import hard_free_gpu, move_to_cpu

                # OfflineSeparator -> Separator -> model_instance -> model_run
                # is a nested ownership chain that upstream keeps alive after
                # separate(). Move the model to host memory before freeing the
                # in-process caching allocator.
                move_to_cpu(separator)
                separator = None
                hard_free_gpu(f"audio-model:{model_spec.id}:{backend}")


def _actual_precision(backend: str, precision_policy: str) -> str:
    if backend != "torch_xpu":
        return "fp32"
    if precision_policy == "auto":
        return "bf16"
    if precision_policy in {"bf16", "fp16"}:
        return precision_policy
    return "fp32"


def _separate_on_xpu(separator, input_path, custom_names, *, precision_policy: str):
    import torch
    from audio_processors.runners.demucs_torch import _matmul_sdpa

    original_sdpa = torch.nn.functional.scaled_dot_product_attention
    original_stft = torch.stft
    original_istft = torch.istft
    original_view_as_complex = torch.view_as_complex

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

    def xpu_view_as_complex(input_tensor):
        if (
            getattr(input_tensor, "device", None) is not None
            and input_tensor.device.type == "xpu"
            and input_tensor.dtype == torch.bfloat16
        ):
            # PyTorch accepts FP16/FP32/FP64 pairs here but rejects BF16.
            # Keep RoFormer activations in BF16 and promote only this final
            # real/imaginary mask boundary to FP32 complex values.
            input_tensor = input_tensor.to(torch.float32)
        return original_view_as_complex(input_tensor)

    torch.nn.functional.scaled_dot_product_attention = _matmul_sdpa
    torch.stft = cpu_stft
    torch.istft = cpu_istft
    torch.view_as_complex = xpu_view_as_complex
    try:
        precision = _actual_precision("torch_xpu", precision_policy)
        dtype = {
            "bf16": torch.bfloat16,
            "fp16": torch.float16,
        }.get(precision)
        if dtype is None:
            return separator.separate(str(input_path), custom_output_names=custom_names)
        # The plan previously reported bf16/fp16 while audio-separator ran
        # with its default use_autocast=False. Honor the requested precision
        # so RoFormer activations do not consume FP32-sized XPU allocations.
        with torch.autocast(device_type="xpu", dtype=dtype):
            return separator.separate(str(input_path), custom_output_names=custom_names)
    finally:
        torch.nn.functional.scaled_dot_product_attention = original_sdpa
        torch.stft = original_stft
        torch.istft = original_istft
        torch.view_as_complex = original_view_as_complex
