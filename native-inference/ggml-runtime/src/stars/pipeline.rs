use std::ops::Range;
use std::path::Path;

use crate::wav::read_f32_wav;

use super::decode::{self, Alignment};
use super::frontend;
use super::model::{MEL_BINS, PITCH_CLASSES, Stars, TECHNIQUE_CLASSES};
use super::stage_d::StyleLogits;
use super::stage_e::aggregate_technique_frames;

/// U-Net shape alignment, not a maximum acoustic or musical context.
pub const FRAME_BUCKET: usize = 16;
const CONTEXT_TARGET_FRAMES: usize = 30 * frontend::SAMPLE_RATE / frontend::HOP_SIZE;
const CONTEXT_MARGIN_FRAMES: usize = frontend::SAMPLE_RATE / frontend::HOP_SIZE;
pub const NOTE_START: usize = 30;
pub const NOTE_END: usize = 85;
pub const TECHNIQUE_TAXONOMY: [&str; TECHNIQUE_CLASSES] = [
    "bubble",
    "breathe",
    "pharyngeal",
    "vibrato",
    "glissando",
    "mixed",
    "falsetto",
    "weak",
    "strong",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptWord {
    pub id: String,
    pub text: String,
    pub start_micros: u64,
    pub duration_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonemeInput {
    pub phone_ids: Vec<i64>,
    pub phone_to_word: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedInputs {
    pub mel: Vec<f32>,
    pub frames: usize,
    pub pitch_coarse: Vec<i32>,
    pub uv: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawNote {
    pub start_frame: usize,
    pub end_frame: usize,
    pub pitch_logits: Vec<f32>,
    pub midi: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawTechnique {
    pub start_frame: usize,
    pub end_frame: usize,
    pub phoneme_id: i64,
    pub raw_logits: Vec<f32>,
    pub source_local_scores: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalStyle {
    pub start_frame: usize,
    pub end_frame: usize,
    pub logits: StyleLogits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StarsResult {
    pub valid_frames: usize,
    pub note_boundary_logits: Vec<f32>,
    pub regulated_note_boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
    pub techniques: Option<Vec<RawTechnique>>,
    pub styles: Option<Vec<GlobalStyle>>,
}

#[derive(Debug, Clone)]
struct Segment {
    start: usize,
    valid: usize,
    words: Vec<TranscriptWord>,
}

impl Segment {
    fn padded(&self) -> usize {
        self.valid.div_ceil(FRAME_BUCKET) * FRAME_BUCKET
    }
}

/// Builds the shared 24 kHz STARS generation from decoded audio and the raw
/// 10 ms annotation-RMVPE F0 curve. RMVPE itself remains an independent model.
pub fn prepare_wav_inputs(
    audio_24k_path: &Path,
    raw_rmvpe_f0: &[f32],
) -> Result<SharedInputs, String> {
    let audio = read_f32_wav(audio_24k_path, frontend::SAMPLE_RATE as u32, 1)?;
    prepare_inputs(&audio, raw_rmvpe_f0)
}

pub fn prepare_inputs(audio_24k: &[f32], raw_rmvpe_f0: &[f32]) -> Result<SharedInputs, String> {
    let (mel, frames) = frontend::mel_80(audio_24k)?;
    let pitch = frontend::annotation_pitch(raw_rmvpe_f0, frames)?;
    let pitch_coarse = pitch
        .pitch_coarse
        .into_iter()
        .map(|value| {
            i32::try_from(value).map_err(|_| "STARS pitch class is out of range".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let uv = pitch
        .uv
        .into_iter()
        .map(|value| i32::try_from(value).map_err(|_| "STARS UV value is out of range".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SharedInputs {
        mel,
        frames,
        pitch_coarse,
        uv,
    })
}

impl Stars {
    /// Runs the complete transcript-conditioned STARS A/B/C pipeline and the
    /// optional D/E technique path. Neural layers execute on the explicitly
    /// selected native device; Rust owns segmentation and discrete decoders.
    pub fn infer_transcript<F, P>(
        &self,
        shared: &SharedInputs,
        words: &[TranscriptWord],
        source_start_micros: u64,
        include_technique: bool,
        phonemize: F,
        progress: P,
    ) -> Result<StarsResult, String>
    where
        F: FnMut(&[String]) -> Result<PhonemeInput, String>,
        P: FnMut(f32, &str),
    {
        self.infer_transcript_with_threshold(
            shared,
            words,
            source_start_micros,
            include_technique,
            0.8,
            phonemize,
            progress,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn infer_transcript_with_threshold<F, P>(
        &self,
        shared: &SharedInputs,
        words: &[TranscriptWord],
        source_start_micros: u64,
        include_technique: bool,
        boundary_threshold: f32,
        mut phonemize: F,
        mut progress: P,
    ) -> Result<StarsResult, String>
    where
        F: FnMut(&[String]) -> Result<PhonemeInput, String>,
        P: FnMut(f32, &str),
    {
        validate_shared_inputs(shared)?;
        let segments = conditioned_segments(words, source_start_micros, shared.frames)?;
        if segments.is_empty() {
            return Err("STARS has no TimedTranscript-conditioned frames".to_string());
        }
        let mut all_logits = vec![0.0_f32; shared.frames];
        let mut all_notes = Vec::new();
        let mut all_techniques = include_technique.then(Vec::new);
        let mut all_styles = include_technique.then(Vec::new);

        for (index, segment) in segments.iter().enumerate() {
            let message = if include_technique {
                "Running STARS Stage A/B/C/D/E segments"
            } else {
                "Running STARS Stage A/B/C segments"
            };
            progress(index as f32 / segments.len() as f32, message);
            let frames = segment.padded();
            let mel = padded_rows(&shared.mel, MEL_BINS, segment);
            let pitch = padded_indices(&shared.pitch_coarse, segment);
            let uv = padded_indices(&shared.uv, segment);
            let mel_pitch = self.encode_mel_with_pitch(&mel, &pitch, &uv, segment.valid, frames)?;
            let utterance = self.encode_utterance(&mel_pitch.embedded, segment.valid, frames)?;
            let phone_input = phonemize(
                &segment
                    .words
                    .iter()
                    .map(|word| word.text.clone())
                    .collect::<Vec<_>>(),
            )?;
            let alignment = decode::align(
                &utterance.phoneme_logits[..segment.valid * 61],
                61,
                &utterance.boundary_probabilities[..segment.valid],
                &phone_input.phone_ids,
                &phone_input.phone_to_word,
            )?;
            let (mel_to_phoneme, mel_to_word) = padded_alignment(&alignment, frames);
            let rhythm = self.encode_rhythm(
                &mel_pitch.embedded,
                &utterance.features,
                segment.valid,
                frames,
                &mel_to_phoneme,
                alignment.phoneme_intervals.len(),
                &mel_to_word,
                alignment.word_intervals.len(),
            )?;
            all_logits[segment.start..segment.start + segment.valid]
                .copy_from_slice(&rhythm.note_boundary_logits[..segment.valid]);
            let regulated = decode::regulate_boundaries(
                &rhythm.note_boundary_logits,
                boundary_threshold,
                17,
                segment.valid,
            )?;
            let local_boundaries = boundary_indices(&regulated, segment.valid);
            let ranges = note_ranges(&local_boundaries, segment.valid);
            let mel_to_note = mapping_from_boundaries(&regulated, segment.valid);
            let pitch = self.encode_pitch(
                &mel_pitch.embedded,
                &rhythm.features,
                segment.valid,
                frames,
                &mel_to_note,
                ranges.len(),
                &regulated,
            )?;
            let mut local_notes = Vec::new();
            append_notes(&mut local_notes, 0, &ranges, &pitch.note_logits);
            for mut note in consolidate_word_unisons(local_notes, &alignment.word_intervals) {
                note.start_frame += segment.start;
                note.end_frame += segment.start;
                all_notes.push(note);
            }

            if include_technique {
                append_technique_outputs(
                    self,
                    segment,
                    &alignment,
                    &mel_pitch.embedded,
                    &pitch.features,
                    all_techniques.as_mut().expect("technique output exists"),
                    all_styles.as_mut().expect("style output exists"),
                )?;
            }
        }
        // These are complete native note events. Equal pitch alone is not a
        // continuation: merging it here would erase repeated syllable attacks.
        let all_boundaries = all_notes
            .iter()
            .skip(1)
            .map(|note| note.start_frame)
            .collect();
        progress(1.0, "STARS inference complete");
        Ok(StarsResult {
            valid_frames: shared.frames,
            note_boundary_logits: all_logits,
            regulated_note_boundaries: all_boundaries,
            notes: all_notes,
            techniques: all_techniques,
            styles: all_styles,
        })
    }
}

fn append_technique_outputs(
    model: &Stars,
    segment: &Segment,
    alignment: &Alignment,
    mel_embedding: &super::model::FrameFeatures,
    pitch_features: &super::model::FrameFeatures,
    techniques: &mut Vec<RawTechnique>,
    styles: &mut Vec<GlobalStyle>,
) -> Result<(), String> {
    if alignment.phoneme_intervals.is_empty() {
        return Err("STARS has no phoneme intervals for technique inference".to_string());
    }
    let sentence = model.encode_sentence(
        mel_embedding,
        pitch_features,
        segment.valid,
        segment.padded(),
    )?;
    let intervals = alignment
        .phoneme_intervals
        .iter()
        .map(|interval| interval.start..interval.end)
        .collect::<Vec<_>>();
    let aggregated =
        aggregate_technique_frames(&sentence.weighted_features, &sentence.attention, &intervals)?;
    let technique = model.encode_techniques(&aggregated, intervals.len())?;
    for (phoneme, interval) in alignment.phoneme_intervals.iter().enumerate() {
        let raw_logits = technique.logits
            [phoneme * TECHNIQUE_CLASSES..(phoneme + 1) * TECHNIQUE_CLASSES]
            .to_vec();
        techniques.push(RawTechnique {
            start_frame: segment.start + interval.start,
            end_frame: segment.start + interval.end,
            phoneme_id: interval.label,
            source_local_scores: raw_logits.iter().map(|value| sigmoid(*value)).collect(),
            raw_logits,
        });
    }
    styles.push(GlobalStyle {
        start_frame: segment.start,
        end_frame: segment.start + segment.valid,
        logits: sentence.styles,
    });
    Ok(())
}

fn validate_shared_inputs(shared: &SharedInputs) -> Result<(), String> {
    if shared.frames == 0
        || shared.mel.len() != shared.frames * MEL_BINS
        || shared.pitch_coarse.len() != shared.frames
        || shared.uv.len() != shared.frames
        || shared.mel.iter().any(|value| !value.is_finite())
    {
        Err("STARS shared input shape is invalid".to_string())
    } else {
        Ok(())
    }
}

fn canonical_to_frame(value: u64) -> Result<usize, String> {
    usize::try_from(
        (u128::from(value) * frontend::SAMPLE_RATE as u128 + frontend::HOP_SIZE as u128 * 500_000)
            / (frontend::HOP_SIZE as u128 * 1_000_000),
    )
    .map_err(|_| "STARS transcript frame projection overflows".to_string())
}

fn conditioned_segments(
    words: &[TranscriptWord],
    source_start_micros: u64,
    frames: usize,
) -> Result<Vec<Segment>, String> {
    let mut spans = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let start = canonical_to_frame(word.start_micros.saturating_sub(source_start_micros))?;
        let end = canonical_to_frame(
            word.start_micros
                .saturating_add(word.duration_micros)
                .saturating_sub(source_start_micros),
        )?
        .min(frames);
        if start < frames && end > start {
            spans.push((index, start, end));
        }
    }
    if spans.is_empty() {
        return Ok(Vec::new());
    }
    // Audio visibility follows complete lexical contexts, never a fixture
    // allocation grid. A long held word is not repeated across fixed cuts.
    let mut groups = Vec::new();
    let mut first = 0;
    let mut previous_end = spans[0].2;
    for index in 1..spans.len() {
        let start = spans[index].1;
        if start.saturating_sub(previous_end) > CONTEXT_MARGIN_FRAMES * 2
            || (start.saturating_sub(spans[first].1) >= CONTEXT_TARGET_FRAMES
                && start >= previous_end)
        {
            groups.push((first, index, previous_end));
            first = index;
        }
        previous_end = previous_end.max(spans[index].2);
    }
    groups.push((first, spans.len(), previous_end));
    let mut segments = Vec::new();
    for (index, &(first, stop, end)) in groups.iter().enumerate() {
        let mut start = spans[first].1.saturating_sub(CONTEXT_MARGIN_FRAMES);
        let mut end = end.saturating_add(CONTEXT_MARGIN_FRAMES).min(frames);
        if index > 0 {
            let preceding_end = groups[index - 1].2;
            start = start.max(preceding_end + spans[first].1.saturating_sub(preceding_end) / 2);
        }
        if let Some(next) = groups.get(index + 1) {
            let measured_end = groups[index].2;
            end = end.min(measured_end + spans[next.0].1.saturating_sub(measured_end) / 2);
        }
        if end > start {
            segments.push(Segment {
                start,
                valid: end - start,
                words: spans[first..stop]
                    .iter()
                    .map(|span| words[span.0].clone())
                    .collect(),
            });
        }
    }
    Ok(segments)
}

fn padded_rows(values: &[f32], width: usize, segment: &Segment) -> Vec<f32> {
    let mut result = vec![0.0_f32; segment.padded() * width];
    result[..segment.valid * width]
        .copy_from_slice(&values[segment.start * width..(segment.start + segment.valid) * width]);
    result
}

fn padded_indices(values: &[i32], segment: &Segment) -> Vec<i32> {
    let mut result = vec![0_i32; segment.padded()];
    result[..segment.valid].copy_from_slice(&values[segment.start..segment.start + segment.valid]);
    result
}

fn padded_alignment(alignment: &Alignment, frames: usize) -> (Vec<i64>, Vec<i64>) {
    let mut phonemes = alignment.mel_to_phoneme.clone();
    let mut words = alignment.mel_to_word.clone();
    phonemes.resize(frames, 0);
    words.resize(frames, 0);
    (phonemes, words)
}

fn boundary_indices(values: &[i64], valid: usize) -> Vec<usize> {
    values[..valid]
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (*value == 1).then_some(index))
        .collect()
}

fn note_ranges(boundaries: &[usize], valid: usize) -> Vec<Range<usize>> {
    let mut starts = Vec::with_capacity(boundaries.len() + 1);
    starts.push(0);
    starts.extend(
        boundaries
            .iter()
            .copied()
            .filter(|value| *value > 0 && *value < valid),
    );
    starts.sort_unstable();
    starts.dedup();
    starts
        .iter()
        .enumerate()
        .filter_map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(valid);
            (end > *start).then_some(*start..end)
        })
        .collect()
}

fn mapping_from_boundaries(values: &[i64], valid: usize) -> Vec<i64> {
    let mut mapping = vec![0_i64; values.len()];
    let mut note = 0_i64;
    for frame in 0..valid {
        note += values[frame];
        mapping[frame] = note;
    }
    mapping[valid..].fill(note);
    mapping
}

fn append_notes(
    notes: &mut Vec<RawNote>,
    segment_start: usize,
    ranges: &[Range<usize>],
    logits: &[f32],
) {
    for (index, range) in ranges.iter().enumerate() {
        let row = logits[index * PITCH_CLASSES..(index + 1) * PITCH_CLASSES].to_vec();
        let midi = row
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .and_then(|(class, _)| {
                (NOTE_START..=NOTE_END)
                    .contains(&class)
                    .then_some(class as u8)
            });
        notes.push(RawNote {
            start_frame: segment_start + range.start,
            end_frame: segment_start + range.end,
            pitch_logits: row,
            midi,
        });
    }
}

/// STARS's published MIDI decoder consolidates unison fragments within a word.
/// Use its own acoustic alignment for ownership, never the human benchmark or
/// a caller's evenly divided word duration. Ambiguous overlap is left intact.
fn word_owner(note: &RawNote, words: &[decode::Interval]) -> Option<usize> {
    let mut best = None;
    let mut maximum = 0;
    let mut ambiguous = false;
    for (index, word) in words.iter().enumerate() {
        let overlap = note
            .end_frame
            .min(word.end)
            .saturating_sub(note.start_frame.max(word.start));
        if overlap > maximum {
            maximum = overlap;
            best = (word.label >= 0).then_some(index);
            ambiguous = false;
        } else if overlap > 0 && overlap == maximum {
            ambiguous = true;
        }
    }
    if ambiguous { None } else { best }
}

fn consolidate_word_unisons(notes: Vec<RawNote>, words: &[decode::Interval]) -> Vec<RawNote> {
    let mut output: Vec<(RawNote, Option<usize>)> = Vec::with_capacity(notes.len());
    for note in notes {
        let owner = word_owner(&note, words);
        if let Some((previous, previous_owner)) = output.last_mut()
            && owner.is_some()
            && owner == *previous_owner
            && previous.end_frame == note.start_frame
            && previous.midi.is_some()
            && previous.midi == note.midi
            && owner.is_some_and(|index| {
                words[index].start < note.start_frame && note.start_frame < words[index].end
            })
        {
            let previous_duration = previous.end_frame - previous.start_frame;
            let duration = note.end_frame - note.start_frame;
            let total = previous_duration + duration;
            for (left, right) in previous.pitch_logits.iter_mut().zip(note.pitch_logits) {
                *left = (*left * previous_duration as f32 + right * duration as f32) / total as f32;
            }
            previous.end_frame = note.end_frame;
        } else {
            output.push((note, owner));
        }
    }
    output.into_iter().map(|(note, _)| note).collect()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn predicted_word(start: usize, end: usize, label: i64) -> decode::Interval {
        decode::Interval {
            start,
            end,
            label,
            word: None,
        }
    }

    fn predicted_note(start_frame: usize, end_frame: usize, midi: Option<u8>) -> RawNote {
        RawNote {
            start_frame,
            end_frame,
            pitch_logits: vec![1.0; PITCH_CLASSES],
            midi,
        }
    }

    #[test]
    fn same_word_unison_fragments_merge_without_erasing_cross_word_attacks() {
        let notes = vec![
            predicted_note(0, 20, Some(60)),
            predicted_note(20, 80, Some(60)),
            predicted_note(80, 100, Some(60)),
            predicted_note(100, 120, Some(62)),
        ];
        let words = [predicted_word(0, 80, 0), predicted_word(80, 120, 1)];
        let result = consolidate_word_unisons(notes, &words);
        assert_eq!(result.len(), 3);
        assert_eq!((result[0].start_frame, result[0].end_frame), (0, 80));
        assert_eq!((result[1].start_frame, result[1].end_frame), (80, 100));
        assert_eq!(result[2].midi, Some(62));
    }

    #[test]
    fn gaps_silence_and_ambiguous_words_do_not_license_consolidation() {
        let notes = vec![
            predicted_note(0, 40, Some(60)),
            predicted_note(40, 80, Some(60)),
        ];
        assert_eq!(consolidate_word_unisons(notes.clone(), &[]), notes);
        assert_eq!(
            consolidate_word_unisons(notes.clone(), &[predicted_word(0, 80, -1)]),
            notes
        );
        let words = [predicted_word(0, 20, 0), predicted_word(20, 80, 1)];
        assert_eq!(consolidate_word_unisons(notes.clone(), &words), notes);
        let gaps = vec![
            predicted_note(0, 20, Some(60)),
            predicted_note(30, 80, Some(60)),
        ];
        assert_eq!(
            consolidate_word_unisons(gaps.clone(), &[predicted_word(0, 80, 0)]),
            gaps
        );
    }

    #[test]
    fn many_repeated_syllables_remain_separate_after_word_consolidation() {
        let notes = (0..48)
            .map(|index| predicted_note(index * 30, (index + 1) * 30, Some(60)))
            .collect::<Vec<_>>();
        let words = (0..48)
            .map(|index| predicted_word(index * 30, (index + 1) * 30, index as i64))
            .collect::<Vec<_>>();
        assert_eq!(consolidate_word_unisons(notes.clone(), &words), notes);
    }

    #[test]
    fn conditioned_segments_use_the_canonical_source_offset() {
        let words = vec![TranscriptWord {
            id: "word".to_string(),
            text: "你".to_string(),
            start_micros: 2_000_000,
            duration_micros: 200_000,
        }];
        let segments = conditioned_segments(&words, 2_000_000, 300).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, 0);
    }

    #[test]
    fn note_mapping_and_ranges_share_the_same_zero_based_generation() {
        let mut boundaries = vec![0_i64; FRAME_BUCKET];
        boundaries[4] = 1;
        boundaries[9] = 1;
        let mapping = mapping_from_boundaries(&boundaries, 12);
        assert_eq!(&mapping[..12], &[0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2]);
        assert_eq!(note_ranges(&[4, 9], 12), [0..4, 4..9, 9..12]);
    }

    #[test]
    fn native_repeated_notes_and_large_note_counts_survive_without_stitching() {
        let ranges = (0..48)
            .map(|index| index * 30..(index + 1) * 30)
            .collect::<Vec<_>>();
        let mut logits = vec![-10.0; ranges.len() * PITCH_CLASSES];
        for row in logits.chunks_exact_mut(PITCH_CLASSES) {
            row[60] = 10.0;
        }
        let mut notes = Vec::new();
        append_notes(&mut notes, 17, &ranges, &logits);
        assert_eq!(notes.len(), 48);
        assert!(notes.iter().all(|note| note.midi == Some(60)));
        assert!(
            notes
                .windows(2)
                .all(|pair| pair[0].end_frame == pair[1].start_frame)
        );
        assert_eq!(notes.last().unwrap().end_frame, 17 + 48 * 30);
    }

    #[test]
    fn full_words_are_not_repeated_at_fixture_frame_cuts() {
        let words = (0..12)
            .map(|index| TranscriptWord {
                id: format!("word-{index}"),
                text: "啦".into(),
                start_micros: 1_000_000 + index * 500_000,
                duration_micros: 500_000,
            })
            .collect::<Vec<_>>();
        let frames = canonical_to_frame(8_000_000).unwrap();
        let segments = conditioned_segments(&words, 0, frames).unwrap();
        assert_eq!(segments.len(), 1);
        let segment = &segments[0];
        assert_eq!(segment.words, words);
        assert!(segment.valid > 256);
        assert_eq!(segment.padded() % 16, 0);
        assert!(segment.padded() - segment.valid < 16);
        let values = (0..frames * MEL_BINS)
            .map(|index| index as f32)
            .collect::<Vec<_>>();
        let padded = padded_rows(&values, MEL_BINS, segment);
        assert_eq!(
            &padded[..segment.valid * MEL_BINS],
            &values[segment.start * MEL_BINS..(segment.start + segment.valid) * MEL_BINS]
        );
        assert!(
            padded[segment.valid * MEL_BINS..]
                .iter()
                .all(|value| *value == 0.0)
        );
    }

    #[test]
    fn long_contexts_preserve_complete_transcript_and_nonoverlapping_audio() {
        let words = (0..80)
            .map(|index| TranscriptWord {
                id: format!("word-{index}"),
                text: "啦".into(),
                start_micros: index * 1_000_000,
                duration_micros: 1_000_000,
            })
            .collect::<Vec<_>>();
        let segments =
            conditioned_segments(&words, 0, canonical_to_frame(80_000_000).unwrap()).unwrap();
        assert!(segments.len() > 1);
        assert!(
            segments
                .windows(2)
                .all(|pair| pair[0].start + pair[0].valid <= pair[1].start)
        );
        assert_eq!(
            segments
                .iter()
                .flat_map(|segment| segment.words.clone())
                .collect::<Vec<_>>(),
            words
        );
        for segment in segments {
            for word in segment.words {
                assert!(canonical_to_frame(word.start_micros).unwrap() >= segment.start);
                assert!(
                    canonical_to_frame(word.start_micros + word.duration_micros).unwrap()
                        <= segment.start + segment.valid
                );
            }
        }
    }
}
