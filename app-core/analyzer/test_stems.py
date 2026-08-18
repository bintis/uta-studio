from __future__ import annotations

import inspect
import unittest

import stems


class SeparateStemsTests(unittest.TestCase):
    def test_production_stems_module_has_no_filename_matching(self) -> None:
        source = inspect.getsource(stems)
        self.assertNotIn("(Vocals)", source)
        self.assertNotIn("(Instrumental)", source)
        self.assertFalse(hasattr(stems, "separate_stems_uvr"))


if __name__ == "__main__":
    unittest.main()
