//! Reuses the existing lossless-output decode facts. Does not run inference or
//! compare an estimate to the mixture as if it were a ground-truth stem.
use super::DecodedAudio;
use crate::contract::SeparatedStemMeasurement;
use std::path::Path;

pub(crate) fn separated_stem_measurement(
    role: &str,
    artifact_path: &Path,
    decoded: &DecodedAudio,
) -> SeparatedStemMeasurement {
    SeparatedStemMeasurement {
        role: role.to_string(),
        artifact_path: artifact_path.to_path_buf(),
        sample_rate: decoded.facts.sample_rate,
        channels: decoded.facts.channels,
        frame_count: decoded.facts.frame_count,
        duration_seconds: decoded.facts.frame_count as f64 / f64::from(decoded.facts.sample_rate),
        sample_count: decoded.metrics.sample_count,
        finite_samples: decoded.metrics.finite_samples,
        peak_amplitude: decoded.metrics.peak,
        rms_amplitude: decoded.metrics.rms,
        near_full_scale_ratio: decoded.metrics.clipping_ratio,
        silent_sample_ratio: decoded.metrics.silence_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{SignalAccumulator, SignalProfile};
    use crate::contract::DecodedAudioFacts;

    #[test]
    fn separation_measurements_preserve_decoded_stem_statistics_and_binding() {
        let mut accumulator = SignalAccumulator::new();
        for sample in [0.0, 0.5, -0.5, 1.0] {
            accumulator.push(sample);
        }
        let decoded = DecodedAudio {
            facts: DecodedAudioFacts {
                source_id: "guide_vocals".into(),
                container: "flac".into(),
                codec: "flac".into(),
                sample_rate: 44_100,
                channels: 2,
                frame_count: 2,
                duration: 45,
                peak: 1.0,
                decode_backend: "fixture".into(),
            },
            metrics: accumulator.finish(),
            profile: SignalProfile::default(),
        };
        let report = separated_stem_measurement(
            "guide_vocals",
            Path::new("stems/guide_vocals.flac"),
            &decoded,
        );
        assert_eq!(report.artifact_path, Path::new("stems/guide_vocals.flac"));
        assert_eq!(report.role, "guide_vocals");
        assert_eq!(report.sample_count, 4);
        assert_eq!(report.frame_count, 2);
        assert_eq!(report.duration_seconds, 2.0 / 44_100.0);
        assert_eq!(report.peak_amplitude, 1.0);
        assert_eq!(report.rms_amplitude, 0.375_f64.sqrt());
        assert_eq!(report.near_full_scale_ratio, 0.25);
        assert_eq!(report.silent_sample_ratio, 0.25);
        assert!(report.finite_samples);
        let value = serde_json::to_value(report).unwrap();
        assert!(value.get("sdr").is_none());
        assert!(value.get("si_sdr").is_none());
    }
}
