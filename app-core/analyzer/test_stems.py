from __future__ import annotations

import os
import sys
import tempfile
import types
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch

try:
    import stems  # type: ignore[import]
except Exception as exc:  # pragma: no cover - environment-specific dependency issue
    stems = None
    stems_import_error = exc
else:
    stems_import_error = None


class _FakeSeparator:
    def __init__(self, model_file_dir: str, output_dir: str):
        self.model_file_dir = model_file_dir
        self.output_dir = output_dir
        self.loaded_model = None
        self.input_path = None

    def load_model(self, model_file: str) -> None:
        self.loaded_model = model_file

    def separate(self, audio_path: str) -> list[str]:
        self.input_path = audio_path
        vocals_file = "song(Vocals).wav"
        instrumental_file = "song(Instrumental).wav"
        (Path(self.output_dir) / vocals_file).touch()
        (Path(self.output_dir) / instrumental_file).touch()
        return [vocals_file, instrumental_file]


@unittest.skipUnless(stems is not None, f"stems import failed: {stems_import_error}")
class SeparateStemsTests(unittest.TestCase):
    def setUp(self) -> None:
        self._last_separator = None

        fake_separator_module = types.ModuleType("audio_separator.separator")

        def make_separator(*args, **kwargs) -> _FakeSeparator:
            separator = _FakeSeparator(*args, **kwargs)
            self._last_separator = separator
            return separator

        fake_separator_module.Separator = make_separator
        fake_audio_separator_module = types.ModuleType("audio_separator")
        fake_audio_separator_module.separator = fake_separator_module

        self._fake_modules = {
            "audio_separator": fake_audio_separator_module,
            "audio_separator.separator": fake_separator_module,
        }

    def _run_with_fake_uvr_modules(self):
        return patch.dict(sys.modules, self._fake_modules)

    @contextmanager
    def _fake_gpu_ctx(self, _name: str):
        yield []

    def test_uvr_converts_input_to_wav_before_separate(self) -> None:
        with tempfile.TemporaryDirectory(prefix="uta-studio-uvr-stems-") as work_dir:
            input_audio = os.path.join(work_dir, "input.opus")
            wav_path = os.path.join(work_dir, "input.wav")
            Path(input_audio).write_bytes(b"\x00")

            with self._run_with_fake_uvr_modules():
                with patch.object(
                    stems,
                    "_ensure_wav",
                    return_value=wav_path,
                ) as ensure_wav_mock:
                    with patch.object(stems, "gpu_model", self._fake_gpu_ctx):
                        vocal_path, instrumental_path = stems.separate_stems_uvr(
                            input_audio,
                            work_dir,
                            "/tmp/models",
                        )

            ensure_wav_mock.assert_called_once_with(input_audio, work_dir)
            assert self._last_separator is not None
            self.assertEqual(self._last_separator.input_path, wav_path)
            self.assertEqual(self._last_separator.loaded_model, stems.KARAOKE_MODEL)

            self.assertEqual(vocal_path, os.path.join(work_dir, "song(Vocals).wav"))
            self.assertEqual(instrumental_path, os.path.join(work_dir, "song(Instrumental).wav"))


if __name__ == "__main__":
    unittest.main()
