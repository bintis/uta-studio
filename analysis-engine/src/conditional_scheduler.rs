// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

use std::path::{Path, PathBuf};
use std::time::Duration;

use uta_runtime_manager::ResolvedModel;

use crate::artifact::{
    BasicPitchEvidenceV3, BasicPitchFrameV3, PitchEvidenceV03, TranscriptArtifactV1,
    parse_basic_pitch_evidence, parse_fcpe_pitch, parse_firered_transcript,
};
use crate::audio::extract_audio_window;
use crate::contract::{AnalysisProfile, EngineError, EngineErrorCode, EngineResult};
use crate::execution::{
    CancellationToken, NativeTask, NativeTaskOutput, SupervisedWorker, WorkerExpectation,
};
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

pub(crate) fn run_firered_schedule(
    model: Option<&ResolvedModel>,
    analysis_input: &Path,
    output_root: &Path,
    scheduled: &ScheduledExecution,
    cancellation: &CancellationToken,
) -> EngineResult<Option<TranscriptArtifactV1>> {
    match (scheduled, model) {
        (ScheduledExecution::Skip(_), _) | (ScheduledExecution::FullInput, None) => Ok(None),
        (ScheduledExecution::FullInput, Some(model)) => {
            let outputs = run_native_task(
                model,
                "task-firered",
                "speech.transcribe.challenger",
                analysis_input,
                &output_root.join("worker/firered"),
                cancellation,
            )?;
            parse_firered_transcript(typed_worker_output(&outputs, "transcript_evidence")?)
                .map(Some)
        }
        (ScheduledExecution::Windows(_), _) => Err(EngineError::new(
            EngineErrorCode::InternalError,
            "FireRed window execution was scheduled without a bounded transcript contract",
        )),
    }
}

pub(crate) fn run_fcpe_schedule(
    model: &ResolvedModel,
    ffmpeg: &Path,
    analysis_input: &Path,
    output_root: &Path,
    source_range: TimeRange,
    scheduled: &ScheduledExecution,
    cancellation: &CancellationToken,
) -> EngineResult<Option<PitchEvidenceV03>> {
    match scheduled {
        ScheduledExecution::Skip(_) => Ok(None),
        ScheduledExecution::FullInput => run_fcpe_task(
            model,
            analysis_input,
            output_root.join("worker/fcpe"),
            source_range,
            "task-fcpe",
            cancellation,
        )
        .map(Some),
        ScheduledExecution::Windows(ranges) => {
            let mut evidence = Vec::with_capacity(ranges.len());
            for (index, range) in ranges.iter().copied().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(cancelled());
                }
                let range = align_range_to_grid(source_range, range, 10_000)?;
                let directory = output_root.join(format!("worker/conditional/fcpe/{index:04}"));
                let input = prepare_window_input(
                    ffmpeg,
                    analysis_input,
                    &directory,
                    source_range,
                    range,
                    cancellation,
                )?;
                evidence.push(run_fcpe_task(
                    model,
                    &input,
                    directory.join("output"),
                    range,
                    &format!("task-fcpe-{index:04}"),
                    cancellation,
                )?);
            }
            merge_fcpe_windows(source_range, evidence).map(Some)
        }
    }
}

pub(crate) fn run_basic_pitch_schedule(
    model: &ResolvedModel,
    ffmpeg: &Path,
    analysis_input: &Path,
    output_root: &Path,
    source_range: TimeRange,
    scheduled: &ScheduledExecution,
    cancellation: &CancellationToken,
) -> EngineResult<Option<BasicPitchEvidenceV3>> {
    match scheduled {
        ScheduledExecution::Skip(_) => Ok(None),
        ScheduledExecution::FullInput => run_basic_pitch_task(
            model,
            analysis_input,
            output_root.join("worker/basic-pitch"),
            source_range,
            "task-basic-pitch",
            cancellation,
        )
        .map(Some),
        ScheduledExecution::Windows(ranges) => {
            let mut evidence = Vec::with_capacity(ranges.len());
            for (index, range) in ranges.iter().copied().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(cancelled());
                }
                let directory =
                    output_root.join(format!("worker/conditional/basic-pitch/{index:04}"));
                let input = prepare_window_input(
                    ffmpeg,
                    analysis_input,
                    &directory,
                    source_range,
                    range,
                    cancellation,
                )?;
                evidence.push(run_basic_pitch_task(
                    model,
                    &input,
                    directory.join("output"),
                    range,
                    &format!("task-basic-pitch-{index:04}"),
                    cancellation,
                )?);
            }
            merge_basic_pitch_windows(evidence).map(Some)
        }
    }
}

fn run_fcpe_task(
    model: &ResolvedModel,
    input: &Path,
    output_dir: PathBuf,
    range: TimeRange,
    task_id: &str,
    cancellation: &CancellationToken,
) -> EngineResult<PitchEvidenceV03> {
    let outputs = run_native_task(
        model,
        task_id,
        "pitch.secondary",
        input,
        &output_dir,
        cancellation,
    )?;
    parse_fcpe_pitch(
        typed_worker_output(&outputs, "pitch_evidence")?,
        range.start,
        range.end.saturating_sub(range.start),
    )
}

fn run_basic_pitch_task(
    model: &ResolvedModel,
    input: &Path,
    output_dir: PathBuf,
    range: TimeRange,
    task_id: &str,
    cancellation: &CancellationToken,
) -> EngineResult<BasicPitchEvidenceV3> {
    let outputs = run_native_task(
        model,
        task_id,
        "notes.basic_pitch",
        input,
        &output_dir,
        cancellation,
    )?;
    parse_basic_pitch_evidence(
        typed_worker_output(&outputs, "basic_pitch_evidence")?,
        range.start,
        range.end.saturating_sub(range.start),
    )
}

fn run_native_task(
    model: &ResolvedModel,
    task_id: &str,
    node_id: &str,
    input: &Path,
    output_dir: &Path,
    cancellation: &CancellationToken,
) -> EngineResult<Vec<NativeTaskOutput>> {
    fresh_directory(output_dir)?;
    SupervisedWorker::run(
        &model.runtime_executable,
        &WorkerExpectation {
            component: "uta-openvino-worker".to_string(),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
        &NativeTask {
            task_id: task_id.to_string(),
            node_id: node_id.to_string(),
            presentation_node_id: None,
            model_id: model.model_id.clone(),
            input_artifacts: vec![input.to_path_buf()],
            output_dir: output_dir.to_path_buf(),
            config: serde_json::json!({
                "model_path": model.model_path,
                "backend": openvino_backend(model)?
            }),
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )
}

fn openvino_backend(model: &ResolvedModel) -> EngineResult<&'static str> {
    match model.backend {
        uta_runtime_manager::NativeBackend::OpenVino => Ok("openvino_gpu"),
        uta_runtime_manager::NativeBackend::CpuReference => Ok("openvino_cpu"),
        _ => Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved to a backend unsupported by the OpenVINO worker",
                model.model_id
            ),
        )),
    }
}

fn prepare_window_input(
    ffmpeg: &Path,
    source: &Path,
    directory: &Path,
    source_range: TimeRange,
    window: TimeRange,
    cancellation: &CancellationToken,
) -> EngineResult<PathBuf> {
    if window.start < source_range.start || window.end > source_range.end {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional window is outside the source timeline",
        ));
    }
    fresh_directory(directory)?;
    let input = directory.join("window.flac");
    extract_audio_window(
        ffmpeg,
        source,
        &input,
        window.start.saturating_sub(source_range.start),
        window.end.saturating_sub(window.start),
        cancellation,
    )?;
    Ok(input)
}

fn fresh_directory(path: &Path) -> EngineResult<()> {
    let parent = path.parent().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "conditional task directory has no parent",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create conditional task parent: {error}"),
        )
    })?;
    std::fs::create_dir(path).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("conditional task directory already exists or cannot be created: {error}"),
        )
    })
}

fn typed_worker_output<'a>(
    outputs: &'a [NativeTaskOutput],
    artifact: &str,
) -> EngineResult<&'a Path> {
    let mut matching = outputs.iter().filter(|output| output.artifact == artifact);
    let output = matching.next().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker omitted required typed output {artifact}"),
        )
    })?;
    if matching.next().is_some() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker emitted duplicate typed output {artifact}"),
        ));
    }
    Ok(&output.path)
}

fn align_range_to_grid(
    source: TimeRange,
    requested: TimeRange,
    hop: u64,
) -> EngineResult<TimeRange> {
    if hop == 0 || requested.start < source.start || requested.end > source.end {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional range cannot be aligned to the source grid",
        ));
    }
    let relative_start = requested.start.saturating_sub(source.start);
    let relative_end = requested.end.saturating_sub(source.start);
    let source_duration = source.end.saturating_sub(source.start);
    let start = relative_start / hop * hop;
    let end = relative_end
        .saturating_add(hop - 1)
        .checked_div(hop)
        .and_then(|value| value.checked_mul(hop))
        .unwrap_or(source_duration)
        .min(source_duration);
    if end <= start {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional range collapsed while aligning to the source grid",
        ));
    }
    Ok(TimeRange {
        start: source.start.saturating_add(start),
        end: source.start.saturating_add(end),
    })
}

fn merge_fcpe_windows(
    source_range: TimeRange,
    windows: Vec<PitchEvidenceV03>,
) -> EngineResult<PitchEvidenceV03> {
    let mut iter = windows.into_iter();
    let first = iter.next().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "conditional FCPE schedule produced no evidence",
        )
    })?;
    if first.hop == 0 || first.start < source_range.start {
        return Err(invalid_merge("conditional FCPE evidence grid is invalid"));
    }
    let frame_count = source_range
        .end
        .saturating_sub(source_range.start)
        .checked_div(first.hop)
        .and_then(|count| count.checked_add(1))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| invalid_merge("conditional FCPE frame count overflows"))?;
    let mut frequency_hz = vec![None; frame_count];
    let mut confidence = vec![None; frame_count];
    let mut filled = vec![false; frame_count];
    let identity = (
        first.format.clone(),
        first.format_version.clone(),
        first.timebase,
        first.hop,
        first.model.clone(),
    );
    merge_fcpe_part(
        source_range,
        &first,
        &mut frequency_hz,
        &mut confidence,
        &mut filled,
    )?;
    for part in iter {
        if (
            part.format.clone(),
            part.format_version.clone(),
            part.timebase,
            part.hop,
            part.model.clone(),
        ) != identity
        {
            return Err(invalid_merge(
                "conditional FCPE windows have inconsistent identities",
            ));
        }
        merge_fcpe_part(
            source_range,
            &part,
            &mut frequency_hz,
            &mut confidence,
            &mut filled,
        )?;
    }
    Ok(PitchEvidenceV03 {
        format: identity.0,
        format_version: identity.1,
        timebase: identity.2,
        start: source_range.start,
        hop: identity.3,
        frequency_hz,
        confidence,
        model: identity.4,
    })
}

fn merge_fcpe_part(
    source_range: TimeRange,
    part: &PitchEvidenceV03,
    frequency_hz: &mut [Option<f64>],
    confidence: &mut [Option<f64>],
    filled: &mut [bool],
) -> EngineResult<()> {
    if part.frequency_hz.len() != part.confidence.len()
        || part.start < source_range.start
        || !part
            .start
            .saturating_sub(source_range.start)
            .is_multiple_of(part.hop)
    {
        return Err(invalid_merge(
            "conditional FCPE evidence cannot map to the canonical grid",
        ));
    }
    let offset = usize::try_from(part.start.saturating_sub(source_range.start) / part.hop)
        .map_err(|_| invalid_merge("conditional FCPE offset overflows"))?;
    for (index, (hz, score)) in part
        .frequency_hz
        .iter()
        .copied()
        .zip(part.confidence.iter().copied())
        .enumerate()
    {
        let target = offset
            .checked_add(index)
            .filter(|target| *target < frequency_hz.len())
            .ok_or_else(|| invalid_merge("conditional FCPE evidence exceeds source duration"))?;
        if filled[target] && (frequency_hz[target] != hz || confidence[target] != score) {
            return Err(invalid_merge(
                "overlapping conditional FCPE windows produced conflicting evidence",
            ));
        }
        frequency_hz[target] = hz;
        confidence[target] = score;
        filled[target] = true;
    }
    Ok(())
}

fn merge_basic_pitch_windows(
    windows: Vec<BasicPitchEvidenceV3>,
) -> EngineResult<BasicPitchEvidenceV3> {
    let mut iter = windows.into_iter();
    let first = iter
        .next()
        .ok_or_else(|| invalid_merge("conditional Basic Pitch schedule produced no evidence"))?;
    let model_manifest_sha256 = first.model_manifest_sha256;
    let runtime_manifest_sha256 = first.runtime_manifest_sha256;
    let mut frames = first.frames;
    for window in iter {
        frames.extend(window.frames);
    }
    frames.sort_by_key(|frame| frame.time);
    let mut merged: Vec<BasicPitchFrameV3> = Vec::with_capacity(frames.len());
    for frame in frames {
        if let Some(previous) = merged.last()
            && previous.time == frame.time
        {
            if previous != &frame {
                return Err(invalid_merge(
                    "overlapping Basic Pitch windows produced conflicting evidence",
                ));
            }
            continue;
        }
        merged.push(frame);
    }
    Ok(BasicPitchEvidenceV3 {
        frames: merged,
        model_manifest_sha256,
        runtime_manifest_sha256,
    })
}

fn invalid_merge(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

fn cancelled() -> EngineError {
    EngineError::new(
        EngineErrorCode::Cancelled,
        "conditional expert scheduling was cancelled",
    )
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
