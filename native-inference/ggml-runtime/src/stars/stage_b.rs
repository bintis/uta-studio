use super::model::{HIDDEN_DIM, Stars};
use super::stage_a::get_f32;
use super::utterance::absolute_position_values;

const VQ_CODES: usize = 48;
const NOTE_BOUNDARY_TEMPERATURE: f32 = 0.2;

#[derive(Debug, Clone, PartialEq)]
pub struct RhythmEncoding {
    /// Frame-major `[frames, 256]` feature consumed by Stage C.
    pub features: Vec<f32>,
    /// Raw, temperature-scaled and clamped note-boundary logits.
    pub note_boundary_logits: Vec<f32>,
    pub frames: usize,
}

impl Stars {
    /// Runs the phoneme- and word-grouped Stage B prosody adaptors and note
    /// boundary head. Grouping, VQ selection, and expansion are discrete host
    /// orchestration; every neural layer executes through upstream GGML.
    pub fn encode_rhythm(
        &self,
        mel_embedding: &[f32],
        utterance_features: &[f32],
        valid_frames: usize,
        frames: usize,
        mel_to_phoneme: &[i64],
        phoneme_count: usize,
        mel_to_word: &[i64],
        word_count: usize,
    ) -> Result<RhythmEncoding, String> {
        validate_stage_b_inputs(
            mel_embedding,
            utterance_features,
            valid_frames,
            frames,
            mel_to_phoneme,
            phoneme_count,
            mel_to_word,
            word_count,
        )?;
        let phoneme_prosody = self.grouped_prosody(
            mel_embedding,
            valid_frames,
            frames,
            mel_to_phoneme,
            phoneme_count,
            "prosody_extractor_ph",
            "l1_ph",
        )?;
        let word_prosody = self.grouped_prosody(
            mel_embedding,
            valid_frames,
            frames,
            mel_to_word,
            word_count,
            "prosody_extractor_word",
            "l1_word",
        )?;
        let features = utterance_features
            .iter()
            .zip(phoneme_prosody.iter())
            .zip(word_prosody.iter())
            .map(|((utterance, phoneme), word)| utterance + phoneme + word)
            .collect::<Vec<_>>();
        let frame_logits = self.project_linear_values(
            &features,
            frames,
            HIDDEN_DIM,
            90,
            "note_frame_predictor.note_head",
        )?;
        let note_boundary_logits = frame_logits
            .chunks_exact(90)
            .map(|row| (row[0] / NOTE_BOUNDARY_TEMPERATURE).clamp(-16.0, 16.0))
            .collect();
        Ok(RhythmEncoding {
            features,
            note_boundary_logits,
            frames,
        })
    }

    pub(super) fn grouped_prosody(
        &self,
        mel_embedding: &[f32],
        valid_frames: usize,
        frames: usize,
        segment_ids: &[i64],
        segment_count: usize,
        adaptor: &str,
        projection: &str,
    ) -> Result<Vec<f32>, String> {
        let frame_features =
            self.encode_local_style_frames(mel_embedding, valid_frames, frames, adaptor)?;
        let grouped =
            group_hidden_by_segments(&frame_features, frames, segment_ids, segment_count)?;
        let encoded =
            self.encode_conv_blocks_values(&grouped, segment_count, &format!("{adaptor}.encoder"))?;
        let codebook = get_f32(
            self.api(),
            self.weight(&format!("{adaptor}.vqvae.embedding"))?,
        )?;
        let quantized = quantize(&encoded, segment_count, &codebook)?;
        let positions = absolute_position_values(segment_count, segment_count);
        let mut positioned = vec![0.0_f32; segment_count * 2 * HIDDEN_DIM];
        for row in 0..segment_count {
            let output = &mut positioned[row * 2 * HIDDEN_DIM..(row + 1) * 2 * HIDDEN_DIM];
            output[..HIDDEN_DIM]
                .copy_from_slice(&quantized[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM]);
            output[HIDDEN_DIM..]
                .copy_from_slice(&positions[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM]);
        }
        let projected = self.project_linear_values(
            &positioned,
            segment_count,
            2 * HIDDEN_DIM,
            HIDDEN_DIM,
            projection,
        )?;
        expand_states(&projected, segment_count, segment_ids, frames)
    }
}

fn validate_stage_b_inputs(
    mel_embedding: &[f32],
    utterance_features: &[f32],
    valid_frames: usize,
    frames: usize,
    mel_to_phoneme: &[i64],
    phoneme_count: usize,
    mel_to_word: &[i64],
    word_count: usize,
) -> Result<(), String> {
    if frames == 0
        || frames % 16 != 0
        || valid_frames == 0
        || valid_frames > frames
        || phoneme_count == 0
        || word_count == 0
        || mel_embedding.len() != frames * HIDDEN_DIM
        || utterance_features.len() != frames * HIDDEN_DIM
        || mel_to_phoneme.len() != frames
        || mel_to_word.len() != frames
        || mel_embedding.iter().any(|value| !value.is_finite())
        || utterance_features.iter().any(|value| !value.is_finite())
        || !valid_segment_ids(mel_to_phoneme, phoneme_count)
        || !valid_segment_ids(mel_to_word, word_count)
    {
        Err("STARS Stage B input shape is invalid".to_string())
    } else {
        Ok(())
    }
}

fn valid_segment_ids(segment_ids: &[i64], segment_count: usize) -> bool {
    segment_ids
        .iter()
        .all(|value| *value >= 0 && (*value as usize) <= segment_count)
}

fn group_hidden_by_segments(
    features: &[f32],
    frames: usize,
    segment_ids: &[i64],
    segment_count: usize,
) -> Result<Vec<f32>, String> {
    if features.len() != frames * HIDDEN_DIM
        || segment_ids.len() != frames
        || segment_count == 0
        || !valid_segment_ids(segment_ids, segment_count)
    {
        return Err("STARS grouped-prosody input shape is invalid".to_string());
    }
    let mut grouped = vec![0.0_f32; segment_count * HIDDEN_DIM];
    let mut counts = vec![0_usize; segment_count];
    for (frame, segment) in segment_ids.iter().copied().enumerate() {
        if segment <= 0 {
            continue;
        }
        let index = segment as usize - 1;
        counts[index] += 1;
        for channel in 0..HIDDEN_DIM {
            grouped[index * HIDDEN_DIM + channel] += features[frame * HIDDEN_DIM + channel];
        }
    }
    for (index, count) in counts.into_iter().enumerate() {
        let denominator = count.max(1) as f32;
        for value in &mut grouped[index * HIDDEN_DIM..(index + 1) * HIDDEN_DIM] {
            *value /= denominator;
        }
    }
    Ok(grouped)
}

fn quantize(values: &[f32], rows: usize, codebook: &[f32]) -> Result<Vec<f32>, String> {
    if values.len() != rows * HIDDEN_DIM || codebook.len() != VQ_CODES * HIDDEN_DIM {
        return Err("STARS VQ input shape is invalid".to_string());
    }
    let mut output = vec![0.0_f32; values.len()];
    for row in 0..rows {
        let value = &values[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM];
        let mut best_code = 0;
        let mut best_distance = f32::INFINITY;
        for code in 0..VQ_CODES {
            let candidate = &codebook[code * HIDDEN_DIM..(code + 1) * HIDDEN_DIM];
            let distance = value
                .iter()
                .zip(candidate)
                .map(|(left, right)| (left - right).powi(2))
                .sum::<f32>();
            if distance < best_distance {
                best_distance = distance;
                best_code = code;
            }
        }
        output[row * HIDDEN_DIM..(row + 1) * HIDDEN_DIM]
            .copy_from_slice(&codebook[best_code * HIDDEN_DIM..(best_code + 1) * HIDDEN_DIM]);
    }
    Ok(output)
}

fn expand_states(
    grouped: &[f32],
    rows: usize,
    segment_ids: &[i64],
    frames: usize,
) -> Result<Vec<f32>, String> {
    if grouped.len() != rows * HIDDEN_DIM
        || segment_ids.len() != frames
        || !valid_segment_ids(segment_ids, rows)
    {
        return Err("STARS expanded-prosody input shape is invalid".to_string());
    }
    let mut output = vec![0.0_f32; frames * HIDDEN_DIM];
    for (frame, segment) in segment_ids.iter().copied().enumerate() {
        if segment <= 0 {
            continue;
        }
        let index = segment as usize - 1;
        output[frame * HIDDEN_DIM..(frame + 1) * HIDDEN_DIM]
            .copy_from_slice(&grouped[index * HIDDEN_DIM..(index + 1) * HIDDEN_DIM]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct MelFixture {
        mel: Vec<Vec<f32>>,
    }

    #[derive(Deserialize)]
    struct StageFixture {
        mel_embed_a0: Vec<Vec<f32>>,
        feat_a1: Vec<Vec<f32>>,
        prosody_ph_mel: Vec<Vec<f32>>,
        prosody_word_mel: Vec<Vec<f32>>,
        feat_b0: Vec<Vec<f32>>,
        b1_note_bd_logits: Vec<f32>,
        mel2ph: Vec<i64>,
        mel2word: Vec<i64>,
        num_ph: usize,
        num_word: usize,
    }

    #[test]
    fn grouping_uses_one_indexed_segments_and_ignores_zero() {
        let mut values = vec![0.0_f32; 4 * HIDDEN_DIM];
        values[0] = 3.0;
        values[HIDDEN_DIM] = 2.0;
        values[2 * HIDDEN_DIM] = 6.0;
        values[3 * HIDDEN_DIM] = 99.0;
        let grouped = group_hidden_by_segments(&values, 4, &[1, 2, 2, 0], 2).unwrap();
        assert_eq!(grouped[0], 3.0);
        assert_eq!(grouped[HIDDEN_DIM], 4.0);
    }

    #[test]
    fn expansion_uses_zero_as_padding() {
        let mut grouped = vec![0.0_f32; 2 * HIDDEN_DIM];
        grouped[0] = 7.0;
        grouped[HIDDEN_DIM] = 8.0;
        let expanded = expand_states(&grouped, 2, &[0, 2, 1], 3).unwrap();
        assert_eq!(expanded[0], 0.0);
        assert_eq!(expanded[HIDDEN_DIM], 8.0);
        assert_eq!(expanded[2 * HIDDEN_DIM], 7.0);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, GGUF, and PyTorch fixture"]
    fn stage_b_matches_pytorch_on_explicit_device() {
        let runtime_path = PathBuf::from(
            std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR").expect("set UTA_TEST_GGML_RUNTIME_DIR"),
        );
        let model_path = PathBuf::from(
            std::env::var_os("UTA_TEST_STARS_GGUF").expect("set UTA_TEST_STARS_GGUF"),
        );
        let fixture_path = PathBuf::from(
            std::env::var_os("UTA_TEST_STARS_STAGE_A_FIXTURE")
                .expect("set UTA_TEST_STARS_STAGE_A_FIXTURE"),
        );
        let requested_kind = std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND to cpu or integrated_gpu");
        let description_filter = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => crate::DeviceKind::Cpu,
            "integrated_gpu" => crate::DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let mel_fixture: MelFixture = serde_json::from_str(include_str!(
            "../../fixtures/stars/shared-singing-frontend-upstream.json"
        ))
        .unwrap();
        let expected: StageFixture = serde_json::from_slice(
            &std::fs::read(fixture_path).expect("read STARS Stage B fixture"),
        )
        .unwrap();
        let frames = expected.mel_embed_a0.len();
        let valid_frames = mel_fixture.mel.len();
        let mel_embedding = expected
            .mel_embed_a0
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let utterance_features = expected
            .feat_a1
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description_filter)
            })
            .expect("requested GGML test device is unavailable");
        let model = Stars::load(runtime, &device, &model_path).unwrap();
        let phoneme_prosody = model
            .grouped_prosody(
                &mel_embedding,
                valid_frames,
                frames,
                &expected.mel2ph,
                expected.num_ph,
                "prosody_extractor_ph",
                "l1_ph",
            )
            .unwrap();
        let word_prosody = model
            .grouped_prosody(
                &mel_embedding,
                valid_frames,
                frames,
                &expected.mel2word,
                expected.num_word,
                "prosody_extractor_word",
                "l1_word",
            )
            .unwrap();
        let actual = model
            .encode_rhythm(
                &mel_embedding,
                &utterance_features,
                valid_frames,
                frames,
                &expected.mel2ph,
                expected.num_ph,
                &expected.mel2word,
                expected.num_word,
            )
            .unwrap();
        let expected_phoneme = expected
            .prosody_ph_mel
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_word = expected
            .prosody_word_mel
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_features = expected.feat_b0.into_iter().flatten().collect::<Vec<_>>();
        let phoneme_error = max_abs_difference(&phoneme_prosody, &expected_phoneme);
        let word_error = max_abs_difference(&word_prosody, &expected_word);
        let feature_error = max_abs_difference(&actual.features, &expected_features);
        let boundary_error =
            max_abs_difference(&actual.note_boundary_logits, &expected.b1_note_bd_logits);
        eprintln!(
            "STARS Stage B on {}: phoneme {phoneme_error:.8}, word {word_error:.8}, features {feature_error:.8}, boundaries {boundary_error:.8}",
            device.description
        );
        let (prosody_tolerance, feature_tolerance, boundary_tolerance) = match expected_kind {
            crate::DeviceKind::Cpu => (1.0e-4, 1.0e-4, 1.0e-4),
            crate::DeviceKind::IntegratedGpu => (1.0e-4, 1.0e-4, 2.0e-3),
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(phoneme_error < prosody_tolerance);
        assert!(word_error < prosody_tolerance);
        assert!(feature_error < feature_tolerance);
        assert!(boundary_error < boundary_tolerance);
    }

    fn max_abs_difference(actual: &[f32], expected: &[f32]) -> f32 {
        assert_eq!(actual.len(), expected.len());
        actual
            .iter()
            .zip(expected)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0, f32::max)
    }
}
