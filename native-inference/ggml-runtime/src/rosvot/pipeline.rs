use std::ops::Range;
use std::path::Path;

use super::decode::{aggregate_notes, decode_pitch, regulate_boundaries};
use super::model::{HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, Rosvot};
use crate::stars::frontend;
use crate::wav::read_f32_wav;

/// U-Net shape alignment, not a neural context or a musical boundary.
pub const FRAME_BUCKET: usize = 16;
const CONTEXT_TARGET_FRAMES: usize = 30 * frontend::SAMPLE_RATE / frontend::HOP_SIZE;
const CONTEXT_MARGIN_FRAMES: usize = frontend::SAMPLE_RATE / frontend::HOP_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptWord {
    pub id: String,
    pub text: String,
    pub start_micros: u64,
    pub duration_micros: u64,
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
pub struct RosvotResult {
    pub valid_frames: usize,
    pub note_boundary_logits: Vec<f32>,
    pub regulated_note_boundaries: Vec<usize>,
    pub notes: Vec<RawNote>,
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

/// Builds the shared ROSVOT input from a decoded 24 kHz mono WAV and raw
/// annotation-RMVPE F0. RMVPE itself remains an independent model.
pub fn prepare_wav_inputs(
    audio_24k_path: &Path,
    raw_rmvpe_f0: &[f32],
) -> Result<SharedInputs, String> {
    let audio = read_f32_wav(audio_24k_path, frontend::SAMPLE_RATE as u32, 1)?;
    prepare_inputs(&audio, raw_rmvpe_f0)
}

/// Builds the shared ROSVOT input from 24 kHz audio and raw annotation-RMVPE F0.
pub fn prepare_inputs(audio_24k: &[f32], raw_rmvpe_f0: &[f32]) -> Result<SharedInputs, String> {
    let (mel_80, frames) = frontend::mel_80(audio_24k)?;
    let mel = frontend::rosvot_mel_prefix(&mel_80, frames)?;
    let pitch = frontend::annotation_pitch(raw_rmvpe_f0, frames)?;
    let pitch_coarse = pitch
        .pitch_coarse
        .into_iter()
        .map(|value| {
            i32::try_from(value).map_err(|_| "ROSVOT pitch class is out of range".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let uv = pitch
        .uv
        .into_iter()
        .map(|value| {
            i32::try_from(value).map_err(|_| "ROSVOT UV value is out of range".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SharedInputs {
        mel,
        frames,
        pitch_coarse,
        uv,
    })
}

impl Rosvot {
    /// Runs the transcript-conditioned ROSVOT frame and pitch stages.
    /// Neural layers execute on the selected upstream-GGML backend; Rust owns
    /// segment framing, transcript boundary preservation, and note decoding.
    pub fn infer_transcript<P>(
        &self,
        shared: &SharedInputs,
        words: &[TranscriptWord],
        source_start_micros: u64,
        progress: P,
    ) -> Result<RosvotResult, String>
    where
        P: FnMut(f32, &str),
    {
        self.infer_transcript_with_threshold(shared, words, source_start_micros, 0.85, progress)
    }

    pub fn infer_transcript_with_threshold<P>(
        &self,
        shared: &SharedInputs,
        words: &[TranscriptWord],
        source_start_micros: u64,
        boundary_threshold: f32,
        mut progress: P,
    ) -> Result<RosvotResult, String>
    where
        P: FnMut(f32, &str),
    {
        validate_shared_inputs(shared)?;
        let segments = conditioned_segments(words, source_start_micros, shared.frames)?;
        if segments.is_empty() {
            return Err("ROSVOT has no TimedTranscript-conditioned frames".to_string());
        }
        let mut all_logits = vec![0.0_f32; shared.frames];
        let mut all_notes = Vec::new();

        for (index, segment) in segments.iter().enumerate() {
            progress(
                index as f32 / segments.len() as f32,
                "Running ROSVOT frame/pitch segments",
            );
            let mel = padded_rows(&shared.mel, MEL_BINS, segment);
            let pitch = padded_indices(&shared.pitch_coarse, segment);
            let uv = padded_indices(&shared.uv, segment);
            let reference = segment_word_boundaries(segment, source_start_micros)?;
            let conditioning = self.encode_conditioning(
                &mel,
                &pitch,
                &uv,
                &reference,
                segment.valid,
                segment.padded(),
            )?;
            let features =
                self.encode_backbone(&conditioning.conditioned, segment.valid, segment.padded())?;
            let frame = self.encode_frame_heads(&features, segment.padded())?;
            all_logits[segment.start..segment.start + segment.valid]
                .copy_from_slice(&frame.boundary_logits[..segment.valid]);
            let regulated = regulate_boundaries(
                &frame.boundary_logits,
                boundary_threshold,
                17,
                &reference,
                8,
                segment.valid,
            )?;
            let aggregated = aggregate_notes(
                &frame.weighted_features,
                &frame.attention,
                &regulated,
                HIDDEN_DIM,
                segment.valid,
            )?;
            // The native pitch head accepts the actual note count. A former
            // small fixture bucket must not limit complete musical phrases.
            let pitch = self.encode_pitch_head(&aggregated.features, aggregated.count)?;
            let local_boundaries = boundary_indices(&regulated, segment.valid);
            let ranges = note_ranges(&local_boundaries, segment.valid);
            append_notes(&mut all_notes, segment.start, &ranges, &pitch.logits)?;
        }
        // Every retained window ends at a real transcript boundary or a gap.
        // Equal pitch does not imply continuation: preserve repeated notes.
        let all_boundaries = all_notes
            .iter()
            .skip(1)
            .map(|note| note.start_frame)
            .collect();
        progress(1.0, "ROSVOT inference complete");
        Ok(RosvotResult {
            valid_frames: shared.frames,
            note_boundary_logits: all_logits,
            regulated_note_boundaries: all_boundaries,
            notes: all_notes,
        })
    }
}

fn validate_shared_inputs(shared: &SharedInputs) -> Result<(), String> {
    if shared.frames == 0
        || shared.mel.len() != shared.frames * MEL_BINS
        || shared.pitch_coarse.len() != shared.frames
        || shared.uv.len() != shared.frames
        || shared.mel.iter().any(|value| !value.is_finite())
        || shared
            .pitch_coarse
            .iter()
            .any(|value| !(0..300).contains(value))
        || shared.uv.iter().any(|value| !(0..3).contains(value))
    {
        Err("ROSVOT shared input is invalid".to_string())
    } else {
        Ok(())
    }
}

fn frame_to_micros(frame: usize) -> u64 {
    (frame as u128 * frontend::HOP_SIZE as u128 * 1_000_000 / frontend::SAMPLE_RATE as u128) as u64
}

fn canonical_to_frame(value: u64) -> Result<usize, String> {
    usize::try_from(
        (u128::from(value) * frontend::SAMPLE_RATE as u128 + frontend::HOP_SIZE as u128 * 500_000)
            / (frontend::HOP_SIZE as u128 * 1_000_000),
    )
    .map_err(|_| "TimedTranscript frame projection overflows".to_string())
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
    // Keep full words and their acoustic context together. The work target
    // bounds ordinary phrases, but cannot cut a word at an allocation grid.
    let mut groups = Vec::new();
    let mut first = 0;
    let mut previous_end = spans[0].2;
    for index in 1..spans.len() {
        let start = spans[index].1;
        let gap = start.saturating_sub(previous_end);
        if gap > CONTEXT_MARGIN_FRAMES * 2
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
            let cut = preceding_end + spans[first].1.saturating_sub(preceding_end) / 2;
            start = start.max(cut);
        }
        if let Some(next) = groups.get(index + 1) {
            let measured_end = groups[index].2;
            let cut = measured_end + spans[next.0].1.saturating_sub(measured_end) / 2;
            end = end.min(cut);
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

/// Each measured word onset is a condition, including the first onset after
/// actual leading silence. Context-window starts are not word boundaries.
fn segment_word_boundaries(
    segment: &Segment,
    source_start_micros: u64,
) -> Result<Vec<i32>, String> {
    let mut boundaries = vec![0_i32; segment.padded()];
    let timeline_start = frame_to_micros(segment.start).saturating_add(source_start_micros);
    for word in &segment.words {
        let local = word.start_micros.saturating_sub(timeline_start);
        let frame = canonical_to_frame(local)?.min(segment.valid.saturating_sub(1));
        if frame > 0 {
            boundaries[frame] = 1;
        }
    }
    Ok(boundaries)
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

fn boundary_indices(values: &[i32], valid: usize) -> Vec<usize> {
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

fn append_notes(
    notes: &mut Vec<RawNote>,
    segment_start: usize,
    ranges: &[Range<usize>],
    logits: &[f32],
) -> Result<(), String> {
    let pitches = decode_pitch(logits, ranges.len())?;
    for (index, range) in ranges.iter().enumerate() {
        notes.push(RawNote {
            start_frame: segment_start + range.start,
            end_frame: segment_start + range.end,
            pitch_logits: logits[index * PITCH_CLASSES..(index + 1) * PITCH_CLASSES].to_vec(),
            midi: pitches[index],
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start_micros: u64) -> TranscriptWord {
        TranscriptWord {
            id: start_micros.to_string(),
            text: "啦".to_string(),
            start_micros,
            duration_micros: 100_000,
        }
    }

    #[test]
    fn transcript_boundaries_skip_the_first_word() {
        let segment = Segment {
            start: 0,
            valid: 100,
            words: vec![word(0), word(100_000), word(200_000)],
        };
        let actual = segment_word_boundaries(&segment, 0).unwrap();
        assert_eq!(actual.iter().sum::<i32>(), 2);
        assert_eq!(actual[canonical_to_frame(100_000).unwrap()], 1);
        assert_eq!(actual[canonical_to_frame(200_000).unwrap()], 1);
    }

    #[test]
    fn note_ranges_cover_the_complete_valid_segment() {
        assert_eq!(note_ranges(&[5, 10], 13), [0..5, 5..10, 10..13]);
    }

    #[test]
    fn segments_without_transcript_are_skipped() {
        assert!(conditioned_segments(&[], 0, 300).unwrap().is_empty());
        assert_eq!(conditioned_segments(&[word(0)], 0, 300).unwrap().len(), 1);
    }

    #[test]
    fn a_phrase_keeps_more_than_one_fixture_bucket_of_acoustic_context() {
        let words = (0..12)
            .map(|index| TranscriptWord {
                id: format!("unit-{index}"),
                text: "la".into(),
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
        let boundaries = segment_word_boundaries(segment, 0).unwrap();
        assert_eq!(boundaries.iter().sum::<i32>(), words.len() as i32);
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
    fn long_contexts_split_only_between_whole_transcript_words() {
        let words = (0..80)
            .map(|index| TranscriptWord {
                id: format!("word-{index}"),
                text: "la".into(),
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

    #[test]
    fn native_same_pitch_rearticulations_are_not_stitched_away() {
        let mut notes = Vec::new();
        let mut logits = vec![-10.0; PITCH_CLASSES * 2];
        logits[60] = 10.0;
        logits[PITCH_CLASSES + 60] = 10.0;
        append_notes(&mut notes, 0, &[0..400, 400..800], &logits).unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].midi, notes[1].midi);
        assert_eq!(notes[0].end_frame, notes[1].start_frame);
        assert_eq!(notes[0].end_frame, 400);
    }
}
