#!/usr/bin/env python3
"""Evaluate Uta! Studio notes against human note annotations.

Reference JSON: {"notes": [{"start": <microseconds>, "end": <microseconds>,
                           "midi": <fractional MIDI>}, ...]}.
Prediction JSON: a VocalChart with its own timebase, or the same normalized
note representation. Only pitched chart notes are evaluated; cents are included.

The note matching definitions follow mir_eval.transcription.match_notes:
https://github.com/craffel/mir_eval/blob/main/mir_eval/transcription.py
Defaults: onset <= 50 ms, pitch <= 50 cents, and (for the offset score)
offset <= max(50 ms, 20% of reference duration). Time differences are rounded to
four decimals in seconds as in mir_eval. These configurable measurement
tolerances are not acceptance thresholds or application restrictions.

A maximum-cardinality bipartite matching is computed independently for the
onset/pitch and onset/pitch/offset metrics. Error distributions use the resulting
deterministic matching, which does not claim minimum total matching error.
Fragment and coverage diagnostics are separate, explicitly defined measurements;
they are not mir_eval metrics. No inference, source modification or cache update
is performed.
"""

import argparse
from collections import deque
import json
import math
from pathlib import Path
import statistics


SOURCE_URL = "https://github.com/craffel/mir_eval/blob/main/mir_eval/transcription.py"
MICROSECONDS_PER_SECOND = 1_000_000


def note(start, end, midi, identity=None):
    values = (start, end, midi)
    if not all(isinstance(value, (float, int)) and math.isfinite(value) for value in values):
        raise ValueError("note times and MIDI must be finite numbers")
    if end <= start:
        raise ValueError("note end must be later than its start")
    return {"start": start, "end": end, "midi": midi, "id": identity}


def normalized_notes(document):
    return [
        note(item["start"], item["end"], item["midi"], item.get("id"))
        for item in document["notes"]
    ]


def prediction_notes(document, track_id=None):
    if "tracks" not in document:
        if track_id is not None:
            raise ValueError("--track-id requires a VocalChart prediction")
        return normalized_notes(document), {"representation": "normalized_notes", "excluded_unpitched": 0}
    timebase = document["timebase"]
    if not isinstance(timebase, (float, int)) or not math.isfinite(timebase) or timebase <= 0:
        raise ValueError("chart timebase must be positive")
    scale = MICROSECONDS_PER_SECOND / timebase
    selected_tracks = [
        track for track in document["tracks"] if track_id is None or track["id"] == track_id
    ]
    if track_id is not None and not selected_tracks:
        raise ValueError(f"chart has no track {track_id!r}")
    notes = []
    unpitched = 0
    for track in selected_tracks:
        for phrase in track["phrases"]:
            for item in phrase["notes"]:
                pitch = item.get("pitch")
                if pitch is None or item.get("vocal_mode") not in (None, "pitched"):
                    unpitched += 1
                    continue
                notes.append(note(
                    item["start"] * scale,
                    (item["start"] + item["duration"]) * scale,
                    pitch["midi"] + pitch.get("cents", 0) / 100.0,
                    item.get("id"),
                ))
    return notes, {
        "representation": "vocal_chart",
        "chart_timebase": timebase,
        "selected_track_ids": [track["id"] for track in selected_tracks],
        "excluded_unpitched": unpitched,
    }


def rounded_time_distance(left, right):
    return round(abs(left - right) / MICROSECONDS_PER_SECOND, 4)


def compare_distance(distance, tolerance, strict):
    return distance < tolerance if strict else distance <= tolerance


def matching_graph(reference, prediction, onset_tolerance_ms, pitch_tolerance_cents,
                   offset_ratio, offset_min_tolerance_ms, strict):
    graph = []
    for expected in reference:
        matches = []
        for index, actual in enumerate(prediction):
            onset_error = rounded_time_distance(expected["start"], actual["start"])
            pitch_error = abs(expected["midi"] - actual["midi"]) * 100.0
            if not compare_distance(onset_error, onset_tolerance_ms / 1000.0, strict):
                continue
            if not compare_distance(pitch_error, pitch_tolerance_cents, strict):
                continue
            offset_error = rounded_time_distance(expected["end"], actual["end"])
            if offset_ratio is not None:
                tolerance = max(
                    offset_min_tolerance_ms / 1000.0,
                    (expected["end"] - expected["start"]) / MICROSECONDS_PER_SECOND * offset_ratio,
                )
                if not compare_distance(offset_error, tolerance, strict):
                    continue
            matches.append((onset_error, offset_error, pitch_error, index))
        # A deterministic edge order prefers nearby notes. Augmenting paths
        # still reassign earlier pairs whenever this increases cardinality.
        graph.append([index for _, _, _, index in sorted(matches)])
    return graph


def maximum_matching(graph, prediction_count):
    """Iterative augmenting paths: maximum cardinality, not greedy pairing."""
    prediction_owner = [None] * prediction_count
    reference_owner = [None] * len(graph)
    for root in range(len(graph)):
        queue = deque([root])
        visited_reference = {root}
        predecessor_prediction = {}
        free_prediction = None
        while queue and free_prediction is None:
            current = queue.popleft()
            for target in graph[current]:
                if target in predecessor_prediction:
                    continue
                predecessor_prediction[target] = current
                owner = prediction_owner[target]
                if owner is None:
                    free_prediction = target
                    break
                if owner not in visited_reference:
                    visited_reference.add(owner)
                    queue.append(owner)
        if free_prediction is None:
            continue
        target = free_prediction
        while target is not None:
            current = predecessor_prediction[target]
            previous_target = reference_owner[current]
            reference_owner[current] = target
            prediction_owner[target] = current
            target = previous_target
    return [(index, target) for index, target in enumerate(reference_owner) if target is not None]


def percentile(values, fraction):
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def error_distribution(values):
    if not values:
        return {"count": 0}
    absolute = [abs(value) for value in values]
    return {
        "count": len(values),
        "mean_signed": statistics.fmean(values),
        "median_signed": statistics.median(values),
        "minimum_signed": min(values),
        "maximum_signed": max(values),
        "mean_absolute": statistics.fmean(absolute),
        "median_absolute": statistics.median(absolute),
        "percentile_ninety_absolute": percentile(absolute, 0.9),
        "percentile_ninety_five_absolute": percentile(absolute, 0.95),
        "maximum_absolute": max(absolute),
    }


def overlap_duration(left, right):
    return max(0, min(left["end"], right["end"]) - max(left["start"], right["start"]))


def matching_metrics(reference, prediction, matching):
    matched_count = len(matching)
    precision = matched_count / len(prediction) if prediction else 0.0
    recall = matched_count / len(reference) if reference else 0.0
    matched_reference = {expected for expected, _ in matching}
    matched_prediction = {actual for _, actual in matching}
    errors = []
    overlaps = []
    for expected_index, actual_index in matching:
        expected = reference[expected_index]
        actual = prediction[actual_index]
        errors.append({
            "reference_index": expected_index,
            "prediction_index": actual_index,
            "onset_error_ms": (actual["start"] - expected["start"]) / 1000.0,
            "offset_error_ms": (actual["end"] - expected["end"]) / 1000.0,
            "pitch_error_cents": (actual["midi"] - expected["midi"]) * 100.0,
        })
        union = max(expected["end"], actual["end"]) - min(expected["start"], actual["start"])
        overlaps.append(overlap_duration(expected, actual) / union)
    return {
        "matched_notes": matched_count,
        "false_positive_notes": len(prediction) - matched_count,
        "false_negative_notes": len(reference) - matched_count,
        "precision": precision,
        "recall": recall,
        "f_measure": 2 * precision * recall / (precision + recall) if precision + recall else 0.0,
        "average_overlap_ratio": statistics.fmean(overlaps) if overlaps else 0.0,
        "onset_error_ms": error_distribution([item["onset_error_ms"] for item in errors]),
        "offset_error_ms": error_distribution([item["offset_error_ms"] for item in errors]),
        "pitch_error_cents": error_distribution([item["pitch_error_cents"] for item in errors]),
        "matches": errors,
        "unmatched_reference_indices": sorted(set(range(len(reference))) - matched_reference),
        "unmatched_prediction_indices": sorted(set(range(len(prediction))) - matched_prediction),
    }


def covered_duration(expected, estimates):
    intervals = sorted(
        (max(expected["start"], actual["start"]), min(expected["end"], actual["end"]))
        for actual in estimates
        if overlap_duration(expected, actual) > 0
    )
    total = 0
    current_end = expected["start"]
    for start, end in intervals:
        total += max(0, end - max(start, current_end))
        current_end = max(current_end, end)
    return total


def reference_diagnostics(reference, prediction, pitch_tolerance_cents, strict, onset_pairs, full_pairs):
    # Each prediction gets at most one owner, by greatest temporal overlap.
    # This prevents a note crossing a GT seam being counted as a split in both
    # neighbors. Pitch/onset errors only break equal-overlap ties.
    ownership = [[] for _ in reference]
    for actual_index, actual in enumerate(prediction):
        candidates = [
            (-overlap_duration(expected, actual),
             abs(expected["midi"] - actual["midi"]),
             abs(expected["start"] - actual["start"]), expected_index)
            for expected_index, expected in enumerate(reference)
            if overlap_duration(expected, actual) > 0
        ]
        if candidates:
            ownership[min(candidates)[-1]].append(actual_index)
    onset_matches = dict(onset_pairs)
    full_matches = dict(full_pairs)
    details = []
    for expected_index, expected in enumerate(reference):
        temporal = [
            index for index, actual in enumerate(prediction)
            if overlap_duration(expected, actual) > 0
        ]
        correct_pitch = [
            index for index in temporal
            if compare_distance(
                abs(prediction[index]["midi"] - expected["midi"]) * 100.0,
                pitch_tolerance_cents, strict,
            )
        ]
        owned_correct_pitch = sorted(
            set(ownership[expected_index]).intersection(correct_pitch),
            key=lambda index: (prediction[index]["start"], prediction[index]["end"], index),
        )
        first_start = (
            prediction[owned_correct_pitch[0]]["start"] if owned_correct_pitch else expected["start"]
        )
        extra_splits = []
        seen_starts = {first_start}
        for index in owned_correct_pitch[1:]:
            start = prediction[index]["start"]
            if max(expected["start"], first_start) < start < expected["end"] and start not in seen_starts:
                extra_splits.append(index)
            seen_starts.add(start)
        duration = expected["end"] - expected["start"]
        voiced = covered_duration(expected, [prediction[index] for index in temporal])
        correct = covered_duration(expected, [prediction[index] for index in correct_pitch])
        details.append({
            "reference_index": expected_index,
            "reference_note": expected,
            "onset_pitch_match_prediction_index": onset_matches.get(expected_index),
            "with_offset_match_prediction_index": full_matches.get(expected_index),
            "temporal_prediction_indices": temporal,
            "owned_prediction_indices": ownership[expected_index],
            "owned_matching_pitch_prediction_indices": owned_correct_pitch,
            "extra_same_pitch_segments": max(0, len(owned_correct_pitch) - 1),
            "extra_split_prediction_indices": extra_splits,
            "extra_split_count": len(extra_splits),
            "has_no_temporal_prediction": not temporal,
            "has_only_wrong_pitch_predictions": bool(temporal) and not correct_pitch,
            "uncovered_duration_microseconds": duration - voiced,
            "wrong_pitch_only_duration_microseconds": voiced - correct,
            "correct_pitch_covered_duration_microseconds": correct,
            "correct_pitch_coverage_ratio": correct / duration,
        })
    return {
        "definition": (
            "Predictions are owned by the GT note with greatest positive overlap. "
            "Extra splits are additional owned same-pitch prediction onsets strictly inside "
            "that GT note, after its first owned same-pitch onset. Duplicate segments are "
            "also counted separately. No minimum note duration is applied."
        ),
        "reference_notes_with_extra_splits": sum(item["extra_split_count"] > 0 for item in details),
        "extra_split_count": sum(item["extra_split_count"] for item in details),
        "extra_same_pitch_segments": sum(item["extra_same_pitch_segments"] for item in details),
        "reference_notes_without_temporal_prediction": sum(item["has_no_temporal_prediction"] for item in details),
        "reference_notes_with_only_wrong_pitch_predictions": sum(item["has_only_wrong_pitch_predictions"] for item in details),
        "uncovered_duration_microseconds": sum(item["uncovered_duration_microseconds"] for item in details),
        "wrong_pitch_only_duration_microseconds": sum(item["wrong_pitch_only_duration_microseconds"] for item in details),
        "per_reference_note": details,
    }


def evaluate(reference, prediction, onset_tolerance_ms=50.0, pitch_tolerance_cents=50.0,
             offset_ratio=0.2, offset_min_tolerance_ms=50.0, strict=False):
    onset_graph = matching_graph(
        reference, prediction, onset_tolerance_ms, pitch_tolerance_cents,
        None, offset_min_tolerance_ms, strict,
    )
    full_graph = matching_graph(
        reference, prediction, onset_tolerance_ms, pitch_tolerance_cents,
        offset_ratio, offset_min_tolerance_ms, strict,
    )
    onset_pairs = maximum_matching(onset_graph, len(prediction))
    full_pairs = maximum_matching(full_graph, len(prediction))
    return {
        "scope": "Offline comparison against supplied human note annotations; no acceptance gate.",
        "definition_source": SOURCE_URL,
        "matching": "deterministic maximum-cardinality bipartite matching",
        "reference_notes": len(reference),
        "prediction_notes": len(prediction),
        "parameters": {
            "onset_tolerance_ms": onset_tolerance_ms,
            "pitch_tolerance_cents": pitch_tolerance_cents,
            "offset_ratio": offset_ratio,
            "offset_min_tolerance_ms": offset_min_tolerance_ms,
            "strict": strict,
            "time_distance_decimal_places_in_seconds": 4,
        },
        "onset_pitch": matching_metrics(reference, prediction, onset_pairs),
        "onset_pitch_offset": matching_metrics(reference, prediction, full_pairs),
        "reference_diagnostics": reference_diagnostics(
            reference, prediction, pitch_tolerance_cents, strict, onset_pairs, full_pairs,
        ),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("prediction", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--track-id")
    parser.add_argument("--onset-tolerance-ms", type=float, default=50.0)
    parser.add_argument("--pitch-tolerance-cents", type=float, default=50.0)
    parser.add_argument("--offset-ratio", type=float, default=0.2)
    parser.add_argument("--offset-min-tolerance-ms", type=float, default=50.0)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()
    reference = normalized_notes(json.loads(args.reference.read_text()))
    prediction, metadata = prediction_notes(json.loads(args.prediction.read_text()), args.track_id)
    report = evaluate(
        reference, prediction, args.onset_tolerance_ms, args.pitch_tolerance_cents,
        args.offset_ratio, args.offset_min_tolerance_ms, args.strict,
    )
    report["inputs"] = {
        "reference": str(args.reference), "prediction": str(args.prediction),
        "prediction_projection": metadata,
    }
    with args.output.open("x") as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")
    print(json.dumps({
        "report": str(args.output),
        "reference_notes": len(reference),
        "prediction_notes": len(prediction),
        "onset_pitch": {key: report["onset_pitch"][key] for key in ("precision", "recall", "f_measure")},
        "onset_pitch_offset": {
            key: report["onset_pitch_offset"][key] for key in ("precision", "recall", "f_measure")
        },
        "extra_split_count": report["reference_diagnostics"]["extra_split_count"],
    }, indent=2))


if __name__ == "__main__":
    main()
