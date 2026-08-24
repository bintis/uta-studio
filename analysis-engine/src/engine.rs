use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use uta_runtime_manager::{RuntimeManager, StorePaths};

use crate::artifact::{
    AdvancedNoteEvidenceV1, AlignmentArtifactV1, BasicPitchEvidenceV3, DependencyKind,
    PitchEvidenceV03, SingingAnalysisV1, TranscriptArtifactV1, TranscriptAuthorityV1,
    TranscriptTokenV1, artifact_ref_for_existing, finalize_candidate_vocal_chart,
    parse_advanced_note_evidence, parse_game_evidence, parse_qwen_alignment, parse_qwen_transcript,
    parse_rmvpe_pitch, write_json_artifact,
};
use crate::audio::{analyze_acoustic_evidence, decode_audio, decode_audio_with_cancellation};
use crate::candidate_pipeline::{
    SingingStagesOutput, build_baseline_review_regions, execute_candidate_graph_stage,
    execute_singing_fusion_stage, fuse_alignment_stage, fuse_transcript_stage,
};
use crate::conditional_scheduler::{
    ConditionalScheduleRequest, ScheduleSkipReason, ScheduledExecution, run_basic_pitch_schedule,
    run_fcpe_schedule, run_firered_schedule, schedule,
};
use crate::contract::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AnalysisArtifactsV1, AnalysisDiagnosticsV1,
    AnalysisProvenanceV1, AnalysisResultManifestV1, AnalysisStatus, AnalyzeRequestV1,
    CapabilityDescriptor, DecodedAudioFactsV1, EngineError, EngineErrorCode, EngineRequirementsV1,
    EngineResult, ExportRequestV1, LyricsMode, ResolvedResourceProvenanceV1, StemArtifactRefV1,
};
use crate::execution::{
    CancellationToken, NativeTask, NativeTaskOutput, SupervisedWorker, WorkerExpectation,
};
use crate::fingerprint::{
    ACOUSTIC_DSP_VERSION, CALIBRATION_VERSION, FINALIZE_VOCAL_CHART_VERSION, FUSION_VERSION,
    HSMM_VERSION, POSTPROCESS_VERSION, QUANTIZATION_VERSION, deterministic_fingerprint,
};
use crate::fusion::{SingingReviewReason, TimeRange};
use crate::planner::{EnginePlan, Planner};
use crate::quantization::quantize_singing_track;
use crate::separation::SeparationOutput;
use crate::workflow::{WorkflowExecutionPolicyV1, WorkflowExecutionV1};

const TRANSCRIPT_MEDIA_TYPE: &str = "application/vnd.uta.transcript+json;version=1";
const ALIGNMENT_MEDIA_TYPE: &str = "application/vnd.uta.alignment+json;version=1";
const PITCH_MEDIA_TYPE: &str = "application/vnd.uta.pitch-evidence+json;version=0.3";
const ACOUSTIC_MEDIA_TYPE: &str = "application/vnd.uta.acoustic-evidence+json;version=1";
const SINGING_ANALYSIS_MEDIA_TYPE: &str = "application/vnd.uta.singing-analysis+json;version=0.3";
const VOCAL_CHART_MEDIA_TYPE: &str = "application/vnd.uta.vocal-chart+json;version=0.3";

const PITCH_DISAGREEMENT_REASONS: &[SingingReviewReason] = &[
    SingingReviewReason::PitchDisagreement,
    SingingReviewReason::LowPitchCoverage,
    SingingReviewReason::PitchInstability,
    SingingReviewReason::OctaveRisk,
    SingingReviewReason::VoicingConflict,
];
const NOTE_DISAGREEMENT_REASONS: &[SingingReviewReason] = &[
    SingingReviewReason::BoundaryDisagreement,
    SingingReviewReason::PitchDisagreement,
    SingingReviewReason::WordNoteMismatch,
    SingingReviewReason::VoicingConflict,
];
#[derive(Debug, Serialize)]
struct ExecutionIdentity<'a> {
    request: serde_json::Value,
    resources: Vec<FingerprintResource<'a>>,
    acoustic_dsp_version: &'static str,
    calibration_version: &'static str,
    finalize_vocal_chart_version: &'static str,
    fusion_version: &'static str,
    hsmm_version: &'static str,
    quantization_version: &'static str,
    postprocess_version: &'static str,
}

#[derive(Debug, Serialize)]
struct FingerprintResource<'a> {
    model_id: &'a str,
    generation: &'a str,
    content_digest: &'a str,
    model_recipe_digest: &'a str,
    runtime_id: &'a str,
    runtime_generation: &'a str,
    runtime_recipe_digest: Option<&'a str>,
    backend: uta_runtime_manager::NativeBackend,
    device: &'static str,
}

#[derive(Debug, Clone)]
pub struct AnalysisEngine {
    runtime_manager: RuntimeManager,
}

impl AnalysisEngine {
    pub fn from_env() -> EngineResult<Self> {
        let runtime_manager = RuntimeManager::with_default_catalog(StorePaths::from_env())?;
        Ok(Self { runtime_manager })
    }

    pub fn new(runtime_manager: RuntimeManager) -> Self {
        Self { runtime_manager }
    }

    pub fn runtime_manager(&self) -> &RuntimeManager {
        &self.runtime_manager
    }

    pub fn validate(&self, request: &AnalyzeRequestV1) -> EngineResult<()> {
        request.validate()
    }

    pub fn validate_inputs(&self, request: &AnalyzeRequestV1) -> EngineResult<()> {
        self.validate_inputs_with_cancellation(request, &CancellationToken::default())
    }

    fn validate_inputs_with_cancellation(
        &self,
        request: &AnalyzeRequestV1,
        cancellation: &CancellationToken,
    ) -> EngineResult<()> {
        request.validate()?;
        for source in &request.audio_sources {
            if cancellation.is_cancelled() {
                return Err(cancelled(request));
            }
            if !source.path.is_file() {
                return Err(EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    format!("audio source is unavailable: {}", source.path.display()),
                )
                .for_request(&request.request_id));
            }
            let actual =
                sha256_file_with_cancellation(&source.path, cancellation).map_err(|error| {
                    if error.code == EngineErrorCode::Cancelled {
                        error.for_request(&request.request_id)
                    } else {
                        EngineError::new(
                            EngineErrorCode::MissingRequiredInput,
                            format!(
                                "could not read audio source {}: {}",
                                source.path.display(),
                                error.message
                            ),
                        )
                        .for_request(&request.request_id)
                    }
                })?;
            if actual != source.sha256 {
                return Err(EngineError::new(
                    EngineErrorCode::InputHashMismatch,
                    format!("audio source hash does not match: {}", source.id),
                )
                .for_request(&request.request_id));
            }
        }
        Ok(())
    }

    pub fn decoded_audio_facts(
        &self,
        request: &AnalyzeRequestV1,
    ) -> EngineResult<Vec<DecodedAudioFactsV1>> {
        self.validate_inputs(request)?;
        self.decode_validated_audio(request, &CancellationToken::default())
    }

    fn decode_validated_audio(
        &self,
        request: &AnalyzeRequestV1,
        cancellation: &CancellationToken,
    ) -> EngineResult<Vec<DecodedAudioFactsV1>> {
        let ffmpeg = self
            .runtime_manager
            .paths()
            .tool_executable("ffmpeg")
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::WorkerUnavailable,
                    "packaged ffmpeg is unavailable for audio decode",
                )
                .with_resource("tool:ffmpeg")
            })?;
        request
            .audio_sources
            .iter()
            .map(|source| {
                let facts = decode_audio_with_cancellation(
                    &ffmpeg,
                    &source.id,
                    &source.path,
                    cancellation,
                )?
                .facts;
                source
                    .timeline
                    .source_start
                    .checked_add(facts.duration)
                    .ok_or_else(|| {
                        EngineError::new(
                            EngineErrorCode::TimelineInvalid,
                            format!(
                                "decoded source {} overflows its canonical timeline",
                                source.id
                            ),
                        )
                    })?;
                Ok(facts)
            })
            .collect()
    }

    pub fn requirements(&self, request: &AnalyzeRequestV1) -> EngineResult<EngineRequirementsV1> {
        Planner::requirements(request)
    }

    pub fn plan(&self, request: &AnalyzeRequestV1) -> EngineResult<EnginePlan> {
        Planner::plan(request, Some(&self.runtime_manager))
    }

    pub fn capabilities(
        &self,
        policy: uta_runtime_manager::RuntimePolicy,
    ) -> Vec<CapabilityDescriptor> {
        Planner::capabilities(Some(&self.runtime_manager), policy)
    }

    pub fn analyze(
        &self,
        request: &AnalyzeRequestV1,
        output_dir: impl AsRef<Path>,
    ) -> EngineResult<AnalysisResultManifestV1> {
        self.analyze_with_cancellation(request, output_dir, &CancellationToken::default())
    }

    pub fn analyze_with_cancellation(
        &self,
        request: &AnalyzeRequestV1,
        output_dir: impl AsRef<Path>,
        cancellation: &CancellationToken,
    ) -> EngineResult<AnalysisResultManifestV1> {
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        self.validate_inputs_with_cancellation(request, cancellation)?;
        let mut run_guard = OutputRunGuard::new(output_dir.as_ref())?;
        let output_root = run_guard.root().to_path_buf();
        let plan = self.plan(request)?;
        Planner::ensure_required_capabilities(&plan)?;
        let workflow = WorkflowExecutionV1::from_request(request)?;
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }

        // Each resolved model owns an immutable generation lease. Keep both the
        // resolved handles and aggregate compatibility lease alive for the run.
        let (resolved, mut degraded_reasons) = self.resolve_execution_resources(request, &plan)?;
        let _lease = self.runtime_manager.lease_resolved_models(&resolved);
        let decoded_audio = self.decode_validated_audio(request, cancellation)?;

        let primary = request.primary_source()?;
        let source_start = primary.timeline.source_start;
        let source_duration = decoded_audio
            .iter()
            .find(|facts| facts.source_id == primary.id)
            .map(|facts| facts.duration)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::InternalError,
                    "decoded facts are missing for the primary source",
                )
                .for_request(&request.request_id)
            })?;
        let source_range = TimeRange {
            start: source_start,
            end: source_start.checked_add(source_duration).ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::TimelineInvalid,
                    "primary source duration overflows the canonical timeline",
                )
                .for_request(&request.request_id)
            })?,
        };
        let mut artifacts = AnalysisArtifactsV1::default();
        let ffmpeg = self
            .runtime_manager
            .paths()
            .tool_executable("ffmpeg")
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::WorkerUnavailable,
                    "packaged ffmpeg is unavailable for analysis execution",
                )
                .with_resource("tool:ffmpeg")
            })?;
        let mut analysis_input = primary.path.clone();
        let mut analysis_role = primary.role.as_str();
        if has_capability(&plan, "audio.extract_vocals") {
            let model = resolved_model(&resolved, "bs_roformer_vocals_ep317")?;
            let output = run_openvino_vocals(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    ffmpeg: &ffmpeg,
                    input: &primary.path,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-extract-vocals", request.request_id),
                },
                cancellation,
            )?;
            analysis_input = output_root.join(&output.artifact.path);
            analysis_role = crate::contract::AudioRole::GuideVocals.as_str();
            if request
                .requested_artifacts
                .stems
                .contains(&crate::contract::AudioRole::GuideVocals)
            {
                artifacts.stems.push(StemArtifactRefV1 {
                    role: output.role,
                    artifact: output.artifact,
                });
            }
        }
        if has_capability(&plan, "audio.lead_isolate") {
            let model = resolved_model(&resolved, "melband_roformer_harmony")?;
            let output = run_openvino_harmony(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    ffmpeg: &ffmpeg,
                    input: &analysis_input,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-lead-isolate", request.request_id),
                },
                cancellation,
            )?;
            analysis_input = output_root.join(&output.artifact.path);
            analysis_role = crate::contract::AudioRole::LeadVocal.as_str();
            if request
                .requested_artifacts
                .stems
                .contains(&crate::contract::AudioRole::LeadVocal)
            {
                artifacts.stems.push(StemArtifactRefV1 {
                    role: output.role,
                    artifact: output.artifact,
                });
            }
        }
        let mut cleanup_output = None;
        let mut denoise_participated = false;
        let mut dereverb_participated = false;
        if let Some(model) = resolved
            .iter()
            .find(|model| model.model_id == "melband_roformer_denoise_aufr33")
        {
            let result = run_openvino_denoise(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    ffmpeg: &ffmpeg,
                    input: &analysis_input,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-denoise", request.request_id),
                },
                cancellation,
            );
            match result {
                Ok(output) => {
                    analysis_input = output_root.join(&output.artifact.path);
                    analysis_role = crate::contract::AudioRole::CleanLeadVocal.as_str();
                    cleanup_output = Some(output);
                    denoise_participated = true;
                }
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => degraded_reasons.push(format!(
                    "optional capability audio.denoise failed: {}",
                    error.message
                )),
            }
        }
        if let Some(model) = resolved
            .iter()
            .find(|model| model.model_id == "melband_roformer_dereverb_anvuew")
        {
            let result = run_openvino_dereverb(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    ffmpeg: &ffmpeg,
                    input: &analysis_input,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-dereverb", request.request_id),
                },
                cancellation,
            );
            match result {
                Ok(output) => {
                    analysis_input = output_root.join(&output.artifact.path);
                    analysis_role = crate::contract::AudioRole::CleanLeadVocal.as_str();
                    cleanup_output = Some(output);
                    dereverb_participated = true;
                }
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => degraded_reasons.push(format!(
                    "optional capability audio.dereverb failed: {}",
                    error.message
                )),
            }
        }
        if request
            .requested_artifacts
            .stems
            .contains(&crate::contract::AudioRole::CleanLeadVocal)
            && let Some(output) = cleanup_output
        {
            artifacts.stems.push(StemArtifactRefV1 {
                role: output.role,
                artifact: output.artifact,
            });
        }
        if has_capability(&plan, "audio.extract_instrumental") {
            let model = resolved_model(&resolved, "melband_roformer_inst_v2")?;
            let output = run_openvino_instrumental(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    ffmpeg: &ffmpeg,
                    input: &primary.path,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-instrumental", request.request_id),
                },
                cancellation,
            )?;
            artifacts.stems.push(StemArtifactRefV1 {
                role: output.role,
                artifact: output.artifact,
            });
        }
        let (acoustic_evidence, acoustic_artifact) =
            if has_capability(&plan, "analysis.acoustic_dsp") {
                let evidence = analyze_acoustic_evidence(
                    &ffmpeg,
                    &analysis_input,
                    analysis_role,
                    source_start,
                    source_duration,
                    cancellation,
                )?;
                let artifact = write_json_artifact(
                    &output_root,
                    Path::new("evidence/acoustic-evidence.json"),
                    ACOUSTIC_MEDIA_TYPE,
                    &evidence,
                )?;
                (Some(evidence), Some(artifact))
            } else {
                (None, None)
            };
        let needs_transcribe = has_capability(&plan, "speech.transcribe");
        let needs_alignment = has_capability(&plan, "speech.align");
        let needs_pitch = has_capability(&plan, "pitch.track");
        let transcript_evidence = if needs_transcribe {
            let model = resolved_model(&resolved, "qwen3_asr_1_7b")?;
            let directory = create_task_dir(&output_root, "worker/asr")?;
            let outputs = run_native_task(
                model,
                "uta-qwen-asr-worker",
                "task-asr",
                "speech.transcribe",
                &analysis_input,
                &directory,
                // Qwen ASR language contract v1 is runtime-detected only. The
                // caller language remains reference metadata and is never sent
                // as an inference hint.
                serde_json::json!({"model_path": model.model_path}),
                cancellation,
            )?;
            Some(parse_qwen_transcript(typed_worker_output(
                &outputs,
                "transcript_evidence",
            )?)?)
        } else if request.lyrics.mode == LyricsMode::Canonical {
            Some(caller_transcript(request)?)
        } else {
            None
        };
        let firered_evidence = if has_capability(&plan, "speech.transcribe.challenger") {
            let model = resolved
                .iter()
                .find(|model| model.model_id == "firered_asr2_aed");
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: "speech.transcribe.challenger",
                policy: execution_policy_for(
                    workflow.as_ref(),
                    "speech.transcribe.challenger",
                    WorkflowExecutionPolicyV1::OnDisagreement,
                ),
                profile: request.analysis.profile,
                source_range,
                review_regions: &[],
                relevant_reasons: &[],
                optional_usable: model.is_some(),
                required: false,
                supports_windowed_input: false,
            })?;
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(
                    &mut degraded_reasons,
                    "speech.transcribe.challenger",
                    reason,
                );
            }
            match run_firered_schedule(
                model,
                &analysis_input,
                &output_root,
                &scheduled,
                cancellation,
            ) {
                Ok(evidence) => evidence,
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(output_root.join("worker/firered"));
                    degraded_reasons.push(format!(
                        "optional capability speech.transcribe.challenger failed: {}",
                        error.message
                    ));
                    None
                }
            }
        } else {
            None
        };
        let (transcript, canonical_lyrics) = if has_capability(&plan, "fusion.transcript") {
            let primary = transcript_evidence.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "fusion.transcript requires canonical or baseline-generated evidence",
                )
            })?;
            let reference = (request.lyrics.mode == LyricsMode::Reference)
                .then(|| request_lyrics_text(request));
            let (mut artifact, mut canonical) =
                fuse_transcript_stage(std::slice::from_ref(primary), reference.as_deref())?;
            // FireRed is a challenger only. Preserve disagreement as an
            // alternative; it can never replace caller/Qwen canonical lyrics.
            if let Some(challenger) = firered_evidence.as_ref()
                && normalized_transcript(&challenger.text) != normalized_transcript(&artifact.text)
            {
                if !artifact.alternatives.contains(&challenger.text) {
                    artifact.alternatives.push(challenger.text.clone());
                }
                if !canonical.alternatives.contains(&challenger.text) {
                    canonical.alternatives.push(challenger.text.clone());
                }
            }
            (Some(artifact), Some(canonical))
        } else {
            (None, None)
        };
        if request.requested_artifacts.transcript {
            let value = transcript.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::OutputValidationFailed,
                    "requested transcript was not produced",
                )
            })?;
            artifacts.transcript = Some(write_json_artifact(
                &output_root,
                Path::new("transcript/transcript.json"),
                TRANSCRIPT_MEDIA_TYPE,
                value,
            )?);
        }

        let alignment_evidence: Option<AlignmentArtifactV1> = if needs_alignment {
            let transcript = transcript.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "alignment requires canonical or transcribed text",
                )
            })?;
            let model = resolved_model(&resolved, "qwen3_forced_aligner_0_6b")?;
            let directory = create_task_dir(&output_root, "worker/alignment")?;
            let outputs = run_native_task(
                model,
                "uta-qwen-align-worker",
                "task-align",
                "speech.align",
                &analysis_input,
                &directory,
                serde_json::json!({
                    "model_path": model.model_path,
                    "text": transcript.text,
                    "language": transcript.language
                }),
                cancellation,
            )?;
            Some(parse_qwen_alignment(
                typed_worker_output(&outputs, "alignment_evidence")?,
                source_start,
                source_duration,
            )?)
        } else {
            None
        };
        let (alignment, canonical_words) = if has_capability(&plan, "fusion.alignment") {
            let transcript = canonical_lyrics.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "fusion.alignment requires canonical transcript",
                )
            })?;
            let evidence = alignment_evidence.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "fusion.alignment requires alignment evidence",
                )
            })?;
            let (artifact, words) = fuse_alignment_stage(
                transcript,
                std::slice::from_ref(evidence),
                source_start,
                source_duration,
            )?;
            (Some(artifact), Some(words))
        } else {
            (None, None)
        };
        if request.requested_artifacts.alignment {
            artifacts.alignment = Some(write_json_artifact(
                &output_root,
                Path::new("alignment/alignment.json"),
                ALIGNMENT_MEDIA_TYPE,
                alignment.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "requested alignment was not produced",
                    )
                })?,
            )?);
        }

        let pitch_evidence = if needs_pitch {
            let model = resolved_model(&resolved, "rmvpe")?;
            let directory = create_task_dir(&output_root, "worker/rmvpe")?;
            let outputs = run_native_task(
                model,
                "uta-openvino-worker",
                "task-rmvpe",
                "pitch.track",
                &analysis_input,
                &directory,
                serde_json::json!({"model_path": model.model_path}),
                cancellation,
            )?;
            let pitch = parse_rmvpe_pitch(
                typed_worker_output(&outputs, "pitch_evidence")?,
                source_start,
                source_duration,
            )?;
            if request.requested_artifacts.pitch_evidence {
                artifacts.pitch_evidence = Some(write_json_artifact(
                    &output_root,
                    Path::new("pitch/pitch-evidence.json"),
                    PITCH_MEDIA_TYPE,
                    &pitch,
                )?);
            }
            Some(pitch)
        } else {
            None
        };
        let mut fcpe_evidence: Option<PitchEvidenceV03> = None;
        let mut basic_pitch_evidence: Option<BasicPitchEvidenceV3> = None;
        let game_evidence = if has_capability(&plan, "notes.game") {
            let model = resolved_model(&resolved, "game")?;
            let directory = create_task_dir(&output_root, "worker/game")?;
            let outputs = run_native_task(
                model,
                "uta-openvino-worker",
                "task-game",
                "notes.game",
                &analysis_input,
                &directory,
                serde_json::json!({
                    "model_path": model.model_path,
                    "language": request.lyrics.language
                }),
                cancellation,
            )?;
            Some(parse_game_evidence(
                typed_worker_output(&outputs, "note_candidate_evidence")?,
                source_start,
                source_duration,
            )?)
        } else {
            None
        };

        let baseline_review_regions = match (
            transcript.as_ref(),
            alignment.as_ref(),
            canonical_lyrics.clone(),
            canonical_words.clone(),
            game_evidence.as_ref(),
            acoustic_evidence.as_ref(),
        ) {
            (
                Some(transcript),
                Some(alignment),
                Some(lyrics),
                Some(words),
                Some(game),
                Some(acoustic),
            ) if has_capability(&plan, "fusion.candidate_graph") => build_baseline_review_regions(
                transcript,
                lyrics,
                alignment,
                words,
                pitch_evidence.as_ref(),
                game,
                acoustic,
                source_start,
                source_duration,
            )?,
            _ => Vec::new(),
        };
        if has_capability(&plan, "pitch.secondary") {
            let model = resolved.iter().find(|model| model.model_id == "fcpe");
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: "pitch.secondary",
                policy: execution_policy_for(
                    workflow.as_ref(),
                    "pitch.secondary",
                    WorkflowExecutionPolicyV1::DisagreementWindows,
                ),
                profile: request.analysis.profile,
                source_range,
                review_regions: &baseline_review_regions,
                relevant_reasons: PITCH_DISAGREEMENT_REASONS,
                optional_usable: model.is_some(),
                required: false,
                supports_windowed_input: true,
            })
            .map_err(|error| error.for_request(&request.request_id))?;
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(&mut degraded_reasons, "pitch.secondary", reason);
            } else if let Some(model) = model {
                match run_fcpe_schedule(
                    model,
                    &ffmpeg,
                    &analysis_input,
                    &output_root,
                    source_range,
                    &scheduled,
                    cancellation,
                ) {
                    Ok(evidence) => fcpe_evidence = evidence,
                    Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(output_root.join("worker/fcpe"));
                        let _ =
                            std::fs::remove_dir_all(output_root.join("worker/conditional/fcpe"));
                        degraded_reasons.push(format!(
                            "optional capability pitch.secondary failed: {}",
                            error.message
                        ));
                    }
                }
            }
        }
        if has_capability(&plan, "notes.basic_pitch") {
            let model = resolved
                .iter()
                .find(|model| model.model_id == "basic_pitch");
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: "notes.basic_pitch",
                policy: execution_policy_for(
                    workflow.as_ref(),
                    "notes.basic_pitch",
                    WorkflowExecutionPolicyV1::OnDisagreement,
                ),
                profile: request.analysis.profile,
                source_range,
                review_regions: &baseline_review_regions,
                relevant_reasons: NOTE_DISAGREEMENT_REASONS,
                optional_usable: model.is_some(),
                required: false,
                supports_windowed_input: true,
            })
            .map_err(|error| error.for_request(&request.request_id))?;
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(&mut degraded_reasons, "notes.basic_pitch", reason);
            } else if let Some(model) = model {
                match run_basic_pitch_schedule(
                    model,
                    &ffmpeg,
                    &analysis_input,
                    &output_root,
                    source_range,
                    &scheduled,
                    cancellation,
                ) {
                    Ok(evidence) => basic_pitch_evidence = evidence,
                    Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(output_root.join("worker/basic-pitch"));
                        let _ = std::fs::remove_dir_all(
                            output_root.join("worker/conditional/basic-pitch"),
                        );
                        degraded_reasons.push(format!(
                            "optional capability notes.basic_pitch failed: {}",
                            error.message
                        ));
                    }
                }
            }
        }
        let mut advanced_note_evidence = Vec::new();
        for model_id in ["rosvot", "stars"] {
            let Some(model) = resolved.iter().find(|model| model.model_id == model_id) else {
                continue;
            };
            let capability = format!("notes.{model_id}");
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: &capability,
                policy: execution_policy_for(
                    workflow.as_ref(),
                    &capability,
                    WorkflowExecutionPolicyV1::MaximumOnly,
                ),
                profile: request.analysis.profile,
                source_range,
                review_regions: &baseline_review_regions,
                relevant_reasons: NOTE_DISAGREEMENT_REASONS,
                optional_usable: true,
                required: false,
                supports_windowed_input: false,
            })?;
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(&mut degraded_reasons, &capability, reason);
                continue;
            }
            let result = run_advanced_note_challenger(
                model,
                &analysis_input,
                &output_root,
                canonical_words.as_deref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "advanced-note challenger requires TimedTranscript word boundaries",
                    )
                })?,
                source_start,
                source_duration,
                cancellation,
            );
            match result {
                Ok(evidence) => advanced_note_evidence.push(evidence),
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(output_root.join(format!("worker/{model_id}")));
                    degraded_reasons.push(format!(
                        "optional capability notes.{model_id} failed: {}",
                        error.message
                    ));
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        let singing_fusion = if has_capability(&plan, "fusion.singing") {
            Some(execute_singing_fusion_stage(
                transcript.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.singing requires fused transcript",
                    )
                })?,
                alignment.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.singing requires fused alignment",
                    )
                })?,
                canonical_words.as_deref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.singing requires canonical word boundaries",
                    )
                })?,
                pitch_evidence.as_ref(),
                fcpe_evidence.as_ref(),
                basic_pitch_evidence.as_ref(),
                game_evidence.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.singing requires GAME evidence",
                    )
                })?,
                acoustic_evidence.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.singing requires acoustic DSP evidence",
                    )
                })?,
                &advanced_note_evidence,
                source_start,
                source_duration,
            )?)
        } else {
            None
        };
        let mut singing = if has_capability(&plan, "fusion.candidate_graph") {
            Some(execute_candidate_graph_stage(
                canonical_lyrics.clone().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.candidate_graph requires canonical lyrics",
                    )
                })?,
                canonical_words.clone().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.candidate_graph requires canonical word boundaries",
                    )
                })?,
                singing_fusion.ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "fusion.candidate_graph requires singing candidates",
                    )
                })?,
            )?)
        } else {
            None
        };
        let quantization = if has_capability(&plan, "rhythm.quantize") {
            let singing = singing.as_mut().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "rhythm.quantize requires a canonical singing track",
                )
                .with_capability("rhythm.quantize")
            })?;
            let report = quantize_singing_track(
                &mut singing.track,
                request.musical_context.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "rhythm.quantize requires musical context",
                    )
                    .with_capability("rhythm.quantize")
                })?,
            )?;
            singing.review_regions = crate::fusion::build_review_regions(&singing.track);
            Some(report)
        } else {
            None
        };
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        let participating_resources = resolved
            .iter()
            .filter(|resource| match resource.model_id.as_str() {
                "fcpe" => fcpe_evidence.is_some(),
                "basic_pitch" => basic_pitch_evidence.is_some(),
                "firered_asr2_aed" => firered_evidence.is_some(),
                "melband_roformer_denoise_aufr33" => denoise_participated,
                "melband_roformer_dereverb_anvuew" => dereverb_participated,
                "stars" | "rosvot" => advanced_note_evidence
                    .iter()
                    .any(|evidence| evidence.model_id == resource.model_id),
                _ => true,
            })
            .collect::<Vec<_>>();
        let fingerprint = deterministic_fingerprint(&ExecutionIdentity {
            request: fingerprint_request(request)?,
            resources: participating_resources
                .iter()
                .map(|resource| FingerprintResource {
                    model_id: &resource.model_id,
                    generation: &resource.generation,
                    content_digest: &resource.model_content_digest,
                    model_recipe_digest: &resource.model_recipe_digest,
                    runtime_id: &resource.runtime_id,
                    runtime_generation: &resource.runtime_generation,
                    runtime_recipe_digest: resource.runtime_recipe_digest.as_deref(),
                    backend: resource.backend,
                    device: execution_device(resource.backend),
                })
                .collect(),
            acoustic_dsp_version: ACOUSTIC_DSP_VERSION,
            calibration_version: CALIBRATION_VERSION,
            finalize_vocal_chart_version: FINALIZE_VOCAL_CHART_VERSION,
            fusion_version: FUSION_VERSION,
            hsmm_version: HSMM_VERSION,
            quantization_version: QUANTIZATION_VERSION,
            postprocess_version: POSTPROCESS_VERSION,
        })?;
        publish_candidate_artifacts(
            &output_root,
            request.requested_artifacts.singing_analysis,
            has_capability(&plan, "finalize.vocal_chart"),
            request.analysis.preserve_continuous_pitch,
            &fingerprint,
            singing.as_ref(),
            &mut artifacts,
            cancellation,
        )?;
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        if let Some(missing) = request
            .requested_artifacts
            .stems
            .iter()
            .find(|role| !artifacts.stems.iter().any(|stem| &stem.role == *role))
        {
            return Err(EngineError::new(
                EngineErrorCode::MissingCapability,
                format!(
                    "requested semantic stem {} was not produced by the selected route",
                    missing.as_str()
                ),
            ));
        }

        let singing_candidate_count = singing
            .as_ref()
            .map(|output| output.fusion.candidates.len());
        let singing_analysis_emitted = artifacts.singing_analysis.is_some();
        let candidate_vocal_chart_emitted = artifacts.candidate_vocal_chart.is_some();
        let provenance = AnalysisProvenanceV1 {
            resources: participating_resources
                .iter()
                .map(|resource| resource_provenance(resource))
                .collect(),
            calibration_version: CALIBRATION_VERSION.to_string(),
            fusion_version: FUSION_VERSION.to_string(),
            hsmm_version: HSMM_VERSION.to_string(),
            quantization_version: QUANTIZATION_VERSION.to_string(),
            postprocess_version: POSTPROCESS_VERSION.to_string(),
        };
        let result = AnalysisResultManifestV1 {
            contract: ANALYSIS_RESULT_CONTRACT.to_string(),
            version: ANALYSIS_RESULT_VERSION,
            request_id: request.request_id.clone(),
            status: if degraded_reasons.is_empty() {
                AnalysisStatus::Ok
            } else {
                AnalysisStatus::OkDegraded
            },
            artifacts,
            diagnostics: AnalysisDiagnosticsV1 {
                decoded_audio,
                warnings: Vec::new(),
                evidence: serde_json::json!({
                    "acoustic": acoustic_artifact,
                    "acoustic_algorithm": ACOUSTIC_DSP_VERSION,
                    "game_note_count": game_evidence.as_ref().map(|evidence| evidence.notes.len()),
                    "fcpe_frame_count": fcpe_evidence.as_ref().map(|evidence| evidence.frequency_hz.len()),
                    "advanced_note_counts": advanced_note_evidence.iter().map(|evidence| {
                        (evidence.model_id.clone(), evidence.notes.len())
                    }).collect::<std::collections::BTreeMap<_, _>>(),
                    "quantization": quantization,
                    "singing_candidate_count": singing_candidate_count,
                    "singing_analysis_emitted": singing_analysis_emitted,
                    "candidate_vocal_chart_emitted": candidate_vocal_chart_emitted
                }),
            },
            provenance,
            fingerprint,
            degraded_reasons,
        };
        result.validate()?;
        write_json_artifact(
            &output_root,
            Path::new("analysis-result.json"),
            "application/vnd.uta.analysis-result+json;version=1",
            &result,
        )?;
        run_guard.commit();
        Ok(result)
    }

    fn resolve_execution_resources(
        &self,
        request: &AnalyzeRequestV1,
        plan: &EnginePlan,
    ) -> EngineResult<(Vec<uta_runtime_manager::ResolvedModel>, Vec<String>)> {
        let mut resolved = Vec::new();
        let mut degraded = Vec::new();
        for requirement in plan.requirements.resources.iter().filter(|requirement| {
            requirement.required || optional_execution_supported(&requirement.reason)
        }) {
            let resource: uta_runtime_manager::ResourceRef =
                requirement.resource.parse().map_err(|error| {
                    EngineError::new(
                        EngineErrorCode::RuntimeResolutionFailed,
                        format!("invalid planned resource: {error}"),
                    )
                })?;
            match resource.kind {
                uta_runtime_manager::ResourceKind::Model => match self
                    .runtime_manager
                    .resolve_model(&resource.id, request.execution_policy.runtime_policy)
                {
                    Ok(model) => resolved.push(model),
                    Err(error) if !requirement.required => degraded.push(format!(
                        "optional capability {} skipped: {}",
                        requirement.reason, error
                    )),
                    Err(error) => return Err(EngineError::from(error)),
                },
                uta_runtime_manager::ResourceKind::Tool => {
                    let status = self
                        .runtime_manager
                        .status(&resource, request.execution_policy.runtime_policy)
                        .map_err(EngineError::from)?;
                    if !status.usable {
                        return Err(EngineError::new(
                            EngineErrorCode::WorkerUnavailable,
                            format!("required execution tool is unavailable: {resource}"),
                        )
                        .with_resource(&resource));
                    }
                }
                uta_runtime_manager::ResourceKind::Runtime
                | uta_runtime_manager::ResourceKind::Bundle => {
                    return Err(EngineError::new(
                        EngineErrorCode::RuntimeResolutionFailed,
                        format!("unsupported direct execution requirement: {resource}"),
                    ));
                }
            }
        }
        Ok((resolved, degraded))
    }

    pub fn export(&self, request: &ExportRequestV1) -> EngineResult<()> {
        request.validate()?;
        Err(EngineError::new(
            EngineErrorCode::ExportFailed,
            "standalone representation export is not implemented in this build",
        )
        .for_request(&request.request_id))
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_candidate_artifacts(
    output_root: &Path,
    request_singing_analysis: bool,
    request_vocal_chart: bool,
    preserve_continuous_pitch: bool,
    fingerprint: &str,
    singing: Option<&SingingStagesOutput>,
    artifacts: &mut AnalysisArtifactsV1,
    cancellation: &CancellationToken,
) -> EngineResult<()> {
    if !request_singing_analysis && !request_vocal_chart {
        return Ok(());
    }
    if cancellation.is_cancelled() {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "candidate artifact publication was cancelled",
        ));
    }
    let singing = singing.ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "requested Candidate outputs were not produced",
        )
    })?;
    if request_singing_analysis {
        let analysis = SingingAnalysisV1::new(
            singing.track.clone(),
            singing.fusion.candidates.clone(),
            singing.review_regions.clone(),
            fingerprint,
        )?;
        artifacts.singing_analysis = Some(write_json_artifact(
            output_root,
            Path::new("analysis/singing-analysis.json"),
            SINGING_ANALYSIS_MEDIA_TYPE,
            &analysis,
        )?);
    }
    if cancellation.is_cancelled() {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "candidate artifact publication was cancelled",
        ));
    }
    if request_vocal_chart {
        let chart =
            finalize_candidate_vocal_chart(&singing.track, fingerprint, preserve_continuous_pitch)?;
        artifacts.candidate_vocal_chart = Some(write_json_artifact(
            output_root,
            Path::new("candidate/vocal-chart.json"),
            VOCAL_CHART_MEDIA_TYPE,
            &chart,
        )?);
    }
    Ok(())
}

fn optional_execution_supported(capability: &str) -> bool {
    // Central registry for optional capabilities whose execution path can
    // truthfully consume a missing/failed expert as degradation.
    matches!(
        capability,
        "pitch.secondary"
            | "notes.basic_pitch"
            | "speech.transcribe.challenger"
            | "audio.denoise"
            | "audio.dereverb"
            | "notes.rosvot"
            | "notes.stars"
    )
}

fn execution_policy_for(
    workflow: Option<&WorkflowExecutionV1>,
    capability: &str,
    default: WorkflowExecutionPolicyV1,
) -> WorkflowExecutionPolicyV1 {
    match workflow {
        Some(workflow) => workflow
            .policy_for_engine_capability(capability)
            .unwrap_or(default),
        None => WorkflowExecutionPolicyV1::Always,
    }
}

fn record_schedule_skip(
    degraded_reasons: &mut Vec<String>,
    capability: &str,
    reason: ScheduleSkipReason,
) {
    if reason == ScheduleSkipReason::WindowedInputUnsupported {
        degraded_reasons.push(format!(
            "optional capability {capability} was not scheduled: {}",
            reason.message()
        ));
    }
}

fn run_advanced_note_challenger(
    model: &uta_runtime_manager::ResolvedModel,
    analysis_input: &Path,
    output_root: &Path,
    words: &[crate::fusion::CanonicalWordBoundary],
    source_start: u64,
    source_duration: u64,
    cancellation: &CancellationToken,
) -> EngineResult<AdvancedNoteEvidenceV1> {
    let model_id = model.model_id.as_str();
    if !matches!(model_id, "stars" | "rosvot") {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "advanced-note route rejects baseline substitution",
        ));
    }
    let word_config = words
        .iter()
        .map(|word| {
            serde_json::json!({
                "id": word.word_id,
                "text": word.text,
                "start": word.range.start,
                "duration": word.range.end - word.range.start
            })
        })
        .collect::<Vec<_>>();
    let timed_transcript_generation = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&serde_json::json!({
                "schema": "uta.timed-transcript/1",
                "source_start": source_start,
                "source_duration": source_duration,
                "words": &word_config
            }))
            .map_err(|error| {
                EngineError::new(
                    EngineErrorCode::InternalError,
                    format!("could not fingerprint TimedTranscript: {error}"),
                )
            })?
        )
    );
    let device = match std::env::var("UTA_STUDIO_ADVANCED_NOTE_DIAGNOSTIC_DEVICE") {
        Ok(value) if value.eq_ignore_ascii_case("cpu") => "cpu",
        Ok(value) if value.eq_ignore_ascii_case("gpu") => "gpu",
        Ok(_) => {
            return Err(EngineError::new(
                EngineErrorCode::InvalidContract,
                "UTA_STUDIO_ADVANCED_NOTE_DIAGNOSTIC_DEVICE must be cpu or gpu",
            ));
        }
        Err(std::env::VarError::NotPresent) => "gpu",
        Err(error) => {
            return Err(EngineError::new(
                EngineErrorCode::InvalidContract,
                format!("advanced-note diagnostic device is invalid: {error}"),
            ));
        }
    };
    let directory = create_task_dir(output_root, &format!("worker/{model_id}"))?;
    let outputs = run_native_task(
        model,
        "uta-openvino-worker",
        &format!("task-{model_id}"),
        &format!("notes.{model_id}"),
        analysis_input,
        &directory,
        serde_json::json!({
            "model_path": model.model_path,
            "model_generation": model.generation,
            "source_start": source_start,
            "source_duration": source_duration,
            "timed_transcript_generation": timed_transcript_generation,
            "words": word_config,
            "device": device
        }),
        cancellation,
    )?;
    let evidence = parse_advanced_note_evidence(
        typed_worker_output(&outputs, "advanced_note_evidence")?,
        model_id,
    )?;
    let transcript_generation_matches = evidence.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::TimedTranscript
            && dependency.generation == timed_transcript_generation
    });
    if evidence.model_generation != model.generation || !transcript_generation_matches {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "advanced-note evidence generation does not match its resolved model or TimedTranscript lease",
        ));
    }
    Ok(evidence)
}

fn has_capability(plan: &EnginePlan, capability: &str) -> bool {
    plan.execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == capability)
}

fn resolved_model<'a>(
    resolved: &'a [uta_runtime_manager::ResolvedModel],
    model_id: &str,
) -> EngineResult<&'a uta_runtime_manager::ResolvedModel> {
    resolved
        .iter()
        .find(|model| model.model_id == model_id)
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::RuntimeResolutionFailed,
                format!("resolved execution set omitted required model {model_id}"),
            )
            .with_resource(format!("model:{model_id}"))
        })
}

struct DenoiseTask<'a> {
    model_path: &'a Path,
    executable: &'a Path,
    runtime_recipe_digest: Option<&'a str>,
    ffmpeg: &'a Path,
    input: &'a Path,
    output_root: &'a Path,
    source_duration: u64,
    task_id: &'a str,
}

struct CleanupSpec<'a> {
    model_id: &'a str,
    role: crate::contract::AudioRole,
    node_id: &'a str,
    semantic_output: &'a str,
    artifact: &'a str,
    worker_directory: &'a str,
    destination: &'a str,
}

fn run_openvino_vocals(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_openvino_cleanup(
        task,
        &CleanupSpec {
            model_id: "bs_roformer_vocals_ep317",
            role: crate::contract::AudioRole::GuideVocals,
            node_id: "audio.extract_vocals",
            semantic_output: "guide_vocals",
            artifact: "guide_vocals",
            worker_directory: "worker/guide-vocals",
            destination: "stems/guide_vocals.flac",
        },
        cancellation,
    )
}

fn run_openvino_harmony(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let directory = create_task_dir(task.output_root, "worker/lead-isolate")?;
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: "uta-openvino-worker".to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: "audio.lead_isolate".to_string(),
            model_id: "melband_roformer_harmony".to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config: serde_json::json!({
                "model_path": task.model_path,
                "backend": "openvino_gpu",
                "input_semantics": "all_vocals",
                "semantic_output": "lead_vocal+backing_vocal_residual"
            }),
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    if outputs.len() != 2 {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "Harmony worker must publish exactly lead_vocal and vocal_residual",
        ));
    }
    let lead = typed_worker_output(&outputs, "lead_vocal")?.to_path_buf();
    let residual = typed_worker_output(&outputs, "vocal_residual")?.to_path_buf();
    for (artifact, path) in [("lead_vocal", &lead), ("vocal_residual", &residual)] {
        if outputs
            .iter()
            .find(|output| output.artifact == artifact)
            .is_none_or(|output| output.media_type != "audio/flac")
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("Harmony worker output {artifact} is not lossless FLAC"),
            ));
        }
        let facts = decode_audio(task.ffmpeg, artifact, path)?.facts;
        if facts.sample_rate != 44_100
            || facts.channels != 2
            || facts.frame_count == 0
            || facts.duration.abs_diff(task.source_duration) > 2_000
        {
            return Err(EngineError::new(
                EngineErrorCode::TimelineInvalid,
                format!("Harmony {artifact} did not preserve the vocal input timeline"),
            ));
        }
    }
    let relative = PathBuf::from("stems/lead_vocal.flac");
    let destination = task.output_root.join(&relative);
    let parent = destination.parent().expect("lead stem has parent");
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create lead stem directory: {error}"),
        )
    })?;
    if destination.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "lead stem target already exists",
        ));
    }
    std::fs::rename(&lead, &destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish lead stem: {error}"),
        )
    })?;
    std::fs::remove_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not clean Harmony worker directory: {error}"),
        )
    })?;
    Ok(SeparationOutput {
        role: crate::contract::AudioRole::LeadVocal,
        artifact: artifact_ref_for_existing(task.output_root, &relative, "audio/flac")?,
    })
}

fn run_openvino_denoise(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_openvino_cleanup(
        task,
        &CleanupSpec {
            model_id: "melband_roformer_denoise_aufr33",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.denoise",
            semantic_output: "dry",
            artifact: "clean_lead_vocal",
            worker_directory: "worker/denoise",
            destination: "stems/clean_lead_vocal.flac",
        },
        cancellation,
    )
}

fn run_openvino_dereverb(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_openvino_cleanup(
        task,
        &CleanupSpec {
            model_id: "melband_roformer_dereverb_anvuew",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.dereverb",
            semantic_output: "noreverb",
            artifact: "dereverbed_vocal",
            worker_directory: "worker/dereverb",
            destination: "stems/dereverbed_clean_lead_vocal.flac",
        },
        cancellation,
    )
}

fn run_openvino_instrumental(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_openvino_cleanup(
        task,
        &CleanupSpec {
            model_id: "melband_roformer_inst_v2",
            role: crate::contract::AudioRole::Instrumental,
            node_id: "audio.extract_instrumental",
            semantic_output: "instrumental",
            artifact: "instrumental",
            worker_directory: "worker/instrumental",
            destination: "stems/instrumental.flac",
        },
        cancellation,
    )
}

fn run_openvino_cleanup(
    task: &DenoiseTask<'_>,
    spec: &CleanupSpec<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let directory = create_task_dir(task.output_root, spec.worker_directory)?;
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: "uta-openvino-worker".to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: spec.node_id.to_string(),
            model_id: spec.model_id.to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config: serde_json::json!({
                "model_path": task.model_path,
                "backend": "openvino_gpu",
                "semantic_output": spec.semantic_output
            }),
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    let worker_output = typed_worker_output(&outputs, spec.artifact)?;
    if outputs
        .iter()
        .find(|output| output.artifact == spec.artifact)
        .is_none_or(|output| output.media_type != "audio/flac")
    {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "MelBand worker did not publish its declared lossless FLAC stem",
        ));
    }
    let facts = decode_audio(task.ffmpeg, spec.node_id, worker_output)?.facts;
    if facts.sample_rate != 44_100
        || facts.channels != 2
        || facts.frame_count == 0
        || facts.duration.abs_diff(task.source_duration) > 2_000
    {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "MelBand separation did not preserve the 44.1 kHz stereo source timeline",
        ));
    }
    let relative = PathBuf::from(spec.destination);
    let destination = task.output_root.join(&relative);
    let parent = destination.parent().expect("cleanup stem has parent");
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create cleanup stem directory: {error}"),
        )
    })?;
    if destination.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "cleanup stem target already exists",
        ));
    }
    std::fs::rename(worker_output, &destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish cleanup stem: {error}"),
        )
    })?;
    std::fs::remove_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not clean cleanup worker directory: {error}"),
        )
    })?;
    Ok(SeparationOutput {
        role: spec.role,
        artifact: artifact_ref_for_existing(task.output_root, &relative, "audio/flac")?,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_native_task(
    model: &uta_runtime_manager::ResolvedModel,
    component: &str,
    task_id: &str,
    node_id: &str,
    input: &Path,
    output_dir: &Path,
    config: serde_json::Value,
    cancellation: &CancellationToken,
) -> EngineResult<Vec<NativeTaskOutput>> {
    SupervisedWorker::run(
        &model.runtime_executable,
        &WorkerExpectation {
            component: component.to_string(),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
        &NativeTask {
            task_id: task_id.to_string(),
            node_id: node_id.to_string(),
            model_id: model.model_id.clone(),
            input_artifacts: vec![input.to_path_buf()],
            output_dir: output_dir.to_path_buf(),
            config,
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )
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

fn create_task_dir(root: &Path, relative: &str) -> EngineResult<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "worker task directory is invalid",
        ));
    }
    let directory = root.join(relative);
    let parent = directory.parent().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "worker task directory has no parent",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create worker parent directory: {error}"),
        )
    })?;
    std::fs::create_dir(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("worker task directory already exists or cannot be created: {error}"),
        )
    })?;
    directory.canonicalize().map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not authorize worker task directory: {error}"),
        )
    })
}

fn caller_transcript(request: &AnalyzeRequestV1) -> EngineResult<TranscriptArtifactV1> {
    let text = request_lyrics_text(request);
    if text.is_empty() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "canonical lyrics contain no text",
        ));
    }
    let artifact = TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthorityV1::CallerCanonical,
        language: request.lyrics.language.clone(),
        text,
        tokens: request
            .lyrics
            .tokens
            .iter()
            .map(|token| TranscriptTokenV1 {
                id: token.id.clone(),
                text: token.text.clone(),
                confidence: None,
            })
            .collect(),
        // Caller authority is categorical request authority, not probability.
        confidence: None,
        source_experts: vec!["caller.canonical_lyrics".to_string()],
        alternatives: Vec::new(),
        model_sha256: None,
        runtime_manifest_sha256: None,
        backend: "caller".to_string(),
    };
    artifact.validate()?;
    Ok(artifact)
}

fn request_lyrics_text(request: &AnalyzeRequestV1) -> String {
    let separator = match request.lyrics.language.as_deref() {
        Some(language)
            if language.starts_with("zh")
                || language.starts_with("ja")
                || language.starts_with("ko") =>
        {
            ""
        }
        _ => " ",
    };
    request
        .lyrics
        .tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(separator)
}

fn normalized_transcript(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn execution_device(backend: uta_runtime_manager::NativeBackend) -> &'static str {
    match backend {
        uta_runtime_manager::NativeBackend::OpenVino
        | uta_runtime_manager::NativeBackend::Vulkan => "device:0",
        uta_runtime_manager::NativeBackend::NativeDsp => "native",
        uta_runtime_manager::NativeBackend::CpuReference => "diagnostic_cpu",
    }
}

fn resource_provenance(
    resource: &uta_runtime_manager::ResolvedModel,
) -> ResolvedResourceProvenanceV1 {
    ResolvedResourceProvenanceV1 {
        resource: format!("model:{}", resource.model_id),
        generation: resource.generation.clone(),
        content_digest: resource.model_content_digest.clone(),
        runtime: resource.runtime_id.clone(),
        runtime_generation: resource.runtime_generation.clone(),
        runtime_recipe_digest: resource.runtime_recipe_digest.clone(),
        backend: match resource.backend {
            uta_runtime_manager::NativeBackend::OpenVino => "openvino",
            uta_runtime_manager::NativeBackend::Vulkan => "vulkan",
            uta_runtime_manager::NativeBackend::NativeDsp => "native_dsp",
            uta_runtime_manager::NativeBackend::CpuReference => "cpu_reference",
        }
        .to_string(),
        device: execution_device(resource.backend).to_string(),
    }
}

fn cancelled(request: &AnalyzeRequestV1) -> EngineError {
    EngineError::new(EngineErrorCode::Cancelled, "analysis request was cancelled")
        .for_request(&request.request_id)
}

fn fingerprint_request(request: &AnalyzeRequestV1) -> EngineResult<serde_json::Value> {
    let mut value = serde_json::to_value(request).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not serialize request fingerprint identity: {error}"),
        )
    })?;
    if let Some(sources) = value
        .get_mut("audio_sources")
        .and_then(serde_json::Value::as_array_mut)
    {
        for source in sources {
            if let Some(source) = source.as_object_mut() {
                source.remove("path");
            }
        }
    }
    Ok(value)
}

struct OutputRunGuard {
    root: PathBuf,
    committed: bool,
}

impl OutputRunGuard {
    fn new(path: &Path) -> EngineResult<Self> {
        let root = authorize_output_root(path)?;
        let mut entries = std::fs::read_dir(&root).map_err(|error| {
            EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("could not inspect authorized output directory: {error}"),
            )
        })?;
        if entries.next().is_some() {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "authorized analysis output directory must be empty",
            ));
        }
        Ok(Self {
            root,
            committed: false,
        })
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for OutputRunGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn authorize_output_root(path: &Path) -> EngineResult<PathBuf> {
    if !path.is_dir() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!(
                "authorized output directory is unavailable: {}",
                path.display()
            ),
        ));
    }
    path.canonicalize().map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not authorize output directory: {error}"),
        )
    })
}

#[cfg(test)]
fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn sha256_file_with_cancellation(
    path: &Path,
    cancellation: &CancellationToken,
) -> EngineResult<String> {
    let mut file = File::open(path).map_err(|error| {
        EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!("could not open input for hashing: {error}"),
        )
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "input hashing was cancelled",
            ));
        }
        let count = file.read(&mut buffer).map_err(|error| {
            EngineError::new(
                EngineErrorCode::MissingRequiredInput,
                format!("could not hash input: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;
