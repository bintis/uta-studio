from __future__ import annotations

import unittest

from audio_models.catalog import load_catalog
from audio_models.errors import ParameterValidationError
from audio_models.parameters import resolve_parameters
from audio_processors.executor import signature_hash


class AudioParameterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.catalog = load_catalog()
        self.model = self.catalog.get("bs_roformer_vocals_ep317")

    def test_global_song_run_precedence(self) -> None:
        resolved = resolve_parameters(
            self.model,
            global_overrides={"mdxc.overlapCount": 2},
            song_overrides={"mdxc.overlapCount": 4},
            run_overrides={"mdxc.overlapCount": 6},
        )
        item = resolved.values["mdxc.overlapCount"]
        self.assertEqual(item.value.as_json(), 6)
        self.assertEqual(item.source, "run_override")

    def test_model_locked_override_is_rejected(self) -> None:
        with self.assertRaises(ParameterValidationError):
            resolve_parameters(self.model, run_overrides={"mdxc.segmentFrames": 512})

    def test_foreign_architecture_parameter_is_rejected(self) -> None:
        with self.assertRaises(ParameterValidationError):
            resolve_parameters(self.model, run_overrides={"demucs.shifts": 2})

    def test_overlap_count_and_ratio_cannot_mix(self) -> None:
        with self.assertRaises(ParameterValidationError):
            resolve_parameters(
                self.model,
                song_overrides={"mdxc.overlapCount": 4, "mdx.overlapRatio": 0.25},
            )

    def test_clamp_is_recorded_in_effective_parameters(self) -> None:
        resolved = resolve_parameters(
            self.model,
            run_overrides={"mdxc.overlapCount": 99},
        )
        item = resolved.values["mdxc.overlapCount"]
        self.assertEqual(item.value.as_json(), 32)
        self.assertEqual(item.source, "runtime_clamp")
        self.assertTrue(item.clamped)

    def test_canonical_json_is_stable_and_order_independent(self) -> None:
        a = resolve_parameters(
            self.model,
            global_overrides={"mdxc.overlapCount": 4, "common.normalizationThreshold": 0.8},
        )
        b = resolve_parameters(
            self.model,
            global_overrides={"common.normalizationThreshold": 0.8, "mdxc.overlapCount": 4},
        )
        self.assertEqual(a.canonical_json(), b.canonical_json())
        self.assertEqual(signature_hash(a.as_map()), signature_hash(b.as_map()))

    def test_value_change_changes_hash(self) -> None:
        a = resolve_parameters(self.model, run_overrides={"mdxc.overlapCount": 4})
        b = resolve_parameters(self.model, run_overrides={"mdxc.overlapCount": 8})
        self.assertNotEqual(signature_hash(a.as_map()), signature_hash(b.as_map()))


if __name__ == "__main__":
    unittest.main()
