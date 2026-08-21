"""Execute an immutable audio processing plan without rereading settings."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

from audio_models.catalog import AudioModelCatalog, installed_model_dir, load_catalog
from audio_models.errors import AudioProcessingError, OutputContractError
from audio_models.parameters import ResolvedParameters, resolve_parameters
from audio_models.plan import (
    AudioProcessingPlanSnapshot,
    AudioProcessingStep,
    AudioRuntimeRequest,
)
from audio_processors.contracts import ProcessorResult, ProgressSink, StemArtifact
from audio_processors.runners import RUNNERS
from audio_processors.runners.base import RUNNER_BUILD_ID

INTERMEDIATE_SUFFIX = ".wav"


@dataclass(frozen=True)
class AudioProcessingExecutionResult:
    plan: AudioProcessingPlanSnapshot
    step_results: Mapping[str, ProcessorResult]
    bindings: Mapping[str, StemArtifact]

    def binding(self, role: str) -> StemArtifact:
        try:
            return self.bindings[role]
        except KeyError as exc:
            raise OutputContractError(f"plan did not bind {role!r}") from exc


def canonical_signature_payload(
    *,
    catalog: AudioModelCatalog,
    step: AudioProcessingStep,
    model_files: Mapping[str, str],
    metadata_sha256: str | None,
    input_revisions: Mapping[str, str],
    effective_parameters: Mapping[str, object],
    effective_backend: str,
    precision: str,
) -> dict[str, object]:
    return {
        "schema_version": 1,
        "catalog_version": catalog.catalog_version,
        "step_id": step.step_id,
        "model_id": step.model_id,
        "architecture": catalog.get(step.model_id).architecture,
        "model_files": dict(sorted(model_files.items())),
        "normalized_model_metadata_sha256": metadata_sha256,
        "runner_build_id": RUNNER_BUILD_ID,
        "input_artifact_revisions": dict(sorted(input_revisions.items())),
        "effective_parameters": dict(sorted(effective_parameters.items())),
        "effective_backend": effective_backend,
        "precision": precision,
        "selected_output_roles": list(step.selected_output_roles),
    }


def signature_hash(payload: Mapping[str, object]) -> str:
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def chain_signature(plan: AudioProcessingPlanSnapshot) -> str:
    payload = {
        "steps": [step.step_id for step in plan.steps],
        "edges": [
            {
                "step_id": step.step_id,
                "input": step.input.as_json(),
            }
            for step in plan.steps
        ],
        "bindings": [binding.as_json() for binding in plan.output_bindings],
    }
    return signature_hash(payload)


def _copy_intermediate(source: Path, destination: Path) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.",
        suffix=".tmp",
        dir=destination.parent,
    )
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        if source.suffix.lower() == ".wav":
            shutil.copy2(source, temporary)
        else:
            # Keep intermediate audio lossless. Re-encode only into WAV/float,
            # never MP3. The explicit format is required because the atomic
            # temporary file intentionally has a non-audio suffix.
            import soundfile as sf

            data, sample_rate = sf.read(str(source), dtype="float32", always_2d=True)
            sf.write(
                str(temporary),
                data,
                sample_rate,
                format="WAV",
                subtype="FLOAT",
            )

        # A step-completed event publishes destination to the Rust side. Flush
        # the bytes and the atomic rename first so a hard power loss cannot turn
        # an already-published intermediate into the zero-byte file observed in
        # the 2026-08-21 XPU lockup.
        with temporary.open("rb") as persisted:
            os.fsync(persisted.fileno())
        os.replace(temporary, destination)
        try:
            directory_fd = os.open(
                destination.parent,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0),
            )
        except OSError:
            # Directory handles are not available on every supported platform.
            pass
        else:
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        return destination
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _selected_progress_artifacts(
    persisted: dict[str, StemArtifact], selected_output_roles: tuple[str, ...]
) -> list[dict[str, str]]:
    selected = set(selected_output_roles)
    return [
        {"role": artifact.role, "path": str(artifact.path)}
        for role, artifact in persisted.items()
        if role in selected
    ]


def execute_audio_processing_plan(
    plan: AudioProcessingPlanSnapshot,
    *,
    source_path: Path,
    work_root: Path,
    models_dir: Path,
    progress_sink: ProgressSink | None = None,
    catalog: AudioModelCatalog | None = None,
) -> AudioProcessingExecutionResult:
    loaded = catalog or load_catalog()
    if plan.catalog_version != loaded.catalog_version:
        # Snapshots stay executable against the catalog version they froze.
        # A mismatch is recorded but does not reread user settings.
        pass
    work_root.mkdir(parents=True, exist_ok=True)
    step_results: dict[str, ProcessorResult] = {}
    produced: dict[tuple[str, str], StemArtifact] = {}

    for step in plan.steps:
        model = loaded.get(step.model_id)
        runner = RUNNERS.get(model.runner)
        if runner is None:
            raise AudioProcessingError(f"no runner registered for {model.runner}", model_id=model.id)
        if step.input.kind == "source_media":
            input_path = Path(source_path)
        else:
            artifact = produced.get((step.input.step_id or "", step.input.role or ""))
            if artifact is None:
                raise OutputContractError(
                    f"{step.step_id} is missing input {step.input.as_json()}",
                    step_id=step.step_id,
                    model_id=step.model_id,
                )
            input_path = artifact.path
        step_dir = Path(tempfile.mkdtemp(prefix=f"{step.step_id}_", dir=work_root))
        try:
            resolved = ResolvedParameters({})
            if step.effective_parameters:
                from audio_models.schema import coerce_parameter_value
                from audio_models.parameters import ResolvedParameter

                resolved = ResolvedParameters(
                    {
                        key: ResolvedParameter(key, coerce_parameter_value(value), "plan_snapshot")
                        for key, value in step.effective_parameters.items()
                    }
                )
            else:
                resolved = resolve_parameters(model)
            step_progress = None
            if progress_sink is not None:
                def step_progress(percent, message, **metadata):
                    # Runners may already attach their model identity. Merge
                    # the plan identity without passing duplicate keywords.
                    metadata.setdefault("step_id", step.step_id)
                    metadata.setdefault("model_id", step.model_id)
                    progress_sink(percent, message, **metadata)
            result = runner.run(
                model_spec=model,
                input_path=input_path,
                work_dir=step_dir,
                parameters=resolved,
                runtime_request=plan.requested_runtime,
                progress_sink=step_progress,
                installed_dir=installed_model_dir(models_dir, model.id),
                step_id=step.step_id,
            )
            if any(role not in result.artifacts for role in step.selected_output_roles):
                missing = [role for role in step.selected_output_roles if role not in result.artifacts]
                raise OutputContractError(
                    f"{step.step_id} missing roles {missing}",
                    step_id=step.step_id,
                    model_id=step.model_id,
                )
            persisted: dict[str, StemArtifact] = {}
            for role, artifact in result.artifacts.items():
                dest = work_root / f"{step.step_id}__{role}{INTERMEDIATE_SUFFIX}"
                copied = _copy_intermediate(artifact.path, dest)
                persisted[role] = StemArtifact(
                    role=artifact.role,
                    source_stem_name=artifact.source_stem_name,
                    path=copied,
                    sample_rate=artifact.sample_rate,
                    channels=artifact.channels,
                )
                produced[(step.step_id, role)] = persisted[role]
            step_results[step.step_id] = ProcessorResult(
                model_id=result.model_id,
                architecture=result.architecture,
                artifacts=persisted,
                requested_backend=result.requested_backend,
                actual_backend=result.actual_backend,
                precision=result.precision,
                fallback_from=result.fallback_from,
                fallback_reason=result.fallback_reason,
                effective_parameters=result.effective_parameters,
            )
            if progress_sink is not None:
                progress_sink(
                    100,
                    f"{model.display_name} complete",
                    step_id=step.step_id,
                    model_id=step.model_id,
                    lifecycle="step_completed",
                    implementation=result.architecture,
                    requested_device=result.requested_backend,
                    actual_device=result.actual_backend,
                    fallback_from=result.fallback_from,
                    fallback_reason=result.fallback_reason,
                    artifacts=_selected_progress_artifacts(
                        persisted, step.selected_output_roles
                    ),
                )
        except Exception:
            shutil.rmtree(step_dir, ignore_errors=True)
            raise
        else:
            shutil.rmtree(step_dir, ignore_errors=True)
        finally:
            # Release accelerators already initialized in this process. XPU
            # model workers exit at each step boundary, and hard_free_gpu must
            # not create a new persistent Level Zero context in this parent.
            from gpu import hard_free_gpu

            hard_free_gpu(f"audio-step:{step.step_id}")

    bindings: dict[str, StemArtifact] = {}
    for binding in plan.output_bindings:
        if binding.expression and "sum" in binding.expression:
            bindings[binding.artifact_role] = _sum_stems(
                binding,
                produced,
                work_root,
                step_results,
            )
            continue
        artifact = produced.get((binding.step_id, binding.role))
        if artifact is None:
            raise OutputContractError(
                f"binding {binding.artifact_role} missing {binding.step_id}/{binding.role}"
            )
        bindings[binding.artifact_role] = artifact
    return AudioProcessingExecutionResult(plan=plan, step_results=step_results, bindings=bindings)


def _sum_stems(
    binding,
    produced: Mapping[tuple[str, str], StemArtifact],
    work_root: Path,
    step_results: Mapping[str, ProcessorResult],
) -> StemArtifact:
    import numpy as np
    import soundfile as sf

    names = list(binding.expression["sum"])
    tensors = []
    sample_rate = None
    for name in names:
        artifact = produced.get((binding.step_id, name))
        if artifact is None:
            raise OutputContractError(
                f"cannot sum missing stem {name} from {binding.step_id}"
            )
        data, rate = sf.read(str(artifact.path), dtype="float32", always_2d=True)
        if sample_rate is None:
            sample_rate = rate
        tensors.append(data)
    mixed = np.sum(np.stack(tensors, axis=0), axis=0)
    dest = work_root / f"{binding.step_id}__{binding.artifact_role}{INTERMEDIATE_SUFFIX}"
    sf.write(str(dest), mixed, sample_rate, subtype="FLOAT")
    return StemArtifact(
        role=binding.artifact_role,
        source_stem_name="+".join(names),
        path=dest,
        sample_rate=int(sample_rate or 44100),
        channels=mixed.shape[1],
    )
