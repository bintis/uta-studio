from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import model_setup


class ModelSetupRoutingTests(unittest.TestCase):
    def run_target(
        self,
        target: str,
        *,
        engine: str = "whisper",
        backend: str = "cpu",
        separator: str = "none",
        alignment: str = "whisperx",
    ) -> list[str]:
        with tempfile.TemporaryDirectory(prefix="uta-studio-model-routing-") as folder:
            with patch.object(model_setup, "download_huggingface") as download:
                model_setup.download_selected_models(
                    Path(folder),
                    backend,
                    engine,
                    "medium",
                    separator,
                    alignment,
                    target,
                )
                return [call.args[0] for call in download.call_args_list]

    def test_whisper_target_downloads_only_the_selected_size(self) -> None:
        self.assertEqual(
            self.run_target("whisper", engine="parakeet"),
            ["Systran/faster-whisper-medium"],
        )

    def test_language_detection_target_downloads_only_whisper_tiny(self) -> None:
        self.assertEqual(
            self.run_target("language_detection", engine="parakeet"),
            ["Systran/faster-whisper-tiny"],
        )

    def test_parakeet_target_does_not_download_whisper_fallbacks(self) -> None:
        self.assertEqual(
            self.run_target("parakeet", engine="parakeet"),
            ["istupakov/parakeet-tdt-0.6b-v3-onnx"],
        )

    def test_qwen_alignment_target_is_independent(self) -> None:
        self.assertEqual(
            self.run_target("alignment", alignment="qwen"),
            ["Qwen/Qwen3-ForcedAligner-0.6B-hf"],
        )

    def test_mms_karaoke_alignment_target_is_independent(self) -> None:
        self.assertEqual(
            self.run_target("alignment", alignment="mms_karaoke"),
            ["NextFire/mms-300m-ForcedAligner-karaoke-ja-Latn"],
        )

    def test_complete_parakeet_plan_includes_primary_and_both_fallback_models(self) -> None:
        self.assertEqual(
            self.run_target("all", engine="parakeet"),
            [
                "istupakov/parakeet-tdt-0.6b-v3-onnx",
                "Systran/faster-whisper-medium",
                "Systran/faster-whisper-tiny",
            ],
        )


if __name__ == "__main__":
    unittest.main()
