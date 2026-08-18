"""Hardware smokes for catalog runners. Uses already-installed user models."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

import numpy as np
import soundfile as sf

from audio_models.catalog import DEFAULT_LEGACY_KARAOKE_MODEL_ID, load_catalog
from audio_models.install import model_install_status
from audio_models.parameters import resolve_parameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.runners.base import run_with_whole_model_fallback
from audio_processors.runners.demucs_torch import DemucsTorchRunner
from audio_processors.runners.mdx_onnx import MdxOnnxRunner
from audio_processors.runners.mdxc_torch import MdxcTorchRunner


def _models_dir() -> Path:
    return Path(os.environ.get("UTA_STUDIO_MODELS_DIR", Path.home() / "Documents" / "uta-studio" / "models"))


def _write_fixture(path: Path, seconds: float = 1.5) -> None:
    sample_rate = 44100
    frames = int(sample_rate * seconds)
    t = np.linspace(0, seconds, frames, endpoint=False, dtype=np.float32)
    left = 0.08 * np.sin(2 * np.pi * 220 * t)
    right = 0.08 * np.sin(2 * np.pi * 330 * t)
    sf.write(str(path), np.stack([left, right], axis=1), sample_rate, subtype="FLOAT")


class AudioHardwareSmokeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = load_catalog()
        cls.models_dir = _models_dir()

    def _require_installed(self, model_id: str):
        model = self.catalog.get(model_id)
        status = model_install_status(self.models_dir, model)
        if status["state"] != "installed":
            self.skipTest(f"{model_id} is not installed")
        return model

    def _runtime(self, torch_backend: str, onnx_backend: str) -> AudioRuntimeRequest:
        return AudioRuntimeRequest(
            torch_backend=torch_backend,
            onnx_backend=onnx_backend,
            precision_policy="fp32",
        )

    def test_htdemucs_runs_on_xpu(self) -> None:
        import torch

        if not getattr(torch, "xpu", None) or not torch.xpu.is_available():
            self.skipTest("torch.xpu is not available")
        model = self._require_installed("htdemucs_6s")
        runner = DemucsTorchRunner()
        with tempfile.TemporaryDirectory(prefix="uta-hw-demucs-") as raw:
            work = Path(raw)
            source = work / "in.wav"
            _write_fixture(source, 1.0)
            result = runner.run(
                model_spec=model,
                input_path=source,
                work_dir=work / "out",
                parameters=resolve_parameters(model),
                runtime_request=self._runtime("torch_xpu", "onnx_cpu"),
                installed_dir=self.models_dir / "audio-processing" / model.id,
            )
        self.assertEqual(result.actual_backend, "torch_xpu")
        self.assertEqual(set(result.artifacts), set(model.output_contract.values()))

    def test_default_karaoke_runs_on_xpu(self) -> None:
        import torch

        if not getattr(torch, "xpu", None) or not torch.xpu.is_available():
            self.skipTest("torch.xpu is not available")
        model = self._require_installed(DEFAULT_LEGACY_KARAOKE_MODEL_ID)
        runner = MdxcTorchRunner()
        with tempfile.TemporaryDirectory(prefix="uta-hw-karaoke-") as raw:
            work = Path(raw)
            source = work / "in.wav"
            _write_fixture(source, 12.0)
            result = runner.run(
                model_spec=model,
                input_path=source,
                work_dir=work / "out",
                parameters=resolve_parameters(model),
                runtime_request=self._runtime("torch_xpu", "onnx_cpu"),
                installed_dir=self.models_dir / "audio-processing" / model.id,
                step_id="extract_vocals",
            )
        self.assertEqual(result.actual_backend, "torch_xpu")
        self.assertIn("extracted_vocal", result.artifacts)
        self.assertIn("residual_instrumental", result.artifacts)

    def test_karaoke2_openvino_helper(self) -> None:
        model = self._require_installed("uvr_mdxnet_karaoke_2")
        runner = MdxOnnxRunner()
        with tempfile.TemporaryDirectory(prefix="uta-hw-kara2-") as raw:
            work = Path(raw)
            source = work / "in.wav"
            _write_fixture(source, 1.0)
            result = runner.run(
                model_spec=model,
                input_path=source,
                work_dir=work / "out",
                parameters=resolve_parameters(model),
                runtime_request=self._runtime("torch_cpu", "openvino_gpu"),
                installed_dir=self.models_dir / "audio-processing" / model.id,
                step_id="extract_karaoke",
            )
        self.assertIn(result.actual_backend, {"openvino_gpu", "openvino_cpu"})
        self.assertIn("karaoke_instrumental", result.artifacts)

    def test_whole_model_fallback_records_reason(self) -> None:
        calls: list[str] = []

        def execute(backend: str):
            calls.append(backend)
            if backend == "torch_xpu":
                raise RuntimeError("forced xpu failure")
            from audio_processors.contracts import ProcessorResult

            return ProcessorResult(
                model_id="probe",
                architecture="demucs",
                artifacts={},
                requested_backend="torch_xpu",
                actual_backend=backend,
                precision="fp32",
            )

        model = self.catalog.get("htdemucs_6s")
        result = run_with_whole_model_fallback(
            model_spec=model,
            runtime_request=self._runtime("torch_xpu", "onnx_cpu"),
            parameters=resolve_parameters(model),
            execute=execute,
        )
        self.assertEqual(calls, ["torch_xpu", "torch_cpu"])
        self.assertEqual(result.actual_backend, "torch_cpu")
        self.assertEqual(result.fallback_from, "torch_xpu")
        self.assertIn("forced xpu failure", result.fallback_reason or "")


if __name__ == "__main__":
    unittest.main()
