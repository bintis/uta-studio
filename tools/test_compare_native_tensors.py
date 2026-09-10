#!/usr/bin/env python3
"""Isolated CPU-only fixtures for complete native-output comparison."""

import importlib.util
import io
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).with_name("compare-native-tensors.py")
SPEC = importlib.util.spec_from_file_location("compare_native_tensors", SOURCE)
COMPARISON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARISON)


def floats(values):
    return io.BytesIO(struct.pack("<" + "f" * len(values), *values))


class NativeTensorComparisonTests(unittest.TestCase):
    def test_identical_complete_output(self):
        values = [0.0, 1.0, -2.0, 3.0]
        result = COMPARISON.compare_streams(floats(values), floats(values))
        self.assertEqual(result["elements"], 4)
        self.assertEqual(result["exact_equal_elements"], 4)
        self.assertTrue(result["all_finite"])
        self.assertEqual(result["nmse"], 0.0)
        self.assertTrue(result["zero_error"])
        self.assertIsNone(result["snr_db"])
        json.dumps(result, allow_nan=False)

    def test_nonzero_error_has_full_reference_energy(self):
        result = COMPARISON.compare_streams(floats([1.0, 2.0]), floats([2.0, 2.0]))
        self.assertAlmostEqual(result["nmse"], 0.2)
        self.assertAlmostEqual(result["snr_db"], 10 * math.log10(5.0))
        self.assertAlmostEqual(result["rmse"], math.sqrt(0.5))
        self.assertAlmostEqual(result["mean_absolute_error"], 0.5)
        self.assertAlmostEqual(result["mean_signed_error"], 0.5)
        self.assertEqual(result["max_absolute_error_index"], 0)

    def test_last_element_is_not_only_sampled(self):
        expected = [1.0] * (COMPARISON.BLOCK_ELEMENTS * 2 + 3)
        actual = expected.copy()
        actual[-1] = 5.0
        result = COMPARISON.compare_streams(floats(expected), floats(actual))
        self.assertEqual(result["elements"], len(expected))
        self.assertEqual(result["max_absolute_error_index"], len(expected) - 1)
        self.assertEqual(result["max_absolute_error"], 4.0)
        self.assertAlmostEqual(result["nmse"], 16 / len(expected))

    def test_nonfinite_values_do_not_get_accepted_or_emit_json_nan(self):
        result = COMPARISON.compare_streams(
            floats([1.0, math.nan, 3.0, math.inf]),
            floats([1.0, 2.0, -math.inf, math.nan]),
        )
        self.assertFalse(result["all_finite"])
        self.assertEqual(result["reference_nonfinite"], 2)
        self.assertEqual(result["candidate_nonfinite"], 2)
        self.assertEqual(result["first_nonfinite_indices"], [1, 2, 3])
        self.assertEqual(result["compared_elements"], 1)
        self.assertIsNone(result["nmse"])
        self.assertIsNone(result["max_absolute_error"])
        json.dumps(result, allow_nan=False)

    def test_all_zero_and_undefined_relative_error_are_distinct(self):
        equal = COMPARISON.compare_streams(floats([0.0]), floats([0.0]))
        unequal = COMPARISON.compare_streams(floats([0.0]), floats([1.0]))
        self.assertEqual(equal["nmse"], 0.0)
        self.assertTrue(equal["zero_error"])
        self.assertIsNone(unequal["nmse"])
        self.assertFalse(unequal["zero_error"])
        self.assertTrue(unequal["zero_reference"])
        self.assertEqual(unequal["rmse"], 1.0)

    def test_empty_mismatched_and_truncated_inputs_fail(self):
        for expected, actual in [
            (b"", b""),
            (b"\x00" * 4, b"\x00" * 8),
            (b"\x00" * 5, b"\x00" * 5),
        ]:
            with self.subTest(lengths=(len(expected), len(actual))):
                with self.assertRaises(ValueError):
                    COMPARISON.compare_streams(io.BytesIO(expected), io.BytesIO(actual))

    def test_large_finite_float_values_reduce_without_float_overflow(self):
        result = COMPARISON.compare_streams(floats([3e38]), floats([-3e38]))
        self.assertTrue(result["all_finite"])
        self.assertAlmostEqual(result["nmse"], 4.0)
        json.dumps(result, allow_nan=False)

    def test_files_are_read_only_and_same_path_is_not_a_comparison(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-native-comparison-") as directory:
            reference = Path(directory, "reference.f32")
            candidate = Path(directory, "candidate.f32")
            reference.write_bytes(floats([1.0, 2.0]).getvalue())
            candidate.write_bytes(floats([1.0, 3.0]).getvalue())
            before = (reference.read_bytes(), candidate.read_bytes())
            result = COMPARISON.compare_files(reference, candidate)
            self.assertEqual(result["elements"], 2)
            self.assertEqual(before, (reference.read_bytes(), candidate.read_bytes()))
            with self.assertRaises(ValueError):
                COMPARISON.compare_files(reference, reference)

    def test_cli_reports_shape_error_and_nonfinite_failure(self):
        with tempfile.TemporaryDirectory(prefix="uta-studio-native-comparison-") as directory:
            reference = Path(directory, "reference.f32")
            candidate = Path(directory, "candidate.f32")
            reference.write_bytes(floats([1.0]).getvalue())
            candidate.write_bytes(floats([1.0]).getvalue())
            command = [sys.executable, str(SOURCE), str(reference), str(candidate),
                       "--model", "fixture", "--reference-backend", "reference",
                       "--candidate-backend", "candidate"]
            shape = subprocess.run(command + ["--shape", "2"], capture_output=True, text=True)
            self.assertEqual(shape.returncode, 1)
            self.assertIn("shape", json.loads(shape.stderr)["error"])
            candidate.write_bytes(floats([math.nan]).getvalue())
            nonfinite = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(nonfinite.returncode, 1)
            self.assertFalse(json.loads(nonfinite.stdout)["all_finite"])


if __name__ == "__main__":
    unittest.main()
