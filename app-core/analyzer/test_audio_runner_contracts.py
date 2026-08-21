from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from audio_models.catalog import load_catalog
from audio_models.errors import OutputContractError
from audio_models.parameters import resolve_parameters
from audio_models.plan import (
    AudioInputReference,
    AudioProcessingPlanSnapshot,
    AudioProcessingStep,
    AudioRuntimeRequest,
)
from audio_processors.contracts import ProcessorResult, StemArtifact, deterministic_output_names
from audio_processors.executor import (
    _copy_intermediate,
    _selected_progress_artifacts,
    execute_audio_processing_plan,
)
from audio_processors.outputs import map_named_outputs
from audio_processors.runners.base import fallback_backend, run_with_whole_model_fallback
from audio_processors.runners.mdxc_torch import (
    _actual_precision,
    _separate_offline,
    _separate_on_xpu,
)


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

    def test_executor_merges_runner_progress_identity(self) -> None:
        class ProgressRunner:
            def run(inner_self, **kwargs):
                kwargs["progress_sink"](
                    8,
                    "loading",
                    model_id=kwargs["model_spec"].id,
                    architecture=kwargs["model_spec"].architecture,
                )
                path = kwargs["work_dir"] / "vocals.wav"
                path.write_bytes(b"RIFF")
                return ProcessorResult(
                    model_id=kwargs["model_spec"].id,
                    architecture=kwargs["model_spec"].architecture,
                    artifacts={
                        "extracted_vocal": StemArtifact(
                            role="extracted_vocal",
                            source_stem_name="Vocals",
                            path=path,
                            sample_rate=44100,
                            channels=2,
                        )
                    },
                    requested_backend="torch_xpu",
                    actual_backend="torch_xpu",
                    precision="bf16",
                )

        plan = AudioProcessingPlanSnapshot(
            schema_version=1,
            catalog_version=self.catalog.catalog_version,
            steps=(
                AudioProcessingStep(
                    step_id="extract_vocals",
                    model_id=self.model.id,
                    input=AudioInputReference.source_media(),
                    selected_output_roles=("extracted_vocal",),
                    effective_parameters={},
                ),
            ),
            output_bindings=(),
            requested_runtime=AudioRuntimeRequest(
                torch_backend="torch_xpu",
                onnx_backend="onnx_cpu",
                precision_policy="bf16",
                fallback_policy="fail",
            ),
        )
        events = []
        with tempfile.TemporaryDirectory() as raw, patch.dict(
            "audio_processors.executor.RUNNERS",
            {self.model.runner: ProgressRunner()},
        ), patch.dict(
            "sys.modules",
            {"gpu": SimpleNamespace(hard_free_gpu=MagicMock())},
        ):
            execute_audio_processing_plan(
                plan,
                source_path=Path(raw) / "source.flac",
                work_root=Path(raw) / "work",
                models_dir=Path(raw) / "models",
                progress_sink=lambda percent, message, **metadata: events.append(
                    (percent, message, metadata)
                ),
                catalog=self.catalog,
            )

        loading = next(event for event in events if event[1] == "loading")
        self.assertEqual(loading[2]["step_id"], "extract_vocals")
        self.assertEqual(loading[2]["model_id"], self.model.id)

    def test_intermediate_copy_is_atomic_and_cleans_temporary_file(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source.wav"
            destination = root / "persisted.wav"
            source.write_bytes(b"new audio bytes")
            destination.write_bytes(b"previous complete bytes")

            copied = _copy_intermediate(source, destination)

            self.assertEqual(copied, destination)
            self.assertEqual(destination.read_bytes(), b"new audio bytes")
            self.assertEqual(list(root.glob(f".{destination.name}.*.tmp")), [])

    def test_intermediate_copy_keeps_previous_file_if_publish_fails(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source.wav"
            destination = root / "persisted.wav"
            source.write_bytes(b"new audio bytes")
            destination.write_bytes(b"previous complete bytes")

            with patch(
                "audio_processors.executor.os.replace",
                side_effect=OSError("rename failed"),
            ), self.assertRaisesRegex(OSError, "rename failed"):
                _copy_intermediate(source, destination)

            self.assertEqual(destination.read_bytes(), b"previous complete bytes")
            self.assertEqual(list(root.glob(f".{destination.name}.*.tmp")), [])

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

    def test_empty_output_fails(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work = Path(raw)
            vocals = work / "empty.wav"
            residual = work / "residual.wav"
            vocals.touch()
            residual.write_bytes(b"RIFF")
            with self.assertRaisesRegex(OutputContractError, "is empty"):
                map_named_outputs(
                    self.model,
                    {"Vocals": vocals, "Instrumental": residual},
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

    def test_vocal_progress_does_not_commit_the_models_residual_bgm(self) -> None:
        artifacts = {
            "extracted_vocal": StemArtifact(
                role="extracted_vocal",
                source_stem_name="Vocals",
                path=Path("vocal.wav"),
                sample_rate=44100,
                channels=2,
            ),
            "residual_instrumental": StemArtifact(
                role="residual_instrumental",
                source_stem_name="Instrumental",
                path=Path("residual.wav"),
                sample_rate=44100,
                channels=2,
            ),
        }
        committed = _selected_progress_artifacts(artifacts, ("extracted_vocal",))
        self.assertEqual(
            committed,
            [{"role": "extracted_vocal", "path": "vocal.wav"}],
        )

    def test_custom_output_names_are_deterministic(self) -> None:
        names = deterministic_output_names("extract_vocals", ("Vocals", "Instrumental"))
        self.assertEqual(names["Vocals"], "step_extract_vocals__vocals")
        self.assertEqual(names, deterministic_output_names("extract_vocals", ("Vocals", "Instrumental")))

    def test_xpu_precision_policy_is_applied_honestly(self) -> None:
        self.assertEqual(_actual_precision("torch_xpu", "bf16"), "bf16")
        self.assertEqual(_actual_precision("torch_xpu", "auto"), "bf16")
        self.assertEqual(_actual_precision("torch_xpu", "fp32"), "fp32")
        self.assertEqual(_actual_precision("torch_cpu", "bf16"), "fp32")

    def test_xpu_bf16_wraps_the_complete_separator_in_autocast(self) -> None:
        calls: list[tuple[str, object]] = []
        original_sdpa = object()
        original_stft = object()
        original_istft = object()
        original_view_as_complex = MagicMock(return_value="complex-mask")
        bf16 = object()
        float32 = object()
        mask = SimpleNamespace(
            device=SimpleNamespace(type="xpu"),
            dtype=bf16,
            to=MagicMock(return_value="fp32-mask"),
        )

        class Autocast:
            def __enter__(self):
                calls.append(("enter", bf16))

            def __exit__(self, *_args):
                calls.append(("exit", bf16))

        fake_torch = SimpleNamespace(
            bfloat16=bf16,
            float16=object(),
            nn=SimpleNamespace(
                functional=SimpleNamespace(
                    scaled_dot_product_attention=original_sdpa
                )
            ),
            stft=original_stft,
            istft=original_istft,
            view_as_complex=original_view_as_complex,
            float32=float32,
            autocast=lambda *, device_type, dtype: (
                calls.append((device_type, dtype)) or Autocast()
            ),
        )
        separator = MagicMock()
        separator.separate.side_effect = lambda *_args, **_kwargs: (
            fake_torch.view_as_complex(mask),
            ["vocals.wav"],
        )[1]

        with patch.dict("sys.modules", {"torch": fake_torch}):
            result = _separate_on_xpu(
                separator,
                Path("source.wav"),
                {"Vocals": "step_test__vocals"},
                precision_policy="bf16",
            )

        self.assertEqual(result, ["vocals.wav"])
        self.assertEqual(calls, [("xpu", bf16), ("enter", bf16), ("exit", bf16)])
        self.assertIs(
            fake_torch.nn.functional.scaled_dot_product_attention,
            original_sdpa,
        )
        self.assertIs(fake_torch.stft, original_stft)
        self.assertIs(fake_torch.istft, original_istft)
        self.assertIs(fake_torch.view_as_complex, original_view_as_complex)
        mask.to.assert_called_once_with(float32)
        original_view_as_complex.assert_called_once_with("fp32-mask")

    def test_mdxc_releases_model_at_the_step_boundary_on_failure(self) -> None:
        separator = MagicMock()
        separator.separate.side_effect = RuntimeError("model failed")
        move_to_cpu = MagicMock()
        hard_free_gpu = MagicMock()
        fake_gpu = SimpleNamespace(
            move_to_cpu=move_to_cpu,
            hard_free_gpu=hard_free_gpu,
        )

        with tempfile.TemporaryDirectory() as raw, patch(
            "audio_separator_adapter.OfflineSeparator", return_value=separator
        ), patch("audio_separator_adapter.apply_torch_device"), patch.dict(
            "sys.modules", {"gpu": fake_gpu}
        ), patch(
            "audio_processors.runners.mdxc_torch._separate_on_xpu",
            side_effect=RuntimeError("model failed"),
        ):
            root = Path(raw)
            with self.assertRaisesRegex(RuntimeError, "model failed"):
                _separate_offline(
                    model_spec=self.model,
                    checkpoint=root / "model.ckpt",
                    config_path=root / "config.yaml",
                    input_path=root / "source.wav",
                    work_dir=root / "outputs",
                    parameters=resolve_parameters(self.model),
                    backend="torch_xpu",
                    precision_policy="bf16",
                    descriptor_names={
                        "Vocals": "step_test__vocals",
                        "Instrumental": "step_test__instrumental",
                    },
                )

        move_to_cpu.assert_called_once_with(separator)
        hard_free_gpu.assert_called_once_with(
            f"audio-model:{self.model.id}:torch_xpu"
        )

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
