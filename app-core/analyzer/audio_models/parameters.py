"""Four-layer parameter resolution with explicit sources."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping

from .catalog import ModelSpec
from .errors import ParameterValidationError
from .schema import (
    LOCKED_PARAMETER_KEYS,
    PARAMETER_SPECS,
    AudioParameterSpec,
    AudioParameterValue,
    schema_for,
)

ParameterMap = dict[str, AudioParameterValue]


@dataclass(frozen=True)
class ResolvedParameter:
    key: str
    value: AudioParameterValue
    source: str
    clamped: bool = False

    def as_json(self) -> dict[str, object]:
        return {
            "value": self.value.as_json(),
            "source": self.source,
            "clamped": self.clamped,
        }


@dataclass(frozen=True)
class ResolvedParameters:
    values: dict[str, ResolvedParameter]

    def as_map(self) -> dict[str, Any]:
        return {key: item.value.as_json() for key, item in sorted(self.values.items())}

    def as_json(self) -> dict[str, object]:
        return {key: item.as_json() for key, item in sorted(self.values.items())}

    def canonical_json(self) -> str:
        import json

        return json.dumps(self.as_map(), sort_keys=True, separators=(",", ":"), ensure_ascii=False)

    def get(self, key: str, default: Any = None) -> Any:
        item = self.values.get(key)
        if item is None:
            return default
        return item.value.as_json()


def _as_user_map(raw: Mapping[str, Any] | None) -> dict[str, Any]:
    if not raw:
        return {}
    return {str(key): value for key, value in raw.items()}


def _architecture_prefix(model: ModelSpec) -> str | None:
    if model.architecture.startswith("mdxc_"):
        return "mdxc."
    if model.architecture == "mdx_onnx":
        return "mdx."
    if model.architecture == "demucs":
        return "demucs."
    if model.architecture == "vr":
        return "vr."
    return None


def _is_applicable(spec: AudioParameterSpec, model: ModelSpec) -> bool:
    if spec.key in LOCKED_PARAMETER_KEYS or spec.locked:
        return False
    if spec.architecture is None:
        return spec.key.startswith(("common.", "runtime."))
    prefix = _architecture_prefix(model)
    if prefix is None:
        return False
    return spec.key.startswith(prefix)


def _reject_locked_override(key: str) -> None:
    if key in LOCKED_PARAMETER_KEYS or (
        key in PARAMETER_SPECS and PARAMETER_SPECS[key].locked
    ):
        raise ParameterValidationError(
            f"{key} is model-locked and cannot be overridden",
            model_id=None,
        )


def resolve_parameters(
    model_spec: ModelSpec,
    *,
    global_overrides: Mapping[str, Any] | None = None,
    song_overrides: Mapping[str, Any] | None = None,
    run_overrides: Mapping[str, Any] | None = None,
    device_capabilities: Mapping[str, Any] | None = None,
    model_defaults: Mapping[str, Any] | None = None,
) -> ResolvedParameters:
    schema = schema_for(model_spec.parameter_schema_id)
    layers = [
        ("global_settings", _as_user_map(global_overrides)),
        ("song_profile", _as_user_map(song_overrides)),
        ("run_override", _as_user_map(run_overrides)),
    ]
    overlap_keys = {"mdxc.overlapCount", "mdx.overlapRatio", "demucs.overlapRatio"}
    used_overlap = {
        key
        for _, mapping in layers
        for key in mapping
        if key in overlap_keys
    }
    if "mdxc.overlapCount" in used_overlap and "mdx.overlapRatio" in used_overlap:
        raise ParameterValidationError(
            "overlapCount and overlapRatio cannot be mixed",
            model_id=model_spec.id,
        )
    known = set(schema.specs)
    for _, mapping in layers:
        for key in mapping:
            _reject_locked_override(key)
            if key not in known:
                raise ParameterValidationError(
                    f"{key} does not apply to {model_spec.architecture}",
                    model_id=model_spec.id,
                )

    resolved: dict[str, ResolvedParameter] = {}
    for key, spec in schema.specs.items():
        if not _is_applicable(spec, model_spec):
            for _, mapping in layers:
                if key in mapping:
                    raise ParameterValidationError(
                        f"{key} does not apply to {model_spec.architecture}",
                        model_id=model_spec.id,
                    )
            continue
        if spec.locked:
            continue
        source = "model_default"
        raw_value: Any = spec.default.as_json()
        if model_defaults and key in model_defaults:
            raw_value = model_defaults[key]
            source = "model_default"
        for layer_name, mapping in layers:
            if key not in mapping:
                continue
            _reject_locked_override(key)
            raw_value = mapping[key]
            source = layer_name
        try:
            value, clamped = spec.clamp(raw_value)
        except ValueError as exc:
            raise ParameterValidationError(str(exc), model_id=model_spec.id) from exc
        if clamped:
            source = "runtime_clamp"
        resolved[key] = ResolvedParameter(key=key, value=value, source=source, clamped=clamped)

    capabilities = device_capabilities or {}
    requested_precision = resolved.get("runtime.precisionPolicy")
    if requested_precision and requested_precision.value.as_json() != "fp32":
        if not capabilities.get("allow_reduced_precision"):
            resolved["runtime.precisionPolicy"] = ResolvedParameter(
                key="runtime.precisionPolicy",
                value=AudioParameterValue("fp32"),
                source="backend_resolution",
                clamped=True,
            )

    for key, spec in PARAMETER_SPECS.items():
        if spec.locked:
            for _, mapping in layers:
                if key in mapping:
                    raise ParameterValidationError(
                        f"{key} is model-locked and cannot be overridden",
                        model_id=model_spec.id,
                    )

    return ResolvedParameters(resolved)
