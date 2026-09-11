#!/usr/bin/env python3
"""CPU-only tests for offline linguistic-data export; requires pypinyin."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("stars_lexicon", Path(__file__).with_name("export-stars-lexicon.py"))
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)


class LexiconTests(unittest.TestCase):
    def test_contracted_finals_all_tones(self):
        allowed = {"ch", "uei", "q", "iou", "uen", "x", "ve", "l", "v"}
        for reading in ["chuī", "chuí", "chuǐ", "chuì", "chui"]:
            self.assertEqual(exporter.syllable_phones(reading, allowed), ["ch", "uei"])
        for reading, phones in [("qiū", ["q", "iou"]), ("chūn", ["ch", "uen"]),
                                ("xué", ["x", "ve"]), ("lǜ", ["l", "v"])]:
            self.assertEqual(exporter.syllable_phones(reading, allowed), phones)

    def test_source_phrase_readings_and_checkpoint_phone_order(self):
        phone_set = ["uei", "ch", "zh", "ong", "q", "ing"]
        asset, report = exporter.export_lexicon(
            phone_set, {ord("吹"): "chuī", ord("重"): "zhòng,chóng"},
            {"重庆": [["chóng"], ["qìng"]]},
        )
        self.assertEqual(asset["phone_set"], phone_set)
        self.assertEqual(asset["characters"]["重"], ["zh", "ong"])
        self.assertEqual(asset["phrases"]["重庆"], [["ch", "ong"], ["q", "ing"]])
        self.assertEqual(report["exported_characters"], 2)

    def test_unrepresentable_source_is_reported_without_substitution(self):
        asset, report = exporter.export_lexicon(["ch", "uei"], {ord("学"): "xué"}, {})
        self.assertEqual(asset["characters"], {})
        self.assertEqual(report["unsupported_characters"], {"学": "xué"})


if __name__ == "__main__":
    unittest.main()
