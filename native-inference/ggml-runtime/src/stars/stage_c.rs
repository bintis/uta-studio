use super::model::{HIDDEN_DIM, PITCH_CLASSES, Stars};

const ATTENTION_HEADS: usize = 4;
const PITCH_TEMPERATURE: f32 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct PitchEncoding {
    /// Frame-major `[frames, 256]` feature consumed by optional style stages.
    pub features: Vec<f32>,
    /// Note-major `[note_count, 89]` temperature-scaled pitch logits.
    pub note_logits: Vec<f32>,
    pub note_count: usize,
    pub frames: usize,
}

impl Stars {
    /// Runs the note-grouped Stage C prosody adaptor and pitch decoder.
    ///
    /// The grouped adaptor deliberately retains the pinned model's mixed
    /// conventions: segment zero is omitted by VQ grouping, while the pitch
    /// attention aggregator includes segment zero as the first note.
    pub fn encode_pitch(
        &self,
        mel_embedding: &[f32],
        rhythm_features: &[f32],
        valid_frames: usize,
        frames: usize,
        mel_to_note: &[i64],
        note_count: usize,
        note_boundaries: &[i64],
    ) -> Result<PitchEncoding, String> {
        validate_stage_c_inputs(
            mel_embedding,
            rhythm_features,
            valid_frames,
            frames,
            mel_to_note,
            note_count,
            note_boundaries,
        )?;
        let note_prosody = self.grouped_prosody(
            mel_embedding,
            valid_frames,
            frames,
            mel_to_note,
            note_count,
            "prosody_extractor_note",
            "l1_note",
        )?;
        let features = rhythm_features
            .iter()
            .zip(note_prosody.iter())
            .map(|(rhythm, note)| rhythm + note)
            .collect::<Vec<_>>();
        let attention_logits = self.project_linear_values(
            &features,
            frames,
            HIDDEN_DIM,
            ATTENTION_HEADS,
            "pitch_decoder.multihead_dot_attn",
        )?;
        // PitchDecoder re-derives a zero-indexed note map from the boundary
        // sequence instead of reusing get_prosody_note's 1-indexed map.
        let pitch_mapping = mapping_from_boundaries(note_boundaries);
        let aggregated = aggregate_notes(
            &features,
            &attention_logits,
            frames,
            &pitch_mapping,
            note_count,
        )?;
        let post = self.encode_conv_blocks_values(&aggregated, note_count, "pitch_decoder.post")?;
        let mut note_logits = self.project_linear_values(
            &post,
            note_count,
            HIDDEN_DIM,
            PITCH_CLASSES,
            "pitch_decoder.pitch_out",
        )?;
        note_logits
            .iter_mut()
            .for_each(|value| *value /= PITCH_TEMPERATURE);
        Ok(PitchEncoding {
            features,
            note_logits,
            note_count,
            frames,
        })
    }
}

fn validate_stage_c_inputs(
    mel_embedding: &[f32],
    rhythm_features: &[f32],
    valid_frames: usize,
    frames: usize,
    mel_to_note: &[i64],
    note_count: usize,
    note_boundaries: &[i64],
) -> Result<(), String> {
    if frames == 0
        || frames % 16 != 0
        || valid_frames == 0
        || valid_frames > frames
        || note_count == 0
        || mel_embedding.len() != frames * HIDDEN_DIM
        || rhythm_features.len() != frames * HIDDEN_DIM
        || mel_to_note.len() != frames
        || note_boundaries.len() != frames
        || mel_embedding.iter().any(|value| !value.is_finite())
        || rhythm_features.iter().any(|value| !value.is_finite())
        || mel_to_note
            .iter()
            .any(|value| *value < 0 || *value as usize > note_count)
        || note_boundaries.iter().any(|value| !matches!(value, 0 | 1))
        || note_boundaries[..valid_frames]
            .iter()
            .map(|value| *value as usize)
            .sum::<usize>()
            + 1
            != note_count
    {
        Err("STARS Stage C input shape is invalid".to_string())
    } else {
        Ok(())
    }
}

fn mapping_from_boundaries(boundaries: &[i64]) -> Vec<i64> {
    let mut note = 0_i64;
    boundaries
        .iter()
        .map(|boundary| {
            note += *boundary;
            note
        })
        .collect()
}

fn aggregate_notes(
    features: &[f32],
    attention_logits: &[f32],
    frames: usize,
    mel_to_note: &[i64],
    note_count: usize,
) -> Result<Vec<f32>, String> {
    if features.len() != frames * HIDDEN_DIM
        || attention_logits.len() != frames * ATTENTION_HEADS
        || mel_to_note.len() != frames
        || mel_to_note
            .iter()
            .any(|value| *value < 0 || *value as usize >= note_count)
    {
        return Err("STARS pitch-aggregation input shape is invalid".to_string());
    }
    let mut output = vec![0.0_f32; note_count * HIDDEN_DIM];
    let mut denominator = vec![0.0_f32; note_count];
    for frame in 0..frames {
        let note = mel_to_note[frame] as usize;
        let attention = attention_logits[frame * ATTENTION_HEADS..(frame + 1) * ATTENTION_HEADS]
            .iter()
            .map(|value| sigmoid(*value))
            .sum::<f32>()
            / ATTENTION_HEADS as f32;
        denominator[note] += attention;
        for channel in 0..HIDDEN_DIM {
            output[note * HIDDEN_DIM + channel] +=
                features[frame * HIDDEN_DIM + channel] * attention;
        }
    }
    for note in 0..note_count {
        let divisor = denominator[note] + 1.0e-5;
        for value in &mut output[note * HIDDEN_DIM..(note + 1) * HIDDEN_DIM] {
            *value /= divisor;
        }
    }
    Ok(output)
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
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
        feat_b0: Vec<Vec<f32>>,
        prosody_note_mel: Vec<Vec<f32>>,
        feat_c0: Vec<Vec<f32>>,
        note_bd_arr: Vec<i64>,
        mel2note: Vec<i64>,
        num_note: usize,
    }

    #[derive(Deserialize)]
    struct StageCInferenceFixture {
        note_logits: Vec<Vec<f32>>,
    }

    #[test]
    fn attention_aggregation_includes_zero_indexed_first_note() {
        let mut features = vec![0.0_f32; 3 * HIDDEN_DIM];
        features[0] = 2.0;
        features[HIDDEN_DIM] = 4.0;
        features[2 * HIDDEN_DIM] = 9.0;
        let logits = vec![0.0_f32; 3 * ATTENTION_HEADS];
        let actual = aggregate_notes(&features, &logits, 3, &[0, 0, 1], 2).unwrap();
        assert!((actual[0] - 3.0).abs() < 1.0e-4);
        assert!((actual[HIDDEN_DIM] - 9.0).abs() < 2.0e-4);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, GGUF, and PyTorch fixture"]
    fn stage_c_matches_pytorch_on_explicit_device() {
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
            &std::fs::read(fixture_path).expect("read STARS Stage C fixture"),
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
        let rhythm_features = expected
            .feat_b0
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
        let note_prosody = model
            .grouped_prosody(
                &mel_embedding,
                valid_frames,
                frames,
                &expected.mel2note,
                expected.num_note,
                "prosody_extractor_note",
                "l1_note",
            )
            .unwrap();
        let actual = model
            .encode_pitch(
                &mel_embedding,
                &rhythm_features,
                valid_frames,
                frames,
                &expected.mel2note,
                expected.num_note,
                &expected.note_bd_arr,
            )
            .unwrap();
        let expected_prosody = expected
            .prosody_note_mel
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let expected_features = expected.feat_c0.into_iter().flatten().collect::<Vec<_>>();
        // The historical debug fixture's pitch logits do not match the pinned
        // checkpoint's official train=false path. Compare the pitch head to a
        // train=false oracle generated from that checkpoint instead.
        let inference: StageCInferenceFixture = serde_json::from_str(include_str!(
            "../../fixtures/stars/stars-stage-c-inference-logits.json"
        ))
        .expect("valid STARS Stage C inference fixture");
        let expected_logits = inference
            .note_logits
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let prosody_error = max_abs_difference(&note_prosody, &expected_prosody);
        let feature_error = max_abs_difference(&actual.features, &expected_features);
        let logits_error = max_abs_difference(&actual.note_logits, &expected_logits);
        eprintln!(
            "STARS Stage C on {}: prosody {prosody_error:.8}, features {feature_error:.8}, logits {logits_error:.8}",
            device.description
        );
        for (actual_row, expected_row) in actual
            .note_logits
            .chunks_exact(PITCH_CLASSES)
            .zip(expected_logits.chunks_exact(PITCH_CLASSES))
        {
            assert_eq!(argmax(actual_row), argmax(expected_row));
        }
        let logits_tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 1.0e-3,
            crate::DeviceKind::IntegratedGpu => 3.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(prosody_error < 5.0e-3);
        assert!(feature_error < 5.0e-3);
        assert!(logits_error < logits_tolerance);
    }

    fn argmax(values: &[f32]) -> usize {
        values
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .expect("non-empty STARS note-logit row")
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
