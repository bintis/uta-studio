use std::collections::BTreeMap;

use super::frontend::{HOP_SAMPLES, SAMPLE_RATE};

pub const ONSET_THRESHOLD: f32 = 0.32;
pub const OFFSET_THRESHOLD: f32 = 0.70;

#[derive(Debug, Clone, PartialEq)]
pub struct NoteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub range: NoteRange,
    pub midi: u8,
    pub onset_score: f32,
    pub offset_score: Option<f32>,
    pub pitch_score: f32,
}

pub fn decode_notes(
    on_off: &[f32],
    octave: &[f32],
    pitch_class: &[f32],
    frames: usize,
) -> Result<Vec<Note>, String> {
    if on_off.len() != frames * 4 || octave.len() != frames * 5 || pitch_class.len() != frames * 13
    {
        return Err("JBM555 decoder received malformed output tensors".to_string());
    }
    if on_off
        .iter()
        .chain(octave)
        .chain(pitch_class)
        .any(|value| !value.is_finite())
    {
        return Err("JBM555 decoder received a non-finite output value".to_string());
    }

    let onset = (0..frames)
        .map(|frame| on_off[frame * 4 + 1])
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    let mut active = None;
    let mut pitches = Vec::new();
    for frame in 0..frames {
        let backward = frame.saturating_sub(3);
        let forward = (frame + 4).min(frames);
        let is_peak = onset[frame] >= ONSET_THRESHOLD
            && onset[backward..forward]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .is_some_and(|(index, _)| backward + index == frame);
        if is_peak {
            finish_note(frame, &mut active, &mut pitches, &mut notes, None);
            active = Some((frame, onset[frame]));
        } else if on_off[frame * 4 + 2] >= OFFSET_THRESHOLD {
            finish_note(
                frame,
                &mut active,
                &mut pitches,
                &mut notes,
                Some(on_off[frame * 4 + 2]),
            );
        }
        if active.is_some() {
            let octave_probabilities = softmax(&octave[frame * 5..frame * 5 + 5]);
            let class_probabilities = softmax(&pitch_class[frame * 13..frame * 13 + 13]);
            let octave_index = maximum_index(&octave_probabilities[..4]);
            let class_index = maximum_index(&class_probabilities[..12]);
            pitches.push((
                (36 + octave_index * 12 + class_index) as u8,
                octave_probabilities[octave_index] * class_probabilities[class_index],
            ));
        }
    }
    finish_note(frames, &mut active, &mut pitches, &mut notes, None);
    Ok(notes)
}

fn finish_note(
    end: usize,
    active: &mut Option<(usize, f32)>,
    pitches: &mut Vec<(u8, f32)>,
    notes: &mut Vec<Note>,
    offset_score: Option<f32>,
) {
    let Some((start, onset_score)) = active.take() else {
        return;
    };
    if pitches.is_empty() || end <= start {
        pitches.clear();
        return;
    }
    let mut counts = BTreeMap::<u8, (usize, f32)>::new();
    for (midi, score) in pitches.drain(..) {
        let entry = counts.entry(midi).or_default();
        entry.0 += 1;
        entry.1 += score;
    }
    let (midi, (count, score)) = counts
        .into_iter()
        .max_by(|left, right| {
            left.1
                .0
                .cmp(&right.1.0)
                .then_with(|| left.1.1.total_cmp(&right.1.1))
        })
        .expect("non-empty pitch histogram");
    notes.push(Note {
        range: NoteRange {
            start: frame_time(start),
            end: frame_time(end),
        },
        midi,
        onset_score,
        offset_score,
        pitch_score: score / count as f32,
    });
}

fn frame_time(frame: usize) -> u64 {
    frame as u64 * HOP_SAMPLES as u64 * 1_000_000 / SAMPLE_RATE as u64
}

fn softmax(values: &[f32]) -> Vec<f32> {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut result = values
        .iter()
        .map(|value| (*value - maximum).exp())
        .collect::<Vec<_>>();
    let total = result.iter().sum::<f32>().max(f32::MIN_POSITIVE);
    result.iter_mut().for_each(|value| *value /= total);
    result
}

fn maximum_index(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map_or(0, |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_same_pitch_attacks_remain_separate_notes() {
        let frames = 12;
        let mut on_off = vec![0.0; frames * 4];
        let mut octave = vec![0.0; frames * 5];
        let mut class = vec![0.0; frames * 13];
        for frame in 0..frames {
            on_off[frame * 4 + 2] = 0.1;
            octave[frame * 5 + 2] = 4.0;
            class[frame * 13 + 9] = 4.0;
        }
        on_off[4 + 1] = 0.8;
        on_off[7 * 4 + 1] = 0.9;
        let notes = decode_notes(&on_off, &octave, &class, frames).unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].midi, notes[1].midi);
        assert!(notes[0].range.end <= notes[1].range.start);
    }

    #[test]
    fn malformed_tensor_lengths_are_rejected() {
        assert_eq!(
            decode_notes(&[0.0; 3], &[0.0; 5], &[0.0; 13], 1).unwrap_err(),
            "JBM555 decoder received malformed output tensors"
        );
    }
}
