from __future__ import annotations

import importlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

import numpy as np
import soundfile as sf

from audio_models.catalog import load_catalog
from audio_models.parameters import resolve_parameters
from audio_models.plan import AudioRuntimeRequest
from audio_processors.runners.mdxc_torch import MdxcTorchRunner
from audio_processors.xpu_worker import (
    RESULT_FILENAME,
    _dispatch,
    run_isolated_xpu,
)


class XpuWorkerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.model = load_catalog().get("bs_roformer_vocals_ep317")

    def test_parent_launches_one_worker_and_reads_file_only_result(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)
            vocals = work_dir / "vocals.wav"
            residual = work_dir / "instrumental.wav"
            vocals.write_bytes(b"RIFF")
            residual.write_bytes(b"RIFF")

            def complete_worker(command, **kwargs):
                self.assertEqual(
                    command,
                    [sys.executable, "-m", "audio_processors.xpu_worker"],
                )
                request = json.loads(kwargs["input"])
                self.assertEqual(request["runner"], "mdxc_torch")
                self.assertEqual(kwargs["env"]["UTA_STUDIO_XPU_WORKER"], "1")
                self.assertEqual(kwargs["env"]["SYCL_UR_USE_LEVEL_ZERO_V2"], "0")
                self.assertEqual(kwargs["env"]["UR_L0_USE_COPY_ENGINE"], "0")
                self.assertEqual(kwargs["env"]["UR_L0_USE_IMMEDIATE_COMMANDLISTS"], "0")
                (work_dir / RESULT_FILENAME).write_text(
                    json.dumps(
                        {
                            "stems": {
                                "Vocals": str(vocals),
                                "Instrumental": str(residual),
                            },
                            "sample_rate": 44100,
                            "channels": 2,
                        }
                    ),
                    encoding="utf-8",
                )
                return SimpleNamespace(returncode=0)

            request = {
                "runner": "mdxc_torch",
                "model_id": self.model.id,
                "work_dir": str(work_dir),
            }
            with patch(
                "audio_processors.xpu_segmented.intel_battlemage_present",
                return_value=True,
            ), patch(
                "audio_processors.xpu_worker.subprocess.run",
                side_effect=complete_worker,
            ) as run:
                result = run_isolated_xpu(request)

            self.assertEqual(Path(result["stems"]["Vocals"]), vocals)
            self.assertEqual(run.call_count, 1)
            self.assertFalse((work_dir / RESULT_FILENAME).exists())

    def test_worker_failure_is_propagated_without_retrying_in_process(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)

            def fail_worker(*_args, **_kwargs):
                (work_dir / RESULT_FILENAME).write_text(
                    json.dumps({"error": "Level Zero worker failed"}),
                    encoding="utf-8",
                )
                return SimpleNamespace(returncode=1)

            with patch(
                "audio_processors.xpu_worker.subprocess.run",
                side_effect=fail_worker,
            ):
                with self.assertRaisesRegex(RuntimeError, "Level Zero worker failed"):
                    run_isolated_xpu(
                        {
                            "runner": "mdxc_torch",
                            "model_id": self.model.id,
                            "work_dir": str(work_dir),
                        }
                    )

    def test_real_worker_process_reports_protocol_errors_without_loading_xpu(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)
            with self.assertRaisesRegex(RuntimeError, "does not match model"):
                run_isolated_xpu(
                    {
                        "runner": "invalid_test_runner",
                        "model_id": self.model.id,
                        "work_dir": str(work_dir),
                        "parameters": {},
                    }
                )
            self.assertFalse((work_dir / RESULT_FILENAME).exists())

    def test_worker_dispatch_keeps_mdxc_xpu_execution_process_local(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)
            output = work_dir / "vocals.wav"
            parameters = resolve_parameters(
                self.model,
                device_capabilities={"allow_reduced_precision": True},
            )
            request = {
                "runner": "mdxc_torch",
                "model_id": self.model.id,
                "checkpoint": str(work_dir / "model.ckpt"),
                "config_path": str(work_dir / "config.yaml"),
                "input_path": str(work_dir / "source.wav"),
                "work_dir": str(work_dir),
                "parameters": parameters.as_map(),
                "precision_policy": "bf16",
                "descriptor_names": {"Vocals": "step_test__vocals"},
            }
            with patch(
                "audio_processors.runners.mdxc_torch._separate_offline",
                return_value={"Vocals": output},
            ) as separate:
                result = _dispatch(request)

            self.assertEqual(result["stems"]["Vocals"], str(output))
            kwargs = separate.call_args.kwargs
            self.assertEqual(kwargs["backend"], "torch_xpu")
            self.assertTrue(kwargs["process_isolated"])
            self.assertEqual(kwargs["precision_policy"], "bf16")
            self.assertTrue(kwargs["require_all_outputs"])

    def test_segment_worker_allows_a_semantically_silent_stem(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)
            output = work_dir / "vocals.wav"
            request = {
                "runner": "mdxc_torch",
                "model_id": self.model.id,
                "checkpoint": str(work_dir / "model.ckpt"),
                "config_path": str(work_dir / "config.yaml"),
                "input_path": str(work_dir / "source.wav"),
                "work_dir": str(work_dir),
                "parameters": resolve_parameters(self.model).as_map(),
                "precision_policy": "bf16",
                "descriptor_names": {"Vocals": "step_test__vocals"},
                "allow_missing_stems": True,
            }
            with patch(
                "audio_processors.runners.mdxc_torch._separate_offline",
                return_value={"Vocals": output},
            ) as separate:
                result = _dispatch(request)

            self.assertEqual(result["stems"], {"Vocals": str(output)})
            self.assertFalse(separate.call_args.kwargs["require_all_outputs"])

    def test_worker_dispatch_keeps_demucs_xpu_execution_process_local(self) -> None:
        model = load_catalog().get("htdemucs_6s")
        with tempfile.TemporaryDirectory() as raw:
            work_dir = Path(raw)
            output = work_dir / "vocals.wav"
            request = {
                "runner": "demucs_torch",
                "model_id": model.id,
                "yaml_path": str(work_dir / "config.yaml"),
                "weight_path": str(work_dir / "model.th"),
                "input_path": str(work_dir / "source.wav"),
                "work_dir": str(work_dir),
                "parameters": resolve_parameters(model).as_map(),
            }
            with patch(
                "audio_processors.runners.demucs_torch._separate_demucs",
                return_value=({"vocals": output}, 48000, 2),
            ) as separate:
                result = _dispatch(request)

            self.assertEqual(result["stems"]["vocals"], str(output))
            kwargs = separate.call_args.kwargs
            self.assertEqual(kwargs["backend"], "torch_xpu")
            self.assertTrue(kwargs["process_isolated"])

    def test_mdxc_runner_keeps_xpu_enabled_but_never_runs_it_in_parent(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            work_dir = root / "work"
            source = root / "source.wav"
            sf.write(
                str(source),
                np.zeros((4410, 2), dtype=np.float32),
                44100,
                format="WAV",
                subtype="FLOAT",
            )
            checkpoint = root / "model.ckpt"
            config_path = root / "config.yaml"
            local_inference = MagicMock(side_effect=AssertionError("parent XPU inference"))

            def isolated_worker(request):
                attempt_dir = Path(request["work_dir"])
                self.assertEqual(attempt_dir, work_dir / "torch_xpu")
                stems = {}
                for stem, token in request["descriptor_names"].items():
                    path = attempt_dir / f"{token}.wav"
                    path.write_bytes(b"RIFF")
                    stems[stem] = str(path)
                return {"stems": stems, "sample_rate": 44100, "channels": 2}

            def installed_file(_root, _model, file_spec):
                return checkpoint if file_spec.role == "checkpoint" else config_path

            with patch(
                "audio_processors.runners.mdxc_torch.resolve_installed_file",
                side_effect=installed_file,
            ), patch(
                "audio_processors.xpu_worker.run_isolated_xpu",
                side_effect=isolated_worker,
            ) as worker, patch(
                "audio_processors.runners.mdxc_torch._separate_offline",
                local_inference,
            ):
                result = MdxcTorchRunner().run(
                    model_spec=self.model,
                    input_path=source,
                    work_dir=work_dir,
                    parameters=resolve_parameters(
                        self.model,
                        run_overrides={"runtime.precisionPolicy": "bf16"},
                        device_capabilities={"allow_reduced_precision": True},
                    ),
                    runtime_request=AudioRuntimeRequest(
                        torch_backend="torch_xpu",
                        onnx_backend="onnx_cpu",
                        precision_policy="bf16",
                    ),
                    installed_dir=root / "audio-processing" / self.model.id,
                    step_id="extract_vocals",
                )

            self.assertEqual(result.actual_backend, "torch_xpu")
            self.assertEqual(result.precision, "bf16")
            self.assertEqual(worker.call_count, 1)
            local_inference.assert_not_called()

    def test_cpu_fallback_uses_a_different_attempt_directory(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            work_dir = root / "work"
            source = root / "source.wav"
            sf.write(
                str(source),
                np.zeros((4410, 2), dtype=np.float32),
                44100,
                format="WAV",
                subtype="FLOAT",
            )

            def installed_file(_root, _model, file_spec):
                return root / file_spec.install_filename

            def cpu_inference(**kwargs):
                self.assertEqual(kwargs["backend"], "torch_cpu")
                self.assertEqual(kwargs["work_dir"], work_dir / "torch_cpu")
                named = {}
                for stem, token in kwargs["descriptor_names"].items():
                    path = kwargs["work_dir"] / f"{token}.wav"
                    path.write_bytes(b"RIFF")
                    named[stem] = path
                return named

            with patch(
                "audio_processors.runners.mdxc_torch.resolve_installed_file",
                side_effect=installed_file,
            ), patch(
                "audio_processors.xpu_worker.run_isolated_xpu",
                side_effect=RuntimeError("isolated XPU failed"),
            ) as worker, patch(
                "audio_processors.runners.mdxc_torch._separate_offline",
                side_effect=cpu_inference,
            ) as local_inference:
                result = MdxcTorchRunner().run(
                    model_spec=self.model,
                    input_path=source,
                    work_dir=work_dir,
                    parameters=resolve_parameters(self.model),
                    runtime_request=AudioRuntimeRequest(
                        torch_backend="torch_xpu",
                        onnx_backend="onnx_cpu",
                        precision_policy="fp32",
                    ),
                    installed_dir=root / "audio-processing" / self.model.id,
                )

            self.assertEqual(result.actual_backend, "torch_cpu")
            self.assertEqual(result.fallback_from, "torch_xpu")
            self.assertEqual(
                Path(worker.call_args.args[0]["work_dir"]),
                work_dir / "torch_xpu",
            )
            self.assertEqual(local_inference.call_count, 1)

    def test_parent_gpu_cleanup_does_not_initialize_xpu(self) -> None:
        fake_cuda = SimpleNamespace(is_available=MagicMock(return_value=False))
        fake_xpu = SimpleNamespace(
            is_available=MagicMock(return_value=True),
            is_initialized=MagicMock(return_value=False),
            synchronize=MagicMock(),
            empty_cache=MagicMock(),
        )
        fake_torch = SimpleNamespace(cuda=fake_cuda, xpu=fake_xpu)
        with patch.dict("sys.modules", {"torch": fake_torch}):
            sys.modules.pop("gpu", None)
            gpu = importlib.import_module("gpu")
            try:
                gpu.hard_free_gpu("parent-after-isolated-worker")
            finally:
                sys.modules.pop("gpu", None)

        fake_xpu.is_available.assert_not_called()
        fake_xpu.synchronize.assert_not_called()
        fake_xpu.empty_cache.assert_not_called()


if __name__ == "__main__":
    unittest.main()
