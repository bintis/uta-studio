#!/usr/bin/env python3
"""Compare two CPU replays over the same retained singing evidence.

Reports measurements without an acceptance threshold. Usage:
  compare-singing-replays.py BEFORE_DIRECTORY AFTER_DIRECTORY NEW_REPORT_JSON
"""

import json
import sys
from pathlib import Path


def read(path):
    return json.loads(path.read_text())


def chart_notes(chart):
    return sorted(
        (
            note
            for track in chart['tracks']
            for phrase in track['phrases']
            for note in phrase['notes']
        ),
        key=lambda note: (note['start'], note['duration']),
    )


def pitch_at(notes, index, time):
    while index < len(notes) and notes[index]['start'] + notes[index]['duration'] <= time:
        index += 1
    if index < len(notes) and notes[index]['start'] <= time:
        return index, notes[index].get('pitch')
    return index, None


def melody_differences(before, after):
    edges = sorted({
        edge
        for note in before + after
        for edge in (note['start'], note['start'] + note['duration'])
    })
    before_index = 0
    after_index = 0
    changed = []
    target_changed = 0
    cents_changed = 0
    removed = 0
    added = 0
    for start, end in zip(edges, edges[1:]):
        before_index, previous = pitch_at(before, before_index, start)
        after_index, current = pitch_at(after, after_index, start)
        previous_midi = previous['midi'] if previous is not None else None
        current_midi = current['midi'] if current is not None else None
        if previous_midi != current_midi:
            target_changed += end - start
            if previous_midi is not None and current_midi is None:
                removed += end - start
            if previous_midi is None and current_midi is not None:
                added += end - start
        elif previous is not None and previous.get('cents', 0) != current.get('cents', 0):
            cents_changed += end - start
        if previous != current:
            changed.append({'start': start, 'end': end, 'before': previous, 'after': current})
    return {
        'changed_midi_or_voicing_duration_micros': target_changed,
        'changed_cents_only_duration_micros': cents_changed,
        'removed_pitched_duration_micros': removed,
        'added_pitched_duration_micros': added,
        'changed_intervals': changed,
    }


def main():
    if len(sys.argv) != 4:
        raise SystemExit('usage: compare-singing-replays.py BEFORE_DIRECTORY AFTER_DIRECTORY NEW_REPORT_JSON')
    before_root, after_root, output = map(Path, sys.argv[1:])
    before_summary = read(before_root / 'summary.json')
    after_summary = read(after_root / 'summary.json')
    before_track = read(before_root / 'canonical.json')
    after_track = read(after_root / 'canonical.json')
    metrics = [
        'candidate_count', 'selected_canonical_notes', 'chart_objects', 'pitched_notes',
        'pitched_duration_micros', 'same_pitch_pairs_gap_at_most_twenty_ms',
        'adjacent_same_pitch_pairs', 'adjacent_same_pitch_pairs_with_note_below_hundred_ms',
        'unassigned_lyric_notes', 'unresolved_words', 'chart_nonspace_chars',
    ]
    deltas = {
        key: {'before': before_summary[key], 'after': after_summary[key],
              'change': after_summary[key] - before_summary[key]}
        for key in metrics
    }
    duration_deltas = {
        key: {'before': count, 'after': after_summary['short_pitched_counts_below_micros'][key],
              'change': after_summary['short_pitched_counts_below_micros'][key] - count}
        for key, count in before_summary['short_pitched_counts_below_micros'].items()
    }
    before_notes = chart_notes(read(before_root / 'vocal-chart.json'))
    after_notes = chart_notes(read(after_root / 'vocal-chart.json'))
    report = {
        'scope': 'CPU replay comparison; source model measurements are shared, not rerun by this tool. Counts alone do not establish musical correctness.',
        'before_directory': str(before_root), 'after_directory': str(after_root),
        'same_input_manifest': read(before_root / 'input-manifest.json') == read(after_root / 'input-manifest.json'),
        'same_source_timeline': all(before_summary[key] == after_summary[key]
                                    for key in ['source_start', 'source_duration']),
        'same_continuous_f0': before_track['f0_curve'] == after_track['f0_curve'],
        'same_measured_word_boundaries': before_track['words'] == after_track['words'],
        'same_complete_lyric_units': before_track['lyric_units'] == after_track['lyric_units'],
        'caller_ranges_preserved': {'before': before_summary['caller_ranges_preserved'],
                                   'after': after_summary['caller_ranges_preserved']},
        'all_text_preserved_in_order': {'before': before_summary['all_canonical_text_preserved_in_order'],
                                        'after': after_summary['all_canonical_text_preserved_in_order']},
        'metrics': deltas, 'short_pitched_counts_below_micros': duration_deltas,
        'melody_geometry': melody_differences(before_notes, after_notes),
        'reported_regions': {'before': before_summary['reported_regions'],
                             'after': after_summary['reported_regions']},
    }
    with output.open('x') as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
    print(json.dumps({key: value for key, value in report.items()
                      if key not in ['melody_geometry', 'reported_regions']},
                     ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
