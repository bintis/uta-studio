"""MDX ONNX runner. OpenVINO GPU work is dispatched to a helper process."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

from audio_models.catalog import ModelSpec
from audio_models.errors import ModelConfigurationError
from audio_models.parameters import ResolvedParameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.contracts import ProcessorResult, ProgressSink, requested_backend_for
from audio_processors.outputs import descriptor_from_spec, map_named_outputs, path_for_stem
from audio_processors.runners.base import (
    emit,
    resolve_installed_file,
    run_with_whole_model_fallback,
)


class MdxOnnxRunner:
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
        step_id: str = "mdx",
    ) -> ProcessorResult:
        if model_spec.architecture != "mdx_onnx":
            raise ModelConfigurationError(
                f"{model_spec.id} is not an MDX ONNX model",
                model_id=model_spec.id,
            )
        if installed_dir is None:
            raise ModelConfigurationError("installed_dir is required", model_id=model_spec.id)
        models_root = installed_dir.parent.parent

        def execute(backend: str) -> ProcessorResult:
            onnx_path = resolve_installed_file(
                models_root, model_spec, model_spec.file("checkpoint")
            )
            metadata_path = resolve_installed_file(
                models_root, model_spec, model_spec.file("normalized_metadata")
            )
            emit(
                progress_sink,
                8,
                f"Loading {model_spec.display_name}",
                model_id=model_spec.id,
                architecture=model_spec.architecture,
                actual_backend=backend,
            )
            if parameters.get("mdx.segmentPolicy") not in {None, "model_shape"}:
                raise ModelConfigurationError(
                    "OpenVINO/ONNX MDX segment overrides are disabled because they change the execution route",
                    model_id=model_spec.id,
                )
            descriptor = descriptor_from_spec(model_spec, step_id)
            if backend in {"openvino_gpu", "openvino_cpu"}:
                named = _run_openvino_helper(
                    onnx_path=onnx_path,
                    metadata_path=metadata_path,
                    input_path=input_path,
                    work_dir=work_dir,
                    backend=backend,
                    output_names=dict(descriptor.output_names),
                    model_spec=model_spec,
                )
            else:
                named = _run_onnx_local(
                    onnx_path=onnx_path,
                    metadata_path=metadata_path,
                    input_path=input_path,
                    work_dir=work_dir,
                    backend=backend,
                    output_names=dict(descriptor.output_names),
                    model_spec=model_spec,
                )
            mapped = {
                stem: named[stem] if stem in named else path_for_stem(work_dir, descriptor, stem)
                for stem in model_spec.expected_stems
            }
            artifacts = map_named_outputs(
                model_spec,
                mapped,
                sample_rate=44100,
                channels=2,
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


def _run_openvino_helper(
    *,
    onnx_path: Path,
    metadata_path: Path,
    input_path: Path,
    work_dir: Path,
    backend: str,
    output_names: dict[str, str],
    model_spec: ModelSpec,
) -> dict[str, Path]:
    helper = Path(__file__).resolve().parents[2] / "openvino_mdx.py"
    command = [
        sys.executable,
        str(helper),
        "--model",
        str(onnx_path),
        "--metadata",
        str(metadata_path),
        "--input",
        str(input_path),
        "--output-dir",
        str(work_dir),
        "--backend",
        backend,
        "--output-names",
        json.dumps(output_names),
    ]
    env = os.environ.copy()
    env["UTA_STUDIO_MDX_MODEL_ID"] = model_spec.id
    completed = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip() or "OpenVINO MDX helper failed")
    payload = json.loads(completed.stdout)
    return {stem: Path(path) for stem, path in payload.get("stems", {}).items()}


def _run_onnx_local(
    *,
    onnx_path: Path,
    metadata_path: Path,
    input_path: Path,
    work_dir: Path,
    backend: str,
    output_names: dict[str, str],
    model_spec: ModelSpec,
) -> dict[str, Path]:
    from openvino_mdx import run_mdx_onnx

    return run_mdx_onnx(
        onnx_path=onnx_path,
        metadata_path=metadata_path,
        input_path=input_path,
        work_dir=work_dir,
        backend=backend,
        output_names=output_names,
        model_id=model_spec.id,
    )
