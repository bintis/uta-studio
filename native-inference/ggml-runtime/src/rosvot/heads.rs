use super::model::{HIDDEN_DIM, PITCH_CLASSES, Rosvot};

const ATTENTION_HEADS: usize = 4;
const BOUNDARY_TEMPERATURE: f32 = 0.2;
const PITCH_TEMPERATURE: f32 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct FrameHeadEncoding {
    /// Frame-major U-Net output `[frames, 256]`.
    pub features: Vec<f32>,
    /// Temperature-scaled and clamped raw boundary logits.
    pub boundary_logits: Vec<f32>,
    /// Mean sigmoid score across four pitch-attention heads.
    pub attention: Vec<f32>,
    /// Frame-major `features * attention` consumed by Rust aggregation.
    pub weighted_features: Vec<f32>,
    pub frames: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PitchHeadEncoding {
    /// Note-major output from `pitch_decoder.post`.
    pub features: Vec<f32>,
    /// Note-major `[notes, 89]` logits after the model's temperature.
    pub logits: Vec<f32>,
    pub notes: usize,
}

impl Rosvot {
    pub fn encode_frame_heads(
        &self,
        features: &[f32],
        frames: usize,
    ) -> Result<FrameHeadEncoding, String> {
        if frames == 0
            || features.len() != frames * HIDDEN_DIM
            || features.iter().any(|value| !value.is_finite())
        {
            return Err("ROSVOT frame-head input is invalid".to_string());
        }
        let mut boundary_logits =
            self.project_linear_values(features, frames, HIDDEN_DIM, 1, "note_bd_out")?;
        boundary_logits
            .iter_mut()
            .for_each(|value| *value = (*value / BOUNDARY_TEMPERATURE).clamp(-16.0, 16.0));
        let attention_logits = self.project_linear_values(
            features,
            frames,
            HIDDEN_DIM,
            ATTENTION_HEADS,
            "pitch_decoder.multihead_dot_attn",
        )?;
        let attention = attention_logits
            .chunks_exact(ATTENTION_HEADS)
            .map(|row| {
                row.iter().map(|value| sigmoid(*value)).sum::<f32>() / ATTENTION_HEADS as f32
            })
            .collect::<Vec<_>>();
        let mut weighted_features = features.to_vec();
        for (frame, score) in attention.iter().enumerate() {
            weighted_features[frame * HIDDEN_DIM..(frame + 1) * HIDDEN_DIM]
                .iter_mut()
                .for_each(|value| *value *= score);
        }
        Ok(FrameHeadEncoding {
            features: features.to_vec(),
            boundary_logits,
            attention,
            weighted_features,
            frames,
        })
    }

    pub fn encode_pitch_head(
        &self,
        note_features: &[f32],
        notes: usize,
    ) -> Result<PitchHeadEncoding, String> {
        if notes == 0
            || note_features.len() != notes * HIDDEN_DIM
            || note_features.iter().any(|value| !value.is_finite())
        {
            return Err("ROSVOT pitch-head input is invalid".to_string());
        }
        let features = self.encode_conv_block_values(note_features, notes, "pitch_decoder.post")?;
        let mut logits = self.project_linear_values(
            &features,
            notes,
            HIDDEN_DIM,
            PITCH_CLASSES,
            "pitch_decoder.pitch_out",
        )?;
        logits
            .iter_mut()
            .for_each(|value| *value /= PITCH_TEMPERATURE);
        Ok(PitchHeadEncoding {
            features,
            logits,
            notes,
        })
    }
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
    struct DebugReference {
        mel_embed_proj: Vec<Vec<f32>>,
        mel_embed: Vec<Vec<f32>>,
        pitch_embed: Vec<Vec<f32>>,
        word_bd_embed: Vec<Vec<f32>>,
        feat_cond_encoder: Vec<Vec<f32>>,
        feat_net: Vec<Vec<f32>>,
    }

    #[derive(Deserialize)]
    struct Reference {
        frames: usize,
        valid: usize,
        note_bd_logits: Vec<f32>,
        attention: Vec<f32>,
        weighted: Vec<Vec<f32>>,
        note_agg: Vec<f32>,
        note_logits: Vec<f32>,
        word_bd: Vec<i32>,
        mel40: Vec<Vec<f32>>,
        pitch_coarse: Vec<i32>,
        uv: Vec<i32>,
    }

    #[test]
    fn attention_sigmoid_is_centered() {
        assert_eq!(sigmoid(0.0), 0.5);
        assert!(sigmoid(8.0) > 0.999);
        assert!(sigmoid(-8.0) < 0.001);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, and ROSVOT GGUF"]
    fn end_to_end_heads_match_pytorch_on_explicit_device() {
        let runtime_path = PathBuf::from(
            std::env::var_os("UTA_TEST_GGML_RUNTIME_DIR").expect("set UTA_TEST_GGML_RUNTIME_DIR"),
        );
        let model_path = PathBuf::from(
            std::env::var_os("UTA_TEST_ROSVOT_GGUF").expect("set UTA_TEST_ROSVOT_GGUF"),
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
        let reference: Reference = serde_json::from_str(include_str!(
            "../../fixtures/rosvot/pytorch-reference-rosvot-output.json"
        ))
        .unwrap();
        let debug: DebugReference = serde_json::from_slice(
            &std::fs::read(
                std::env::var_os("UTA_TEST_ROSVOT_DEBUG_FIXTURE")
                    .expect("set UTA_TEST_ROSVOT_DEBUG_FIXTURE"),
            )
            .expect("read ROSVOT debug fixture"),
        )
        .unwrap();
        let runtime = crate::GgmlRuntime::load(&runtime_path).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description_filter)
            })
            .expect("requested GGML test device is unavailable");
        let model = Rosvot::load(runtime, &device, &model_path).unwrap();
        let conditioning = model
            .encode_conditioning(
                &reference
                    .mel40
                    .iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>(),
                &reference.pitch_coarse,
                &reference.uv,
                &reference.word_bd,
                reference.valid,
                reference.frames,
            )
            .unwrap();
        let features = model
            .encode_backbone(&conditioning.conditioned, reference.valid, reference.frames)
            .unwrap();
        let projected_error = max_abs_difference(
            &conditioning.mel.projected,
            &debug
                .mel_embed_proj
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        );
        let mel_error = max_abs_difference(
            &conditioning.mel.encoded,
            &debug
                .mel_embed
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        );
        let pitch = debug.pitch_embed.iter().flatten();
        let words = debug.word_bd_embed.iter().flatten();
        let expected_embedded = debug
            .mel_embed
            .iter()
            .flatten()
            .zip(pitch)
            .zip(words)
            .map(|((mel, pitch), word)| mel + pitch + word)
            .collect::<Vec<_>>();
        let embedded_error = max_abs_difference(&conditioning.embedded, &expected_embedded);
        let condition_error = max_abs_difference(
            &conditioning.conditioned,
            &debug
                .feat_cond_encoder
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        );
        let backbone_error = max_abs_difference(
            &features,
            &debug.feat_net.iter().flatten().copied().collect::<Vec<_>>(),
        );
        let frame = model
            .encode_frame_heads(&features, reference.frames)
            .unwrap();
        // The upstream oracle isolates PitchDecoder with the arithmetic mean
        // of the first five weighted frame vectors.
        let mut aggregate = vec![0.0_f32; HIDDEN_DIM];
        for frame_index in 0..5 {
            for channel in 0..HIDDEN_DIM {
                aggregate[channel] +=
                    frame.weighted_features[frame_index * HIDDEN_DIM + channel] / 5.0;
            }
        }
        let pitch = model.encode_pitch_head(&aggregate, 1).unwrap();
        let expected_weighted = reference.weighted.into_iter().flatten().collect::<Vec<_>>();
        let boundary_error = max_abs_difference(
            &frame.boundary_logits[..reference.valid],
            &reference.note_bd_logits[..reference.valid],
        );
        let attention_error = max_abs_difference(
            &frame.attention[..reference.valid],
            &reference.attention[..reference.valid],
        );
        let weighted_error = max_abs_difference(
            &frame.weighted_features[..reference.valid * HIDDEN_DIM],
            &expected_weighted[..reference.valid * HIDDEN_DIM],
        );
        let aggregate_error = max_abs_difference(&aggregate, &reference.note_agg);
        let pitch_error = max_abs_difference(&pitch.logits, &reference.note_logits);
        eprintln!(
            "ROSVOT on {}: projected {projected_error:.8}, mel {mel_error:.8}, embedded {embedded_error:.8}, condition {condition_error:.8}, backbone {backbone_error:.8}, boundary {boundary_error:.8}, attention {attention_error:.8}, weighted {weighted_error:.8}, aggregate {aggregate_error:.8}, pitch {pitch_error:.8}",
            device.description
        );
        let tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 5.0e-3,
            crate::DeviceKind::IntegratedGpu => 3.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(boundary_error < tolerance);
        assert!(attention_error < tolerance);
        assert!(weighted_error < tolerance);
        assert!(aggregate_error < tolerance);
        let pitch_tolerance = match expected_kind {
            crate::DeviceKind::Cpu => 1.0e-2,
            // The final head divides logits by 0.01, amplifying ordinary
            // Vulkan accumulation differences by two orders of magnitude.
            crate::DeviceKind::IntegratedGpu => 5.0e-2,
            crate::DeviceKind::DiscreteGpu => unreachable!(),
        };
        assert!(pitch_error < pitch_tolerance);
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
