// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

use crate::contract::{AnalysisProfile, EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;
use crate::fusion::{SingingReviewReason, SingingReviewRegion, TimeRange};
use crate::workflow::WorkflowExecutionPolicyV1;

pub const CONDITIONAL_SCHEDULER_VERSION: &str = "uta.conditional-scheduler.v1";
const DEFAULT_REGION_PADDING: u64 = 250_000;
const DEFAULT_COALESCE_GAP: u64 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleSkipReason {
    Disabled,
    ProfileMismatch,
    OptionalUnavailable,
    NoRelevantDisagreement,
    WindowedInputUnsupported,
}

impl ScheduleSkipReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::ProfileMismatch => "profile_mismatch",
            Self::OptionalUnavailable => "optional_unavailable",
            Self::NoRelevantDisagreement => "no_relevant_disagreement",
            Self::WindowedInputUnsupported => "windowed_input_unsupported",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Disabled => "disabled by workflow execution policy",
            Self::ProfileMismatch => "available only in the Maximum analysis profile",
            Self::OptionalUnavailable => "optional expert is not currently usable",
            Self::NoRelevantDisagreement => "no relevant disagreement region was produced",
            Self::WindowedInputUnsupported => {
                "expert cannot safely consume bounded disagreement windows"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledExecution {
    FullInput,
    Windows(Vec<TimeRange>),
    Skip(ScheduleSkipReason),
}

#[derive(Debug, Clone, Copy)]
pub struct ConditionalScheduleRequest<'a> {
    pub capability: &'a str,
    pub policy: WorkflowExecutionPolicyV1,
    pub profile: AnalysisProfile,
    pub source_range: TimeRange,
    pub review_regions: &'a [SingingReviewRegion],
    pub relevant_reasons: &'a [SingingReviewReason],
    pub optional_usable: bool,
    pub required: bool,
    pub supports_windowed_input: bool,
    /// An expert with an explicit whole-source contract may run the full input
    /// when disagreement exists but bounded windows are not supported.
    pub full_input_on_disagreement: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConditionalScheduleRecordV1 {
    pub scheduler: &'static str,
    pub capability: String,
    pub policy: WorkflowExecutionPolicyV1,
    pub decision: String,
    pub windows: Vec<TimeRange>,
}

impl ConditionalScheduleRecordV1 {
    pub fn new(
        capability: &str,
        policy: WorkflowExecutionPolicyV1,
        scheduled: &ScheduledExecution,
    ) -> Self {
        let (decision, windows) = match scheduled {
            ScheduledExecution::FullInput => ("full_input".to_string(), Vec::new()),
            ScheduledExecution::Windows(windows) => {
                ("bounded_windows".to_string(), windows.clone())
            }
            ScheduledExecution::Skip(reason) => (format!("skipped:{}", reason.code()), Vec::new()),
        };
        Self {
            scheduler: CONDITIONAL_SCHEDULER_VERSION,
            capability: capability.to_string(),
            policy,
            decision,
            windows,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledWindow {
    pub index: usize,
    pub canonical_range: TimeRange,
    pub local_range: TimeRange,
}

pub fn schedule(request: ConditionalScheduleRequest<'_>) -> EngineResult<ScheduledExecution> {
    if request.source_range.end <= request.source_range.start {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional scheduler source range is empty",
        )
        .with_capability(request.capability));
    }
    if request.policy == WorkflowExecutionPolicyV1::Disabled {
        return Ok(ScheduledExecution::Skip(ScheduleSkipReason::Disabled));
    }
    if request.policy == WorkflowExecutionPolicyV1::MaximumOnly
        && request.profile != AnalysisProfile::Maximum
    {
        return Ok(ScheduledExecution::Skip(
            ScheduleSkipReason::ProfileMismatch,
        ));
    }
    if !request.optional_usable {
        if request.required {
            return Err(EngineError::new(
                EngineErrorCode::MissingCapability,
                "required conditional expert is not currently usable",
            )
            .with_capability(request.capability));
        }
        return Ok(ScheduledExecution::Skip(
            ScheduleSkipReason::OptionalUnavailable,
        ));
    }

    match request.policy {
        WorkflowExecutionPolicyV1::Always | WorkflowExecutionPolicyV1::MaximumOnly => {
            Ok(ScheduledExecution::FullInput)
        }
        WorkflowExecutionPolicyV1::Disabled => unreachable!("disabled policy returned above"),
        WorkflowExecutionPolicyV1::OnDisagreement
        | WorkflowExecutionPolicyV1::DisagreementWindows => {
            let ranges = disagreement_windows(
                request.source_range,
                request.review_regions,
                request.relevant_reasons,
                DEFAULT_REGION_PADDING,
                DEFAULT_COALESCE_GAP,
            );
            if ranges.is_empty() {
                Ok(ScheduledExecution::Skip(
                    ScheduleSkipReason::NoRelevantDisagreement,
                ))
            } else if request.supports_windowed_input {
                Ok(ScheduledExecution::Windows(ranges))
            } else if request.full_input_on_disagreement {
                Ok(ScheduledExecution::FullInput)
            } else {
                Ok(ScheduledExecution::Skip(
                    ScheduleSkipReason::WindowedInputUnsupported,
                ))
            }
        }
    }
}

pub fn disagreement_windows(
    source_range: TimeRange,
    review_regions: &[SingingReviewRegion],
    relevant_reasons: &[SingingReviewReason],
    padding: u64,
    coalesce_gap: u64,
) -> Vec<TimeRange> {
    let mut ranges = review_regions
        .iter()
        .filter(|region| {
            relevant_reasons.is_empty()
                || region
                    .reasons
                    .iter()
                    .any(|reason| relevant_reasons.contains(reason))
        })
        .filter_map(|region| {
            let start = region
                .range
                .start
                .saturating_sub(padding)
                .max(source_range.start);
            let end = region
                .range
                .end
                .saturating_add(padding)
                .min(source_range.end);
            (end > start).then_some(TimeRange { start, end })
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| (range.start, range.end));

    let mut merged: Vec<TimeRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end.saturating_add(coalesce_gap)
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

pub fn execute_scheduled<T, F>(
    scheduled: &ScheduledExecution,
    source_range: TimeRange,
    cancellation: &CancellationToken,
    mut execute: F,
) -> EngineResult<Vec<T>>
where
    F: FnMut(ScheduledWindow) -> EngineResult<T>,
{
    let full_input;
    let ranges = match scheduled {
        ScheduledExecution::FullInput => {
            if source_range.end <= source_range.start {
                return Err(EngineError::new(
                    EngineErrorCode::TimelineInvalid,
                    "scheduled full input range is empty",
                ));
            }
            full_input = [source_range];
            &full_input[..]
        }
        ScheduledExecution::Skip(_) => return Ok(Vec::new()),
        ScheduledExecution::Windows(ranges) => ranges,
    };
    let mut outputs = Vec::with_capacity(ranges.len());
    for (index, canonical_range) in ranges.iter().copied().enumerate() {
        if cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "conditional expert scheduling was cancelled",
            ));
        }
        let duration = canonical_range.end.saturating_sub(canonical_range.start);
        let output = execute(ScheduledWindow {
            index,
            canonical_range,
            local_range: TimeRange {
                start: 0,
                end: duration,
            },
        })?;
        outputs.push(output);
    }
    if cancellation.is_cancelled() {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "conditional expert scheduling was cancelled",
        ));
    }
    Ok(outputs)
}

pub fn local_to_canonical(window: ScheduledWindow, local: TimeRange) -> EngineResult<TimeRange> {
    if local.end <= local.start || local.end > window.local_range.end {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional expert output is outside its scheduled window",
        ));
    }
    let start = window
        .canonical_range
        .start
        .checked_add(local.start)
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::TimelineInvalid,
                "conditional expert output timeline overflows",
            )
        })?;
    let end = window
        .canonical_range
        .start
        .checked_add(local.end)
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::TimelineInvalid,
                "conditional expert output timeline overflows",
            )
        })?;
    Ok(TimeRange { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: &str, start: u64, end: u64, reason: SingingReviewReason) -> SingingReviewRegion {
        SingingReviewRegion {
            id: id.to_string(),
            range: TimeRange { start, end },
            confidence: None,
            reasons: vec![reason],
            evidence_experts: vec!["baseline".to_string()],
            reviewed: false,
        }
    }

    fn request<'a>(
        policy: WorkflowExecutionPolicyV1,
        profile: AnalysisProfile,
        regions: &'a [SingingReviewRegion],
    ) -> ConditionalScheduleRequest<'a> {
        ConditionalScheduleRequest {
            capability: "pitch.secondary",
            policy,
            profile,
            source_range: TimeRange {
                start: 1_000_000,
                end: 11_000_000,
            },
            review_regions: regions,
            relevant_reasons: &[SingingReviewReason::PitchDisagreement],
            optional_usable: true,
            required: false,
            supports_windowed_input: true,
            full_input_on_disagreement: false,
        }
    }

    #[test]
    fn always_disabled_and_maximum_only_are_truthful() {
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::Always,
                AnalysisProfile::Fast,
                &[]
            ))
            .unwrap(),
            ScheduledExecution::FullInput
        );
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::Disabled,
                AnalysisProfile::Maximum,
                &[]
            ))
            .unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::Disabled)
        );
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::MaximumOnly,
                AnalysisProfile::Balanced,
                &[]
            ))
            .unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::ProfileMismatch)
        );
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::MaximumOnly,
                AnalysisProfile::Maximum,
                &[]
            ))
            .unwrap(),
            ScheduledExecution::FullInput
        );
    }

    #[test]
    fn always_executes_exactly_once_and_typed_record_preserves_decision() {
        let scheduled = schedule(request(
            WorkflowExecutionPolicyV1::Always,
            AnalysisProfile::Balanced,
            &[],
        ))
        .unwrap();
        let cancellation = CancellationToken::default();
        let mut calls = 0;
        let output = execute_scheduled(
            &scheduled,
            TimeRange {
                start: 1_000_000,
                end: 11_000_000,
            },
            &cancellation,
            |window| {
                calls += 1;
                Ok(window.canonical_range)
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(
            output,
            [TimeRange {
                start: 1_000_000,
                end: 11_000_000
            }]
        );
        let record = ConditionalScheduleRecordV1::new(
            "pitch.secondary",
            WorkflowExecutionPolicyV1::Always,
            &scheduled,
        );
        assert_eq!(record.decision, "full_input");
        assert!(record.windows.is_empty());
    }

    #[test]
    fn disagreement_policy_skips_without_relevant_regions() {
        let regions = [region(
            "boundary",
            2_000_000,
            2_200_000,
            SingingReviewReason::BoundaryDisagreement,
        )];
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::OnDisagreement,
                AnalysisProfile::Balanced,
                &regions
            ))
            .unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::NoRelevantDisagreement)
        );
    }

    #[test]
    fn nearby_regions_coalesce_and_disjoint_regions_remain_distinct() {
        let regions = [
            region(
                "a",
                2_000_000,
                2_200_000,
                SingingReviewReason::PitchDisagreement,
            ),
            region(
                "b",
                2_450_000,
                2_600_000,
                SingingReviewReason::PitchDisagreement,
            ),
            region(
                "c",
                8_000_000,
                8_100_000,
                SingingReviewReason::PitchDisagreement,
            ),
        ];
        assert_eq!(
            schedule(request(
                WorkflowExecutionPolicyV1::DisagreementWindows,
                AnalysisProfile::Balanced,
                &regions
            ))
            .unwrap(),
            ScheduledExecution::Windows(vec![
                TimeRange {
                    start: 1_750_000,
                    end: 2_850_000,
                },
                TimeRange {
                    start: 7_750_000,
                    end: 8_350_000,
                },
            ])
        );
    }

    #[test]
    fn optional_unavailability_degrades_but_required_loss_fails_closed() {
        let mut optional = request(
            WorkflowExecutionPolicyV1::Always,
            AnalysisProfile::Balanced,
            &[],
        );
        optional.optional_usable = false;
        assert_eq!(
            schedule(optional).unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::OptionalUnavailable)
        );
        let mut disabled = optional;
        disabled.policy = WorkflowExecutionPolicyV1::Disabled;
        assert_eq!(
            schedule(disabled).unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::Disabled),
            "Disabled remains authoritative even when the optional resource is absent"
        );
        optional.required = true;
        assert_eq!(
            schedule(optional).unwrap_err().code,
            EngineErrorCode::MissingCapability
        );
    }

    #[test]
    fn unsupported_bounded_contract_never_silently_runs_full_input() {
        let regions = [region(
            "pitch",
            2_000_000,
            2_200_000,
            SingingReviewReason::PitchDisagreement,
        )];
        let mut value = request(
            WorkflowExecutionPolicyV1::OnDisagreement,
            AnalysisProfile::Balanced,
            &regions,
        );
        value.supports_windowed_input = false;
        assert_eq!(
            schedule(value).unwrap(),
            ScheduledExecution::Skip(ScheduleSkipReason::WindowedInputUnsupported)
        );
        value.full_input_on_disagreement = true;
        assert_eq!(
            schedule(value).unwrap(),
            ScheduledExecution::FullInput,
            "whole-source fallback requires an explicit expert contract"
        );
    }

    #[test]
    fn canonical_mapping_and_cancellation_are_deterministic() {
        let scheduled = ScheduledExecution::Windows(vec![
            TimeRange {
                start: 2_000_000,
                end: 2_500_000,
            },
            TimeRange {
                start: 4_000_000,
                end: 4_500_000,
            },
        ]);
        let cancellation = CancellationToken::default();
        let trigger = cancellation.clone();
        let mut calls = 0;
        let error = execute_scheduled(
            &scheduled,
            TimeRange {
                start: 1_000_000,
                end: 5_000_000,
            },
            &cancellation,
            |window| {
                calls += 1;
                let mapped = local_to_canonical(
                    window,
                    TimeRange {
                        start: 100_000,
                        end: 200_000,
                    },
                )?;
                assert_eq!(mapped.start, window.canonical_range.start + 100_000);
                trigger.cancel();
                Ok(mapped)
            },
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.code, EngineErrorCode::Cancelled);
    }
}
