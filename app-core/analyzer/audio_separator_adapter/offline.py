"""Load audio-separator architectures from verified local files only."""

from __future__ import annotations

import importlib
import logging
import os
from pathlib import Path
from typing import Any, Mapping

from audio_models.errors import ModelConfigurationError

_ARCHITECTURE_ALIASES = {
    "MDX": "MDX",
    "MDXC": "MDXC",
    "mdx_onnx": "MDX",
    "mdxc_bs_roformer": "MDXC",
    "mdxc_melband_roformer": "MDXC",
    "mdxc": "MDXC",
}

_SEPARATOR_CLASSES = {
    "MDX": ("mdx_separator", "MDXSeparator"),
    "MDXC": ("mdxc_separator", "MDXCSeparator"),
}

_ROFORMER_ARCHITECTURES = {"mdxc_bs_roformer", "mdxc_melband_roformer"}


def apply_torch_device(separator: Any, backend: str) -> None:
    """Force the separator onto the requested device. Never auto-picks CUDA/MPS."""
    import torch

    if backend == "torch_cuda":
        device = torch.device("cuda")
    elif backend == "torch_xpu":
        device = torch.device("xpu")
    else:
        device = torch.device("cpu")
    cpu = torch.device("cpu")
    separator.torch_device = device
    separator.torch_device_cpu = cpu
    separator.torch_device_mps = None
    separator.torch_device_override = device
    if backend in {"openvino_gpu", "openvino_cpu", "onnx_cuda", "onnx_cpu"}:
        return
    if hasattr(separator, "model_instance") and separator.model_instance is not None:
        instance = separator.model_instance
        if hasattr(instance, "torch_device"):
            instance.torch_device = device
        if hasattr(instance, "model") and hasattr(instance.model, "to"):
            instance.model.to(device)


def _load_yaml(path: Path) -> dict[str, Any]:
    import yaml

    with path.open(encoding="utf-8") as handle:
        payload = yaml.load(handle, Loader=yaml.FullLoader)
    if not isinstance(payload, dict):
        raise ModelConfigurationError(f"model config {path} is not a mapping")
    return payload


def _architecture_family(architecture: str) -> str:
    family = _ARCHITECTURE_ALIASES.get(architecture)
    if family is None:
        raise ModelConfigurationError(f"unsupported separator architecture {architecture!r}")
    return family


def load_model_from_spec(
    separator: Any,
    *,
    model_path: str | Path,
    architecture: str,
    model_data: Mapping[str, Any] | None = None,
    config_path: str | Path | None = None,
) -> None:
    """Instantiate an architecture class from local files. No downloads."""
    checkpoint = Path(model_path)
    if not checkpoint.is_file():
        raise ModelConfigurationError(f"model file is not installed: {checkpoint}")
    if any(
        hasattr(separator, name) and getattr(separator, name) is not None
        for name in ("download_model_files",)
    ):
        # Never call the upstream download entry points from this path.
        pass

    family = _architecture_family(architecture)
    loaded_data: dict[str, Any]
    if config_path is not None:
        loaded_data = _load_yaml(Path(config_path))
        if model_data:
            loaded_data = {**loaded_data, **dict(model_data)}
    elif model_data is not None:
        loaded_data = dict(model_data)
    else:
        raise ModelConfigurationError(
            "load_model_from_spec requires local model_data or config_path"
        )
    if architecture in _ROFORMER_ARCHITECTURES:
        loaded_data["is_roformer"] = True

    model_name = checkpoint.stem
    separator.model_filename = checkpoint.name
    separator.model_filenames = [checkpoint.name]
    separator.model_friendly_name = model_name
    if separator.torch_device is None:
        apply_torch_device(separator, "torch_cpu")

    common_params = {
        "logger": separator.logger,
        "log_level": separator.log_level,
        "torch_device": separator.torch_device,
        "torch_device_cpu": separator.torch_device_cpu,
        "torch_device_mps": getattr(separator, "torch_device_mps", None),
        "onnx_execution_provider": separator.onnx_execution_provider
        or ["CPUExecutionProvider"],
        "model_name": model_name,
        "model_path": str(checkpoint),
        "model_data": loaded_data,
        "output_format": separator.output_format,
        "output_bitrate": separator.output_bitrate,
        "output_dir": separator.output_dir,
        "normalization_threshold": separator.normalization_threshold,
        "amplification_threshold": separator.amplification_threshold,
        "output_single_stem": separator.output_single_stem,
        "invert_using_spec": separator.invert_using_spec,
        "sample_rate": separator.sample_rate,
        "use_soundfile": separator.use_soundfile,
    }
    module_name, class_name = _SEPARATOR_CLASSES[family]
    module = importlib.import_module(
        f"audio_separator.separator.architectures.{module_name}"
    )
    separator_class = getattr(module, class_name)
    arch_params = separator.arch_specific_params[family]
    separator.model_instance = separator_class(
        common_config=common_params,
        arch_config=arch_params,
    )


class OfflineSeparator:
    """audio-separator facade that refuses implicit downloads and implicit devices."""

    def __init__(
        self,
        *,
        model_file_dir: str,
        output_dir: str,
        normalization_threshold: float = 0.9,
        output_format: str = "WAV",
        mdx_params: dict[str, Any] | None = None,
        mdxc_params: dict[str, Any] | None = None,
        torch_backend: str = "torch_cpu",
        log_level: int = logging.WARNING,
    ) -> None:
        from audio_separator.separator import Separator

        os.makedirs(output_dir, exist_ok=True)
        os.makedirs(model_file_dir, exist_ok=True)
        self._separator = Separator(
            log_level=log_level,
            model_file_dir=model_file_dir,
            output_dir=output_dir,
            output_format=output_format,
            normalization_threshold=normalization_threshold,
            use_soundfile=True,
            info_only=True,
            mdx_params=mdx_params
            or {
                "hop_length": 1024,
                "segment_size": 256,
                "overlap": 0.25,
                "batch_size": 1,
                "enable_denoise": False,
            },
            mdxc_params=mdxc_params
            or {
                "segment_size": 256,
                "override_model_segment_size": False,
                "batch_size": 1,
                "overlap": 8,
                "pitch_shift": 0,
            },
        )
        apply_torch_device(self._separator, torch_backend)
        self.torch_device_override = self._separator.torch_device

    def load_model_from_spec(
        self,
        *,
        model_path: str | Path,
        architecture: str,
        model_data: Mapping[str, Any] | None = None,
        config_path: str | Path | None = None,
    ) -> None:
        load_model_from_spec(
            self._separator,
            model_path=model_path,
            architecture=architecture,
            model_data=model_data,
            config_path=config_path,
        )

    def load_model(self, *_args: object, **_kwargs: object) -> None:
        raise ModelConfigurationError(
            "offline adapter refuses load_model(); use load_model_from_spec"
        )

    def separate(
        self,
        audio_file_path: str,
        custom_output_names: Mapping[str, str] | None = None,
    ) -> list[str]:
        return self._separator.separate(
            audio_file_path,
            custom_output_names=dict(custom_output_names) if custom_output_names else None,
        )

    def __getattr__(self, name: str) -> Any:
        return getattr(self._separator, name)
