from __future__ import annotations

import unittest
from unittest.mock import patch

from audio_models.yaml_util import RestrictedYamlError, load_restricted_yaml


class RestrictedYamlFallbackTests(unittest.TestCase):
    def load_without_pyyaml(self, text: str):
        with patch.dict("sys.modules", {"yaml": None}):
            return load_restricted_yaml(text)

    def test_indentless_sequences_and_tuple_tag_match_shipped_model_shape(self) -> None:
        parsed = self.load_without_pyyaml(
            """
model:
  freqs: !!python/tuple
  - 2
  - 4
training:
  instruments:
  - Vocals
  - Instrumental
  target_instrument: Vocals
"""
        )
        self.assertEqual(parsed["model"]["freqs"], (2, 4))
        self.assertEqual(
            parsed["training"]["instruments"], ["Vocals", "Instrumental"]
        )
        self.assertEqual(parsed["training"]["target_instrument"], "Vocals")

    def test_inline_comments_do_not_change_numeric_or_quoted_values(self) -> None:
        parsed = self.load_without_pyyaml(
            'dim_t: 801 # model frames\nlabel: "keep # inside" # outside\n'
        )
        self.assertEqual(parsed, {"dim_t": 801, "label": "keep # inside"})

    def test_tuple_tag_rejects_a_non_sequence_value(self) -> None:
        with self.assertRaises(RestrictedYamlError):
            self.load_without_pyyaml("freqs: !!python/tuple\n")


if __name__ == "__main__":
    unittest.main()
