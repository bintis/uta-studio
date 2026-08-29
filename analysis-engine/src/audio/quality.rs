use crate::contract::{
    AUDIO_QUALITY_ALGORITHM_VERSION, AUDIO_QUALITY_REPORT_CONTRACT, AUDIO_QUALITY_REPORT_VERSION,
    AnalysisProfile, AudioQualityReportV1, CLEANUP_CONSISTENCY_GATE, CLIPPING_GATE,
    ENERGY_RATIO_GATE, EngineError, EngineErrorCode, EngineResult, FINITE_SAMPLES_GATE,
    LEAD_PURITY_GATE, MUSICAL_DAMAGE_GATE, QualityGateOutcomeV1, QualityGateRequirementV1,
    QualityGateStatusV1, QualityMetricV1, QualityRegionV1, SILENCE_RATIO_GATE, TIMELINE_VALID_GATE,
    VOCAL_LEAKAGE_GATE, VOCAL_TOPOLOGY_ESTIMATE_CONTRACT, VOCAL_TOPOLOGY_ESTIMATE_VERSION,
    VOCAL_TOPOLOGY_GATE, VocalTopologyEstimateV1, VocalTopologyModeV1, gate_requirement,
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
const PROFILE_WINDOW_MILLIS: u64 = 250;
const ENVELOPE_BINS: usize = 8;
const ACTIVE_FLOOR: f64 = 1.0e-4;
const SUPPORT_ENERGY_RATIO: f64 = 0.18;
const MAX_SUPPORT_COVERAGE: f64 = 0.25;
const MAX_LEAKAGE_COVERAGE: f64 = 0.10;
const MAX_DAMAGE_DROPOUT_COVERAGE: f64 = 0.20;
const MIN_STRUCTURAL_WINDOWS: usize = 4;
const MIN_ACTIVE_INSTRUMENTAL_COVERAGE: f64 = 0.20;
const MIN_ACTIVE_VOCAL_REFERENCE_COVERAGE: f64 = 0.20;
const MIN_HIGH_FREQUENCY_RATIO: f64 = 0.0001;
const MIN_BROADBAND_WINDOW_COVERAGE: f64 = 0.50;
const MIN_TEMPORAL_STRUCTURE_COVERAGE: f64 = 0.25;
const MIN_RMS_CHANGE: f64 = 0.08;
const MIN_HIGH_FREQUENCY_CHANGE: f64 = 0.00005;
const MIN_CREST_FACTOR_CHANGE: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SignalMetrics {
    pub sample_count: u64,
    pub finite_samples: bool,
    pub peak: f32,
    pub rms: f64,
    pub clipping_ratio: f64,
    pub silence_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SignalWindowMetrics {
    pub start: u64,
    pub end: u64,
    pub rms: f64,
    pub zero_crossing_ratio: f64,
    pub crest_factor: f64,
    pub high_frequency_ratio: f64,
    pub envelope: [f64; ENVELOPE_BINS],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SignalProfile {
    pub windows: Vec<SignalWindowMetrics>,
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

pub(crate) fn signal_profile_window_frames(sample_rate: u32) -> usize {
    ((u64::from(sample_rate) * PROFILE_WINDOW_MILLIS) / 1_000).max(1) as usize
}

pub(crate) fn build_signal_window(
    start_frame: u64,
    sample_rate: u32,
    samples: &[f32],
) -> Option<SignalWindowMetrics> {
    if samples.is_empty() || sample_rate == 0 || samples.iter().any(|sample| !sample.is_finite()) {
        return None;
    }
    let rms = (samples
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    let zero_crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0.0 && pair[1] >= 0.0) || (pair[0] >= 0.0 && pair[1] < 0.0))
        .count();
    let zero_crossing_ratio = zero_crossings as f64 / samples.len().saturating_sub(1).max(1) as f64;
    let peak = samples
        .iter()
        .map(|sample| f64::from(sample.abs()))
        .fold(0.0_f64, f64::max);
    let crest_factor = if rms <= f64::EPSILON { 0.0 } else { peak / rms };
    // First-difference energy is a bounded, streaming-friendly proxy for
    // high-frequency structure. It avoids manufacturing a full spectrum from
    // the short diagnostic window while still detecting severe spectral collapse.
    let sample_energy = samples
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>();
    let difference_energy = samples
        .windows(2)
        .map(|pair| {
            let difference = f64::from(pair[1]) - f64::from(pair[0]);
            difference * difference
        })
        .sum::<f64>();
    let high_frequency_ratio = if sample_energy <= f64::EPSILON {
        0.0
    } else {
        (difference_energy / (4.0 * sample_energy)).clamp(0.0, 1.0)
    };
    let mut envelope = [0.0; ENVELOPE_BINS];
    for (bin, value) in envelope.iter_mut().enumerate() {
        let start = bin * samples.len() / ENVELOPE_BINS;
        let end = ((bin + 1) * samples.len() / ENVELOPE_BINS).max(start + 1);
        *value = (samples[start..end]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / (end - start) as f64)
            .sqrt();
    }
    let end_frame = start_frame.saturating_add(samples.len() as u64);
    Some(SignalWindowMetrics {
        start: frames_to_canonical(start_frame, sample_rate),
        end: frames_to_canonical(end_frame, sample_rate),
        rms,
        zero_crossing_ratio,
        crest_factor,
        high_frequency_ratio,
        envelope,
    })
}

fn frames_to_canonical(frame: u64, sample_rate: u32) -> u64 {
    frame.saturating_mul(u64::from(crate::contract::CANONICAL_TIMEBASE))
        / u64::from(sample_rate.max(1))
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstrumentalQualityEvidence {
    vocal_leakage_status: QualityGateStatusV1,
    vocal_leakage_coverage: f64,
    vocal_leakage_regions: Vec<QualityRegionV1>,
    musical_damage_status: QualityGateStatusV1,
    damage_dropout_coverage: f64,
    active_instrumental_coverage: f64,
    broadband_window_coverage: f64,
    temporal_structure_coverage: f64,
    musical_damage_regions: Vec<QualityRegionV1>,
    reference_available: bool,
}

pub(crate) fn estimate_vocal_topology(
    source_start: u64,
    duration: u64,
    lead: Option<&SignalProfile>,
    residual: Option<&SignalProfile>,
) -> EngineResult<VocalTopologyEstimateV1> {
    let evidence_sources = if lead.is_some() && residual.is_some() {
        vec![
            "lead_vocal.window_profile_v1".to_string(),
            "vocal_residual.window_profile_v1".to_string(),
        ]
    } else {
        vec!["caller_or_unpartitioned_vocal_input".to_string()]
    };
    let (Some(lead), Some(residual)) = (lead, residual) else {
        return topology_estimate(
            source_start,
            duration,
            VocalTopologyModeV1::Unknown,
            Vec::new(),
            Vec::new(),
            evidence_sources,
        );
    };
    if lead.windows.is_empty()
        || lead.windows.len() != residual.windows.len()
        || lead
            .windows
            .iter()
            .zip(&residual.windows)
            .any(|(left, right)| left.start != right.start || left.end != right.end)
    {
        return topology_estimate(
            source_start,
            duration,
            VocalTopologyModeV1::Unknown,
            Vec::new(),
            Vec::new(),
            evidence_sources,
        );
    }
    let lead_peak = lead
        .windows
        .iter()
        .map(|window| window.rms)
        .fold(0.0_f64, f64::max);
    let residual_peak = residual
        .windows
        .iter()
        .map(|window| window.rms)
        .fold(0.0_f64, f64::max);
    if lead_peak <= ACTIVE_FLOOR {
        return topology_estimate(
            source_start,
            duration,
            VocalTopologyModeV1::Unknown,
            Vec::new(),
            Vec::new(),
            evidence_sources,
        );
    }
    let lead_floor = (lead_peak * 0.08).max(ACTIVE_FLOOR);
    let residual_floor = (residual_peak * 0.08).max(ACTIVE_FLOOR);
    let mut overlap_windows = Vec::new();
    let mut support_windows = Vec::new();
    let mut exclusive_windows = Vec::new();
    for (lead, residual) in lead.windows.iter().zip(&residual.windows) {
        let lead_active = lead.rms >= lead_floor;
        let residual_active = residual.rms >= residual_floor;
        if lead_active && residual_active {
            let distinct_periodicity = ratio(residual.rms, lead.rms) >= SUPPORT_ENERGY_RATIO
                && (lead.zero_crossing_ratio - residual.zero_crossing_ratio).abs() >= 0.006;
            if distinct_periodicity {
                overlap_windows.push(lead.clone());
            } else {
                // Any independently decoded simultaneous residual activity is
                // support ambiguity. A low ratio is not affirmative evidence
                // that the foreground contains exactly one lead.
                support_windows.push(lead.clone());
            }
        } else if lead_active != residual_active {
            exclusive_windows.push((if lead_active { 1_u8 } else { 2_u8 }, lead.clone()));
        }
    }
    let exclusive_sequence = exclusive_windows
        .iter()
        .map(|(owner, _)| *owner)
        .collect::<Vec<_>>();
    let alternating_residual_ambiguity = exclusive_sequence
        .iter()
        .filter(|value| **value == 1)
        .count()
        >= 2
        && exclusive_sequence
            .iter()
            .filter(|value| **value == 2)
            .count()
            >= 2
        && exclusive_sequence
            .windows(2)
            .filter(|pair| pair[0] != pair[1])
            .count()
            >= 2;
    let overlap_regions = windows_to_regions(
        source_start,
        duration,
        &overlap_windows,
        "simultaneous_distinct_foreground_activity",
    );
    let mut support_regions = windows_to_regions(
        source_start,
        duration,
        &support_windows,
        "support_vocal_activity",
    );
    if alternating_residual_ambiguity {
        support_regions.extend(windows_to_regions(
            source_start,
            duration,
            &exclusive_windows
                .iter()
                .map(|(_, window)| window.clone())
                .collect::<Vec<_>>(),
            "alternating_vocal_residual_foreground_ambiguity",
        ));
        support_regions.sort_by_key(|region| (region.start, region.end));
    }
    let mode = if !overlap_regions.is_empty() {
        VocalTopologyModeV1::OverlappingMultiLead
    } else if !support_regions.is_empty() {
        // The separator's residual is not independent singer-identity evidence.
        // Even clean alternation therefore remains foreground/support ambiguity;
        // AlternatingMultiLead is reserved for a future qualified expert.
        VocalTopologyModeV1::LeadWithSupport
    } else {
        VocalTopologyModeV1::SingleLead
    };
    topology_estimate(
        source_start,
        duration,
        mode,
        overlap_regions,
        support_regions,
        evidence_sources,
    )
}

fn topology_estimate(
    source_start: u64,
    duration: u64,
    mode: VocalTopologyModeV1,
    overlap_regions: Vec<QualityRegionV1>,
    support_regions: Vec<QualityRegionV1>,
    evidence_sources: Vec<String>,
) -> EngineResult<VocalTopologyEstimateV1> {
    let estimate = VocalTopologyEstimateV1 {
        contract: VOCAL_TOPOLOGY_ESTIMATE_CONTRACT.to_string(),
        version: VOCAL_TOPOLOGY_ESTIMATE_VERSION,
        timebase: crate::contract::CANONICAL_TIMEBASE,
        source_start,
        duration,
        mode,
        // No calibrated topology model participates in v1. This source-local
        // deterministic estimate must not manufacture probability.
        confidence: None,
        overlap_regions,
        support_regions,
        evidence_sources,
    };
    estimate.validate()?;
    Ok(estimate)
}

pub(crate) fn topology_review_regions(
    estimate: &VocalTopologyEstimateV1,
) -> Vec<SingingReviewRegion> {
    let mut regions = Vec::new();
    for region in &estimate.overlap_regions {
        regions.push(SingingReviewRegion {
            id: format!("topology-overlap-{}-{}", region.start, region.end),
            range: crate::fusion::TimeRange {
                start: region.start,
                end: region.end,
            },
            confidence: None,
            reasons: vec![
                SingingReviewReason::ForegroundOverlap,
                SingingReviewReason::VoicingConflict,
            ],
            evidence_experts: estimate.evidence_sources.clone(),
            reviewed: false,
        });
    }
    for region in &estimate.support_regions {
        regions.push(SingingReviewRegion {
            id: format!("topology-support-{}-{}", region.start, region.end),
            range: crate::fusion::TimeRange {
                start: region.start,
                end: region.end,
            },
            confidence: None,
            reasons: vec![
                SingingReviewReason::SupportVocalActivity,
                SingingReviewReason::LeadHarmonyLeak,
            ],
            evidence_experts: estimate.evidence_sources.clone(),
            reviewed: false,
        });
    }
    if estimate.mode == VocalTopologyModeV1::Unknown {
        regions.push(SingingReviewRegion {
            id: format!(
                "topology-unknown-{}-{}",
                estimate.source_start,
                estimate.source_start.saturating_add(estimate.duration)
            ),
            range: crate::fusion::TimeRange {
                start: estimate.source_start,
                end: estimate.source_start.saturating_add(estimate.duration),
            },
            confidence: None,
            reasons: vec![SingingReviewReason::VocalTopologyUnknown],
            evidence_experts: estimate.evidence_sources.clone(),
            reviewed: false,
        });
    }
    regions.sort_by_key(|region| (region.range.start, region.range.end, region.id.clone()));
    regions
}

pub(crate) fn estimate_instrumental_quality(
    source_start: u64,
    duration: u64,
    instrumental_metrics: SignalMetrics,
    instrumental: &SignalProfile,
    vocal_reference: Option<&SignalProfile>,
) -> InstrumentalQualityEvidence {
    let peak = instrumental
        .windows
        .iter()
        .map(|window| window.rms)
        .fold(0.0_f64, f64::max);
    let active_floor = (peak * 0.02).max(ACTIVE_FLOOR);
    let active_windows = instrumental
        .windows
        .iter()
        .filter(|window| window.rms >= active_floor)
        .cloned()
        .collect::<Vec<_>>();
    let active_instrumental_coverage = covered_ratio(
        &windows_to_regions(
            source_start,
            duration,
            &active_windows,
            "instrumental_active_structure",
        ),
        duration,
    );
    let broadband_window_coverage = if active_windows.is_empty() {
        0.0
    } else {
        active_windows
            .iter()
            .filter(|window| window.high_frequency_ratio >= MIN_HIGH_FREQUENCY_RATIO)
            .count() as f64
            / active_windows.len() as f64
    };
    let temporal_structure_coverage = if active_windows.len() < 2 {
        0.0
    } else {
        active_windows
            .windows(2)
            .filter(|pair| {
                relative_change(pair[0].rms, pair[1].rms) >= MIN_RMS_CHANGE
                    || (pair[0].high_frequency_ratio - pair[1].high_frequency_ratio).abs()
                        >= MIN_HIGH_FREQUENCY_CHANGE
                    || (pair[0].crest_factor - pair[1].crest_factor).abs()
                        >= MIN_CREST_FACTOR_CHANGE
            })
            .count() as f64
            / active_windows.len().saturating_sub(1) as f64
    };
    let structural_evidence_available = active_windows.len() >= MIN_STRUCTURAL_WINDOWS
        && active_instrumental_coverage >= MIN_ACTIVE_INSTRUMENTAL_COVERAGE
        && broadband_window_coverage >= MIN_BROADBAND_WINDOW_COVERAGE
        && temporal_structure_coverage >= MIN_TEMPORAL_STRUCTURE_COVERAGE;
    let dropout_windows = instrumental
        .windows
        .windows(3)
        .filter(|triple| {
            peak > ACTIVE_FLOOR
                && triple[1].rms <= peak * 0.001
                && triple[0].rms >= peak * 0.10
                && triple[2].rms >= peak * 0.10
        })
        .map(|triple| triple[1].clone())
        .collect::<Vec<_>>();
    let musical_damage_regions = windows_to_regions(
        source_start,
        duration,
        &dropout_windows,
        "instrumental_internal_dropout",
    );
    let damage_dropout_coverage = covered_ratio(&musical_damage_regions, duration);
    let musical_damage_status =
        if instrumental.windows.is_empty() || !instrumental_metrics.finite_samples {
            QualityGateStatusV1::Unknown
        } else if instrumental_metrics.clipping_ratio > MAX_CLIPPING_RATIO
            || instrumental_metrics.silence_ratio > MAX_SILENCE_RATIO
            || damage_dropout_coverage > MAX_DAMAGE_DROPOUT_COVERAGE
        {
            QualityGateStatusV1::Failed
        } else if musical_damage_regions.is_empty() && structural_evidence_available {
            QualityGateStatusV1::Passed
        } else {
            // Intrinsic evidence can detect damage, but a spectrally collapsed or
            // transient-free artifact cannot be certified as undamaged without
            // comparing it to the original mix (which this gate must never do).
            QualityGateStatusV1::Unknown
        };

    let usable_vocal_reference = vocal_reference.filter(|vocal| {
        let vocal_peak = vocal
            .windows
            .iter()
            .map(|window| window.rms)
            .fold(0.0_f64, f64::max);
        let active_floor = (vocal_peak * 0.08).max(ACTIVE_FLOOR);
        let active_regions = windows_to_regions(
            source_start,
            duration,
            &vocal
                .windows
                .iter()
                .filter(|window| window.rms >= active_floor)
                .cloned()
                .collect::<Vec<_>>(),
            "qualified_vocal_reference_activity",
        );
        !instrumental.windows.is_empty()
            && vocal.windows.len() == instrumental.windows.len()
            && !vocal.windows.is_empty()
            && vocal
                .windows
                .iter()
                .zip(&instrumental.windows)
                .all(|(vocal, instrumental)| {
                    vocal.start == instrumental.start && vocal.end == instrumental.end
                })
            && vocal_peak > ACTIVE_FLOOR
            && covered_ratio(&active_regions, duration) >= MIN_ACTIVE_VOCAL_REFERENCE_COVERAGE
    });
    let mut vocal_leakage_regions = Vec::new();
    let vocal_leakage_status = if let Some(vocal) = usable_vocal_reference {
        let vocal_peak = vocal
            .windows
            .iter()
            .map(|window| window.rms)
            .fold(0.0_f64, f64::max);
        let leaked = instrumental
            .windows
            .iter()
            .zip(&vocal.windows)
            .filter(|(instrumental, vocal)| {
                vocal.rms >= (vocal_peak * 0.08).max(ACTIVE_FLOOR)
                    && ratio(instrumental.rms, vocal.rms) >= 0.05
                    && envelope_similarity(&instrumental.envelope, &vocal.envelope) >= 0.995
                    && (instrumental.zero_crossing_ratio - vocal.zero_crossing_ratio).abs()
                        <= 0.0025
            })
            .map(|(instrumental, _)| instrumental.clone())
            .collect::<Vec<_>>();
        vocal_leakage_regions = windows_to_regions(
            source_start,
            duration,
            &leaked,
            "instrumental_matches_generated_vocal_reference",
        );
        let coverage = covered_ratio(&vocal_leakage_regions, duration);
        if coverage > MAX_LEAKAGE_COVERAGE {
            QualityGateStatusV1::Failed
        } else if vocal_leakage_regions.is_empty() {
            QualityGateStatusV1::Passed
        } else {
            QualityGateStatusV1::Unknown
        }
    } else {
        QualityGateStatusV1::Unknown
    };
    let vocal_leakage_coverage = covered_ratio(&vocal_leakage_regions, duration);
    InstrumentalQualityEvidence {
        vocal_leakage_status,
        vocal_leakage_coverage,
        vocal_leakage_regions,
        musical_damage_status,
        damage_dropout_coverage,
        active_instrumental_coverage,
        broadband_window_coverage,
        temporal_structure_coverage,
        musical_damage_regions,
        reference_available: usable_vocal_reference.is_some(),
    }
}

fn windows_to_regions(
    source_start: u64,
    duration: u64,
    windows: &[SignalWindowMetrics],
    reason: &str,
) -> Vec<QualityRegionV1> {
    let source_end = source_start.saturating_add(duration);
    let mut result = Vec::<QualityRegionV1>::new();
    for window in windows {
        let start = source_start.saturating_add(window.start).min(source_end);
        let end = source_start.saturating_add(window.end).min(source_end);
        if start >= end {
            continue;
        }
        if let Some(previous) = result.last_mut()
            && start <= previous.end
        {
            previous.end = previous.end.max(end);
            continue;
        }
        result.push(QualityRegionV1 {
            start,
            end,
            reason: reason.to_string(),
        });
    }
    result
}

fn envelope_similarity(left: &[f64; ENVELOPE_BINS], right: &[f64; ENVELOPE_BINS]) -> f64 {
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm <= f64::EPSILON || right_norm <= f64::EPSILON {
        0.0
    } else {
        (dot / (left_norm * right_norm)).clamp(0.0, 1.0)
    }
}

pub(crate) struct QualityEvaluationInput<'a> {
    pub profile: AnalysisProfile,
    pub planned_gates: &'a [String],
    pub evaluated_audio_role: &'a str,
    pub source_start: u64,
    pub expected_duration: u64,
    pub actual_duration: u64,
    pub source: SignalMetrics,
    pub analyzed: SignalMetrics,
    pub cleanup: Option<CleanupComparison>,
    pub vocal_topology: Option<&'a VocalTopologyEstimateV1>,
    pub instrumental: Option<&'a InstrumentalQualityEvidence>,
}

pub(crate) fn evaluate_audio_quality(
    input: QualityEvaluationInput<'_>,
) -> EngineResult<AudioQualityReportV1> {
    let energy_ratio = ratio(input.analyzed.rms, input.source.rms);
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
            LEAD_PURITY_GATE => lead_purity_outcome(gate, input.vocal_topology),
            VOCAL_LEAKAGE_GATE => instrumental_leakage_outcome(gate, input.instrumental),
            MUSICAL_DAMAGE_GATE => instrumental_damage_outcome(gate, input.instrumental),
            CLEANUP_CONSISTENCY_GATE => cleanup_outcome(gate, input.cleanup),
            VOCAL_TOPOLOGY_GATE => vocal_topology_outcome(gate, input.vocal_topology),
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
        vocal_topology: input.vocal_topology.cloned(),
    };
    report.validate_for_source(input.source_start)?;
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
            VOCAL_LEAKAGE_GATE => "instrumental_vocal_leakage_uncertain".to_string(),
            MUSICAL_DAMAGE_GATE => "instrumental_musical_damage_suspected".to_string(),
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

fn lead_purity_outcome(
    gate: &str,
    topology: Option<&VocalTopologyEstimateV1>,
) -> QualityGateOutcomeV1 {
    let Some(topology) = topology else {
        return outcome(
            gate,
            QualityGateStatusV1::Unknown,
            "lead purity has no independent foreground/residual evidence",
            Vec::new(),
            Vec::new(),
        );
    };
    let regions = topology_regions(topology);
    let coverage = covered_ratio(&regions, topology.duration);
    let (status, summary) = match topology.mode {
        VocalTopologyModeV1::SingleLead | VocalTopologyModeV1::AlternatingMultiLead => (
            QualityGateStatusV1::Passed,
            "foreground/residual evidence supports a usable monophonic lead in each active window",
        ),
        VocalTopologyModeV1::LeadWithSupport if coverage > MAX_SUPPORT_COVERAGE => (
            QualityGateStatusV1::Failed,
            "support-vocal activity materially contaminates the analysis lead",
        ),
        VocalTopologyModeV1::LeadWithSupport => (
            QualityGateStatusV1::Unknown,
            "support-vocal activity is present; lead purity remains source-locally uncertain",
        ),
        VocalTopologyModeV1::OverlappingMultiLead => (
            QualityGateStatusV1::Failed,
            "simultaneous foreground activity is incompatible with a trusted monophonic lead",
        ),
        VocalTopologyModeV1::Unknown => (
            QualityGateStatusV1::Unknown,
            "lead purity is unknown because no independent residual comparison is available",
        ),
    };
    outcome(
        gate,
        status,
        summary,
        vec![metric(
            "ambiguous_foreground_coverage",
            coverage,
            "ratio",
            Some(0.0),
            Some(MAX_SUPPORT_COVERAGE),
        )],
        regions,
    )
}

fn vocal_topology_outcome(
    gate: &str,
    topology: Option<&VocalTopologyEstimateV1>,
) -> QualityGateOutcomeV1 {
    let Some(topology) = topology else {
        return outcome(
            gate,
            QualityGateStatusV1::Unknown,
            "vocal topology was not measured",
            Vec::new(),
            Vec::new(),
        );
    };
    let regions = topology_regions(topology);
    let coverage = covered_ratio(&regions, topology.duration);
    let (status, summary) = match topology.mode {
        VocalTopologyModeV1::SingleLead => (
            QualityGateStatusV1::Passed,
            "foreground/residual evidence supports single-lead topology",
        ),
        VocalTopologyModeV1::AlternatingMultiLead => (
            QualityGateStatusV1::Passed,
            "alternating foreground activity remains monophonic per measured window; singer identity is not inferred",
        ),
        VocalTopologyModeV1::LeadWithSupport if coverage > MAX_SUPPORT_COVERAGE => (
            QualityGateStatusV1::Failed,
            "support-vocal regions occupy too much of the source for trusted monophonic topology",
        ),
        VocalTopologyModeV1::LeadWithSupport => (
            QualityGateStatusV1::Unknown,
            "lead-with-support topology is measured without claiming backing or harmony identity",
        ),
        VocalTopologyModeV1::OverlappingMultiLead => (
            QualityGateStatusV1::Failed,
            "overlapping foreground topology is measured; a second singer track is not fabricated",
        ),
        VocalTopologyModeV1::Unknown => (
            QualityGateStatusV1::Unknown,
            "vocal topology is unknown because independent foreground/residual evidence is insufficient",
        ),
    };
    outcome(
        gate,
        status,
        summary,
        vec![metric(
            "ambiguous_topology_coverage",
            coverage,
            "ratio",
            Some(0.0),
            Some(MAX_SUPPORT_COVERAGE),
        )],
        regions,
    )
}

fn topology_regions(topology: &VocalTopologyEstimateV1) -> Vec<QualityRegionV1> {
    let mut regions = topology.overlap_regions.clone();
    regions.extend(topology.support_regions.clone());
    if topology.mode == VocalTopologyModeV1::Unknown {
        regions.push(QualityRegionV1 {
            start: topology.source_start,
            end: topology.source_start.saturating_add(topology.duration),
            reason: "vocal_topology_unknown".to_string(),
        });
    }
    regions.sort_by_key(|region| (region.start, region.end, region.reason.clone()));
    regions
}

fn instrumental_leakage_outcome(
    gate: &str,
    evidence: Option<&InstrumentalQualityEvidence>,
) -> QualityGateOutcomeV1 {
    let Some(evidence) = evidence else {
        return outcome(
            gate,
            QualityGateStatusV1::Unknown,
            "generated Instrumental artifact was unavailable for vocal-leakage measurement",
            Vec::new(),
            Vec::new(),
        );
    };
    outcome(
        gate,
        evidence.vocal_leakage_status,
        if !evidence.reference_available {
            "generated Instrumental was measured, but no generated vocal reference exists; leakage remains unknown"
        } else if evidence.vocal_leakage_status == QualityGateStatusV1::Passed {
            "generated Instrumental does not match active generated-vocal reference windows"
        } else {
            "generated Instrumental contains source-local activity matching the generated vocal reference"
        },
        vec![metric(
            "instrumental_vocal_leakage_coverage",
            evidence.vocal_leakage_coverage,
            "ratio",
            Some(0.0),
            Some(MAX_LEAKAGE_COVERAGE),
        )],
        evidence.vocal_leakage_regions.clone(),
    )
}

fn instrumental_damage_outcome(
    gate: &str,
    evidence: Option<&InstrumentalQualityEvidence>,
) -> QualityGateOutcomeV1 {
    let Some(evidence) = evidence else {
        return outcome(
            gate,
            QualityGateStatusV1::Unknown,
            "generated Instrumental artifact was unavailable for musical-damage measurement",
            Vec::new(),
            Vec::new(),
        );
    };
    outcome(
        gate,
        evidence.musical_damage_status,
        match evidence.musical_damage_status {
            QualityGateStatusV1::Passed => {
                "generated Instrumental has measurable spectral/transient structure and passes intrinsic clipping, silence and dropout checks"
            }
            QualityGateStatusV1::Failed => {
                "generated Instrumental has intrinsic clipping, silence or structural-dropout evidence"
            }
            QualityGateStatusV1::Unknown => {
                "generated Instrumental lacks enough intrinsic spectral/transient structure to certify musical-damage absence"
            }
        },
        vec![
            metric(
                "instrumental_damage_dropout_coverage",
                evidence.damage_dropout_coverage,
                "ratio",
                Some(0.0),
                Some(MAX_DAMAGE_DROPOUT_COVERAGE),
            ),
            metric(
                "instrumental_active_structure_coverage",
                evidence.active_instrumental_coverage,
                "ratio",
                Some(MIN_ACTIVE_INSTRUMENTAL_COVERAGE),
                Some(1.0),
            ),
            metric(
                "instrumental_broadband_window_coverage",
                evidence.broadband_window_coverage,
                "ratio",
                Some(MIN_BROADBAND_WINDOW_COVERAGE),
                Some(1.0),
            ),
            metric(
                "instrumental_temporal_structure_coverage",
                evidence.temporal_structure_coverage,
                "ratio",
                Some(MIN_TEMPORAL_STRUCTURE_COVERAGE),
                Some(1.0),
            ),
        ],
        evidence.musical_damage_regions.clone(),
    )
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

fn relative_change(left: f64, right: f64) -> f64 {
    (left - right).abs() / left.abs().max(right.abs()).max(ACTIVE_FLOOR)
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
        LEAD_PURITY_GATE, MUSICAL_DAMAGE_GATE, SILENCE_RATIO_GATE, TIMELINE_VALID_GATE,
        VOCAL_LEAKAGE_GATE, VOCAL_TOPOLOGY_GATE,
    };

    fn metrics(samples: &[f32]) -> SignalMetrics {
        let mut accumulator = SignalAccumulator::new();
        for sample in samples {
            accumulator.push(*sample);
        }
        accumulator.finish()
    }

    fn signal_fixture(windows: &[(f32, f32)]) -> (Vec<f32>, SignalProfile) {
        const SAMPLE_RATE: u32 = 8_000;
        let window_frames = signal_profile_window_frames(SAMPLE_RATE);
        let mut samples = Vec::new();
        let mut profile = SignalProfile::default();
        for (window, (amplitude, frequency)) in windows.iter().copied().enumerate() {
            let start = samples.len() as u64;
            let generated = (0..window_frames)
                .map(|index| {
                    let phase =
                        std::f32::consts::TAU * frequency * index as f32 / SAMPLE_RATE as f32;
                    phase.sin() * amplitude
                })
                .collect::<Vec<_>>();
            profile.windows.push(
                build_signal_window(start, SAMPLE_RATE, &generated)
                    .unwrap_or_else(|| panic!("fixture window {window} is valid")),
            );
            samples.extend(generated);
        }
        (samples, profile)
    }

    fn topology(lead: &[(f32, f32)], residual: &[(f32, f32)]) -> VocalTopologyEstimateV1 {
        let (_, lead) = signal_fixture(lead);
        let (_, residual) = signal_fixture(residual);
        estimate_vocal_topology(
            0,
            lead.windows.last().unwrap().end,
            Some(&lead),
            Some(&residual),
        )
        .unwrap()
    }

    fn gates() -> Vec<String> {
        [
            TIMELINE_VALID_GATE,
            FINITE_SAMPLES_GATE,
            CLIPPING_GATE,
            SILENCE_RATIO_GATE,
            ENERGY_RATIO_GATE,
            LEAD_PURITY_GATE,
            VOCAL_LEAKAGE_GATE,
            MUSICAL_DAMAGE_GATE,
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
        topology: Option<&VocalTopologyEstimateV1>,
        instrumental: Option<&InstrumentalQualityEvidence>,
    ) -> AudioQualityReportV1 {
        evaluate_audio_quality(QualityEvaluationInput {
            profile: AnalysisProfile::Balanced,
            planned_gates: &gates(),
            evaluated_audio_role: "lead_vocal",
            source_start: topology.map_or(0, |estimate| estimate.source_start),
            expected_duration: 1_000_000,
            actual_duration: 1_000_000,
            source,
            analyzed,
            cleanup,
            vocal_topology: topology,
            instrumental,
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
        let solo = topology(&[(0.3, 220.0); 4], &[(0.0, 220.0); 4]);
        let (instrumental_samples, instrumental_profile) =
            signal_fixture(&[(0.20, 880.0), (0.28, 440.0), (0.16, 1_320.0), (0.24, 660.0)]);
        let (_, vocal_profile) = signal_fixture(&[(0.3, 220.0); 4]);
        let instrumental = estimate_instrumental_quality(
            0,
            1_000_000,
            metrics(&instrumental_samples),
            &instrumental_profile,
            Some(&vocal_profile),
        );
        let first = evaluate(
            signal,
            signal,
            Some(cleanup),
            Some(&solo),
            Some(&instrumental),
        );
        let second = evaluate(
            signal,
            signal,
            Some(cleanup),
            Some(&solo),
            Some(&instrumental),
        );
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
        let report = evaluate(signal, signal, None, None, None);
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
        let report = evaluate(source, silence, None, None, None);
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
        let report = evaluate(source, amplified, None, None, None);
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
        let report = evaluate(raw, raw, Some(comparison), None, None);
        assert_eq!(
            status(&report, CLEANUP_CONSISTENCY_GATE),
            QualityGateStatusV1::Failed
        );
        assert!(
            quality_degraded_reasons(&report).contains(&"cleanup_damage_suspected".to_string())
        );
    }

    #[test]
    fn simultaneous_overlap_is_typed_without_inventing_singer_identity_or_probability() {
        let overlap = topology(&[(0.3, 220.0); 4], &[(0.25, 440.0); 4]);
        assert_eq!(overlap.mode, VocalTopologyModeV1::OverlappingMultiLead);
        assert!(!overlap.overlap_regions.is_empty());
        assert!(overlap.confidence.is_none());
        let signal = metrics(&vec![0.2; 16_000]);
        let report = evaluate(signal, signal, None, Some(&overlap), None);
        assert_eq!(
            status(&report, LEAD_PURITY_GATE),
            QualityGateStatusV1::Failed
        );
        assert_eq!(
            status(&report, VOCAL_TOPOLOGY_GATE),
            QualityGateStatusV1::Failed
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            json.contains("singer identity is not inferred") || json.contains("not fabricated")
        );
        assert!(!json.contains("singer_id"));
        assert!(!json.contains("backing_vocal"));
        assert!(!json.contains("harmony_vocal"));
        assert!(!json.contains("probability"));
    }

    #[test]
    fn support_residual_alternation_and_insufficient_topologies_degrade_without_identity() {
        let support = topology(&[(0.3, 220.0); 4], &[(0.1, 220.0); 4]);
        assert_eq!(support.mode, VocalTopologyModeV1::LeadWithSupport);
        assert!(!support.support_regions.is_empty());

        let quiet_support = topology(&[(0.3, 220.0); 4], &[(0.03, 220.0); 4]);
        assert_eq!(quiet_support.mode, VocalTopologyModeV1::LeadWithSupport);
        assert!(!quiet_support.support_regions.is_empty());

        let alternating = topology(
            &[(0.3, 220.0), (0.0, 220.0), (0.3, 220.0), (0.0, 220.0)],
            &[(0.0, 330.0), (0.3, 330.0), (0.0, 330.0), (0.3, 330.0)],
        );
        assert_eq!(alternating.mode, VocalTopologyModeV1::LeadWithSupport);
        assert!(alternating.overlap_regions.is_empty());
        assert!(
            alternating.support_regions.iter().any(|region| {
                region.reason == "alternating_vocal_residual_foreground_ambiguity"
            })
        );
        let signal = metrics(&vec![0.2; 16_000]);
        let report = evaluate(signal, signal, None, Some(&alternating), None);
        assert_eq!(
            status(&report, LEAD_PURITY_GATE),
            QualityGateStatusV1::Failed
        );

        let unknown = estimate_vocal_topology(0, 1_000_000, None, None).unwrap();
        assert_eq!(unknown.mode, VocalTopologyModeV1::Unknown);
        assert!(unknown.confidence.is_none());
        assert_eq!(topology_review_regions(&unknown).len(), 1);
    }

    #[test]
    fn generated_instrumental_is_the_measured_input_for_leakage_and_damage() {
        let (vocal_samples, vocal) = signal_fixture(&[(0.3, 220.0); 4]);
        let leaked = estimate_instrumental_quality(
            0,
            1_000_000,
            metrics(&vocal_samples),
            &vocal,
            Some(&vocal),
        );
        assert_eq!(leaked.vocal_leakage_status, QualityGateStatusV1::Failed);
        assert!(leaked.vocal_leakage_coverage > MAX_LEAKAGE_COVERAGE);

        let damaged_pattern = [
            (0.3, 660.0),
            (0.0, 660.0),
            (0.3, 660.0),
            (0.0, 660.0),
            (0.3, 660.0),
        ];
        let (damaged_samples, damaged) = signal_fixture(&damaged_pattern);
        let damage = estimate_instrumental_quality(
            0,
            damaged.windows.last().unwrap().end,
            metrics(&damaged_samples),
            &damaged,
            None,
        );
        assert_eq!(damage.musical_damage_status, QualityGateStatusV1::Failed);
        assert!(damage.damage_dropout_coverage > MAX_DAMAGE_DROPOUT_COVERAGE);
        assert_eq!(damage.vocal_leakage_status, QualityGateStatusV1::Unknown);

        let (collapsed_samples, spectrally_collapsed) = signal_fixture(&[(0.2, 1.0); 4]);
        let collapsed = estimate_instrumental_quality(
            0,
            1_000_000,
            metrics(&collapsed_samples),
            &spectrally_collapsed,
            None,
        );
        assert_eq!(
            collapsed.musical_damage_status,
            QualityGateStatusV1::Unknown
        );

        let (stationary_samples, stationary_tone) = signal_fixture(&[(0.2, 880.0); 4]);
        let stationary = estimate_instrumental_quality(
            0,
            1_000_000,
            metrics(&stationary_samples),
            &stationary_tone,
            None,
        );
        assert_eq!(
            stationary.musical_damage_status,
            QualityGateStatusV1::Unknown
        );
    }

    #[test]
    fn unusable_vocal_reference_never_false_passes_leakage() {
        let (instrumental_samples, instrumental) = signal_fixture(&[(0.2, 880.0); 4]);
        let empty = SignalProfile::default();
        let (_, silent) = signal_fixture(&[(0.0, 220.0); 4]);
        let (_, mut truncated) = signal_fixture(&[(0.3, 220.0); 4]);
        truncated.windows.pop();
        let (_, mut misaligned) = signal_fixture(&[(0.3, 220.0); 4]);
        misaligned.windows[1].start += 1;
        for unusable in [&empty, &silent, &truncated, &misaligned] {
            let evidence = estimate_instrumental_quality(
                0,
                1_000_000,
                metrics(&instrumental_samples),
                &instrumental,
                Some(unusable),
            );
            assert_eq!(evidence.vocal_leakage_status, QualityGateStatusV1::Unknown);
            assert!(!evidence.reference_available);
        }

        let (long_instrumental_samples, long_instrumental) = signal_fixture(&[(0.2, 880.0); 8]);
        let (_, sparse_reference) = signal_fixture(&[
            (0.3, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
            (0.0, 220.0),
        ]);
        let sparse = estimate_instrumental_quality(
            0,
            2_000_000,
            metrics(&long_instrumental_samples),
            &long_instrumental,
            Some(&sparse_reference),
        );
        assert_eq!(sparse.vocal_leakage_status, QualityGateStatusV1::Unknown);
        assert!(!sparse.reference_available);
    }

    #[test]
    fn quality_regions_are_source_timeline_bound_and_ordered() {
        let signal = metrics(&vec![0.2; 16_000]);
        let topology = estimate_vocal_topology(100, 1_000_000, None, None).unwrap();
        let mut report = evaluate(signal, signal, None, Some(&topology), None);
        let mut legacy = report.clone();
        legacy.algorithm = "audio-quality-gates-v1".to_string();
        legacy.vocal_topology = None;
        assert!(legacy.validate().is_ok());

        report.outcomes[2].regions = vec![QualityRegionV1 {
            start: 999_000,
            end: 1_001_000,
            reason: "outside_source".to_string(),
        }];
        assert!(report.validate().is_ok());
        assert!(report.validate_for_source(100).is_err());
        report.outcomes[2].regions = vec![
            QualityRegionV1 {
                start: 100,
                end: 300,
                reason: "first".to_string(),
            },
            QualityRegionV1 {
                start: 200,
                end: 400,
                reason: "overlap".to_string(),
            },
        ];
        assert!(report.validate().is_ok());
        assert!(report.validate_for_source(100).is_err());
    }

    #[test]
    fn non_finite_samples_and_timeline_drift_are_required_failures() {
        let mut invalid = metrics(&[0.1, f32::NAN, 0.2]);
        invalid.silence_ratio = 0.0;
        let report = evaluate_audio_quality(QualityEvaluationInput {
            profile: AnalysisProfile::Balanced,
            planned_gates: &gates(),
            evaluated_audio_role: "lead_vocal",
            source_start: 0,
            expected_duration: 1_000_000,
            actual_duration: 1_010_000,
            source: invalid,
            analyzed: invalid,
            cleanup: None,
            vocal_topology: None,
            instrumental: None,
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
