use super::model::{HIDDEN_DIM, Stars};

const TECHNIQUE_ATTENTION_HEADS: usize = 4;
const SENTENCE_TOKENS: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct StyleLogits {
    pub technique_group: Vec<f32>,
    pub language: Vec<f32>,
    pub gender: Vec<f32>,
    pub emotion: Vec<f32>,
    pub method: Vec<f32>,
    pub pace: Vec<f32>,
    pub range: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SentenceEncoding {
    /// Frame-major `[frames, 256]` features after sentence prosody injection.
    pub features: Vec<f32>,
    /// Frame-major `[frames, 256]` features weighted for phoneme aggregation.
    pub weighted_features: Vec<f32>,
    /// One scalar technique-attention value per padded frame.
    pub attention: Vec<f32>,
    pub styles: StyleLogits,
    pub frames: usize,
}

impl Stars {
    /// Runs sentence prosody, CLS-token style prediction, and technique attention.
    ///
    /// Neural operations execute through the selected upstream-GGML backend.
    /// Rust owns the pinned model's plain padded-frame mean, sigmoid reduction,
    /// and feature weighting.
    pub fn encode_sentence(
        &self,
        mel_embedding: &[f32],
        pitch_features: &[f32],
        valid_frames: usize,
        frames: usize,
    ) -> Result<SentenceEncoding, String> {
        validate_stage_d_inputs(mel_embedding, pitch_features, valid_frames, frames)?;
        let sentence_prosody =
            self.encode_sentence_style_frames(mel_embedding, valid_frames, frames)?;
        let mean = mean_frames(&sentence_prosody, frames)?;
        let features = add_frame_bias(pitch_features, &mean, frames)?;
        let tokens = self.align_sentence_values(&features, valid_frames, frames)?;
        if tokens.len() != SENTENCE_TOKENS * HIDDEN_DIM {
            return Err("STARS sentence token output shape is invalid".to_string());
        }
        let styles = StyleLogits {
            technique_group: self.style_logits(&tokens, 0, 10, "tech")?,
            language: self.style_logits(&tokens, 1, 9, "lan")?,
            gender: self.style_logits(&tokens, 2, 2, "gen")?,
            emotion: self.style_logits(&tokens, 3, 4, "emo")?,
            method: self.style_logits(&tokens, 4, 2, "meth")?,
            pace: self.style_logits(&tokens, 5, 3, "pace")?,
            range: self.style_logits(&tokens, 6, 3, "range")?,
        };
        let attention_logits = self.project_linear_values(
            &features,
            frames,
            HIDDEN_DIM,
            TECHNIQUE_ATTENTION_HEADS,
            "tech_predictor.multihead_tech_attn",
        )?;
        let (attention, weighted_features) =
            technique_attention(&features, &attention_logits, frames)?;
        Ok(SentenceEncoding {
            features,
            weighted_features,
            attention,
            styles,
            frames,
        })
    }

    fn style_logits(
        &self,
        tokens: &[f32],
        token: usize,
        output_dimensions: usize,
        name: &str,
    ) -> Result<Vec<f32>, String> {
        let start = token * HIDDEN_DIM;
        self.project_normalized_linear_values(
            &tokens[start..start + HIDDEN_DIM],
            output_dimensions,
            &format!("style_predict.{name}_norm"),
            &format!("style_predict.{name}_head"),
        )
    }
}

fn validate_stage_d_inputs(
    mel_embedding: &[f32],
    pitch_features: &[f32],
    valid_frames: usize,
    frames: usize,
) -> Result<(), String> {
    if frames == 0
        || frames % 16 != 0
        || valid_frames == 0
        || valid_frames > frames
        || mel_embedding.len() != frames * HIDDEN_DIM
        || pitch_features.len() != frames * HIDDEN_DIM
        || mel_embedding.iter().any(|value| !value.is_finite())
        || pitch_features.iter().any(|value| !value.is_finite())
    {
        Err("STARS Stage D input shape is invalid".to_string())
    } else {
        Ok(())
    }
}

fn mean_frames(values: &[f32], frames: usize) -> Result<Vec<f32>, String> {
    if frames == 0 || values.len() != frames * HIDDEN_DIM {
        return Err("STARS sentence prosody shape is invalid".to_string());
    }
    let mut mean = vec![0.0_f32; HIDDEN_DIM];
    for frame in values.chunks_exact(HIDDEN_DIM) {
        for (sum, value) in mean.iter_mut().zip(frame) {
            *sum += *value;
        }
    }
    mean.iter_mut().for_each(|value| *value /= frames as f32);
    Ok(mean)
}

fn add_frame_bias(features: &[f32], bias: &[f32], frames: usize) -> Result<Vec<f32>, String> {
    if features.len() != frames * HIDDEN_DIM || bias.len() != HIDDEN_DIM {
        return Err("STARS sentence feature shape is invalid".to_string());
    }
    Ok(features
        .chunks_exact(HIDDEN_DIM)
        .flat_map(|frame| frame.iter().zip(bias).map(|(value, bias)| value + bias))
        .collect())
}

fn technique_attention(
    features: &[f32],
    logits: &[f32],
    frames: usize,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    if features.len() != frames * HIDDEN_DIM || logits.len() != frames * TECHNIQUE_ATTENTION_HEADS {
        return Err("STARS technique attention shape is invalid".to_string());
    }
    let attention = logits
        .chunks_exact(TECHNIQUE_ATTENTION_HEADS)
        .map(|heads| {
            heads
                .iter()
                .map(|value| 1.0 / (1.0 + (-value).exp()))
                .sum::<f32>()
                / TECHNIQUE_ATTENTION_HEADS as f32
        })
        .collect::<Vec<_>>();
    let weighted = features
        .chunks_exact(HIDDEN_DIM)
        .zip(&attention)
        .flat_map(|(frame, weight)| frame.iter().map(move |value| value * weight))
        .collect();
    Ok((attention, weighted))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct StageDInferenceFixture {
        frames: usize,
        valid_frames: usize,
        features: Vec<Vec<f32>>,
        attention: Vec<f32>,
        weighted_features: Vec<Vec<f32>>,
        styles: StyleFixture,
    }

    #[derive(Deserialize)]
    struct StyleFixture {
        technique_group: Vec<f32>,
        language: Vec<f32>,
        gender: Vec<f32>,
        emotion: Vec<f32>,
        method: Vec<f32>,
        pace: Vec<f32>,
        range: Vec<f32>,
    }

    #[test]
    fn sentence_mean_includes_padded_frames() {
        let mut frames = vec![0.0_f32; 2 * HIDDEN_DIM];
        frames[..HIDDEN_DIM].fill(2.0);
        frames[HIDDEN_DIM..].fill(4.0);
        assert_eq!(mean_frames(&frames, 2).unwrap(), vec![3.0; HIDDEN_DIM]);
    }

    #[test]
    fn technique_attention_averages_sigmoid_heads() {
        let features = vec![2.0_f32; HIDDEN_DIM];
        let logits = [0.0_f32, 0.0, 0.0, 0.0];
        let (attention, weighted) = technique_attention(&features, &logits, 1).unwrap();
        assert_eq!(attention, vec![0.5]);
        assert_eq!(weighted, vec![1.0; HIDDEN_DIM]);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and STARS GGUF"]
    fn stage_d_matches_checkpoint_on_explicit_device() {
        let runtime_path = PathBuf::from(
            std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR").expect("set UTA_TEST_GGML_RUNTIME_DIR"),
        );
        let model_path = PathBuf::from(
            std::env::var_os("UTA_TEST_STARS_GGUF").expect("set UTA_TEST_STARS_GGUF"),
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
        let expected: StageDInferenceFixture = serde_json::from_str(include_str!(
            "../../fixtures/stars/stars-stage-d-inference.json"
        ))
        .expect("valid STARS Stage D inference fixture");
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
        let mel_embedding = deterministic_input(
            expected.frames,
            expected.valid_frames,
            0.013,
            0.031,
            0.15,
            0.05,
        );
        let pitch_features = deterministic_input(
            expected.frames,
            expected.valid_frames,
            0.019,
            0.011,
            0.10,
            -0.04,
        );
        let actual = model
            .encode_sentence(
                &mel_embedding,
                &pitch_features,
                expected.valid_frames,
                expected.frames,
            )
            .unwrap();
        let expected_features = expected.features.into_iter().flatten().collect::<Vec<_>>();
        let expected_weighted = expected
            .weighted_features
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let feature_error = max_abs_difference(&actual.features, &expected_features);
        let attention_error = max_abs_difference(&actual.attention, &expected.attention);
        let weighted_error = max_abs_difference(&actual.weighted_features, &expected_weighted);
        let style_errors = [
            style_error(
                &actual.styles.technique_group,
                &expected.styles.technique_group,
            ),
            style_error(&actual.styles.language, &expected.styles.language),
            style_error(&actual.styles.gender, &expected.styles.gender),
            style_error(&actual.styles.emotion, &expected.styles.emotion),
            style_error(&actual.styles.method, &expected.styles.method),
            style_error(&actual.styles.pace, &expected.styles.pace),
            style_error(&actual.styles.range, &expected.styles.range),
        ];
        let style_error = style_errors.into_iter().fold(0.0, f32::max);
        eprintln!(
            "STARS Stage D on {}: features {feature_error:.8}, attention {attention_error:.8}, weighted {weighted_error:.8}, styles {style_error:.8}",
            device.description
        );
        let tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 5.0e-3,
            crate::DeviceKind::IntegratedGpu => 2.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(feature_error < tolerance);
        assert!(attention_error < tolerance);
        assert!(weighted_error < tolerance);
        assert!(style_error < tolerance);
    }

    fn deterministic_input(
        frames: usize,
        valid_frames: usize,
        first_rate: f64,
        second_rate: f64,
        first_scale: f64,
        second_scale: f64,
    ) -> Vec<f32> {
        (0..frames * HIDDEN_DIM)
            .map(|index| {
                if index / HIDDEN_DIM < valid_frames {
                    ((index as f64 * first_rate).sin() * first_scale
                        + (index as f64 * second_rate).cos() * second_scale)
                        as f32
                } else {
                    0.0
                }
            })
            .collect()
    }

    fn style_error(actual: &[f32], expected: &[f32]) -> f32 {
        assert_eq!(argmax(actual), argmax(expected));
        max_abs_difference(actual, expected)
    }

    fn argmax(values: &[f32]) -> usize {
        values
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .expect("non-empty STARS style-logit row")
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
