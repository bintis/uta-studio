#!/usr/bin/env python3
"""Read-only, full-output comparison of native little-endian float tensors.

The tool performs no inference and loads no accelerator library. Export complete
FP32 tensors (including decoded waveform samples) from both native executions;
compare matching shape/layout/input/precision metadata separately. A low error
is an implementation comparison, not proof of checkpoint or perceptual quality.
"""

from __future__ import annotations

import argparse
import array
import json
import math
from pathlib import Path
import sys
from typing import BinaryIO


BLOCK_ELEMENTS = 16_384


def read_block(stream: BinaryIO, count: int) -> array.array:
    data = stream.read(count * 4)
    if len(data) % 4:
        raise ValueError("tensor ends with an incomplete little-endian float")
    values = array.array("f")
    values.frombytes(data)
    if values.itemsize != 4:
        raise RuntimeError("this interpreter does not expose four-byte floats")
    if sys.byteorder != "little":
        values.byteswap()
    return values


def compare_streams(reference: BinaryIO, candidate: BinaryIO) -> dict:
    """Compare every element with bounded memory and double-precision reduction.

    Nonfinite pairs are counted and excluded from numerical reductions rather
    than causing JSON NaNs. Their indices are reported (first sixteen), and
    numerical metrics are null whenever either output contains nonfinite data.
    Equal zero tensors report NMSE zero and SNR null (unbounded), not JSON inf.
    A nonzero candidate against an all-zero reference has undefined relative
    error: absolute-error statistics remain available and NMSE/SNR are null.
    """
    elements = 0
    finite_pairs = 0
    reference_nonfinite = 0
    candidate_nonfinite = 0
    nonfinite_indices: list[int] = []
    maximum_error = 0.0
    maximum_index: int | None = None
    reference_peak = 0.0
    candidate_peak = 0.0
    reference_energy = 0.0
    candidate_energy = 0.0
    error_energy = 0.0
    absolute_error = 0.0
    signed_error = 0.0
    exact_equal = 0
    while True:
        expected = read_block(reference, BLOCK_ELEMENTS)
        actual = read_block(candidate, BLOCK_ELEMENTS)
        if len(expected) != len(actual):
            raise ValueError("tensor element counts differ")
        if not expected:
            break
        reference_squares = []
        candidate_squares = []
        error_squares = []
        differences = []
        absolute_differences = []
        for offset, (left, right) in enumerate(zip(expected, actual)):
            left_finite = math.isfinite(left)
            right_finite = math.isfinite(right)
            reference_nonfinite += not left_finite
            candidate_nonfinite += not right_finite
            if not left_finite or not right_finite:
                if len(nonfinite_indices) < 16:
                    nonfinite_indices.append(elements + offset)
                continue
            finite_pairs += 1
            difference = right - left
            magnitude = abs(difference)
            exact_equal += left == right
            if maximum_index is None or magnitude > maximum_error:
                maximum_error = magnitude
                maximum_index = elements + offset
            reference_peak = max(reference_peak, abs(left))
            candidate_peak = max(candidate_peak, abs(right))
            reference_squares.append(left * left)
            candidate_squares.append(right * right)
            error_squares.append(difference * difference)
            differences.append(difference)
            absolute_differences.append(magnitude)
        reference_energy = math.fsum((reference_energy, math.fsum(reference_squares)))
        candidate_energy = math.fsum((candidate_energy, math.fsum(candidate_squares)))
        error_energy = math.fsum((error_energy, math.fsum(error_squares)))
        signed_error = math.fsum((signed_error, math.fsum(differences)))
        absolute_error = math.fsum((absolute_error, math.fsum(absolute_differences)))
        elements += len(expected)
    if not elements:
        raise ValueError("empty tensors cannot establish output correctness")
    all_finite = reference_nonfinite == 0 and candidate_nonfinite == 0
    relative_defined = all_finite and reference_energy > 0
    both_zero = all_finite and reference_energy == 0 and candidate_energy == 0
    nmse = error_energy / reference_energy if relative_defined else (0.0 if both_zero else None)
    snr = 10 * math.log10(reference_energy / error_energy) if relative_defined and error_energy else None
    return {
        "format": "f32le",
        "elements": elements,
        "compared_elements": finite_pairs,
        "comparison_scope": "complete_output",
        "all_finite": all_finite,
        "reference_nonfinite": reference_nonfinite,
        "candidate_nonfinite": candidate_nonfinite,
        "first_nonfinite_indices": nonfinite_indices,
        "exact_equal_elements": exact_equal,
        "reference_peak": reference_peak if all_finite else None,
        "candidate_peak": candidate_peak if all_finite else None,
        "max_absolute_error": maximum_error if all_finite else None,
        "max_absolute_error_index": maximum_index if all_finite else None,
        "mean_absolute_error": absolute_error / elements if all_finite else None,
        "mean_signed_error": signed_error / elements if all_finite else None,
        "rmse": math.sqrt(error_energy / elements) if all_finite else None,
        "reference_rms": math.sqrt(reference_energy / elements) if all_finite else None,
        "candidate_rms": math.sqrt(candidate_energy / elements) if all_finite else None,
        "nmse": nmse,
        "snr_db": snr,
        "zero_reference": all_finite and reference_energy == 0,
        "zero_error": all_finite and error_energy == 0,
        "quality_claim": "none; assess model-specific discrete and perceptual outputs separately",
    }


def compare_files(reference: Path, candidate: Path) -> dict:
    if reference.resolve() == candidate.resolve():
        raise ValueError("reference and candidate must be separate files")
    if not reference.is_file() or not candidate.is_file():
        raise ValueError("both tensor inputs must be regular files")
    if reference.stat().st_size != candidate.stat().st_size:
        raise ValueError("tensor byte lengths differ")
    with reference.open("rb") as expected, candidate.open("rb") as actual:
        result = compare_streams(expected, actual)
    return dict(result, reference=str(reference), candidate=str(candidate))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--model", required=True, help="actual catalog resource identifier")
    parser.add_argument("--reference-backend", required=True)
    parser.add_argument("--candidate-backend", required=True)
    parser.add_argument("--shape", type=int, nargs="+", help="matching physical layout dimensions")
    args = parser.parse_args()
    try:
        result = compare_files(args.reference, args.candidate)
        if args.shape and (any(dimension <= 0 for dimension in args.shape) or math.prod(args.shape) != result["elements"]):
            raise ValueError("declared shape does not match complete tensor element count")
        result.update(model=args.model, reference_backend=args.reference_backend,
                      candidate_backend=args.candidate_backend, shape=args.shape)
        print(json.dumps(result, indent=2, allow_nan=False))
        return 0 if result["all_finite"] else 1
    except (OSError, ValueError, RuntimeError) as error:
        print(json.dumps({"error": str(error)}, allow_nan=False), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
