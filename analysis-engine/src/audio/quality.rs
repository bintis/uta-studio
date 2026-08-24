use crate::contract::{
    AUDIO_QUALITY_ALGORITHM_VERSION, AUDIO_QUALITY_REPORT_CONTRACT, AUDIO_QUALITY_REPORT_VERSION,
    AnalysisProfile, AudioQualityReportV1, CLEANUP_CONSISTENCY_GATE, CLIPPING_GATE,
    ENERGY_RATIO_GATE, EngineError, EngineErrorCode, EngineResult, FINITE_SAMPLES_GATE,
    LEAD_PURITY_GATE, QualityGateOutcomeV1, QualityGateRequirementV1, QualityGateStatusV1,
    QualityMetricV1, QualityRegionV1, SILENCE_RATIO_GATE, TIMELINE_VALID_GATE, VOCAL_TOPOLOGY_GATE,
    gate_requirement,
};
use crate::fusion::{SingingReviewReason, SingingReviewRegion};

const SILENCE_AMPLITUDE: f32 = 1.0e-4;
const CLIPPING_AMPLITUDE: f32 = 0.999;
const MAX_CLIPPING_RATIO: f64 = 0.01;
const MAX_SILENCE_RATIO: f64 = 0.995;
const MIN_ENERGY_RATIO: f64 = 0.0001;
const MAX_ENERGY_RATIO: f64 = 4.0;
const MAX_TIMELINE_DELTA: u64 = 2_000;
const MIN_CLEANUP_ENERGY_RATIO: f64 = 0.25;
const MAX_CLEANUP_ENERGY_RATIO: f64 = 2.5;
const MAX_CLEANUP_SILENCE_DELTA: f64 = 0.35;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SignalMetrics {
    pub sample_count: u64,
    pub finite_samples: bool,
    pub peak: f32,
    pub rms: f64,
    pub clipping_ratio: f64,
    pub silence_ratio: f64,
}

#[derive(Debug, Default)]
pub(crate) struct SignalAccumulator {
    sample_count: u64,
    finite_samples: bool,
    peak: f32,
    sum_squares: f64,
    clipping_samples: u64,
    silence_samples: u64,
}

impl SignalAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            finite_samples: true,
            ..Self::default()
        }
    }

    pub(crate) fn push(&mut self, sample: f32) {
        self.sample_count = self.sample_count.saturating_add(1);
        if !sample.is_finite() {
            self.finite_samples = false;
            return;
        }
        let magnitude = sample.abs();
        self.peak = self.peak.max(magnitude);
        self.sum_squares += f64::from(sample) * f64::from(sample);
        self.clipping_samples += u64::from(magnitude >= CLIPPING_AMPLITUDE);
        self.silence_samples += u64::from(magnitude <= SILENCE_AMPLITUDE);
    }

    pub(crate) fn finish(self) -> SignalMetrics {
        let denominator = self.sample_count.max(1) as f64;
        SignalMetrics {
            sample_count: self.sample_count,
            finite_samples: self.finite_samples,
            peak: self.peak,
            rms: (self.sum_squares / denominator).sqrt(),
            clipping_ratio: self.clipping_samples as f64 / denominator,
            silence_ratio: self.silence_samples as f64 / denominator,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CleanupComparison {
    pub duration_delta: u64,
    pub energy_ratio: f64,
    pub silence_ratio_delta: f64,
}

impl CleanupComparison {
    pub(crate) fn from_signals(
        raw_duration: u64,
        raw: SignalMetrics,
        clean_duration: u64,
        clean: SignalMetrics,
    ) -> Self {
        Self {
            duration_delta: raw_duration.abs_diff(clean_duration),
            energy_ratio: ratio(clean.rms, raw.rms),
            silence_ratio_delta: clean.silence_ratio - raw.silence_ratio,
        }
    }

    pub(crate) fn damage_suspected(self) -> bool {
        self.duration_delta > MAX_TIMELINE_DELTA
            || self.energy_ratio < MIN_CLEANUP_ENERGY_RATIO
            || self.energy_ratio > MAX_CLEANUP_ENERGY_RATIO
            || self.silence_ratio_delta > MAX_CLEANUP_SILENCE_DELTA
    }
}

pub(crate) struct QualityEvaluationInput<'a> {
    pub profile: AnalysisProfile,
    pub planned_gates: &'a [String],
    pub evaluated_audio_role: &'a str,
    pub expected_duration: u64,
    pub actual_duration: u64,
    pub source: SignalMetrics,
    pub analyzed: SignalMetrics,
    pub cleanup: Option<CleanupComparison>,
    pub foreground_evidence_available: bool,
    pub review_regions: &'a [SingingReviewRegion],
}

pub(crate) fn evaluate_audio_quality(
    input: QualityEvaluationInput<'_>,
) -> EngineResult<AudioQualityReportV1> {
    let energy_ratio = ratio(input.analyzed.rms, input.source.rms);
    let ambiguous_regions = ambiguous_foreground_regions(input.review_regions);
    let ambiguous_coverage = covered_ratio(&ambiguous_regions, input.expected_duration);
    let outcomes = input
        .planned_gates
        .iter()
        .map(|gate| match gate.as_str() {
            TIMELINE_VALID_GATE => outcome(
                gate,
                if input.actual_duration.abs_diff(input.expected_duration) <= MAX_TIMELINE_DELTA {
                    QualityGateStatusV1::Passed
                } else {
                    QualityGateStatusV1::Failed
                },
                "analysis audio preserves the canonical source timeline",
                vec![metric(
                    "duration_delta",
                    input.actual_duration.abs_diff(input.expected_duration) as f64,
                    "canonical_ticks",
                    None,
                    Some(MAX_TIMELINE_DELTA as f64),
                )],
                Vec::new(),
            ),
            FINITE_SAMPLES_GATE => outcome(
                gate,
                if input.source.finite_samples && input.analyzed.finite_samples {
                    QualityGateStatusV1::Passed
                } else {
                    QualityGateStatusV1::Failed
                },
                "decoded source and analysis audio contain only finite samples",
                vec![metric(
                    "analyzed_sample_count",
                    input.analyzed.sample_count as f64,
                    "samples",
                    Some(1.0),
                    None,
                )],
                Vec::new(),
            ),
            CLIPPING_GATE => outcome(
                gate,
                if input.analyzed.clipping_ratio <= MAX_CLIPPING_RATIO {
                    QualityGateStatusV1::Passed
                } else {
                    QualityGateStatusV1::Failed
                },
                "analysis-audio clipping is measured without altering samples",
                vec![
                    metric(
                        "clipped_sample_ratio",
                        input.analyzed.clipping_ratio,
                        "ratio",
                        Some(0.0),
                        Some(MAX_CLIPPING_RATIO),
                    ),
                    metric(
                        "peak_amplitude",
                        f64::from(input.analyzed.peak),
                        "linear",
                        Some(0.0),
                        None,
                    ),
                ],
                Vec::new(),
            ),
            SILENCE_RATIO_GATE => outcome(
                gate,
                if input.analyzed.silence_ratio <= MAX_SILENCE_RATIO {
                    QualityGateStatusV1::Passed
                } else {
                    QualityGateStatusV1::Failed
                },
                "analysis audio is not accidentally all-silence",
                vec![metric(
                    "silent_sample_ratio",
                    input.analyzed.silence_ratio,
                    "ratio",
                    Some(0.0),
                    Some(MAX_SILENCE_RATIO),
                )],
                Vec::new(),
            ),
            ENERGY_RATIO_GATE => outcome(
                gate,
                if (MIN_ENERGY_RATIO..=MAX_ENERGY_RATIO).contains(&energy_ratio) {
                    QualityGateStatusV1::Passed
                } else {
                    QualityGateStatusV1::Failed
                },
                "analysis-audio energy remains within conservative source-relative bounds",
                vec![metric(
                    "analysis_to_source_rms_ratio",
                    energy_ratio,
                    "ratio",
                    Some(MIN_ENERGY_RATIO),
                    Some(MAX_ENERGY_RATIO),
                )],
                Vec::new(),
            ),
            LEAD_PURITY_GATE => foreground_outcome(
                gate,
                input.foreground_evidence_available,
                ambiguous_coverage,
                &ambiguous_regions,
                "foreground evidence supports a predominantly monophonic analysis lead",
                "foreground evidence is ambiguous; lead purity is not claimed",
            ),
            CLEANUP_CONSISTENCY_GATE => cleanup_outcome(gate, input.cleanup),
            VOCAL_TOPOLOGY_GATE => foreground_outcome(
                gate,
                input.foreground_evidence_available,
                ambiguous_coverage,
                &ambiguous_regions,
                "available independent evidence supports monophonic foreground topology",
                "available evidence is compatible with overlapping or ambiguous foreground vocals; singer identity is unknown",
            ),
            _ => outcome(
                gate,
                QualityGateStatusV1::Unknown,
                "the planned quality gate is unknown to this evaluator",
                Vec::new(),
                Vec::new(),
            ),
        })
        .collect();
    let report = AudioQualityReportV1 {
        contract: AUDIO_QUALITY_REPORT_CONTRACT.to_string(),
        version: AUDIO_QUALITY_REPORT_VERSION,
        algorithm: AUDIO_QUALITY_ALGORITHM_VERSION.to_string(),
        profile: input.profile,
        evaluated_audio_role: input.evaluated_audio_role.to_string(),
        duration: input.actual_duration,
        planned_gates: input.planned_gates.to_vec(),
        outcomes,
    };
    report.validate()?;
    Ok(report)
}

pub(crate) fn enforce_required_quality(report: &AudioQualityReportV1) -> EngineResult<()> {
    let failed = report
        .outcomes
        .iter()
        .filter(|outcome| {
            outcome.requirement == QualityGateRequirementV1::Required
                && outcome.status != QualityGateStatusV1::Passed
        })
        .map(|outcome| outcome.gate.as_str())
        .collect::<Vec<_>>();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("required audio quality gate failed: {}", failed.join(", ")),
        ))
    }
}

pub(crate) fn quality_degraded_reasons(report: &AudioQualityReportV1) -> Vec<String> {
    report
        .outcomes
        .iter()
        .filter(|outcome| {
            outcome.requirement == QualityGateRequirementV1::Degrading
                && outcome.status != QualityGateStatusV1::Passed
        })
        .map(|outcome| match outcome.gate.as_str() {
            CLEANUP_CONSISTENCY_GATE if outcome.status == QualityGateStatusV1::Failed => {
                "cleanup_damage_suspected".to_string()
            }
            LEAD_PURITY_GATE => "lead_isolation_uncertain".to_string(),
            VOCAL_TOPOLOGY_GATE => "vocal_topology_ambiguous".to_string(),
            gate => format!("quality_gate_{gate}_{}", status_name(outcome.status)),
        })
        .collect()
}

fn cleanup_outcome(gate: &str, comparison: Option<CleanupComparison>) -> QualityGateOutcomeV1 {
    let Some(comparison) = comparison else {
        return outcome(
            gate,
            QualityGateStatusV1::Unknown,
            "no successful raw-versus-clean pair exists; cleanup consistency is unknown",
            Vec::new(),
            Vec::new(),
        );
    };
    outcome(
        gate,
        if comparison.damage_suspected() {
            QualityGateStatusV1::Failed
        } else {
            QualityGateStatusV1::Passed
        },
        "raw and cleaned lead audio were compared for timeline, energy and silence damage",
        vec![
            metric(
                "duration_delta",
                comparison.duration_delta as f64,
                "canonical_ticks",
                None,
                Some(MAX_TIMELINE_DELTA as f64),
            ),
            metric(
                "clean_to_raw_rms_ratio",
                comparison.energy_ratio,
                "ratio",
                Some(MIN_CLEANUP_ENERGY_RATIO),
                Some(MAX_CLEANUP_ENERGY_RATIO),
            ),
            metric(
                "clean_minus_raw_silence_ratio",
                comparison.silence_ratio_delta,
                "ratio",
                None,
                Some(MAX_CLEANUP_SILENCE_DELTA),
            ),
        ],
        Vec::new(),
    )
}

fn foreground_outcome(
    gate: &str,
    evidence_available: bool,
    ambiguous_coverage: f64,
    regions: &[QualityRegionV1],
    passed_summary: &str,
    ambiguous_summary: &str,
) -> QualityGateOutcomeV1 {
    let (status, summary) = if !evidence_available {
        (
            QualityGateStatusV1::Unknown,
            "independent pitch, note and acoustic evidence is insufficient; no purity or topology claim is made",
        )
    } else if regions.is_empty() {
        (QualityGateStatusV1::Passed, passed_summary)
    } else if ambiguous_coverage > 0.25 {
        (QualityGateStatusV1::Failed, ambiguous_summary)
    } else {
        (QualityGateStatusV1::Unknown, ambiguous_summary)
    };
    outcome(
        gate,
        status,
        summary,
        vec![metric(
            "ambiguous_foreground_coverage",
            ambiguous_coverage,
            "ratio",
            Some(0.0),
            Some(0.25),
        )],
        regions.to_vec(),
    )
}

fn ambiguous_foreground_regions(regions: &[SingingReviewRegion]) -> Vec<QualityRegionV1> {
    regions
        .iter()
        .filter(|region| {
            region.reasons.iter().any(|reason| {
                matches!(
                    reason,
                    SingingReviewReason::PitchDisagreement
                        | SingingReviewReason::PitchInstability
                        | SingingReviewReason::OctaveRisk
                        | SingingReviewReason::VoicingConflict
                        | SingingReviewReason::LeadHarmonyLeak
                )
            })
        })
        .map(|region| QualityRegionV1 {
            start: region.range.start,
            end: region.range.end,
            reason: region
                .reasons
                .iter()
                .filter(|reason| {
                    matches!(
                        reason,
                        SingingReviewReason::PitchDisagreement
                            | SingingReviewReason::PitchInstability
                            | SingingReviewReason::OctaveRisk
                            | SingingReviewReason::VoicingConflict
                            | SingingReviewReason::LeadHarmonyLeak
                    )
                })
                .map(|reason| format!("{reason:?}").to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("+"),
        })
        .collect()
}

fn covered_ratio(regions: &[QualityRegionV1], duration: u64) -> f64 {
    if duration == 0 || regions.is_empty() {
        return 0.0;
    }
    let mut ranges = regions
        .iter()
        .map(|region| (region.start, region.end))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    let mut covered = 0_u64;
    let mut current = ranges[0];
    for range in ranges.into_iter().skip(1) {
        if range.0 <= current.1 {
            current.1 = current.1.max(range.1);
        } else {
            covered = covered.saturating_add(current.1.saturating_sub(current.0));
            current = range;
        }
    }
    covered = covered.saturating_add(current.1.saturating_sub(current.0));
    covered.min(duration) as f64 / duration as f64
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= f64::EPSILON {
        if numerator <= f64::EPSILON {
            1.0
        } else {
            f64::MAX
        }
    } else {
        numerator / denominator
    }
}

fn outcome(
    gate: &str,
    status: QualityGateStatusV1,
    summary: &str,
    metrics: Vec<QualityMetricV1>,
    regions: Vec<QualityRegionV1>,
) -> QualityGateOutcomeV1 {
    QualityGateOutcomeV1 {
        gate: gate.to_string(),
        requirement: gate_requirement(gate),
        status,
        summary: summary.to_string(),
        metrics,
        regions,
    }
}

fn metric(
    name: &str,
    value: f64,
    unit: &str,
    lower_bound: Option<f64>,
    upper_bound: Option<f64>,
) -> QualityMetricV1 {
    QualityMetricV1 {
        name: name.to_string(),
        value,
        unit: unit.to_string(),
        lower_bound,
        upper_bound,
    }
}

fn status_name(status: QualityGateStatusV1) -> &'static str {
    match status {
        QualityGateStatusV1::Passed => "passed",
        QualityGateStatusV1::Failed => "failed",
        QualityGateStatusV1::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{
        CLEANUP_CONSISTENCY_GATE, CLIPPING_GATE, ENERGY_RATIO_GATE, FINITE_SAMPLES_GATE,
        LEAD_PURITY_GATE, SILENCE_RATIO_GATE, TIMELINE_VALID_GATE, VOCAL_TOPOLOGY_GATE,
    };
    use crate::fusion::TimeRange;

    fn metrics(samples: &[f32]) -> SignalMetrics {
        let mut accumulator = SignalAccumulator::new();
        for sample in samples {
            accumulator.push(*sample);
        }
        accumulator.finish()
    }

    fn gates() -> Vec<String> {
        [
            TIMELINE_VALID_GATE,
            FINITE_SAMPLES_GATE,
            CLIPPING_GATE,
            SILENCE_RATIO_GATE,
            ENERGY_RATIO_GATE,
            LEAD_PURITY_GATE,
            CLEANUP_CONSISTENCY_GATE,
            VOCAL_TOPOLOGY_GATE,
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    fn evaluate(
        source: SignalMetrics,
        analyzed: SignalMetrics,
        cleanup: Option<CleanupComparison>,
        evidence_available: bool,
        review_regions: &[SingingReviewRegion],
    ) -> AudioQualityReportV1 {
        evaluate_audio_quality(QualityEvaluationInput {
            profile: AnalysisProfile::Balanced,
            planned_gates: &gates(),
            evaluated_audio_role: "lead_vocal",
            expected_duration: 1_000_000,
            actual_duration: 1_000_000,
            source,
            analyzed,
            cleanup,
            foreground_evidence_available: evidence_available,
            review_regions,
        })
        .unwrap()
    }

    fn status(report: &AudioQualityReportV1, gate: &str) -> QualityGateStatusV1 {
        report
            .outcomes
            .iter()
            .find(|outcome| outcome.gate == gate)
            .unwrap()
            .status
    }

    #[test]
    fn generated_clean_solo_is_deterministic_and_passes_available_gates() {
        let samples = (0..16_000)
            .map(|index| ((index as f32 * 0.071).sin()) * 0.3)
            .collect::<Vec<_>>();
        let unchanged = samples.clone();
        let signal = metrics(&samples);
        assert_eq!(
            samples, unchanged,
            "quality measurement must not alter media samples"
        );
        let cleanup = CleanupComparison::from_signals(1_000_000, signal, 1_000_000, signal);
        let first = evaluate(signal, signal, Some(cleanup), true, &[]);
        let second = evaluate(signal, signal, Some(cleanup), true, &[]);
        assert_eq!(first, second);
        assert!(
            first
                .outcomes
                .iter()
                .all(|outcome| outcome.status == QualityGateStatusV1::Passed)
        );
    }

    #[test]
    fn generated_clipping_is_typed_degradation_not_a_finite_or_timeline_failure() {
        let signal = metrics(&vec![1.0; 16_000]);
        let report = evaluate(signal, signal, None, false, &[]);
        assert_eq!(status(&report, CLIPPING_GATE), QualityGateStatusV1::Failed);
        assert_eq!(
            status(&report, FINITE_SAMPLES_GATE),
            QualityGateStatusV1::Passed
        );
        assert!(enforce_required_quality(&report).is_ok());
        assert!(
            quality_degraded_reasons(&report)
                .iter()
                .any(|reason| reason.contains("clipping"))
        );
    }

    #[test]
    fn generated_silence_and_energy_anomaly_fail_required_gates() {
        let source = metrics(&vec![0.25; 16_000]);
        let silence = metrics(&vec![0.0; 16_000]);
        let report = evaluate(source, silence, None, false, &[]);
        assert_eq!(
            status(&report, SILENCE_RATIO_GATE),
            QualityGateStatusV1::Failed
        );
        assert_eq!(
            status(&report, ENERGY_RATIO_GATE),
            QualityGateStatusV1::Failed
        );
        assert!(enforce_required_quality(&report).is_err());
    }

    #[test]
    fn generated_non_silent_energy_anomaly_fails_closed() {
        let source = metrics(&vec![0.01; 16_000]);
        let amplified = metrics(&vec![0.5; 16_000]);
        let report = evaluate(source, amplified, None, false, &[]);
        assert_eq!(
            status(&report, ENERGY_RATIO_GATE),
            QualityGateStatusV1::Failed
        );
        assert!(enforce_required_quality(&report).is_err());
    }

    #[test]
    fn cleanup_damage_falls_back_by_explicit_failed_evidence() {
        let raw = metrics(&vec![0.2; 16_000]);
        let damaged = metrics(&vec![0.001; 16_000]);
        let comparison = CleanupComparison::from_signals(1_000_000, raw, 1_000_000, damaged);
        assert!(comparison.damage_suspected());
        let report = evaluate(raw, raw, Some(comparison), true, &[]);
        assert_eq!(
            status(&report, CLEANUP_CONSISTENCY_GATE),
            QualityGateStatusV1::Failed
        );
        assert!(
            quality_degraded_reasons(&report).contains(&"cleanup_damage_suspected".to_string())
        );
    }

    #[test]
    fn overlapping_or_ambiguous_foreground_never_invents_singer_identity_or_probability() {
        let signal = metrics(&vec![0.2; 16_000]);
        let region = SingingReviewRegion {
            id: "ambiguous".to_string(),
            range: TimeRange {
                start: 100_000,
                end: 700_000,
            },
            confidence: None,
            reasons: vec![
                SingingReviewReason::VoicingConflict,
                SingingReviewReason::PitchDisagreement,
            ],
            evidence_experts: vec![
                "rmvpe".to_string(),
                "game".to_string(),
                "acoustic_dsp".to_string(),
            ],
            reviewed: false,
        };
        let report = evaluate(signal, signal, None, true, &[region]);
        assert_eq!(
            status(&report, LEAD_PURITY_GATE),
            QualityGateStatusV1::Failed
        );
        assert_eq!(
            status(&report, VOCAL_TOPOLOGY_GATE),
            QualityGateStatusV1::Failed
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("singer identity is unknown"));
        assert!(!json.contains("singer_id"));
        assert!(!json.contains("backing_vocal"));
        assert!(!json.contains("harmony_vocal"));
        assert!(!json.contains("probability"));
    }

    #[test]
    fn non_finite_samples_and_timeline_drift_are_required_failures() {
        let mut invalid = metrics(&[0.1, f32::NAN, 0.2]);
        invalid.silence_ratio = 0.0;
        let report = evaluate_audio_quality(QualityEvaluationInput {
            profile: AnalysisProfile::Balanced,
            planned_gates: &gates(),
            evaluated_audio_role: "lead_vocal",
            expected_duration: 1_000_000,
            actual_duration: 1_010_000,
            source: invalid,
            analyzed: invalid,
            cleanup: None,
            foreground_evidence_available: false,
            review_regions: &[],
        })
        .unwrap();
        assert_eq!(
            status(&report, TIMELINE_VALID_GATE),
            QualityGateStatusV1::Failed
        );
        assert_eq!(
            status(&report, FINITE_SAMPLES_GATE),
            QualityGateStatusV1::Failed
        );
        assert!(enforce_required_quality(&report).is_err());
    }
}
