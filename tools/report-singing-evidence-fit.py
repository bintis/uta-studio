#!/usr/bin/env python3
"""Measure singing replay fit to shared retained evidence; no acceptance gate.

Example:
  python3 tools/report-singing-evidence-fit.py --output NEW_REPORT.json REPLAY_DIR ...
The first replay supplies reference evidence unless --reference is given.
No audio, model, GPU, or application process is launched.
"""

import argparse
import bisect
import json
import math
import statistics
from pathlib import Path

CANONICAL_TIMEBASE = 1_000_000
ASSOCIATION_SECONDS = 0.06
FOCUS_SECONDS = [105.43, 162.10, 204.01, 260.40, 227.18]


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load(root):
    manifest = root / "input-manifest.json"
    return {
        "root": root,
        "canonical": read(root / "canonical.json"),
        "chart": read(root / "vocal-chart.json"),
        "fusion": read(root / "fusion.json"),
        "manifest": read(manifest) if manifest.is_file() else None,
    }


def distribution(values):
    values = sorted(values)
    if not values:
        return {"count": 0, "mean": None, "p50": None, "p90": None,
                "p95": None, "p99": None, "max": None}

    def percentile(fraction):
        position = (len(values) - 1) * fraction
        lower = math.floor(position)
        upper = math.ceil(position)
        return values[lower] + (values[upper] - values[lower]) * (position - lower)

    return {
        "count": len(values),
        "mean": statistics.fmean(values),
        "p50": percentile(0.5),
        "p90": percentile(0.9),
        "p95": percentile(0.95),
        "p99": percentile(0.99),
        "max": values[-1],
    }


def trusted_curve(canonical):
    points = []
    for point in canonical["f0_curve"]:
        frequency = point["hz"]
        confidence = point.get("confidence")
        if (math.isfinite(frequency) and frequency > 0
                and (confidence is None or math.isfinite(confidence) and confidence >= 0.5)):
            points.append((point["time"] / CANONICAL_TIMEBASE,
                           6900 + 1200 * math.log2(frequency / 440)))
    return points


def track_notes(track, timebase):
    notes = []
    for phrase in track["phrases"]:
        for note in phrase["notes"]:
            notes.append({
                **note,
                "phrase_id": phrase["id"],
                "time": note["start"] / timebase,
                "end": (note["start"] + note["duration"]) / timebase,
            })
    return sorted(notes, key=lambda note: (note["time"], note["end"], note["id"]))


def note_summary(note):
    if note is None:
        return None
    return {key: note.get(key) for key in ["id", "time", "end", "pitch"]}


def nearest(entries, time):
    if not entries:
        return None
    times = [entry["time"] for entry in entries]
    index = bisect.bisect_left(times, time)
    choices = entries[max(0, index - 1):index + 1]
    result = min(choices, key=lambda entry: abs(entry["time"] - time))
    return {**result, "signed_offset_ms": round((result["time"] - time) * 1000, 6)}


def boundaries(notes):
    pitched = [note for note in notes if note.get("pitch") is not None]
    starts = []
    changes = []
    for index, note in enumerate(pitched):
        before = pitched[index - 1] if index else None
        entry = {
            "time": note["time"],
            "before": note_summary(before),
            "after": note_summary(note),
            "gap_seconds": note["time"] - before["end"] if before else None,
        }
        starts.append(entry)
        if before and before["pitch"]["midi"] != note["pitch"]["midi"]:
            changes.append(entry)
    return starts, changes


def covering_note(notes, time):
    index = bisect.bisect_right([note["time"] for note in notes], time) - 1
    if index >= 0 and notes[index]["time"] <= time < notes[index]["end"]:
        return notes[index]
    return None


def frame_fit(notes, curve):
    starts = [note["time"] for note in notes]
    errors = []
    midi_errors = []
    covered = 0
    for time, cents in curve:
        index = bisect.bisect_right(starts, time) - 1
        if index < 0 or time >= notes[index]["end"]:
            continue
        note = notes[index]
        if note.get("pitch") is None:
            continue
        covered += 1
        midi_target = note["pitch"]["midi"] * 100
        errors.append(abs(cents - midi_target - note["pitch"].get("cents", 0)))
        midi_errors.append(abs(cents - midi_target))
    return {
        "reference_trustworthy_frames": len(curve),
        "covered_by_pitched_note_frames": covered,
        "uncovered_reference_frames": len(curve) - covered,
        "trustworthy_frame_coverage_fraction": covered / len(curve) if curve else None,
        "absolute_cents_error_including_note_cents": distribution(errors),
        "absolute_cents_error_midi_only": distribution(midi_errors),
    }


def lyric_owners(note):
    return {
        token.get("continuation_of", token.get("id"))
        for token in note["lyrics"]
        if token.get("continuation_of") or token.get("text", "").strip()
    }


def repeat_pairs(notes):
    same_midi = []
    same_word = []
    for before, after in zip(notes, notes[1:]):
        if (before["start"] + before["duration"] != after["start"]
                or before.get("pitch") is None or after.get("pitch") is None
                or before["pitch"]["midi"] != after["pitch"]["midi"]):
            continue
        owners = sorted(lyric_owners(before) & lyric_owners(after))
        pair = {"before": note_summary(before), "after": note_summary(after),
                "shared_lyric_token_ids": owners}
        same_midi.append(pair)
        if owners:
            same_word.append(pair)
    return {
        "touching_same_midi_pairs": len(same_midi),
        "touching_same_word_same_midi_pairs": len(same_word),
        "same_word_pairs": same_word,
    }


def acoustic_attack_events(artifact):
    events = []
    timebase = artifact["timebase"]
    for previous, current in zip(artifact["frames"], artifact["frames"][1:]):
        preceding_flux = previous.get("spectral_flux")
        onset_flux = current.get("spectral_flux")
        if preceding_flux is None or onset_flux is None:
            continue
        if onset_flux < max(preceding_flux * 2, 1.0e-6):
            continue
        rise = current["rms"] / max(previous["rms"], 1.0e-6)
        reentry = previous["periodicity"] < 0.55 and current["periodicity"] >= 0.65
        periodicity = current["periodicity"] >= 0.6 and (
            current["periodicity"] - previous["periodicity"] >= 0.08)
        transition = (current["periodicity"] >= 0.6
                      and current["voicing_transition_activation"] >= 0.12)
        if rise >= 1.08 or reentry or periodicity or transition:
            events.append({"time": current["start"] / timebase, "kind": "acoustic_attack"})
    return events


def reference_events(reference):
    pitch_events = {}
    for candidate in reference["fusion"]["candidates"]:
        for event in candidate.get("boundary_constraints", []):
            if event["kind"] != "pitch_discontinuity":
                continue
            key = (event["source_expert"], event["time"])
            pitch_events[key] = {
                "time": event["time"] / CANONICAL_TIMEBASE,
                "kind": event["kind"],
                "source": event["source_expert"],
                "source_local_strength": event.get("source_local_strength"),
            }
    acoustic_path = (reference["manifest"] or {}).get("acoustic")
    acoustic = []
    acoustic_status = "not supplied in reference input manifest"
    if acoustic_path:
        acoustic_path = Path(acoustic_path)
        if not acoustic_path.is_absolute():
            acoustic_path = reference["root"] / acoustic_path
        if acoustic_path.is_file():
            acoustic = acoustic_attack_events(read(acoustic_path))
            acoustic_status = "read shared acoustic frames; recomputed existing acoustic_attack_score predicate"
        else:
            acoustic_status = f"unavailable: {acoustic_path}"
    return {
        "pitch_discontinuity": sorted(pitch_events.values(), key=lambda event: event["time"]),
        "acoustic_attack": acoustic,
    }, acoustic_status


def event_fit(events, starts, changes):
    rows = []
    for event in events:
        rows.append({
            **event,
            "nearest_pitched_note_start": nearest(starts, event["time"]),
            "nearest_midi_change": nearest(changes, event["time"]),
        })
    summary = {}
    for key in ["nearest_pitched_note_start", "nearest_midi_change"]:
        errors = [abs(row[key]["signed_offset_ms"]) for row in rows if row[key] is not None]
        summary[key] = {
            "absolute_offset_ms": distribution(errors),
            "within_existing_sixty_ms_tolerance": sum(error <= ASSOCIATION_SECONDS * 1000 for error in errors),
            "events_without_matching_boundary": len(rows) - len(errors),
        }
    return {"events": len(events), "summary": summary, "details": rows}


def focus_report(time, notes, starts, changes, curve, events):
    return {
        "time": time,
        "covering_note": note_summary(covering_note(notes, time)),
        "nearest_pitched_note_start": nearest(starts, time),
        "nearest_midi_change": nearest(changes, time),
        "reference_cents_before_eighty_ms": distribution(
            [cents for point_time, cents in curve if time - 0.08 <= point_time < time]),
        "reference_cents_after_eighty_ms": distribution(
            [cents for point_time, cents in curve if time <= point_time < time + 0.08]),
        "nearest_reference_events": {
            kind: nearest(items, time) for kind, items in events.items()
        },
    }


def compare(replay, reference, curve, events, focus):
    results = []
    for track in replay["chart"]["tracks"]:
        notes = track_notes(track, replay["chart"]["timebase"])
        starts, changes = boundaries(notes)
        results.append({
            "track_id": track["id"],
            "track_role": track["role"],
            "pitched_notes": len(starts),
            "midi_changes": len(changes),
            "f0_frame_fit": frame_fit(notes, curve),
            "adjacent_repetition": repeat_pairs(notes),
            "event_alignment": {
                kind: event_fit(items, starts, changes) for kind, items in events.items()
            },
            "focus": [focus_report(time, notes, starts, changes, curve, events) for time in focus],
        })
    return {
        "directory": str(replay["root"]),
        "same_input_manifest_as_reference": (
            replay["manifest"] == reference["manifest"]
            if replay["manifest"] is not None and reference["manifest"] is not None else None),
        "same_continuous_f0_as_reference": (
            replay["canonical"]["f0_curve"] == reference["canonical"]["f0_curve"]),
        "same_measured_words_as_reference": (
            replay["canonical"]["words"] == reference["canonical"]["words"]),
        "tracks": results,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directories", nargs="+", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--focus-seconds", type=float, nargs="+", default=FOCUS_SECONDS)
    args = parser.parse_args()
    reference_root = args.reference or args.directories[0]
    reference = load(reference_root)
    curve = trusted_curve(reference["canonical"])
    events, acoustic_status = reference_events(reference)
    comparisons = [
        compare(load(root) if root != reference_root else reference, reference, curve,
                events, args.focus_seconds)
        for root in args.directories
    ]
    report = {
        "scope": "Read-only comparison against one shared retained evidence set. No model or audio execution.",
        "interpretation": [
            "F0 participated in fusion and is not independent musical ground truth.",
            "Pitch error is frame weighted over shared trustworthy F0 frames covered by a pitched note.",
            "Trustworthy uses the existing finite positive Hz and absent or at least 0.5 confidence rule.",
            "Coverage is reported separately so omitting difficult frames cannot silently improve pitch error.",
            "A same-word same-MIDI touching pair can be a real reattack; the count is not an accuracy score.",
            "MIDI changes compare successive pitched notes; gaps and both note geometries remain in each row.",
            "Events match the nearest boundary independently; multiple events may refer to one boundary.",
            "Sixty milliseconds is the existing association tolerance, reported without pass/fail.",
            "Pitch events come from the reference fusion's retained persistent F0 constraints, not new detections.",
            "No minimum-note-length count or acceptance threshold is used.",
        ],
        "reference_directory": str(reference_root),
        "reference_f0_frames": len(reference["canonical"]["f0_curve"]),
        "reference_trustworthy_f0_frames": len(curve),
        "reference_acoustic_status": acoustic_status,
        "reference_events": events,
        "comparisons": comparisons,
    }
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps({
        "output": str(args.output),
        "reference_acoustic_status": acoustic_status,
        "comparisons": [
            {"directory": result["directory"], "tracks": [
                {key: track[key] for key in ["track_id", "pitched_notes", "midi_changes",
                                            "f0_frame_fit"]}
                | {"touching_same_word_same_midi_pairs":
                   track["adjacent_repetition"]["touching_same_word_same_midi_pairs"]}
                for track in result["tracks"]]}
            for result in comparisons
        ],
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
