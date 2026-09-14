#!/usr/bin/env python3
"""Synthetic human-note evaluation cases, without models or user data."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).with_name("evaluate-singing-notes.py")
SPEC = importlib.util.spec_from_file_location("evaluate_singing_notes", SOURCE)
EVALUATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATION)


def note(start, end, midi=60):
    return EVALUATION.note(start, end, midi)


class SingingNoteEvaluationTests(unittest.TestCase):
    def test_perfect_notes_include_real_short_notes(self):
        notes = [note(0, 30_000, 60), note(30_000, 70_000, 62), note(100_000, 700_000, 64)]
        result = EVALUATION.evaluate(notes, notes)
        for key in ("onset_pitch", "onset_pitch_offset"):
            self.assertEqual(result[key]["precision"], 1.0)
            self.assertEqual(result[key]["recall"], 1.0)
            self.assertEqual(result[key]["f_measure"], 1.0)
            self.assertEqual(result[key]["onset_error_ms"]["maximum_absolute"], 0.0)
        self.assertEqual(result["reference_diagnostics"]["extra_split_count"], 0)
        self.assertEqual(result["reference_diagnostics"]["uncovered_duration_microseconds"], 0)

    def test_one_reference_split_into_three_is_penalized_despite_full_pitch_coverage(self):
        reference = [note(0, 1_000_000)]
        prediction = [note(0, 300_000), note(300_000, 600_000), note(600_000, 1_000_000)]
        result = EVALUATION.evaluate(reference, prediction)
        self.assertEqual(result["onset_pitch"]["matched_notes"], 1)
        self.assertAlmostEqual(result["onset_pitch"]["precision"], 1.0 / 3.0)
        self.assertEqual(result["onset_pitch"]["recall"], 1.0)
        self.assertEqual(result["onset_pitch"]["f_measure"], 0.5)
        self.assertEqual(result["onset_pitch_offset"]["matched_notes"], 0)
        detail = result["reference_diagnostics"]
        self.assertEqual(detail["extra_split_count"], 2)
        self.assertEqual(detail["per_reference_note"][0]["correct_pitch_coverage_ratio"], 1.0)

    def test_wrong_offset_is_distinct_from_onset_and_pitch_error(self):
        result = EVALUATION.evaluate([note(0, 1_000_000)], [note(0, 500_000)])
        self.assertEqual(result["onset_pitch"]["f_measure"], 1.0)
        self.assertEqual(result["onset_pitch_offset"]["f_measure"], 0.0)
        self.assertEqual(result["onset_pitch"]["offset_error_ms"]["mean_signed"], -500.0)
        self.assertEqual(result["reference_diagnostics"]["uncovered_duration_microseconds"], 500_000)

    def test_wrong_pitch_is_reported_separately_from_missing_sound(self):
        result = EVALUATION.evaluate([note(0, 1_000_000)], [note(0, 1_000_000, 61)])
        self.assertEqual(result["onset_pitch"]["matched_notes"], 0)
        detail = result["reference_diagnostics"]
        self.assertEqual(detail["reference_notes_without_temporal_prediction"], 0)
        self.assertEqual(detail["reference_notes_with_only_wrong_pitch_predictions"], 1)
        self.assertEqual(detail["wrong_pitch_only_duration_microseconds"], 1_000_000)

    def test_augmenting_path_recovers_the_match_a_greedy_choice_would_lose(self):
        matching = EVALUATION.maximum_matching([[0, 1], [0]], 2)
        self.assertEqual(matching, [(0, 1), (1, 0)])

    def test_overlap_ratio_follows_official_expression_for_short_disjoint_matches(self):
        result = EVALUATION.evaluate([note(0, 10_000)], [note(20_000, 30_000)])
        self.assertEqual(result["onset_pitch_offset"]["matched_notes"], 1)
        self.assertAlmostEqual(result["onset_pitch_offset"]["average_overlap_ratio"], -1.0 / 3.0)

    def test_tolerance_boundaries_are_inclusive_and_configurable(self):
        reference = [note(0, 1_000_000)]
        prediction = [note(50_000, 1_200_000, 60.5)]
        inclusive = EVALUATION.evaluate(reference, prediction)
        self.assertEqual(inclusive["onset_pitch_offset"]["matched_notes"], 1)
        strict = EVALUATION.evaluate(reference, prediction, strict=True)
        self.assertEqual(strict["onset_pitch"]["matched_notes"], 0)
        tighter = EVALUATION.evaluate(reference, prediction, onset_tolerance_ms=40.0)
        self.assertEqual(tighter["onset_pitch"]["matched_notes"], 0)

    def test_empty_inputs_have_zero_metrics_and_explicit_missing_notes(self):
        empty = EVALUATION.evaluate([], [])
        self.assertEqual(empty["onset_pitch"]["f_measure"], 0.0)
        missing = EVALUATION.evaluate([note(0, 100_000)], [])
        self.assertEqual(missing["onset_pitch"]["false_negative_notes"], 1)
        self.assertEqual(missing["reference_diagnostics"]["reference_notes_without_temporal_prediction"], 1)
        json.dumps(missing, allow_nan=False)

    def test_chart_timebase_cents_and_track_selection_are_projected(self):
        chart = {
            "timebase": 48_000,
            "tracks": [
                {"id": "lead", "phrases": [{"notes": [
                    {"id": "pitched", "start": 4800, "duration": 9600,
                     "pitch": {"midi": 60, "cents": 50}, "vocal_mode": "pitched"},
                    {"id": "spoken", "start": 0, "duration": 4800, "pitch": None,
                     "vocal_mode": "freestyle"},
                ]}]},
                {"id": "harmony", "phrases": [{"notes": [
                    {"id": "harmony-note", "start": 4800, "duration": 9600,
                     "pitch": {"midi": 64, "cents": 0}, "vocal_mode": "pitched"},
                ]}]},
            ],
        }
        notes, metadata = EVALUATION.prediction_notes(chart, "lead")
        self.assertEqual(notes, [note(100_000, 300_000, 60.5) | {"id": "pitched"}])
        self.assertEqual(metadata["excluded_unpitched"], 1)
        self.assertEqual(metadata["selected_track_ids"], ["lead"])

    def test_duplicate_onset_is_extra_segment_without_inventing_an_internal_split(self):
        result = EVALUATION.evaluate(
            [note(0, 1_000_000)], [note(0, 1_000_000), note(0, 1_000_000)]
        )
        self.assertEqual(result["reference_diagnostics"]["extra_same_pitch_segments"], 1)
        self.assertEqual(result["reference_diagnostics"]["extra_split_count"], 0)
        self.assertEqual(result["onset_pitch"]["false_positive_notes"], 1)

    def test_duplicate_fragment_does_not_count_the_same_internal_cut_twice(self):
        result = EVALUATION.evaluate(
            [note(0, 1_000_000)],
            [note(0, 300_000), note(300_000, 1_000_000), note(300_000, 1_000_000)],
        )
        self.assertEqual(result["reference_diagnostics"]["extra_same_pitch_segments"], 2)
        self.assertEqual(result["reference_diagnostics"]["extra_split_count"], 1)

    def test_cli_writes_reviewable_report_in_isolated_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reference = root / "reference.json"
            prediction = root / "prediction.json"
            report = root / "report.json"
            document = {"notes": [{"start": 0, "end": 100_000, "midi": 60.0}]}
            reference.write_text(json.dumps(document))
            prediction.write_text(json.dumps(document))
            completed = subprocess.run(
                [sys.executable, str(SOURCE), str(reference), str(prediction), str(report)],
                check=True, capture_output=True, text=True,
            )
            self.assertEqual(json.loads(completed.stdout)["onset_pitch"]["f_measure"], 1.0)
            self.assertEqual(json.loads(report.read_text())["onset_pitch_offset"]["matched_notes"], 1)
            self.assertEqual(json.loads(reference.read_text()), document)


if __name__ == "__main__":
    unittest.main()
