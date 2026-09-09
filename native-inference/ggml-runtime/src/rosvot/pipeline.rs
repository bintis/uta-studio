use std::ops::Range;
use std::path::Path;

use super::decode::{aggregate_notes, decode_pitch, regulate_boundaries};
use super::model::{HIDDEN_DIM, MEL_BINS, PITCH_CLASSES, Rosvot};
use crate::stars::frontend;
use crate::wav::read_f32_wav;

pub const FRAME_BUCKET: usize = 256;
pub const NOTE_BUCKET: usize = 32;

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
        mut progress: P,
    ) -> Result<RosvotResult, String>
    where
        P: FnMut(f32, &str),
    {
        validate_shared_inputs(shared)?;
        let segments = conditioned_segments(words, source_start_micros, shared.frames);
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
            let mel = padded_rows(&shared.mel, shared.frames, MEL_BINS, segment.start);
            let pitch = padded_i32(&shared.pitch_coarse, segment.start);
            let uv = padded_i32(&shared.uv, segment.start);
            let reference = segment_word_boundaries(segment, source_start_micros)?;
            let conditioning = self.encode_conditioning(
                &mel,
                &pitch,
                &uv,
                &reference,
                segment.valid,
                FRAME_BUCKET,
            )?;
            let features =
                self.encode_backbone(&conditioning.conditioned, segment.valid, FRAME_BUCKET)?;
            let frame = self.encode_frame_heads(&features, FRAME_BUCKET)?;
            all_logits[segment.start..segment.start + segment.valid]
                .copy_from_slice(&frame.boundary_logits[..segment.valid]);
            let regulated = regulate_boundaries(
                &frame.boundary_logits,
                0.85,
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
            if aggregated.count > NOTE_BUCKET {
                return Err("ROSVOT segment exceeds the pinned note bucket".to_string());
            }
            let pitch = self.encode_pitch_head(&aggregated.features, aggregated.count)?;
            let local_boundaries = boundary_indices(&regulated, segment.valid);
            let ranges = note_ranges(&local_boundaries, segment.valid);
            append_notes(&mut all_notes, segment.start, &ranges, &pitch.logits)?;
        }
        stitch_notes(&mut all_notes);
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
) -> Vec<Segment> {
    (0..frames)
        .step_by(FRAME_BUCKET)
        .filter_map(|start| {
            let valid = (frames - start).min(FRAME_BUCKET);
            let segment_start = frame_to_micros(start).saturating_add(source_start_micros);
            let segment_end = frame_to_micros(start + valid).saturating_add(source_start_micros);
            let words = words
                .iter()
                .filter(|word| {
                    word.start_micros < segment_end
                        && word.start_micros.saturating_add(word.duration_micros) > segment_start
                })
                .cloned()
                .collect::<Vec<_>>();
            (!words.is_empty()).then_some(Segment {
                start,
                valid,
                words,
            })
        })
        .collect()
}

/// Returns one at every word start after the first word in this segment.
fn segment_word_boundaries(
    segment: &Segment,
    source_start_micros: u64,
) -> Result<Vec<i32>, String> {
    let mut boundaries = vec![0_i32; FRAME_BUCKET];
    let timeline_start = frame_to_micros(segment.start).saturating_add(source_start_micros);
    for word in segment.words.iter().skip(1) {
        let local = word.start_micros.saturating_sub(timeline_start);
        let frame = canonical_to_frame(local)?.min(segment.valid.saturating_sub(1));
        if frame > 0 {
            boundaries[frame] = 1;
        }
    }
    Ok(boundaries)
}

fn padded_rows(values: &[f32], frames: usize, width: usize, start: usize) -> Vec<f32> {
    let mut result = vec![0.0_f32; FRAME_BUCKET * width];
    let count = frames.saturating_sub(start).min(FRAME_BUCKET);
    result[..count * width].copy_from_slice(&values[start * width..(start + count) * width]);
    result
}

fn padded_i32(values: &[i32], start: usize) -> Vec<i32> {
    let mut result = vec![0_i32; FRAME_BUCKET];
    let count = values.len().saturating_sub(start).min(FRAME_BUCKET);
    result[..count].copy_from_slice(&values[start..start + count]);
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

fn stitch_notes(notes: &mut Vec<RawNote>) {
    let mut stitched: Vec<RawNote> = Vec::with_capacity(notes.len());
    for note in notes.drain(..) {
        if let Some(previous) = stitched.last_mut()
            && previous.end_frame == note.start_frame
            && previous.midi.is_some()
            && previous.midi == note.midi
        {
            let previous_frames = previous.end_frame - previous.start_frame;
            let note_frames = note.end_frame - note.start_frame;
            let total_frames = previous_frames + note_frames;
            previous.end_frame = note.end_frame;
            for (left, right) in previous.pitch_logits.iter_mut().zip(note.pitch_logits) {
                *left = (*left * previous_frames as f32 + right * note_frames as f32)
                    / total_frames as f32;
            }
        } else {
            stitched.push(note);
        }
    }
    *notes = stitched;
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
        assert!(conditioned_segments(&[], 0, 300).is_empty());
        assert_eq!(conditioned_segments(&[word(0)], 0, 300).len(), 1);
    }
}
