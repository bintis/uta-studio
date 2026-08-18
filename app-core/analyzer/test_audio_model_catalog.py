from __future__ import annotations

import unittest
from pathlib import Path

from audio_models.catalog import DEFAULT_LEGACY_KARAOKE_MODEL_ID, REQUIRED_MODEL_IDS, load_catalog
from audio_models.schema import ALLOWED_ARCHITECTURES, ALLOWED_BACKENDS, is_sha256


class AudioModelCatalogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.catalog = load_catalog()

    def test_required_models_exist_and_are_unique(self) -> None:
        ids = self.catalog.ids()
        self.assertEqual(len(ids), len(set(ids)))
        for model_id in REQUIRED_MODEL_IDS:
            self.assertIn(model_id, ids)

    def test_architectures_and_backends_are_explicit(self) -> None:
        for model in self.catalog.models:
            self.assertIn(model.architecture, ALLOWED_ARCHITECTURES)
            self.assertFalse("roformer" in Path(model.files[0].filename).name.lower() and model.architecture == "")
            for backend in model.supported_backends:
                self.assertIn(backend, ALLOWED_BACKENDS)

    def test_checksums_are_full_sha256_without_placeholders(self) -> None:
        for model in self.catalog.models:
            for item in model.files:
                self.assertTrue(is_sha256(item.sha256), item.filename)
                self.assertNotIn(item.sha256.upper(), {"TODO", "UNKNOWN"})
            if model.id == "uvr_mdxnet_karaoke_2":
                self.assertIsNotNone(model.metadata_sha256)
                self.assertTrue(is_sha256(model.metadata_sha256 or ""))
                self.assertEqual(
                    model.file("checkpoint").uvr_metadata_hash,
                    "1d64a6d2c30f709b8c9b4ce1366d96ee",
                )

    def test_output_roles_are_unique_and_inputs_are_legal(self) -> None:
        for model in self.catalog.models:
            roles = list(model.output_contract.values())
            self.assertEqual(len(roles), len(set(roles)), model.id)
            self.assertTrue(model.accepted_roles)

    def test_parameter_schema_exists(self) -> None:
        from audio_models.schema import SCHEMA_BY_ID

        for model in self.catalog.models:
            self.assertIn(model.parameter_schema_id, SCHEMA_BY_ID)

    def test_demucs_yaml_and_weight_are_paired(self) -> None:
        model = self.catalog.get("htdemucs_6s")
        self.assertEqual(model.file("model_config").filename, "htdemucs_6s.yaml")
        self.assertEqual(model.file("checkpoint").filename, "5c90dfd2-34c22ccb.th")

    def test_roformer_pairs_use_catalog_architecture_not_filename(self) -> None:
        vocals = self.catalog.get("bs_roformer_vocals_ep317")
        inst = self.catalog.get("melband_roformer_inst_v2")
        self.assertEqual(vocals.architecture, "mdxc_bs_roformer")
        self.assertEqual(inst.architecture, "mdxc_melband_roformer")
        self.assertEqual(vocals.target_stem, "Vocals")
        self.assertEqual(inst.target_stem, "Instrumental")

    def test_default_karaoke_is_catalogued(self) -> None:
        karaoke = self.catalog.get(DEFAULT_LEGACY_KARAOKE_MODEL_ID)
        self.assertEqual(karaoke.architecture, "mdxc_melband_roformer")
        self.assertEqual(karaoke.output_contract["Vocals"], "extracted_vocal")
        self.assertEqual(karaoke.output_contract["Instrumental"], "residual_instrumental")


if __name__ == "__main__":
    unittest.main()
