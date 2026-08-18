"""CPU model-load smoke: hashes and opens installed catalog files. No inference."""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from audio_models.catalog import DEFAULT_LEGACY_KARAOKE_MODEL_ID, REQUIRED_MODEL_IDS, load_catalog
from audio_models.install import model_install_status


def _models_dir() -> Path:
    override = os.environ.get("UTA_STUDIO_MODELS_DIR")
    if override:
        return Path(override)
    return Path.home() / "Documents" / "uta-studio" / "models"


class AudioModelLoadSmokeTests(unittest.TestCase):
    def test_installed_catalog_files_hash(self) -> None:
        catalog = load_catalog()
        models_dir = _models_dir()
        checked = 0
        for model_id in (*REQUIRED_MODEL_IDS, DEFAULT_LEGACY_KARAOKE_MODEL_ID):
            status = model_install_status(models_dir, catalog.get(model_id))
            if status["state"] != "installed":
                continue
            checked += 1
            for item in status["files"]:
                self.assertTrue(item["present"], item)
                self.assertTrue(item["integrity"], item)
        self.assertGreaterEqual(checked, 3, "expected at least the already-installed catalog models")


if __name__ == "__main__":
    unittest.main()
