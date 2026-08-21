from __future__ import annotations

import importlib
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from audio_models.catalog import DEFAULT_LEGACY_KARAOKE_MODEL_ID, load_catalog
from audio_models.plan import legacy_plan_from_separator
from audio_separator_adapter import OfflineSeparator, load_model_from_spec


class OfflineAdapterTests(unittest.TestCase):
    def test_load_model_from_spec_does_not_download(self) -> None:
        separator = MagicMock()
        separator.logger = MagicMock()
        separator.log_level = 20
        separator.torch_device = "cpu"
        separator.torch_device_cpu = "cpu"
        separator.torch_device_mps = None
        separator.onnx_execution_provider = ["CPUExecutionProvider"]
        separator.output_format = "WAV"
        separator.output_bitrate = None
        separator.output_dir = "/tmp"
        separator.normalization_threshold = 0.9
        separator.amplification_threshold = 0.0
        separator.output_single_stem = None
        separator.invert_using_spec = False
        separator.sample_rate = 44100
        separator.use_soundfile = True
        separator.arch_specific_params = {"MDXC": {"overlap": 8}}
        separator.download_model_files = MagicMock(side_effect=AssertionError("download"))
        separator.download_file_if_not_exists = MagicMock(side_effect=AssertionError("download"))

        fake_module = MagicMock()
        fake_class = MagicMock()
        fake_module.MDXCSeparator = fake_class
        with tempfile.TemporaryDirectory() as raw:
            checkpoint = Path(raw) / "model.ckpt"
            checkpoint.write_bytes(b"ckpt")
            with patch("audio_separator_adapter.offline.importlib.import_module", return_value=fake_module):
                load_model_from_spec(
                    separator,
                    model_path=checkpoint,
                    architecture="mdxc_melband_roformer",
                    model_data={"training": {"instruments": ["Vocals", "Instrumental"]}},
                )
        fake_class.assert_called_once()
        separator.download_model_files.assert_not_called()
        separator.download_file_if_not_exists.assert_not_called()
        self.assertTrue(separator.model_instance)

    def test_offline_separator_refuses_filename_load(self) -> None:
        try:
            import audio_separator.separator  # noqa: F401
        except ImportError:
            self.skipTest("audio-separator is not installed in this Python")
        with patch("audio_separator.separator.Separator") as mocked:
            mocked.return_value.arch_specific_params = {"MDXC": {}, "MDX": {}}
            mocked.return_value.torch_device = None
            adapter = OfflineSeparator(model_file_dir="/tmp", output_dir="/tmp")
            with self.assertRaises(Exception):
                adapter.load_model("anything.ckpt")

    def test_separate_clears_backend_cache_after_success_and_failure(self) -> None:
        fake_gpu = SimpleNamespace(hard_free_gpu=MagicMock())
        adapter = OfflineSeparator.__new__(OfflineSeparator)
        adapter._separator = MagicMock()
        adapter._separator.separate.return_value = ["vocals.wav"]

        with patch.dict("sys.modules", {"gpu": fake_gpu}):
            self.assertEqual(adapter.separate("song.wav"), ["vocals.wav"])
            adapter._separator.separate.side_effect = RuntimeError("inference failed")
            with self.assertRaisesRegex(RuntimeError, "inference failed"):
                adapter.separate("song.wav")

        self.assertEqual(fake_gpu.hard_free_gpu.call_count, 2)
        fake_gpu.hard_free_gpu.assert_called_with("audio-separator-inference")

    def test_process_isolated_separator_leaves_cleanup_to_process_exit(self) -> None:
        fake_gpu = SimpleNamespace(hard_free_gpu=MagicMock())
        adapter = OfflineSeparator.__new__(OfflineSeparator)
        adapter._separator = MagicMock()
        adapter._separator.separate.return_value = ["vocals.wav"]
        adapter._process_isolated = True

        with patch.dict("sys.modules", {"gpu": fake_gpu}):
            self.assertEqual(adapter.separate("song.wav"), ["vocals.wav"])

        fake_gpu.hard_free_gpu.assert_not_called()

    def test_gpu_eviction_reaches_audio_separator_model_run(self) -> None:
        moved_to: list[str] = []

        class Model:
            def to(self, device: str) -> None:
                moved_to.append(device)

        with patch.dict("sys.modules", {"torch": SimpleNamespace()}):
            sys.modules.pop("gpu", None)
            gpu = importlib.import_module("gpu")
            try:
                wrapper = SimpleNamespace(
                    _separator=SimpleNamespace(
                        model_instance=SimpleNamespace(model_run=Model())
                    )
                )
                gpu.move_to_cpu(wrapper)
            finally:
                sys.modules.pop("gpu", None)

        self.assertEqual(moved_to, ["cpu"])

    def test_legacy_karaoke_plan_is_nonempty(self) -> None:
        catalog = load_catalog()
        plan = legacy_plan_from_separator("karaoke", catalog=catalog)
        self.assertTrue(plan.steps)
        self.assertEqual(plan.steps[0].model_id, DEFAULT_LEGACY_KARAOKE_MODEL_ID)
        roles = {binding.artifact_role for binding in plan.output_bindings}
        self.assertIn("vocals", roles)
        self.assertIn("instrumental", roles)


if __name__ == "__main__":
    unittest.main()
