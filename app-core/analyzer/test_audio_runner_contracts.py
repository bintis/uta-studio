from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from audio_models.catalog import load_catalog
from audio_models.errors import OutputContractError
from audio_models.parameters import resolve_parameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.contracts import ProcessorResult, StemArtifact, deterministic_output_names
from audio_processors.outputs import map_named_outputs
from audio_processors.runners.base import fallback_backend, run_with_whole_model_fallback


class AudioRunnerContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.catalog = load_catalog()
        self.model = self.catalog.get("bs_roformer_vocals_ep317")

    def test_outputs_bind_by_metadata_not_filename(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work = Path(raw)
            vocals = work / "step_extract_vocals__vocals.wav"
            residual = work / "step_extract_vocals__instrumental.wav"
            vocals.write_bytes(b"RIFF")
            residual.write_bytes(b"RIFF")
            artifacts = map_named_outputs(
                self.model,
                {"Vocals": vocals, "Instrumental": residual},
                sample_rate=44100,
                channels=2,
            )
            self.assertEqual(artifacts["extracted_vocal"].source_stem_name, "Vocals")
            self.assertEqual(artifacts["residual_instrumental"].path, residual)
            self.assertNotIn("(Vocals)", artifacts["extracted_vocal"].path.name)

    def test_missing_required_output_fails(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work = Path(raw)
            vocals = work / "only_vocals.wav"
            vocals.write_bytes(b"RIFF")
            with self.assertRaises(OutputContractError):
                map_named_outputs(
                    self.model,
                    {"Vocals": vocals},
                    sample_rate=44100,
                    channels=2,
                )

    def test_stem_order_does_not_matter(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work = Path(raw)
            vocals = work / "a.wav"
            residual = work / "b.wav"
            vocals.write_bytes(b"1")
            residual.write_bytes(b"2")
            first = map_named_outputs(
                self.model,
                {"Instrumental": residual, "Vocals": vocals},
                sample_rate=44100,
                channels=2,
            )
            self.assertEqual(first["extracted_vocal"].path, vocals)

    def test_custom_output_names_are_deterministic(self) -> None:
        names = deterministic_output_names("extract_vocals", ("Vocals", "Instrumental"))
        self.assertEqual(names["Vocals"], "step_extract_vocals__vocals")
        self.assertEqual(names, deterministic_output_names("extract_vocals", ("Vocals", "Instrumental")))

    def test_whole_model_fallback_does_not_keep_partial_backend(self) -> None:
        calls: list[str] = []

        def execute(backend: str) -> ProcessorResult:
            calls.append(backend)
            if backend == "torch_xpu":
                raise RuntimeError("unsupported operation")
            return ProcessorResult(
                model_id=self.model.id,
                architecture=self.model.architecture,
                artifacts={},
                requested_backend="torch_xpu",
                actual_backend=backend,
                precision="fp32",
            )

        result = run_with_whole_model_fallback(
            model_spec=self.model,
            runtime_request=AudioRuntimeRequest(
                torch_backend="torch_xpu",
                onnx_backend="onnx_cpu",
                precision_policy="fp32",
            ),
            parameters=resolve_parameters(self.model),
            execute=execute,
        )
        self.assertEqual(calls, ["torch_xpu", "torch_cpu"])
        self.assertEqual(result.actual_backend, "torch_cpu")
        self.assertEqual(result.fallback_from, "torch_xpu")
        self.assertEqual(fallback_backend("torch_xpu"), "torch_cpu")

    def test_runner_does_not_touch_network_or_model_dir(self) -> None:
        opened: list[str] = []

        class _Denied:
            def __init__(self, *args, **kwargs) -> None:
                raise AssertionError("network must not be used during inference")

        with patch("urllib.request.urlopen", _Denied), patch(
            "socket.create_connection", _Denied
        ):
            names = deterministic_output_names("x", ("Vocals",))
            self.assertIn("Vocals", names)
            self.assertEqual(opened, [])


if __name__ == "__main__":
    unittest.main()
