use std::ops::Range;

use super::model::{HIDDEN_DIM, Stars, TECHNIQUE_CLASSES};

#[derive(Debug, Clone, PartialEq)]
pub struct TechniqueEncoding {
    /// Phoneme-major `[phoneme_count, 9]` technique logits.
    pub logits: Vec<f32>,
    pub phoneme_count: usize,
}

impl Stars {
    /// Runs the final phoneme-level STARS technique classifier.
    pub fn encode_techniques(
        &self,
        aggregated: &[f32],
        phoneme_count: usize,
    ) -> Result<TechniqueEncoding, String> {
        if phoneme_count == 0
            || aggregated.len() != phoneme_count * HIDDEN_DIM
            || aggregated.iter().any(|value| !value.is_finite())
        {
            return Err("STARS Stage E input shape is invalid".to_string());
        }
        let post = self.encode_conv_blocks_silu_values(
            aggregated,
            phoneme_count,
            "tech_predictor.tech_post",
        )?;
        let logits = self.project_linear_values(
            &post,
            phoneme_count,
            HIDDEN_DIM,
            TECHNIQUE_CLASSES,
            "tech_predictor.binary_tech_out",
        )?;
        Ok(TechniqueEncoding {
            logits,
            phoneme_count,
        })
    }
}

/// Aggregates frame-level technique features over decoded phoneme intervals.
///
/// `weighted` already contains the Stage D attention multiplication. The
/// divisor therefore uses the matching frame attention sum, preserving the
/// official predictor's scatter-mean convention.
pub fn aggregate_technique_frames(
    weighted: &[f32],
    attention: &[f32],
    intervals: &[Range<usize>],
) -> Result<Vec<f32>, String> {
    let frames = attention.len();
    if frames == 0
        || intervals.is_empty()
        || weighted.len() != frames * HIDDEN_DIM
        || weighted.iter().any(|value| !value.is_finite())
        || attention.iter().any(|value| !value.is_finite())
        || intervals
            .iter()
            .any(|interval| interval.start >= interval.end || interval.end > frames)
    {
        return Err("STARS technique aggregation input shape is invalid".to_string());
    }

    let mut aggregated = vec![0.0_f32; intervals.len() * HIDDEN_DIM];
    for (phoneme, interval) in intervals.iter().enumerate() {
        let denominator = attention[interval.clone()].iter().sum::<f32>() + 1.0e-5;
        for frame in interval.clone() {
            for channel in 0..HIDDEN_DIM {
                aggregated[phoneme * HIDDEN_DIM + channel] +=
                    weighted[frame * HIDDEN_DIM + channel];
            }
        }
        for value in &mut aggregated[phoneme * HIDDEN_DIM..(phoneme + 1) * HIDDEN_DIM] {
            *value /= denominator;
        }
    }
    Ok(aggregated)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct StageEInferenceFixture {
        technique_logits: Vec<Vec<f32>>,
    }

    #[test]
    fn technique_aggregation_uses_attention_denominator() {
        let attention = vec![0.25, 0.75, 0.5];
        let mut weighted = vec![0.0_f32; attention.len() * HIDDEN_DIM];
        weighted[0] = 0.5;
        weighted[HIDDEN_DIM] = 2.25;
        weighted[2 * HIDDEN_DIM] = 2.0;

        let actual = aggregate_technique_frames(&weighted, &attention, &[0..2, 2..3]).unwrap();
        assert!((actual[0] - 2.75 / 1.00001).abs() < 1.0e-6);
        assert!((actual[HIDDEN_DIM] - 2.0 / 0.50001).abs() < 1.0e-6);
    }

    #[test]
    fn technique_aggregation_rejects_invalid_intervals() {
        let weighted = vec![0.0_f32; 2 * HIDDEN_DIM];
        assert!(aggregate_technique_frames(&weighted, &[1.0, 1.0], &[1..1]).is_err());
        assert!(aggregate_technique_frames(&weighted, &[1.0, 1.0], &[0..3]).is_err());
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and STARS GGUF"]
    fn stage_e_matches_checkpoint_on_explicit_device() {
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
        let phoneme_count = 3;
        let aggregated = (0..phoneme_count * HIDDEN_DIM)
            .map(|index| {
                ((index as f64 * 0.017).sin() * 0.2 + (index as f64 * 0.007).cos() * 0.1) as f32
            })
            .collect::<Vec<_>>();
        let actual = model.encode_techniques(&aggregated, phoneme_count).unwrap();
        let expected: StageEInferenceFixture = serde_json::from_str(include_str!(
            "../../fixtures/stars/stars-stage-e-inference-logits.json"
        ))
        .expect("valid STARS Stage E inference fixture");
        let expected_logits = expected
            .technique_logits
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let max_error = actual
            .logits
            .iter()
            .zip(&expected_logits)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0, f32::max);
        eprintln!(
            "STARS Stage E on {}: logits {max_error:.8}",
            device.description
        );
        for (actual_row, expected_row) in actual
            .logits
            .chunks_exact(TECHNIQUE_CLASSES)
            .zip(expected_logits.chunks_exact(TECHNIQUE_CLASSES))
        {
            assert_eq!(argmax(actual_row), argmax(expected_row));
        }
        let tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 1.0e-3,
            crate::DeviceKind::IntegratedGpu => 1.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(max_error < tolerance);
    }

    fn argmax(values: &[f32]) -> usize {
        values
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .expect("non-empty STARS technique-logit row")
    }
}
