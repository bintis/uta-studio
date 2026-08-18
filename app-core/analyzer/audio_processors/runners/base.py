"""Shared runner helpers: offline files, integrity, whole-model fallback."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Callable

from audio_models.catalog import ModelFileSpec, ModelSpec, installed_model_dir
from audio_models.errors import (
    BackendUnavailableError,
    InferenceOutOfMemoryError,
    ModelIntegrityError,
    ModelNotInstalledError,
)
from audio_models.parameters import ResolvedParameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.contracts import ProcessorResult, ProgressSink, requested_backend_for

RUNNER_BUILD_ID = "uta-audio-runner-v1"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_installed_file(models_dir: Path, model: ModelSpec, file_spec: ModelFileSpec) -> Path:
    directory = installed_model_dir(models_dir, model.id)
    path = directory / file_spec.install_filename
    if not path.is_file():
        raise ModelNotInstalledError(
            f"{model.id} is not installed ({file_spec.role})",
            model_id=model.id,
        )
    actual = sha256_file(path)
    if actual != file_spec.sha256:
        raise ModelIntegrityError(
            f"{model.id} {file_spec.role} failed SHA-256 verification",
            model_id=model.id,
        )
    return path


def load_install_manifest(models_dir: Path, model: ModelSpec) -> dict[str, object]:
    path = installed_model_dir(models_dir, model.id) / "install-manifest.json"
    if not path.is_file():
        raise ModelNotInstalledError(f"{model.id} has no install manifest", model_id=model.id)
    return json.loads(path.read_text(encoding="utf-8"))


def fallback_backend(requested: str) -> str | None:
    if requested in {"torch_xpu", "torch_cuda"}:
        return "torch_cpu"
    if requested in {"openvino_gpu", "onnx_cuda"}:
        return "openvino_cpu" if requested == "openvino_gpu" else "onnx_cpu"
    return None


def classify_runtime_error(exc: BaseException) -> type[Exception]:
    text = str(exc).lower()
    if "out of memory" in text or "oom" in text or "cuda error: out of memory" in text:
        return InferenceOutOfMemoryError
    return BackendUnavailableError


def emit(
    progress_sink: ProgressSink | None,
    percent: int,
    message: str,
    **metadata: object,
) -> None:
    if progress_sink is not None:
        progress_sink(percent, message, **metadata)


def run_with_whole_model_fallback(
    *,
    model_spec: ModelSpec,
    runtime_request: AudioRuntimeRequest,
    parameters: ResolvedParameters,
    execute: Callable[[str], ProcessorResult],
    progress_sink: ProgressSink | None = None,
) -> ProcessorResult:
    requested = requested_backend_for(model_spec, runtime_request)
    try:
        result = execute(requested)
        return result
    except Exception as exc:
        if runtime_request.fallback_policy != "whole_model_cpu":
            raise
        fallback = fallback_backend(requested)
        if fallback is None or fallback == requested:
            raise
        error_type = classify_runtime_error(exc)
        emit(
            progress_sink,
            20,
            f"{model_spec.id} failed on {requested}; retrying the complete model on {fallback}",
            requested_backend=requested,
            actual_backend=fallback,
            fallback_reason=str(exc),
        )
        try:
            result = execute(fallback)
        except Exception as fallback_exc:
            raise error_type(
                f"{model_spec.id} failed on {requested} and {fallback}: {fallback_exc}",
                model_id=model_spec.id,
                requested_backend=requested,
                actual_backend=fallback,
            ) from fallback_exc
        return ProcessorResult(
            model_id=result.model_id,
            architecture=result.architecture,
            artifacts=result.artifacts,
            requested_backend=requested,
            actual_backend=fallback,
            precision=result.precision,
            fallback_from=requested,
            fallback_reason=str(exc),
            effective_parameters=result.effective_parameters,
        )
