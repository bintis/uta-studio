"""Restricted value types and parameter specifications."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Mapping


ALLOWED_ARCHITECTURES = {
    "mdxc_bs_roformer",
    "mdxc_melband_roformer",
    "mdx_onnx",
    "demucs",
    "vr",
}

ALLOWED_RUNNERS = {
    "mdxc_torch",
    "mdx_onnx",
    "demucs_torch",
}

ALLOWED_BACKENDS = {
    "torch_cuda",
    "torch_xpu",
    "torch_cpu",
    "openvino_gpu",
    "openvino_cpu",
    "onnx_cuda",
    "onnx_cpu",
}

ALLOWED_INPUT_ROLES = {
    "source_mix",
    "extracted_vocal",
    "clean_audio",
    "dry_audio",
    "instrumental",
}

ALLOWED_OPERATIONS = {
    "separate_vocals",
    "separate_instrumental",
    "separate_karaoke",
    "separate_multistem",
    "denoise",
    "dereverb",
}

LOCKED_PARAMETER_KEYS = {
    "architecture",
    "model_dim",
    "model_depth",
    "model_heads",
    "frequency_bands",
    "sample_rate",
    "n_fft",
    "stft_hop_length",
    "stft_window_length",
    "dim_f",
    "dim_t",
    "stem_count",
    "instrument_ordering",
    "target_instrument",
    "compensation_factor",
    "mask_estimator",
    "mdx.hopLength",
}

PLACEHOLDER_HASH_TOKENS = {
    "REPLACE_WITH_VERIFIED_FULL_SHA256",
    "TODO",
    "UNKNOWN",
    "",
}


def is_sha256(value: str) -> bool:
    if len(value) != 64:
        return False
    return all(char in "0123456789abcdef" for char in value.lower())


def reject_placeholder(value: str, *, field: str) -> None:
    if value.strip() in PLACEHOLDER_HASH_TOKENS or value.upper() in PLACEHOLDER_HASH_TOKENS:
        raise ValueError(f"{field} must not be a placeholder")
    if not is_sha256(value):
        raise ValueError(f"{field} must be a full SHA-256 hex digest")


class AudioParameterValue:
    """Restricted parameter value: bool, int, float, or text."""

    __slots__ = ("kind", "value")

    def __init__(self, value: bool | int | float | str) -> None:
        if isinstance(value, bool):
            self.kind = "bool"
            self.value: bool | int | float | str = value
        elif isinstance(value, int) and not isinstance(value, bool):
            self.kind = "integer"
            self.value = int(value)
        elif isinstance(value, float):
            self.kind = "number"
            self.value = float(value)
        elif isinstance(value, str):
            self.kind = "text"
            self.value = value
        else:
            raise TypeError(f"unsupported audio parameter value: {type(value)!r}")

    def as_json(self) -> bool | int | float | str:
        return self.value

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, AudioParameterValue):
            return NotImplemented
        return self.kind == other.kind and self.value == other.value

    def __repr__(self) -> str:
        return f"AudioParameterValue({self.value!r})"


def coerce_parameter_value(value: Any) -> AudioParameterValue:
    if isinstance(value, AudioParameterValue):
        return value
    if isinstance(value, bool):
        return AudioParameterValue(value)
    if isinstance(value, int) and not isinstance(value, bool):
        return AudioParameterValue(value)
    if isinstance(value, float):
        return AudioParameterValue(value)
    if isinstance(value, str):
        return AudioParameterValue(value)
    raise TypeError(f"cannot store {type(value)!r} as an audio parameter")


@dataclass(frozen=True)
class AudioParameterSpec:
    key: str
    value_type: str
    default: AudioParameterValue
    minimum: float | None = None
    maximum: float | None = None
    allowed_values: tuple[AudioParameterValue, ...] = ()
    advanced: bool = False
    affects_quality: bool = False
    affects_memory: bool = False
    affects_cache: bool = True
    unit: str | None = None
    applicable_backends: tuple[str, ...] = ()
    architecture: str | None = None
    locked: bool = False

    def validate(self, raw: Any) -> AudioParameterValue:
        value = coerce_parameter_value(raw)
        if self.locked:
            raise ValueError(f"{self.key} is model-locked and cannot be overridden")
        if self.value_type == "bool" and value.kind != "bool":
            raise ValueError(f"{self.key} must be a boolean")
        if self.value_type == "integer":
            if value.kind not in {"integer"}:
                raise ValueError(f"{self.key} must be an integer")
            number = float(value.value)
        elif self.value_type == "number":
            if value.kind not in {"integer", "number"}:
                raise ValueError(f"{self.key} must be a number")
            number = float(value.value)
        else:
            number = None
        if number is not None:
            if self.minimum is not None and number < self.minimum:
                raise ValueError(f"{self.key} is below the minimum {self.minimum}")
            if self.maximum is not None and number > self.maximum:
                raise ValueError(f"{self.key} is above the maximum {self.maximum}")
        if self.allowed_values and value not in self.allowed_values:
            allowed = ", ".join(str(item.value) for item in self.allowed_values)
            raise ValueError(f"{self.key} must be one of: {allowed}")
        return value

    def clamp(self, raw: Any) -> tuple[AudioParameterValue, bool]:
        value = coerce_parameter_value(raw)
        if self.value_type not in {"integer", "number"} or value.kind not in {
            "integer",
            "number",
        }:
            return self.validate(raw), False
        number = float(value.value)
        clamped = False
        if self.minimum is not None and number < self.minimum:
            number = self.minimum
            clamped = True
        if self.maximum is not None and number > self.maximum:
            number = self.maximum
            clamped = True
        if self.value_type == "integer":
            return AudioParameterValue(int(round(number))), clamped
        return AudioParameterValue(number), clamped


@dataclass(frozen=True)
class ParameterSchema:
    schema_id: str
    specs: Mapping[str, AudioParameterSpec] = field(default_factory=dict)

    def get(self, key: str) -> AudioParameterSpec | None:
        return self.specs.get(key)


def _enum_spec(
    key: str,
    default: str,
    allowed: tuple[str, ...],
    *,
    advanced: bool = False,
    architecture: str | None = None,
    unit: str | None = None,
) -> AudioParameterSpec:
    values = tuple(AudioParameterValue(item) for item in allowed)
    return AudioParameterSpec(
        key=key,
        value_type="text",
        default=AudioParameterValue(default),
        allowed_values=values,
        advanced=advanced,
        architecture=architecture,
        unit=unit,
        affects_cache=True,
    )


def _int_spec(
    key: str,
    default: int,
    minimum: float,
    maximum: float,
    *,
    advanced: bool = False,
    architecture: str | None = None,
    unit: str | None = None,
    locked: bool = False,
) -> AudioParameterSpec:
    return AudioParameterSpec(
        key=key,
        value_type="integer",
        default=AudioParameterValue(default),
        minimum=minimum,
        maximum=maximum,
        advanced=advanced,
        architecture=architecture,
        unit=unit,
        locked=locked,
        affects_memory=True,
        affects_cache=True,
    )


def _num_spec(
    key: str,
    default: float,
    minimum: float,
    maximum: float,
    *,
    advanced: bool = False,
    architecture: str | None = None,
    unit: str | None = None,
) -> AudioParameterSpec:
    return AudioParameterSpec(
        key=key,
        value_type="number",
        default=AudioParameterValue(default),
        minimum=minimum,
        maximum=maximum,
        advanced=advanced,
        architecture=architecture,
        unit=unit,
        affects_quality=True,
        affects_cache=True,
    )


def _bool_spec(
    key: str,
    default: bool,
    *,
    advanced: bool = False,
    architecture: str | None = None,
) -> AudioParameterSpec:
    return AudioParameterSpec(
        key=key,
        value_type="bool",
        default=AudioParameterValue(default),
        advanced=advanced,
        architecture=architecture,
        affects_cache=True,
    )


def common_parameter_specs() -> dict[str, AudioParameterSpec]:
    return {
        "common.normalizationThreshold": _num_spec(
            "common.normalizationThreshold", 0.9, 0.0, 1.0, unit="peak_ratio"
        ),
        "common.amplificationThreshold": _num_spec(
            "common.amplificationThreshold",
            0.0,
            0.0,
            1.0,
            advanced=True,
            unit="peak_ratio",
        ),
        "runtime.precisionPolicy": _enum_spec(
            "runtime.precisionPolicy",
            "fp32",
            ("fp32", "fp16", "bf16", "auto"),
        ),
        "runtime.memoryPolicy": _enum_spec(
            "runtime.memoryPolicy",
            "normal",
            ("normal", "low_memory"),
        ),
        "runtime.torchBackend": _enum_spec(
            "runtime.torchBackend",
            "torch_cpu",
            ("torch_cuda", "torch_xpu", "torch_cpu"),
        ),
        "runtime.onnxBackend": _enum_spec(
            "runtime.onnxBackend",
            "onnx_cpu",
            ("openvino_gpu", "openvino_cpu", "onnx_cuda", "onnx_cpu"),
        ),
        "runtime.fallbackPolicy": _enum_spec(
            "runtime.fallbackPolicy",
            "whole_model_cpu",
            ("whole_model_cpu", "fail"),
        ),
    }


def mdx_parameter_specs() -> dict[str, AudioParameterSpec]:
    return {
        "mdx.segmentPolicy": _enum_spec(
            "mdx.segmentPolicy",
            "model_shape",
            ("model_shape", "custom"),
            architecture="mdx_onnx",
        ),
        "mdx.segmentFrames": _int_spec(
            "mdx.segmentFrames",
            256,
            64,
            4096,
            advanced=True,
            architecture="mdx_onnx",
            unit="frames",
            locked=True,
        ),
        "mdx.overlapRatio": _num_spec(
            "mdx.overlapRatio", 0.25, 0.01, 0.99, architecture="mdx_onnx", unit="ratio"
        ),
        "mdx.batchSize": _int_spec(
            "mdx.batchSize", 1, 1, 16, architecture="mdx_onnx", unit="batch"
        ),
        "mdx.enableDenoisePass": _bool_spec(
            "mdx.enableDenoisePass", False, advanced=True, architecture="mdx_onnx"
        ),
        "mdx.invertUsingSpectrum": _bool_spec(
            "mdx.invertUsingSpectrum", False, advanced=True, architecture="mdx_onnx"
        ),
        "mdx.hopLength": _int_spec(
            "mdx.hopLength",
            1024,
            1,
            16384,
            architecture="mdx_onnx",
            unit="samples",
            locked=True,
        ),
    }


def vr_parameter_specs() -> dict[str, AudioParameterSpec]:
    return {
        "vr.batchSize": _int_spec(
            "vr.batchSize", 4, 1, 32, architecture="vr", unit="batch"
        ),
        "vr.windowSize": AudioParameterSpec(
            key="vr.windowSize",
            value_type="integer",
            default=AudioParameterValue(512),
            allowed_values=(
                AudioParameterValue(320),
                AudioParameterValue(512),
                AudioParameterValue(1024),
            ),
            architecture="vr",
            unit="bins",
            affects_cache=True,
        ),
        "vr.aggression": _int_spec(
            "vr.aggression", 5, -100, 100, architecture="vr", unit="percent"
        ),
        "vr.enableTta": _bool_spec("vr.enableTta", False, architecture="vr"),
        "vr.enablePostProcess": _bool_spec(
            "vr.enablePostProcess", False, architecture="vr"
        ),
        "vr.postProcessThreshold": _num_spec(
            "vr.postProcessThreshold", 0.2, 0.0, 1.0, architecture="vr"
        ),
        "vr.highEndProcess": _bool_spec(
            "vr.highEndProcess", False, architecture="vr"
        ),
    }


def demucs_parameter_specs() -> dict[str, AudioParameterSpec]:
    return {
        "demucs.segmentPolicy": _enum_spec(
            "demucs.segmentPolicy",
            "model_default",
            ("model_default", "custom"),
            architecture="demucs",
        ),
        "demucs.segmentSeconds": _num_spec(
            "demucs.segmentSeconds",
            10.0,
            1.0,
            60.0,
            architecture="demucs",
            unit="seconds",
        ),
        "demucs.shifts": _int_spec(
            "demucs.shifts", 0, 0, 20, architecture="demucs", unit="count"
        ),
        "demucs.overlapRatio": _num_spec(
            "demucs.overlapRatio",
            0.25,
            0.01,
            0.99,
            architecture="demucs",
            unit="ratio",
        ),
        "demucs.splitEnabled": _bool_spec(
            "demucs.splitEnabled", True, architecture="demucs"
        ),
    }


def mdxc_parameter_specs() -> dict[str, AudioParameterSpec]:
    return {
        "mdxc.segmentPolicy": _enum_spec(
            "mdxc.segmentPolicy",
            "model_default",
            ("model_default", "custom"),
            architecture="mdxc",
        ),
        "mdxc.segmentFrames": _int_spec(
            "mdxc.segmentFrames",
            256,
            64,
            4096,
            advanced=True,
            architecture="mdxc",
            unit="frames",
            locked=True,
        ),
        "mdxc.overlapPolicy": _enum_spec(
            "mdxc.overlapPolicy",
            "model_default",
            ("model_default", "overlap_count"),
            architecture="mdxc",
        ),
        "mdxc.overlapCount": _int_spec(
            "mdxc.overlapCount", 8, 1, 32, architecture="mdxc", unit="count"
        ),
        "mdxc.pitchShiftSemitones": _int_spec(
            "mdxc.pitchShiftSemitones",
            0,
            -12,
            12,
            advanced=True,
            architecture="mdxc",
            unit="semitones",
        ),
        "mdxc.processAllStems": _bool_spec(
            "mdxc.processAllStems", True, architecture="mdxc"
        ),
    }


def all_parameter_specs() -> dict[str, AudioParameterSpec]:
    specs: dict[str, AudioParameterSpec] = {}
    for group in (
        common_parameter_specs(),
        mdx_parameter_specs(),
        vr_parameter_specs(),
        demucs_parameter_specs(),
        mdxc_parameter_specs(),
    ):
        overlap = set(specs).intersection(group)
        if overlap:
            raise RuntimeError(f"duplicate parameter keys: {sorted(overlap)}")
        specs.update(group)
    return specs


PARAMETER_SPECS = all_parameter_specs()

SCHEMA_BY_ID = {
    "common_v1": ParameterSchema(
        "common_v1",
        {key: spec for key, spec in PARAMETER_SPECS.items() if key.startswith(("common.", "runtime."))},
    ),
    "mdx_v1": ParameterSchema(
        "mdx_v1",
        {key: spec for key, spec in PARAMETER_SPECS.items() if key.startswith(("common.", "runtime.", "mdx."))},
    ),
    "vr_v1": ParameterSchema(
        "vr_v1",
        {key: spec for key, spec in PARAMETER_SPECS.items() if key.startswith(("common.", "runtime.", "vr."))},
    ),
    "demucs_v1": ParameterSchema(
        "demucs_v1",
        {key: spec for key, spec in PARAMETER_SPECS.items() if key.startswith(("common.", "runtime.", "demucs."))},
    ),
    "mdxc_roformer_v1": ParameterSchema(
        "mdxc_roformer_v1",
        {key: spec for key, spec in PARAMETER_SPECS.items() if key.startswith(("common.", "runtime.", "mdxc."))},
    ),
}


def schema_for(schema_id: str) -> ParameterSchema:
    try:
        return SCHEMA_BY_ID[schema_id]
    except KeyError as exc:
        raise ValueError(f"unknown parameter schema: {schema_id}") from exc
