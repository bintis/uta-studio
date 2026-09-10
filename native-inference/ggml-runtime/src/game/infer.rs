use std::path::Path;

use super::{
    Game, GameRng, MelConfig, MelExtractor, RandomSource, boundaries_to_regions,
    d3pm_time_schedule, decode_gaussian_blurred_probs, decode_soft_boundaries,
    remove_mutable_boundaries,
};
use crate::wav::read_f32_wav;

#[derive(Debug, Clone, PartialEq)]
pub struct GameInferParams {
    pub language: i32,
    pub d3pm_steps: usize,
    pub boundary_threshold: f32,
    pub boundary_radius: usize,
    pub note_threshold: f32,
    pub seed: u64,
    pub known_boundaries: Vec<usize>,
}

impl Default for GameInferParams {
    fn default() -> Self {
        Self {
            language: 0,
            d3pm_steps: 8,
            boundary_threshold: 0.2,
            boundary_radius: 2,
            note_threshold: 0.2,
            seed: 0,
            known_boundaries: Vec::new(),
        }
    }
}

const SAMPLE_RATE: u64 = 44_100;
const SECOND_MICROS: u64 = 1_000_000;
const FRAME_MICROS: u64 = 10_000;
const SEAM_OWNED_MICROS: u64 = 1_000_000;
const SEAM_BOUNDARY_TOLERANCE_MICROS: u64 = 50_000;
const SEAM_MERGE_MAX_SEMITONES: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameNote {
    pub offset_micros: u64,
    pub duration_micros: u64,
    pub pitch_midi: f32,
    pub voiced: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameInferOutput {
    pub notes: Vec<GameNote>,
    pub boundaries: Vec<u8>,
    pub num_frames: usize,
}

impl Game {
    pub fn process_wav(
        &self,
        path: &Path,
        params: &GameInferParams,
    ) -> Result<GameInferOutput, String> {
        self.process_wav_with_progress(path, params, &mut |_, _| {})
    }

    pub fn process_wav_with_progress(
        &self,
        path: &Path,
        params: &GameInferParams,
        report: &mut dyn FnMut(u64, u64),
    ) -> Result<GameInferOutput, String> {
        const CHUNK_SAMPLES: usize = 44_100 * 30;
        const OVERLAP_SAMPLES: usize = 44_100 * 2;
        const STEP_SAMPLES: usize = CHUNK_SAMPLES - OVERLAP_SAMPLES;

        let mel_extractor = MelExtractor::new(MelConfig::default())?;
        let sample_rate = u32::try_from(mel_extractor.config().sample_rate)
            .map_err(|_| "GAME sample rate exceeds u32".to_string())?;
        let audio = read_f32_wav(path, sample_rate, 1)?;
        if audio.is_empty() || audio.iter().any(|sample| !sample.is_finite()) {
            return Err("GAME input audio is empty or non-finite".to_string());
        }
        let chunk_count = audio.len().saturating_sub(1) / STEP_SAMPLES + 1;
        let total_frames = audio.len().saturating_add(440) / 441;
        let mut notes = Vec::new();
        let mut single_chunk_boundaries = Vec::new();
        for chunk_index in 0..chunk_count {
            let offset = chunk_index * STEP_SAMPLES;
            let valid = (audio.len() - offset).min(CHUNK_SAMPLES);
            let frames = mel_extractor.frame_count(valid);
            let mel = mel_extractor.extract(&audio[offset..offset + valid])?;
            let chunk_offset_frame = offset / 441;
            let chunk_end_frame = chunk_offset_frame.saturating_add(frames);
            let mut chunk_params = params.clone();
            chunk_params.known_boundaries = params
                .known_boundaries
                .iter()
                .copied()
                .filter(|frame| *frame >= chunk_offset_frame && *frame < chunk_end_frame)
                .map(|frame| frame - chunk_offset_frame)
                .collect();
            let result = self.infer_mel(&mel, frames, &chunk_params)?;
            if chunk_count == 1 {
                single_chunk_boundaries = result.boundaries;
            }

            let offset_micros = samples_to_micros(offset)?;
            let chunk_duration_micros = samples_to_micros(valid)?;
            let left_cut = if chunk_index == 0 {
                0
            } else {
                SEAM_OWNED_MICROS
            };
            let right_cut = if chunk_index + 1 == chunk_count {
                chunk_duration_micros
            } else {
                chunk_duration_micros.saturating_sub(SEAM_OWNED_MICROS)
            };
            let seam_time = (chunk_index > 0).then_some(
                offset_micros
                    .checked_add(SEAM_OWNED_MICROS)
                    .ok_or_else(|| "GAME chunk seam overflows".to_string())?,
            );
            for mut note in result.notes {
                let midpoint = note
                    .offset_micros
                    .saturating_add(note.duration_micros / 2);
                if midpoint < left_cut || midpoint >= right_cut {
                    continue;
                }
                note.offset_micros = note
                    .offset_micros
                    .checked_add(offset_micros)
                    .ok_or_else(|| "GAME chunk offset overflows".to_string())?;
                append_stitched_note(&mut notes, note, seam_time)?;
            }
            report((chunk_index + 1) as u64, chunk_count as u64);
        }
        if notes.is_empty() {
            return Err("GAME produced no note evidence".to_string());
        }
        Ok(GameInferOutput {
            notes,
            boundaries: single_chunk_boundaries,
            num_frames: total_frames,
        })
    }

    /// Runs GAME's encoder, eight-step D3PM segmenter, region estimator, and
    /// note decoder on a frame-major `[frames, 80]` log-mel spectrogram.
    pub fn infer_mel(
        &self,
        mel: &[f32],
        frames: usize,
        params: &GameInferParams,
    ) -> Result<GameInferOutput, String> {
        let mut random = GameRng::new(params.seed);
        self.infer_mel_with_rng(mel, frames, params, &mut random)
    }

    pub fn infer_mel_with_rng(
        &self,
        mel: &[f32],
        frames: usize,
        params: &GameInferParams,
        random: &mut impl RandomSource,
    ) -> Result<GameInferOutput, String> {
        validate_params(self, frames, params)?;
        let encoded = self.encode_mel(mel, frames)?;
        let mut known = vec![0_u8; frames];
        for boundary in params.known_boundaries.iter().copied() {
            known[boundary] = 1;
        }
        let mask = vec![1_u8; frames];
        let mut boundaries = known.clone();
        let mut noise_regions = vec![0_i32; frames];

        for step in 0..params.d3pm_steps {
            let time = if params.d3pm_steps == 1 {
                0.0
            } else {
                step as f32 / (params.d3pm_steps - 1) as f32
            };
            boundaries =
                remove_mutable_boundaries(&boundaries, &known, d3pm_time_schedule(time), random)?;
            fill_noise_regions(
                &boundaries,
                self.config().region_cycle_length,
                &mut noise_regions,
            )?;
            let logits = self.segmenter_logits(
                &encoded.segmenter_embeddings,
                &noise_regions,
                time,
                params.language,
            )?;
            let probabilities = logits.iter().copied().map(sigmoid).collect::<Vec<_>>();
            boundaries = decode_soft_boundaries(
                &probabilities,
                Some(&known),
                Some(&mask),
                params.boundary_threshold,
                params.boundary_radius,
            )?;
        }

        let regions = boundaries_to_regions(&boundaries, Some(&mask))?;
        let region_count = regions
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .try_into()
            .map_err(|_| "GAME region count is invalid".to_string())?;
        if region_count == 0 {
            return Ok(GameInferOutput {
                notes: Vec::new(),
                boundaries,
                num_frames: frames,
            });
        }
        let estimator = self.estimate_pitch(&encoded.estimator_embeddings, &regions)?;
        let probabilities = estimator
            .pool_logits
            .iter()
            .copied()
            .map(sigmoid)
            .collect::<Vec<_>>();
        let decoded = decode_gaussian_blurred_probs(
            &probabilities,
            region_count,
            estimator.bins,
            self.config().midi_minimum,
            self.config().midi_maximum,
            self.config().midi_deviation * 3.0,
            params.note_threshold,
        )?;
        let durations = count_region_durations(&regions, region_count)?;
        let mut offset_micros = 0_u64;
        let mut notes = Vec::with_capacity(region_count);
        for note in 0..region_count {
            let duration_micros = (durations[note + 1] as u64)
                .checked_mul(FRAME_MICROS)
                .ok_or_else(|| "GAME note duration overflows".to_string())?;
            if duration_micros == 0 {
                return Err("GAME produced a zero-duration note region".to_string());
            }
            notes.push(GameNote {
                offset_micros,
                duration_micros,
                pitch_midi: decoded.values[note],
                voiced: decoded.presence[note] != 0,
            });
            offset_micros = offset_micros
                .checked_add(duration_micros)
                .ok_or_else(|| "GAME note timeline overflows".to_string())?;
        }
        Ok(GameInferOutput {
            notes,
            boundaries,
            num_frames: frames,
        })
    }
}

fn append_stitched_note(
    notes: &mut Vec<GameNote>,
    mut note: GameNote,
    seam_time: Option<u64>,
) -> Result<(), String> {
    if let Some(previous) = notes.last_mut() {
        let previous_end = previous
            .offset_micros
            .checked_add(previous.duration_micros)
            .ok_or_else(|| "GAME stitched note end overflows".to_string())?;
        let note_end = note
            .offset_micros
            .checked_add(note.duration_micros)
            .ok_or_else(|| "GAME stitched note end overflows".to_string())?;
        if note.offset_micros < previous.offset_micros {
            if let Some(seam) = seam_time
                && previous.offset_micros < seam
                && note_end > seam
            {
                let previous_owned_end = previous_end.min(seam);
                if previous_owned_end <= previous.offset_micros {
                    return Err(
                        "GAME chunk stitching produced an empty left seam interval".to_string()
                    );
                }
                previous.duration_micros = previous_owned_end - previous.offset_micros;
                note.offset_micros = seam;
                note.duration_micros = note_end - seam;
                notes.push(note);
                return Ok(());
            }
            return Err("GAME chunk stitching produced an unordered note".to_string());
        }
        if note.offset_micros < previous_end {
            let seam_continuation = seam_time.is_some_and(|seam| {
                previous_end >= seam.saturating_sub(SEAM_BOUNDARY_TOLERANCE_MICROS)
                    && note.offset_micros <= seam.saturating_add(SEAM_BOUNDARY_TOLERANCE_MICROS)
                    && (previous.pitch_midi - note.pitch_midi).abs() <= SEAM_MERGE_MAX_SEMITONES
            });
            if seam_continuation {
                let total_weight = previous.duration_micros.checked_add(note.duration_micros).ok_or_else(
                    || "GAME chunk stitching produced invalid seam weights".to_string(),
                )?;
                if total_weight == 0 {
                    return Err("GAME chunk stitching produced invalid seam weights".to_string());
                }
                previous.pitch_midi = ((f64::from(previous.pitch_midi)
                    * previous.duration_micros as f64
                    + f64::from(note.pitch_midi) * note.duration_micros as f64)
                    / total_weight as f64) as f32;
                previous.duration_micros = previous_end.max(note_end) - previous.offset_micros;
                previous.voiced = previous.voiced && note.voiced;
                return Ok(());
            }
            if note.offset_micros == previous.offset_micros
                && let Some(seam) = seam_time
            {
                let split = previous_end.min(seam);
                if split > previous.offset_micros && note_end > split {
                    previous.duration_micros = split - previous.offset_micros;
                    note.offset_micros = split;
                    note.duration_micros = note_end - split;
                    notes.push(note);
                    return Ok(());
                }
            }
            let clipped_duration = note.offset_micros - previous.offset_micros;
            if clipped_duration == 0 {
                return Err(
                    "GAME chunk stitching could not resolve a monophonic overlap".to_string(),
                );
            }
            previous.duration_micros = clipped_duration;
        }
    }
    notes.push(note);
    Ok(())
}

fn samples_to_micros(samples: usize) -> Result<u64, String> {
    (samples as u64)
        .checked_mul(SECOND_MICROS)
        .ok_or_else(|| "GAME sample time overflows".to_string())
        .map(|value| value / SAMPLE_RATE)
}

fn validate_params(game: &Game, frames: usize, params: &GameInferParams) -> Result<(), String> {
    if frames == 0 {
        return Err("GAME inference requires at least one mel frame".to_string());
    }
    if params.d3pm_steps == 0 {
        return Err("GAME inference requires at least one D3PM step".to_string());
    }
    if params.language < 0 || params.language as usize >= game.config().language_count {
        return Err("GAME language id is outside the model vocabulary".to_string());
    }
    if !params.boundary_threshold.is_finite() || !params.note_threshold.is_finite() {
        return Err("GAME inference thresholds must be finite".to_string());
    }
    if let Some(boundary) = params
        .known_boundaries
        .iter()
        .copied()
        .find(|boundary| *boundary >= frames)
    {
        return Err(format!(
            "GAME known boundary {boundary} is outside {frames} frames"
        ));
    }
    Ok(())
}

fn fill_noise_regions(
    boundaries: &[u8],
    region_cycle_length: usize,
    output: &mut [i32],
) -> Result<(), String> {
    if boundaries.len() != output.len() || region_cycle_length == 0 {
        return Err("GAME noise-region dimensions are invalid".to_string());
    }
    let cycle = i32::try_from(region_cycle_length)
        .map_err(|_| "GAME region cycle exceeds i32".to_string())?;
    let mut running = 1_i32;
    for (index, value) in output.iter_mut().enumerate() {
        if boundaries[index] != 0 {
            running = running
                .checked_add(1)
                .ok_or_else(|| "GAME region id overflowed".to_string())?;
        }
        *value = running % cycle;
    }
    Ok(())
}

fn count_region_durations(regions: &[i32], count: usize) -> Result<Vec<usize>, String> {
    let mut durations = vec![0_usize; count + 1];
    for region in regions.iter().copied() {
        let region =
            usize::try_from(region).map_err(|_| "GAME region id cannot be negative".to_string())?;
        let duration = durations
            .get_mut(region)
            .ok_or_else(|| "GAME region id exceeds the estimator region count".to_string())?;
        *duration += 1;
    }
    Ok(durations)
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_regions_match_game_cycle_semantics() {
        let mut regions = vec![0; 6];
        fill_noise_regions(&[0, 1, 0, 1, 1, 0], 3, &mut regions).unwrap();
        assert_eq!(regions, vec![1, 2, 2, 0, 1, 1]);
    }

    #[test]
    fn region_durations_include_mask_region_zero() {
        assert_eq!(
            count_region_durations(&[0, 1, 1, 2, 0], 2).unwrap(),
            vec![2, 2, 1]
        );
    }

    #[test]
    fn stitching_merges_matching_pitch_across_seam() {
        let mut notes = vec![GameNote {
            offset_micros: 27_000_000,
            duration_micros: 2_020_000,
            pitch_midi: 60.0,
            voiced: true,
        }];
        append_stitched_note(
            &mut notes,
            GameNote {
                offset_micros: 28_980_000,
                duration_micros: 2_000_000,
                pitch_midi: 60.5,
                voiced: true,
            },
            Some(29_000_000),
        )
        .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].duration_micros, 3_980_000);
        assert!(notes[0].pitch_midi > 60.0 && notes[0].pitch_midi < 60.5);
    }

    #[test]
    fn stitching_clips_nonmatching_monophonic_overlap() {
        let mut notes = vec![GameNote {
            offset_micros: 10_000_000,
            duration_micros: 2_000_000,
            pitch_midi: 60.0,
            voiced: true,
        }];
        append_stitched_note(
            &mut notes,
            GameNote {
                offset_micros: 11_500_000,
                duration_micros: 1_000_000,
                pitch_midi: 64.0,
                voiced: true,
            },
            None,
        )
        .unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].duration_micros, 1_500_000);
        assert_eq!(notes[1].offset_micros, 11_500_000);
    }

    #[test]
    fn sample_times_stay_on_the_canonical_microsecond_grid() {
        assert_eq!(samples_to_micros(44_100).unwrap(), 1_000_000);
        assert_eq!(samples_to_micros(44_100 * 28).unwrap(), 28_000_000);
        assert_eq!(samples_to_micros(44_100 * 336).unwrap(), 336_000_000);
    }
}
