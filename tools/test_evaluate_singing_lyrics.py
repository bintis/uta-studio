#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("lyrics", Path(__file__).with_name("evaluate-singing-lyrics.py"))
lyrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lyrics)


def word(text, start, end, measured=True):
    return {"text": text, "start": start, "end": end, "measured": measured}


class LyricTimingTests(unittest.TestCase):
    def test_identity_and_exact_tolerance(self):
        reference = [word("一", 0, 100000), word("二", 100000, 200000)]
        actual = [word("一", 50000, 150000), word("二", 150001, 250001)]
        result = lyrics.evaluate(reference, actual)["timing_evaluation"]
        self.assertEqual(result["both_correct_count"], 1)
        self.assertEqual(result["start"]["accuracy_all_reference"], 0.5)

    def test_missing_words_do_not_improve_denominator(self):
        reference = [word("一", 0, 100000), word("二", 100000, 200000)]
        actual = [reference[0], word("二", 100000, 200000, False)]
        result = lyrics.evaluate(reference, actual)
        self.assertEqual(result["unresolved_prediction_words"], 1)
        self.assertEqual(result["timing_evaluation"]["both_accuracy_all_reference"], 0.5)

    def test_coarser_segmentation_does_not_invent_inner_boundaries(self):
        reference = [word("光", 0, 100000), word("る", 100000, 200000)]
        actual = [word("光る", 0, 200000)]
        result = lyrics.evaluate(reference, actual)["timing_evaluation"]
        self.assertEqual(result["both_correct_count"], 0)
        self.assertEqual(result["start"]["correct_count"], 1)
        self.assertEqual(result["end"]["correct_count"], 1)

    def test_finer_segmentation_supplies_only_observed_outer_edges(self):
        reference = [word("光る", 0, 200000)]
        actual = [word("光", 0, 100000), word("る", 100000, 200000)]
        self.assertEqual(lyrics.evaluate(reference, actual)["timing_evaluation"]["both_correct_count"], 1)

    def test_repeated_words_match_by_text_position_not_nearest_time(self):
        reference = [word("a", 0, 100000), word("a", 200000, 300000)]
        actual = [word("A!", 200000, 300000), word("a", 0, 100000)]
        result = lyrics.evaluate(reference, actual)["timing_evaluation"]
        self.assertEqual(result["both_correct_count"], 0)

    def test_normalization_and_text_mismatch_are_explicit(self):
        self.assertEqual(lyrics.lexical(" ＡＢＣ, e\u0301!"), "abcé")
        result = lyrics.evaluate([word("一二三", 0, 300000)], [word("二一三", 0, 300000)])
        self.assertFalse(result["text_matches"])
        self.assertIsNone(result["timing_evaluation"])

    def test_note_bound_time_and_unresolved_points_are_not_measurements(self):
        tokens = [{"id": "bound", "text": "一"},
                  {"id": "point", "text": "二", "timing_unresolved": True,
                   "timing": {"start": 1000, "duration": 0}}]
        chart = {"timebase": 1000, "tracks": [{"id": "lead", "phrases": [{"notes": [
            {"start": 0, "duration": 2000, "lyrics": tokens}]}]}]}
        result = lyrics.chart_words(chart)
        self.assertFalse(any(item["measured"] for item in result))
        self.assertEqual(result[1]["start"], 1000000)
        with self.assertRaises(ValueError):
            lyrics.chart_words(chart, "missing")

    def test_empty_and_invalid_reference_do_not_score_perfectly(self):
        self.assertEqual(lyrics.evaluate([], [])["timing_evaluation"]["both_accuracy_all_reference"], 0)
        with self.assertRaises(ValueError):
            lyrics.evaluate([word("x", 0, 0)], [])
        with self.assertRaises(ValueError):
            lyrics.evaluate([], [], float("nan"))


if __name__ == "__main__":
    unittest.main()
