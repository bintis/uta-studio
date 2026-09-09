use std::ops::Range;
use std::path::Path;

use crate::wav::read_f32_wav;

use super::decode::{self, Alignment};
use super::frontend;
use super::model::{MEL_BINS, PITCH_CLASSES, Stars, TECHNIQUE_CLASSES};
use super::stage_d::StyleLogits;
use super::stage_e::aggregate_technique_frames;

pub const FRAME_BUCKET: usize = 256;
pub const NOTE_BUCKET: usize = 32;
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
    /// optional D/E technique path. Neural layers execute on the selected
    /// upstream-GGML device; Rust owns segmentation and discrete decoders.
    pub fn infer_transcript<F, P>(
        &self,
        shared: &SharedInputs,
        words: &[TranscriptWord],
        source_start_micros: u64,
        include_technique: bool,
        mut phonemize: F,
        mut progress: P,
    ) -> Result<StarsResult, String>
    where
        F: FnMut(&[String]) -> Result<PhonemeInput, String>,
        P: FnMut(f32, &str),
    {
        validate_shared_inputs(shared)?;
        let segments = conditioned_segments(words, source_start_micros, shared.frames);
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
            let mel = padded_rows(&shared.mel, shared.frames, MEL_BINS, segment.start);
            let pitch = padded_i32(&shared.pitch_coarse, segment.start);
            let uv = padded_i32(&shared.uv, segment.start);
            let mel_pitch =
                self.encode_mel_with_pitch(&mel, &pitch, &uv, segment.valid, FRAME_BUCKET)?;
            let utterance =
                self.encode_utterance(&mel_pitch.embedded, segment.valid, FRAME_BUCKET)?;
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
            let (mel_to_phoneme, mel_to_word) = padded_alignment(&alignment);
            let rhythm = self.encode_rhythm(
                &mel_pitch.embedded,
                &utterance.features,
                segment.valid,
                FRAME_BUCKET,
                &mel_to_phoneme,
                alignment.phoneme_intervals.len(),
                &mel_to_word,
                alignment.word_intervals.len(),
            )?;
            all_logits[segment.start..segment.start + segment.valid]
                .copy_from_slice(&rhythm.note_boundary_logits[..segment.valid]);
            let regulated =
                decode::regulate_boundaries(&rhythm.note_boundary_logits, 0.8, 17, segment.valid)?;
            let local_boundaries = boundary_indices(&regulated, segment.valid);
            let ranges = note_ranges(&local_boundaries, segment.valid);
            if ranges.len() > NOTE_BUCKET {
                return Err("STARS segment exceeds the note bucket".to_string());
            }
            let mel_to_note = mapping_from_boundaries(&regulated, segment.valid);
            let pitch = self.encode_pitch(
                &mel_pitch.embedded,
                &rhythm.features,
                segment.valid,
                FRAME_BUCKET,
                &mel_to_note,
                ranges.len(),
                &regulated,
            )?;
            append_notes(&mut all_notes, segment.start, &ranges, &pitch.note_logits);

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
        stitch_notes(&mut all_notes);
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
    mel_embedding: &[f32],
    pitch_features: &[f32],
    techniques: &mut Vec<RawTechnique>,
    styles: &mut Vec<GlobalStyle>,
) -> Result<(), String> {
    if alignment.phoneme_intervals.is_empty() {
        return Err("STARS has no phoneme intervals for technique inference".to_string());
    }
    let sentence =
        model.encode_sentence(mel_embedding, pitch_features, segment.valid, FRAME_BUCKET)?;
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

fn frame_to_micros(frame: usize) -> u64 {
    (frame as u128 * frontend::HOP_SIZE as u128 * 1_000_000 / frontend::SAMPLE_RATE as u128) as u64
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
            let start_micros = frame_to_micros(start).saturating_add(source_start_micros);
            let end_micros = frame_to_micros(start + valid).saturating_add(source_start_micros);
            let words = words
                .iter()
                .filter(|word| {
                    word.start_micros < end_micros
                        && word.start_micros.saturating_add(word.duration_micros) > start_micros
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

fn padded_alignment(alignment: &Alignment) -> (Vec<i64>, Vec<i64>) {
    let mut phonemes = alignment.mel_to_phoneme.clone();
    let mut words = alignment.mel_to_word.clone();
    phonemes.resize(FRAME_BUCKET, 0);
    words.resize(FRAME_BUCKET, 0);
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
    let mut mapping = vec![0_i64; FRAME_BUCKET];
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

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditioned_segments_use_the_canonical_source_offset() {
        let words = vec![TranscriptWord {
            id: "word".to_string(),
            text: "你".to_string(),
            start_micros: 2_000_000,
            duration_micros: 200_000,
        }];
        let segments = conditioned_segments(&words, 2_000_000, 300);
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
    fn adjacent_matching_notes_are_stitched() {
        let mut first_logits = vec![0.0_f32; PITCH_CLASSES];
        first_logits[60] = 2.0;
        let mut notes = vec![
            RawNote {
                start_frame: 0,
                end_frame: 256,
                pitch_logits: first_logits.clone(),
                midi: Some(60),
            },
            RawNote {
                start_frame: 256,
                end_frame: 300,
                pitch_logits: first_logits,
                midi: Some(60),
            },
        ];
        stitch_notes(&mut notes);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start_frame, 0);
        assert_eq!(notes[0].end_frame, 300);
        assert!((notes[0].pitch_logits[60] - 2.0).abs() < f32::EPSILON);
    }
}
