"""Immutable processing plans and legacy separator conversion."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping

from .catalog import (
    DEFAULT_LEGACY_KARAOKE_MODEL_ID,
    AudioModelCatalog,
    ModelSpec,
    load_catalog,
)
from .errors import ParameterValidationError
from .parameters import ResolvedParameters, resolve_parameters


LEGACY_PROFILES = {
    "karaoke": "legacy_karaoke_roformer",
    "demucs": "legacy_htdemucs",
    "openvino_demucs": "legacy_openvino_demucs",
}


@dataclass(frozen=True)
class AudioInputReference:
    kind: str
    step_id: str | None = None
    role: str | None = None

    @classmethod
    def source_media(cls) -> "AudioInputReference":
        return cls(kind="source_media")

    @classmethod
    def step_output(cls, step_id: str, role: str) -> "AudioInputReference":
        return cls(kind="step_output", step_id=step_id, role=role)

    def as_json(self) -> dict[str, object]:
        if self.kind == "source_media":
            return {"kind": "source_media"}
        return {"kind": "step_output", "step_id": self.step_id, "role": self.role}


@dataclass(frozen=True)
class AudioProcessingStep:
    step_id: str
    model_id: str
    input: AudioInputReference
    selected_output_roles: tuple[str, ...]
    effective_parameters: Mapping[str, Any]
    parameter_sources: Mapping[str, object] = field(default_factory=dict)

    def as_json(self) -> dict[str, object]:
        return {
            "step_id": self.step_id,
            "model_id": self.model_id,
            "input": self.input.as_json(),
            "selected_output_roles": list(self.selected_output_roles),
            "effective_parameters": dict(self.effective_parameters),
            "parameter_sources": dict(self.parameter_sources),
        }


@dataclass(frozen=True)
class AudioOutputBinding:
    artifact_role: str
    step_id: str
    role: str
    expression: Mapping[str, object] | None = None

    def as_json(self) -> dict[str, object]:
        payload: dict[str, object] = {
            "artifact_role": self.artifact_role,
            "step_id": self.step_id,
            "role": self.role,
        }
        if self.expression is not None:
            payload["expression"] = dict(self.expression)
            if "sum" in self.expression:
                payload["sum"] = list(self.expression["sum"])
        return payload


@dataclass(frozen=True)
class AudioRuntimeRequest:
    torch_backend: str
    onnx_backend: str
    precision_policy: str
    fallback_policy: str = "whole_model_cpu"

    def as_json(self) -> dict[str, object]:
        return {
            "torch_backend": self.torch_backend,
            "onnx_backend": self.onnx_backend,
            "precision_policy": self.precision_policy,
            "fallback_policy": self.fallback_policy,
        }


@dataclass(frozen=True)
class AudioProcessingPlanSnapshot:
    schema_version: int
    catalog_version: str
    steps: tuple[AudioProcessingStep, ...]
    output_bindings: tuple[AudioOutputBinding, ...]
    requested_runtime: AudioRuntimeRequest
    profile_id: str | None = None

    def as_json(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "catalog_version": self.catalog_version,
            "profile_id": self.profile_id,
            "steps": [step.as_json() for step in self.steps],
            "output_bindings": [binding.as_json() for binding in self.output_bindings],
            "requested_runtime": self.requested_runtime.as_json(),
        }

    def step(self, step_id: str) -> AudioProcessingStep:
        for item in self.steps:
            if item.step_id == step_id:
                return item
        raise KeyError(step_id)


def _parse_input(raw: Mapping[str, Any]) -> AudioInputReference:
    if raw.get("kind") == "source_media" or raw == {"kind": "source_media"}:
        return AudioInputReference.source_media()
    if raw.get("kind") == "step_output" or ("step_id" in raw and "role" in raw):
        return AudioInputReference.step_output(str(raw["step_id"]), str(raw["role"]))
    raise ParameterValidationError(f"invalid audio input reference: {raw!r}")


def plan_from_json(raw: Mapping[str, Any]) -> AudioProcessingPlanSnapshot:
    steps = []
    for item in raw.get("steps") or []:
        steps.append(
            AudioProcessingStep(
                step_id=str(item["step_id"]),
                model_id=str(item["model_id"]),
                input=_parse_input(item.get("input") or {}),
                selected_output_roles=tuple(str(role) for role in item.get("selected_output_roles") or ()),
                effective_parameters=dict(item.get("effective_parameters") or {}),
                parameter_sources=dict(item.get("parameter_sources") or {}),
            )
        )
    bindings = []
    for item in raw.get("output_bindings") or []:
        bindings.append(
            AudioOutputBinding(
                artifact_role=str(item["artifact_role"]),
                step_id=str(item["step_id"]),
                role=str(item["role"]),
                expression=(
                    dict(item["expression"])
                    if item.get("expression")
                    else {"sum": list(item["sum"])} if item.get("sum") else None
                ),
            )
        )
    runtime_raw = raw.get("requested_runtime") or {}
    return AudioProcessingPlanSnapshot(
        schema_version=int(raw.get("schema_version") or 1),
        catalog_version=str(raw.get("catalog_version") or ""),
        steps=tuple(steps),
        output_bindings=tuple(bindings),
        requested_runtime=AudioRuntimeRequest(
            torch_backend=str(runtime_raw.get("torch_backend") or "torch_cpu"),
            onnx_backend=str(runtime_raw.get("onnx_backend") or "onnx_cpu"),
            precision_policy=str(runtime_raw.get("precision_policy") or "fp32"),
            fallback_policy=str(runtime_raw.get("fallback_policy") or "whole_model_cpu"),
        ),
        profile_id=str(raw["profile_id"]) if raw.get("profile_id") else None,
    )


def _resolved_for(
    model: ModelSpec,
    *,
    global_overrides: Mapping[str, Any] | None,
    song_overrides: Mapping[str, Any] | None,
    run_overrides: Mapping[str, Any] | None,
    model_defaults: Mapping[str, Any] | None = None,
) -> ResolvedParameters:
    return resolve_parameters(
        model,
        global_overrides=global_overrides,
        song_overrides=song_overrides,
        run_overrides=run_overrides,
        model_defaults=model_defaults,
    )


def _step(
    step_id: str,
    model: ModelSpec,
    input_ref: AudioInputReference,
    roles: tuple[str, ...],
    resolved: ResolvedParameters,
) -> AudioProcessingStep:
    return AudioProcessingStep(
        step_id=step_id,
        model_id=model.id,
        input=input_ref,
        selected_output_roles=roles,
        effective_parameters=resolved.as_map(),
        parameter_sources=resolved.as_json(),
    )


def build_chart_analysis_plan(
    catalog: AudioModelCatalog,
    settings: Mapping[str, Any],
    *,
    global_overrides: Mapping[str, Any] | None = None,
    song_overrides: Mapping[str, Any] | None = None,
    run_overrides: Mapping[str, Any] | None = None,
) -> AudioProcessingPlanSnapshot:
    vocal_id = settings.get("vocal_model_id") or DEFAULT_LEGACY_KARAOKE_MODEL_ID
    accompaniment_id = settings.get("accompaniment_model_id")
    cleanup = list(settings.get("vocal_cleanup_chain") or [])
    vocal = catalog.get(str(vocal_id))
    vocal_roles = ("extracted_vocal", "residual_instrumental") if not accompaniment_id else ("extracted_vocal",)
    steps: list[AudioProcessingStep] = [
        _step(
            "extract_vocals",
            vocal,
            AudioInputReference.source_media(),
            vocal_roles,
            _resolved_for(
                vocal,
                global_overrides=global_overrides,
                song_overrides=song_overrides,
                run_overrides=run_overrides,
            ),
        )
    ]
    current_step = "extract_vocals"
    current_role = "extracted_vocal"
    for model_id in cleanup:
        cleanup_model = catalog.get(str(model_id))
        if "denoise" in str(model_id):
            step_id, role = "denoise_vocals", "clean_audio"
        else:
            step_id, role = "dereverb_vocals", "dry_audio"
        steps.append(
            _step(
                step_id,
                cleanup_model,
                AudioInputReference.step_output(current_step, current_role),
                (role,),
                _resolved_for(
                    cleanup_model,
                    global_overrides=global_overrides,
                    song_overrides=song_overrides,
                    run_overrides=run_overrides,
                ),
            )
        )
        current_step = step_id
        current_role = role
    bindings_list = [
        AudioOutputBinding("analysis_vocal", current_step, current_role),
        AudioOutputBinding("vocals", current_step, current_role),
    ]
    if accompaniment_id:
        accompaniment = catalog.get(str(accompaniment_id))
        steps.append(
            _step(
                "extract_accompaniment",
                accompaniment,
                AudioInputReference.source_media(),
                ("instrumental",),
                _resolved_for(
                    accompaniment,
                    global_overrides=global_overrides,
                    song_overrides=song_overrides,
                    run_overrides=run_overrides,
                ),
            )
        )
        bindings_list.append(
            AudioOutputBinding("instrumental", "extract_accompaniment", "instrumental")
        )
    else:
        bindings_list.append(
            AudioOutputBinding("instrumental", "extract_vocals", "residual_instrumental")
        )
    bindings = tuple(bindings_list)
    return AudioProcessingPlanSnapshot(
        schema_version=1,
        catalog_version=catalog.catalog_version,
        steps=tuple(steps),
        output_bindings=bindings,
        requested_runtime=_runtime_from_settings(settings),
        profile_id="chart_analysis_hq",
    )


def _is_demucs_chart_path(settings: Mapping[str, Any]) -> bool:
    return settings.get("legacy_profile") == "legacy_htdemucs" or (
        settings.get("multistem_model_id") == "htdemucs_6s" and not settings.get("vocal_model_id")
    )


def build_settings_plan(
    catalog: AudioModelCatalog,
    settings: Mapping[str, Any],
    *,
    global_overrides: Mapping[str, Any] | None = None,
    song_overrides: Mapping[str, Any] | None = None,
    run_overrides: Mapping[str, Any] | None = None,
) -> AudioProcessingPlanSnapshot:
    """Compose the chart path plus optional karaoke/six-stem side paths."""
    kwargs = {
        "global_overrides": global_overrides,
        "song_overrides": song_overrides,
        "run_overrides": run_overrides,
    }
    if _is_demucs_chart_path(settings):
        return build_multistem_plan(catalog, settings, **kwargs)
    plan = build_chart_analysis_plan(catalog, settings, **kwargs)
    extra_steps = list(plan.steps)
    extra_bindings = list(plan.output_bindings)
    if settings.get("karaoke_model_id"):
        karaoke = build_karaoke_plan(catalog, settings, **kwargs)
        extra_steps.extend(karaoke.steps)
        extra_bindings.append(
            AudioOutputBinding("karaoke_instrumental", "extract_karaoke", "karaoke_instrumental")
        )
    if settings.get("multistem_model_id"):
        multi = build_multistem_plan(catalog, settings, **kwargs)
        extra_steps.extend(multi.steps)
        extra_bindings.extend(
            binding
            for binding in multi.output_bindings
            if binding.artifact_role not in {"vocals", "instrumental", "analysis_vocal"}
        )
    return AudioProcessingPlanSnapshot(
        schema_version=plan.schema_version,
        catalog_version=plan.catalog_version,
        steps=tuple(extra_steps),
        output_bindings=tuple(extra_bindings),
        requested_runtime=plan.requested_runtime,
        profile_id=plan.profile_id,
    )


def build_karaoke_plan(
    catalog: AudioModelCatalog,
    settings: Mapping[str, Any],
    *,
    global_overrides: Mapping[str, Any] | None = None,
    song_overrides: Mapping[str, Any] | None = None,
    run_overrides: Mapping[str, Any] | None = None,
) -> AudioProcessingPlanSnapshot:
    model = catalog.get(str(settings.get("karaoke_model_id") or "uvr_mdxnet_karaoke_2"))
    resolved = _resolved_for(
        model,
        global_overrides=global_overrides,
        song_overrides=song_overrides,
        run_overrides=run_overrides,
    )
    return AudioProcessingPlanSnapshot(
        schema_version=1,
        catalog_version=catalog.catalog_version,
        steps=(
            _step(
                "extract_karaoke",
                model,
                AudioInputReference.source_media(),
                ("karaoke_instrumental", "extracted_vocal"),
                resolved,
            ),
        ),
        output_bindings=(
            AudioOutputBinding("instrumental", "extract_karaoke", "karaoke_instrumental"),
            AudioOutputBinding("vocals", "extract_karaoke", "extracted_vocal"),
            AudioOutputBinding("analysis_vocal", "extract_karaoke", "extracted_vocal"),
        ),
        requested_runtime=_runtime_from_settings(settings),
        profile_id="karaoke_hq",
    )


def build_multistem_plan(
    catalog: AudioModelCatalog,
    settings: Mapping[str, Any],
    *,
    global_overrides: Mapping[str, Any] | None = None,
    song_overrides: Mapping[str, Any] | None = None,
    run_overrides: Mapping[str, Any] | None = None,
) -> AudioProcessingPlanSnapshot:
    model = catalog.get(str(settings.get("multistem_model_id") or "htdemucs_6s"))
    resolved = _resolved_for(
        model,
        global_overrides=global_overrides,
        song_overrides=song_overrides,
        run_overrides=run_overrides,
    )
    return AudioProcessingPlanSnapshot(
        schema_version=1,
        catalog_version=catalog.catalog_version,
        steps=(
            _step(
                "separate_6s",
                model,
                AudioInputReference.source_media(),
                ("vocals", "drums", "bass", "guitar", "piano", "other"),
                resolved,
            ),
        ),
        output_bindings=(
            AudioOutputBinding("vocals", "separate_6s", "vocals"),
            AudioOutputBinding("analysis_vocal", "separate_6s", "vocals"),
            AudioOutputBinding(
                "instrumental",
                "separate_6s",
                "instrumental",
                expression={"sum": ["drums", "bass", "guitar", "piano", "other"]},
            ),
            AudioOutputBinding("drums", "separate_6s", "drums"),
            AudioOutputBinding("bass", "separate_6s", "bass"),
            AudioOutputBinding("guitar", "separate_6s", "guitar"),
            AudioOutputBinding("piano", "separate_6s", "piano"),
            AudioOutputBinding("other", "separate_6s", "other"),
        ),
        requested_runtime=_runtime_from_settings(settings),
        profile_id="multistem_6s",
    )


def _runtime_from_settings(settings: Mapping[str, Any]) -> AudioRuntimeRequest:
    return AudioRuntimeRequest(
        torch_backend=str(settings.get("torch_backend") or "torch_cpu"),
        onnx_backend=str(settings.get("onnx_backend") or "onnx_cpu"),
        precision_policy=str(settings.get("precision_policy") or "fp32"),
        fallback_policy=str(settings.get("fallback_policy") or "whole_model_cpu"),
    )


def legacy_plan_from_separator(
    separator: str,
    *,
    catalog: AudioModelCatalog | None = None,
    separator_options: Mapping[str, Any] | None = None,
    requested_device: str | None = None,
) -> AudioProcessingPlanSnapshot:
    """Convert a frozen legacy separator string into an immutable plan."""
    loaded = catalog or load_catalog()
    options = dict(separator_options or {})
    torch_backend = _device_to_torch(requested_device)
    onnx_backend = _device_to_onnx(requested_device)
    settings = {
        "torch_backend": torch_backend,
        "onnx_backend": onnx_backend,
        "precision_policy": "fp32",
    }
    if separator == "demucs":
        model = loaded.get("htdemucs_6s")
        defaults = {
            "demucs.shifts": max(0, min(20, int(options.get("demucs_shifts", 1)))),
            "demucs.overlapRatio": max(0.01, min(0.99, int(options.get("demucs_overlap_pct", 25)) / 100.0)),
        }
        resolved = _resolved_for(model, global_overrides=defaults, song_overrides=None, run_overrides=None)
        return AudioProcessingPlanSnapshot(
            schema_version=1,
            catalog_version=loaded.catalog_version,
            steps=(
                _step(
                    "legacy_htdemucs",
                    model,
                    AudioInputReference.source_media(),
                    ("vocals", "drums", "bass", "other"),
                    resolved,
                ),
            ),
            output_bindings=(
                AudioOutputBinding("vocals", "legacy_htdemucs", "vocals"),
                AudioOutputBinding("analysis_vocal", "legacy_htdemucs", "vocals"),
                AudioOutputBinding(
                    "instrumental",
                    "legacy_htdemucs",
                    "instrumental",
                    expression={"sum": ["drums", "bass", "other"]},
                ),
            ),
            requested_runtime=_runtime_from_settings(settings),
            profile_id=LEGACY_PROFILES[separator],
        )
    if separator == "openvino_demucs":
        return AudioProcessingPlanSnapshot(
            schema_version=1,
            catalog_version=loaded.catalog_version,
            steps=(),
            output_bindings=(),
            requested_runtime=AudioRuntimeRequest(
                torch_backend="torch_cpu",
                onnx_backend="openvino_gpu",
                precision_policy="fp32",
            ),
            profile_id=LEGACY_PROFILES[separator],
        )
    karaoke_settings = {
        **settings,
        "vocal_model_id": DEFAULT_LEGACY_KARAOKE_MODEL_ID,
        "accompaniment_model_id": None,
        "vocal_cleanup_chain": [],
    }
    plan = build_chart_analysis_plan(loaded, karaoke_settings)
    return AudioProcessingPlanSnapshot(
        schema_version=plan.schema_version,
        catalog_version=plan.catalog_version,
        steps=plan.steps,
        output_bindings=plan.output_bindings,
        requested_runtime=plan.requested_runtime,
        profile_id=LEGACY_PROFILES.get(separator, "legacy_karaoke_roformer"),
    )


def _device_to_torch(device: str | None) -> str:
    if device == "cuda":
        return "torch_cuda"
    if device == "xpu":
        return "torch_xpu"
    return "torch_cpu"


def _device_to_onnx(device: str | None) -> str:
    if device == "cuda":
        return "onnx_cuda"
    if device in {"xpu", "intel"}:
        return "openvino_gpu"
    return "onnx_cpu"
