#!/usr/bin/env python3
"""Measure lyric boundaries independently of notes, by transcript position.

Reference: {"words": [{"text": ..., "start": microseconds, "end": microseconds}]}.
Prediction: current VocalChart. NFKC/casefold and alphanumeric character positions
match text despite whitespace/punctuation and word-segmentation differences.
Only explicitly measured positive lyric intervals supply boundary observations.
A model word covering several reference words supplies no invented internal times.
Missing/zero-length/unresolved boundaries remain in the reference denominators.
No inference, reference changes, nearest-time matching or timestamp interpolation.
"""
import argparse
import json
import math
from pathlib import Path
import statistics
import unicodedata


def lexical(text):
    return "".join(character for character in unicodedata.normalize("NFKC", text).casefold()
                   if character.isalnum())


def chart_words(chart, track_id="lead"):
    tracks = [track for track in chart["tracks"] if track["id"] == track_id]
    if len(tracks) != 1:
        raise ValueError("select exactly one existing lyric track")
    timebase = chart["timebase"]
    if not isinstance(timebase, (int, float)) or not math.isfinite(timebase) or timebase <= 0:
        raise ValueError("chart timebase must be finite and positive")
    scale = 1_000_000 / timebase
    words = []
    for phrase in tracks[0]["phrases"]:
        for note in phrase["notes"]:
            for token in note["lyrics"]:
                if "text" not in token or not lexical(token["text"]):
                    continue
                timing = token.get("timing")
                # Note timing is not independent evidence of a lyric boundary.
                measured = timing is not None and not token.get("timing_unresolved", False)
                start = timing["start"] * scale if timing is not None else None
                end = start + timing["duration"] * scale if timing is not None else None
                if timing is not None and (not math.isfinite(start) or not math.isfinite(end) or end < start):
                    raise ValueError("invalid lyric placement")
                words.append({"id": token["id"], "text": token["text"], "start": start,
                              "end": end, "measured": measured and end > start})
    return words


def positioned(words, reference=False):
    cursor = 0
    result = []
    for word in words:
        text = lexical(word["text"])
        if not text:
            continue
        if reference and (not all(isinstance(word[edge], (int, float)) and math.isfinite(word[edge])
                                  for edge in ("start", "end")) or word["end"] <= word["start"]):
            raise ValueError("reference word must have a finite positive interval")
        result.append(dict(word, character_start=cursor, character_end=cursor + len(text)))
        cursor += len(text)
    return result


def edge_statistics(rows, edge, tolerance_ms):
    errors = [row[edge + "_error_ms"] for row in rows if row[edge + "_error_ms"] is not None]
    matched = sum(abs(value) <= tolerance_ms for value in errors)
    return {"reference_count": len(rows), "observed_count": len(errors), "correct_count": matched,
            "accuracy_all_reference": matched / len(rows) if rows else 0.0,
            "mean_absolute_error_ms_observed": statistics.mean(map(abs, errors)) if errors else None,
            "median_absolute_error_ms_observed": statistics.median(map(abs, errors)) if errors else None}


def evaluate(reference, prediction, tolerance_ms=50.0):
    if not math.isfinite(tolerance_ms) or tolerance_ms < 0:
        raise ValueError("tolerance must be finite and nonnegative")
    expected = positioned(reference, reference=True)
    actual = positioned(prediction)
    expected_text = "".join(lexical(word["text"]) for word in expected)
    actual_text = "".join(lexical(word["text"]) for word in actual)
    result = {"scope": "Independent reference lyric timing; missing boundaries are not correct.",
              "normalization": "NFKC, casefold, alphanumeric transcript positions; no time-based matching",
              "tolerance_ms": tolerance_ms, "text_matches": expected_text == actual_text,
              "reference_words": len(expected), "prediction_words": len(actual),
              "unresolved_prediction_words": sum(not word.get("measured", False) for word in actual)}
    if expected_text != actual_text:
        result["timing_evaluation"] = None
        result["reason"] = "Transcript differs; positional boundary evaluation is undefined. No favorable text rematching."
        return result
    starts = {word["character_start"]: word["start"] for word in actual if word.get("measured", False)}
    ends = {word["character_end"]: word["end"] for word in actual if word.get("measured", False)}
    rows = []
    for word in expected:
        start, end = starts.get(word["character_start"]), ends.get(word["character_end"])
        rows.append({"text": word["text"], "reference_start": word["start"], "reference_end": word["end"],
                     "predicted_start": start, "predicted_end": end,
                     "start_error_ms": (start - word["start"]) / 1000 if start is not None else None,
                     "end_error_ms": (end - word["end"]) / 1000 if end is not None else None})
    both = sum(all(row[edge + "_error_ms"] is not None and abs(row[edge + "_error_ms"]) <= tolerance_ms
                   for edge in ("start", "end")) for row in rows)
    result["timing_evaluation"] = {"start": edge_statistics(rows, "start", tolerance_ms),
                                   "end": edge_statistics(rows, "end", tolerance_ms),
                                   "both_correct_count": both,
                                   "both_accuracy_all_reference": both / len(rows) if rows else 0.0,
                                   "per_reference_word": rows}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("prediction", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--track-id", default="lead")
    parser.add_argument("--tolerance-ms", type=float, default=50.0)
    args = parser.parse_args()
    reference = json.loads(args.reference.read_text())["words"]
    prediction = chart_words(json.loads(args.prediction.read_text()), args.track_id)
    report = evaluate(reference, prediction, args.tolerance_ms)
    report["inputs"] = {"reference": str(args.reference), "prediction": str(args.prediction), "track_id": args.track_id}
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")
    print(json.dumps({key: value for key, value in report.items() if key != "timing_evaluation"}, ensure_ascii=False))


if __name__ == "__main__":
    main()
