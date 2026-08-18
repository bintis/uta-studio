"""Runner contracts and semantic artifacts."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Mapping, Protocol

from audio_models.catalog import ModelSpec
from audio_models.parameters import ResolvedParameters
from audio_models.plan import AudioRuntimeRequest


@dataclass(frozen=True)
class StemArtifact:
    role: str
    source_stem_name: str
    path: Path
    sample_rate: int
    channels: int


@dataclass(frozen=True)
class LoadedModelDescriptor:
    target_stem: str | None
    source_stems: tuple[str, ...]
    output_names: Mapping[str, str]


@dataclass(frozen=True)
class ProcessorResult:
    model_id: str
    architecture: str
    artifacts: Mapping[str, StemArtifact]
    requested_backend: str
    actual_backend: str
    precision: str
    fallback_from: str | None = None
    fallback_reason: str | None = None
    effective_parameters: Mapping[str, object] | None = None

    def require(self, role: str) -> StemArtifact:
        try:
            return self.artifacts[role]
        except KeyError as exc:
            from audio_models.errors import OutputContractError

            raise OutputContractError(
                f"{self.model_id} did not produce required role {role!r}",
                model_id=self.model_id,
            ) from exc


class ProgressSink(Protocol):
    def __call__(
        self,
        percent: int,
        message: str,
        **metadata: object,
    ) -> None: ...


class AudioProcessorRunner(Protocol):
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
    ) -> ProcessorResult: ...


def deterministic_output_names(step_id: str, stems: tuple[str, ...]) -> dict[str, str]:
    return {stem: f"step_{step_id}__{stem.lower().replace(' ', '_')}" for stem in stems}


def requested_backend_for(model_spec: ModelSpec, runtime: AudioRuntimeRequest) -> str:
    if model_spec.runner == "mdx_onnx":
        return runtime.onnx_backend
    return runtime.torch_backend
