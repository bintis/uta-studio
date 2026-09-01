use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use uta_runtime_manager::{RuntimeManager, StorePaths};

use crate::artifact::{
    AdvancedNoteEvidenceV1, AlignmentArtifactV1, BasicPitchEvidenceV3, DependencyKind,
    Jbm555ExpectedInputsV1, PitchEvidenceV03, SingingAnalysisV1, TechniqueEvidenceV1,
    TimedNoteExpertEvidenceV1, artifact_ref_for_existing, finalize_candidate_vocal_chart,
    parse_advanced_note_evidence, parse_game_evidence, parse_jbm555_evidence, parse_qwen_alignment,
    parse_qwen_transcript, parse_rmvpe_pitch, write_json_artifact,
};
use crate::audio::{
    CleanupComparison, QualityEvaluationInput, analyze_acoustic_evidence, decode_audio,
    decode_audio_with_cancellation, enforce_required_quality, estimate_instrumental_quality,
    estimate_vocal_topology, evaluate_audio_quality, quality_degraded_reasons,
    topology_review_regions,
};
use crate::candidate_pipeline::{
    CandidatePathDecisionV1, FusionDecisionModeV1, SingingStagesOutput, attach_caller_lyric_ranges,
    build_baseline_review_regions, build_transcript_disagreement_regions,
    execute_candidate_graph_stage, execute_singing_fusion_stage_with_timed_notes,
    fuse_alignment_stage, fuse_transcript_stage,
};
use crate::conditional_scheduler::{
    ConditionalScheduleRecordV1, ConditionalScheduleRequest, ScheduleSkipReason,
    ScheduledExecution, run_basic_pitch_schedule, run_fcpe_schedule, run_firered_schedule,
    schedule,
};
use crate::contract::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AnalysisArtifactsV1, AnalysisDiagnosticsV1,
    AnalysisProvenanceV1, AnalysisResultManifestV1, AnalysisReusePolicyV1, AnalysisStatus,
    AnalyzeRequestV1, BoundaryAuthority, CapabilityDescriptor, DecodedAudioFactsV1, EngineError,
    EngineErrorCode, EngineRequirementsV1, EngineResult, ExportRequestV1,
    FUSION_AGENT_ADAPTER_RESOURCE, FUSION_AGENT_PROTOCOL, FusionDecisionProvenanceV1,
    HSMM_VITERBI_SELECTOR, LyricsMode, StemArtifactRefV1, VOCAL_TOPOLOGY_GATE,
};
use crate::events::{EngineEventSink, begin_node, emit_degraded, emit_warning, with_event_sink};
use crate::execution::{
    CancellationToken, NativeTask, NativeTaskOutput, SupervisedWorker, WorkerExpectation,
};
use crate::fingerprint::{
    ACOUSTIC_DSP_VERSION, AUDIO_QUALITY_VERSION, CALIBRATION_VERSION, ExecutionIdentity,
    FINALIZE_VOCAL_CHART_VERSION, FUSION_VERSION, FingerprintResource, HSMM_VERSION,
    POSTPROCESS_VERSION, QUANTIZATION_VERSION, deterministic_fingerprint,
};
use crate::fusion::{SingingReviewReason, TimeRange, merge_regions};
use crate::planner::{EnginePlan, Planner};
use crate::quantization::quantize_singing_track;
use crate::separation::SeparationOutput;
use crate::workflow::{FusionModeV1, WorkflowExecutionPolicyV1, WorkflowExecutionV1};
use crate::workflow_executor::{CompiledWorkflowExecutionPlanV1, WorkflowNodeExecutionStateV1};

mod output_guard;
mod runtime_route;
mod worker_tasks;
mod workflow_execution;
use output_guard::OutputRunGuard;
use runtime_route::{
    caller_transcript, cancelled, execution_device, fingerprint_request,
    ggml_vulkan_device_class, line_anchors_for_lyrics, openvino_backend, request_lyrics_text,
    resource_provenance, roformer_backend, roformer_component,
};
use worker_tasks::{run_native_task, typed_worker_output};
use workflow_execution::*;

const TRANSCRIPT_MEDIA_TYPE: &str = "application/vnd.uta.transcript+json;version=1";
const ALIGNMENT_MEDIA_TYPE: &str = "application/vnd.uta.alignment+json;version=1";
const PITCH_MEDIA_TYPE: &str = "application/vnd.uta.pitch-evidence+json;version=0.3";
const TECHNIQUE_MEDIA_TYPE: &str = "application/vnd.uta.technique-evidence+json;version=1";
const ACOUSTIC_MEDIA_TYPE: &str = "application/vnd.uta.acoustic-evidence+json;version=2";
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
        request.validate()?;
        WorkflowExecutionV1::from_request(request)?;
        Ok(())
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
        }
        Ok(())
    }

    pub fn decoded_audio_facts(
        &self,
        request: &AnalyzeRequestV1,
    ) -> EngineResult<Vec<DecodedAudioFactsV1>> {
        self.validate_inputs(request)?;
        self.decode_validated_audio(request, &CancellationToken::default())
            .map(|decoded| decoded.into_iter().map(|audio| audio.facts).collect())
    }

    fn decode_validated_audio(
        &self,
        request: &AnalyzeRequestV1,
        cancellation: &CancellationToken,
    ) -> EngineResult<Vec<crate::audio::DecodedAudio>> {
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
                let decoded = decode_audio_with_cancellation(
                    &ffmpeg,
                    &source.id,
                    &source.path,
                    cancellation,
                )?;
                source
                    .timeline
                    .source_start
                    .checked_add(decoded.facts.duration)
                    .ok_or_else(|| {
                        EngineError::new(
                            EngineErrorCode::TimelineInvalid,
                            format!(
                                "decoded source {} overflows its canonical timeline",
                                source.id
                            ),
                        )
                    })?;
                Ok(decoded)
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

    pub fn analyze_with_events(
        &self,
        request: &AnalyzeRequestV1,
        output_dir: impl AsRef<Path>,
        cancellation: &CancellationToken,
        sink: EngineEventSink,
    ) -> EngineResult<AnalysisResultManifestV1> {
        let workflow = WorkflowExecutionV1::from_request(request)?;
        let plan_nodes = self
            .plan(request)?
            .execution_nodes
            .into_iter()
            .map(|node| (node.id, node.capability.to_string()))
            .collect();
        with_event_sink(&request.request_id, workflow, plan_nodes, sink, || {
            self.analyze_with_cancellation(request, output_dir, cancellation)
        })
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
        let fusion_policy = workflow
            .as_ref()
            .and_then(|workflow| workflow.resolved_expert_fusion_policy(request.analysis.profile))
            .unwrap_or_default();
        let fusion_pitch_owner = fusion_policy.continuous_f0.model_id().to_string();
        let fusion_mode = workflow
            .as_ref()
            .map(WorkflowExecutionV1::fusion_mode)
            .unwrap_or_default();
        let fusion_adapter = if fusion_mode == FusionModeV1::AiJudgment {
            Some(
                self.runtime_manager
                    .resolve_tool(
                        uta_runtime_manager::FUSION_AGENT_ADAPTER_ID,
                        request.execution_policy.runtime_policy,
                    )
                    .map_err(EngineError::from)?,
            )
        } else {
            None
        };
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }

        let (resolved, mut degraded_reasons) = self.resolve_execution_resources(request, &plan)?;
        let mut conditional_schedule = Vec::new();
        let _lease = self.runtime_manager.lease_resolved_models(&resolved);
        let decode_lifecycle = begin_node("decode", "audio.decode", None, "ffmpeg");
        let decoded_sources = self.decode_validated_audio(request, cancellation)?;
        decode_lifecycle.complete();

        let primary = request.primary_source()?;
        let source_start = primary.timeline.source_start;
        let primary_decoded = decoded_sources
            .iter()
            .find(|decoded| decoded.facts.source_id == primary.id)
            .ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::InternalError,
                    "decoded facts are missing for the primary source",
                )
                .for_request(&request.request_id)
            })?;
        let source_duration = primary_decoded.facts.duration;
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
        let mut guide_vocal_profile = request
            .audio_sources
            .iter()
            .find(|source| {
                matches!(
                    source.role,
                    crate::contract::AudioRole::GuideVocals | crate::contract::AudioRole::VocalStem
                )
            })
            .and_then(|source| {
                decoded_sources
                    .iter()
                    .find(|decoded| decoded.facts.source_id == source.id)
            })
            .map(|decoded| decoded.profile.clone());
        let supplied_instrumental = request
            .audio_sources
            .iter()
            .find(|source| source.role == crate::contract::AudioRole::Instrumental);
        let mut instrumental_audio = supplied_instrumental
            .and_then(|source| {
                decoded_sources
                    .iter()
                    .find(|decoded| decoded.facts.source_id == source.id)
            })
            .cloned();
        let mut isolation_profiles = None;
        let mut workflow_audio = BTreeMap::new();
        record_workflow_audio(
            plan.workflow_execution.as_ref(),
            "audio.decode",
            "mix",
            &mut workflow_audio,
            &analysis_input,
            analysis_role,
        );
        record_reused_workflow_audio(
            plan.workflow_execution.as_ref(),
            primary.role,
            &mut workflow_audio,
            &analysis_input,
        );
        // A Step 1 cache hit changes execution input, but requested reused
        // stems remain first-class outputs of this run. Materialize them
        // before executing downstream stages so final capability validation
        // observes exactly the same semantic results as a fresh separation.
        for source in &request.audio_sources {
            if source.role == crate::contract::AudioRole::OriginalMix
                || !request.requested_artifacts.stems.contains(&source.role)
                || artifacts.stems.iter().any(|stem| stem.role == source.role)
            {
                continue;
            }
            let output = crate::separation::materialize_semantic_stem(
                &ffmpeg,
                &source.path,
                &output_root,
                source.role,
                cancellation,
            )?;
            artifacts.stems.push(StemArtifactRefV1 {
                role: output.role,
                artifact: output.artifact,
            });
        }
        if has_capability(&plan, "audio.extract_vocals") {
            // Defaults to Leap XE90 when no compiled workflow is present
            // (e.g. legacy direct requests); a real compiled workflow always
            // carries an explicit provider via `provider_preferences`.
            let vocal_provider = workflow
                .as_ref()
                .and_then(|workflow| workflow.model_for_engine_capability("audio.extract_vocals"))
                .unwrap_or("bs_roformer_leap_xe90_vocals");
            let model = resolved_model(&resolved, vocal_provider)?;
            let backend = roformer_backend(model)?;
            let task = DenoiseTask {
                model_path: &model.model_path,
                executable: &model.runtime_executable,
                runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                backend,
                device_class: ggml_vulkan_device_class(
                    backend,
                    request.execution_policy.requested_device_for(&model.model_id),
                ),
                ffmpeg: &ffmpeg,
                input: &primary.path,
                output_root: &output_root,
                source_duration,
                task_id: &format!("{}-extract-vocals", request.request_id),
            };
            let output = if vocal_provider == "bs_polarformer_public_instrumental" {
                run_openvino_polarformer_vocals(&task, cancellation)?
            } else {
                run_openvino_vocals(&task, cancellation)?
            };
            analysis_input = output_root.join(&output.artifact.path);
            guide_vocal_profile =
                Some(decode_audio(&ffmpeg, "guide_vocals", &analysis_input)?.profile);
            analysis_role = crate::contract::AudioRole::GuideVocals.as_str();
            record_workflow_audio(
                plan.workflow_execution.as_ref(),
                "audio.extract_vocals",
                "vocal",
                &mut workflow_audio,
                &analysis_input,
                analysis_role,
            );
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
            let backend = roformer_backend(model)?;
            let output = run_openvino_harmony(
                &DenoiseTask {
                    model_path: &model.model_path,
                    executable: &model.runtime_executable,
                    runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                    backend,
                    device_class: ggml_vulkan_device_class(
                        backend,
                        request.execution_policy.requested_device_for(&model.model_id),
                    ),
                    ffmpeg: &ffmpeg,
                    input: &analysis_input,
                    output_root: &output_root,
                    source_duration,
                    task_id: &format!("{}-lead-isolate", request.request_id),
                },
                cancellation,
            )?;
            let lead_input = output_root.join(&output.stem.artifact.path);
            isolation_profiles = Some((output.lead_profile, output.residual_profile));
            record_workflow_audio(
                plan.workflow_execution.as_ref(),
                "audio.lead_isolate",
                "lead",
                &mut workflow_audio,
                &lead_input,
                crate::contract::AudioRole::LeadVocal.as_str(),
            );
            if plan
                .source_route
                .preparation
                .iter()
                .any(|capability| capability.as_str() == "audio.lead_isolate")
            {
                analysis_input = lead_input;
                analysis_role = crate::contract::AudioRole::LeadVocal.as_str();
            }
            if request
                .requested_artifacts
                .stems
                .contains(&crate::contract::AudioRole::LeadVocal)
            {
                artifacts.stems.push(StemArtifactRefV1 {
                    role: output.stem.role,
                    artifact: output.stem.artifact,
                });
            }
        }
        if has_capability(&plan, "audio.extract_instrumental") {
            // Defaults to PolarFormer when no compiled workflow is present
            // (e.g. legacy direct requests); a real compiled workflow always
            // carries an explicit provider via `provider_preferences`.
            let instrumental_provider = workflow
                .as_ref()
                .and_then(|workflow| {
                    workflow.model_for_engine_capability("audio.extract_instrumental")
                })
                .unwrap_or("bs_polarformer_public_instrumental");
            let model = resolved_model(&resolved, instrumental_provider)?;
            let backend = roformer_backend(model)?;
            let task = DenoiseTask {
                model_path: &model.model_path,
                executable: &model.runtime_executable,
                runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                backend,
                device_class: ggml_vulkan_device_class(
                    backend,
                    request.execution_policy.requested_device_for(&model.model_id),
                ),
                ffmpeg: &ffmpeg,
                input: &primary.path,
                output_root: &output_root,
                source_duration,
                task_id: &format!("{}-instrumental", request.request_id),
            };
            let output = if instrumental_provider == "melband_roformer_inst_v2" {
                run_openvino_inst_v2(&task, cancellation)?
            } else {
                run_openvino_instrumental(&task, cancellation)?
            };
            let path = output_root.join(&output.artifact.path);
            instrumental_audio = Some(decode_audio(&ffmpeg, "instrumental", &path)?);
            record_workflow_audio(
                plan.workflow_execution.as_ref(),
                "audio.extract_instrumental",
                "instrumental",
                &mut workflow_audio,
                &path,
                crate::contract::AudioRole::Instrumental.as_str(),
            );
            artifacts.stems.push(StemArtifactRefV1 {
                role: output.role,
                artifact: output.artifact,
            });
        }
        let raw_cleanup_input = analysis_input.clone();
        let raw_cleanup_role = analysis_role.to_string();
        let mut cleanup_output = None;
        let mut instrumental_cleanup_output = None;
        let mut cleanup_workflow_nodes = Vec::new();
        let mut denoise_participated = false;
        let mut dereverb_participated = false;
        let cleanup_steps = workflow_cleanup_steps(plan.workflow_execution.as_ref(), &resolved);
        for (capability, workflow_node) in cleanup_steps {
            let model_id = match capability.as_str() {
                "audio.denoise" => "melband_roformer_denoise_aufr33",
                "audio.dereverb" => "melband_roformer_dereverb_anvuew",
                _ => continue,
            };
            let model = resolved_model(&resolved, model_id)?;
            let (step_input, step_role) = workflow_transform_input(
                plan.workflow_execution.as_ref(),
                workflow_node.as_deref(),
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let task_id = format!(
                "{}-{}",
                request.request_id,
                workflow_node.as_deref().unwrap_or(capability.as_str())
            );
            let backend = roformer_backend(model)?;
            let task = DenoiseTask {
                model_path: &model.model_path,
                executable: &model.runtime_executable,
                runtime_recipe_digest: model.runtime_recipe_digest.as_deref(),
                backend,
                device_class: ggml_vulkan_device_class(
                    backend,
                    request.execution_policy.requested_device_for(&model.model_id),
                ),
                ffmpeg: &ffmpeg,
                input: &step_input,
                output_root: &output_root,
                source_duration,
                task_id: &task_id,
            };
            let output_role = workflow_cleanup_output_role(&step_role);
            let result = match (capability.as_str(), workflow_node.as_deref()) {
                ("audio.denoise", Some(node)) => {
                    run_openvino_workflow_cleanup(&task, node, true, output_role, cancellation)
                }
                ("audio.dereverb", Some(node)) => {
                    run_openvino_workflow_cleanup(&task, node, false, output_role, cancellation)
                }
                ("audio.denoise", None) => run_openvino_denoise(&task, cancellation),
                ("audio.dereverb", None) => run_openvino_dereverb(&task, cancellation),
                _ => unreachable!("cleanup capability was filtered"),
            };
            match result {
                Ok(output) => {
                    let output_path = output_root.join(&output.artifact.path);
                    if let Some(node) = workflow_node.as_deref() {
                        cleanup_workflow_nodes.push(node.to_string());
                        workflow_audio.insert(
                            (node.to_string(), "audio".to_string()),
                            (output_path.clone(), step_role.clone()),
                        );
                    }
                    if step_role == crate::contract::AudioRole::Instrumental.as_str() {
                        instrumental_audio =
                            Some(decode_audio(&ffmpeg, "cleaned_instrumental", &output_path)?);
                        instrumental_cleanup_output = Some(output);
                    } else if matches!(
                        step_role.as_str(),
                        "vocal" | "guide_vocals" | "vocal_stem" | "lead_vocal"
                    ) {
                        analysis_input = output_path;
                        analysis_role = crate::contract::AudioRole::CleanLeadVocal.as_str();
                        cleanup_output = Some(output);
                    }
                    denoise_participated |= capability == "audio.denoise";
                    dereverb_participated |= capability == "audio.dereverb";
                }
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => degraded_reasons.push(format!(
                    "optional capability {capability} failed: {}",
                    error.message
                )),
            }
        }
        let mut cleanup_comparison = None;
        if denoise_participated || dereverb_participated {
            let raw = decode_audio(&ffmpeg, "raw_cleanup_input", &raw_cleanup_input)?;
            let clean = decode_audio(&ffmpeg, "cleaned_analysis_input", &analysis_input)?;
            let comparison = CleanupComparison::from_signals(
                raw.facts.duration,
                raw.metrics,
                clean.facts.duration,
                clean.metrics,
            );
            if comparison.damage_suspected() {
                emit_warning(
                    "cleanup consistency evidence indicated possible damage; using the raw vocal input",
                );
                analysis_input = raw_cleanup_input.clone();
                analysis_role = raw_cleanup_role.as_str();
                cleanup_output = None;
                for node in &cleanup_workflow_nodes {
                    workflow_audio.insert(
                        (node.clone(), "audio".to_string()),
                        (raw_cleanup_input.clone(), raw_cleanup_role.clone()),
                    );
                }
            }
            cleanup_comparison = Some(comparison);
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
        if let Some(output) = instrumental_cleanup_output {
            // The branch's terminal processor replaces the raw accompaniment
            // as the one deliverable Instrumental. Intermediate workflow
            // audio remains available through execution provenance, while
            // downstream quality, audition and authoring use this last stem.
            artifacts
                .stems
                .retain(|stem| stem.role != crate::contract::AudioRole::Instrumental);
            artifacts.stems.push(StemArtifactRefV1 {
                role: output.role,
                artifact: output.artifact,
            });
        }
        // Topology is applicable only when the exact plan selected the
        // foreground/residual-producing lead-isolation route. Do not turn an
        // intentionally bypassed optional processor into a whole-track
        // `Unknown` review or an `ok_degraded` result.
        let vocal_topology = plan
            .quality_gates
            .iter()
            .any(|gate| gate == VOCAL_TOPOLOGY_GATE)
            .then(|| {
                estimate_vocal_topology(
                    source_start,
                    source_duration,
                    isolation_profiles.as_ref().map(|profiles| &profiles.0),
                    isolation_profiles.as_ref().map(|profiles| &profiles.1),
                )
            })
            .transpose()?;
        let topology_reviews = vocal_topology
            .as_ref()
            .map(topology_review_regions)
            .unwrap_or_default();
        let instrumental_quality = instrumental_audio.as_ref().map(|instrumental| {
            estimate_instrumental_quality(
                source_start,
                source_duration,
                instrumental.metrics,
                &instrumental.profile,
                guide_vocal_profile.as_ref(),
            )
        });
        let (acoustic_evidence, acoustic_artifact) =
            if has_capability(&plan, "analysis.acoustic_dsp") {
                let (input, role) = workflow_bound_audio(
                    plan.workflow_execution.as_ref(),
                    "analysis.acoustic_dsp",
                    &workflow_audio,
                    &analysis_input,
                    analysis_role,
                )?;
                let lifecycle = begin_node(
                    "acoustic-dsp",
                    "analysis.acoustic_dsp",
                    None,
                    ACOUSTIC_DSP_VERSION,
                );
                let evidence = analyze_acoustic_evidence(
                    &ffmpeg,
                    &input,
                    &role,
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
                lifecycle.artifact("acoustic_evidence");
                lifecycle.complete();
                (Some(evidence), Some(artifact))
            } else {
                (None, None)
            };
        let needs_transcribe = has_capability(&plan, "speech.transcribe");
        let needs_alignment = has_capability(&plan, "speech.align");
        let needs_pitch = has_capability(&plan, "pitch.track");
        let transcript_evidence = if needs_transcribe {
            let model = resolved_model(&resolved, "qwen3_asr_1_7b")?;
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "speech.transcribe",
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let directory = create_task_dir(&output_root, "worker/asr")?;
            let outputs = run_native_task(
                model,
                "uta-qwen-asr-worker",
                "task-asr",
                "speech.transcribe",
                &input,
                &directory,
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
        let reference_lyrics =
            (request.lyrics.mode == LyricsMode::Reference).then(|| request_lyrics_text(request));
        let transcript_disagreement_regions = transcript_evidence
            .as_ref()
            .map(|transcript| {
                build_transcript_disagreement_regions(
                    transcript,
                    reference_lyrics.as_deref(),
                    request.lyrics.language.as_deref(),
                    source_range,
                )
            })
            .unwrap_or_default();
        let firered_evidence = if has_capability(&plan, "speech.transcribe.challenger") {
            let model = resolved
                .iter()
                .find(|model| model.model_id == "firered_asr2_aed");
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "speech.transcribe.challenger",
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let policy = execution_policy_for(
                workflow.as_ref(),
                "speech.transcribe.challenger",
                WorkflowExecutionPolicyV1::OnDisagreement,
            );
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: "speech.transcribe.challenger",
                policy,
                profile: request.analysis.profile,
                source_range,
                review_regions: &transcript_disagreement_regions,
                relevant_reasons: &[
                    SingingReviewReason::TranscriptLowConfidence,
                    SingingReviewReason::TranscriptReferenceMismatch,
                    SingingReviewReason::TranscriptLanguageMismatch,
                    SingingReviewReason::TranscriptCoverageMismatch,
                ],
                optional_usable: model.is_some(),
                required: false,
                supports_windowed_input: false,
                full_input_on_disagreement: true,
            })?;
            conditional_schedule.push(ConditionalScheduleRecordV1::new(
                "speech.transcribe.challenger",
                policy,
                &scheduled,
            ));
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(
                    &mut degraded_reasons,
                    "speech.transcribe.challenger",
                    reason,
                );
            }
            match run_firered_schedule(model, &input, &output_root, &scheduled, cancellation) {
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
            let lifecycle = begin_node("transcript", "fusion.transcript", None, FUSION_VERSION);
            let primary = transcript_evidence.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "fusion.transcript requires canonical or baseline-generated evidence",
                )
            })?;
            let mut transcript_candidates = vec![primary.clone()];
            if let Some(challenger) = firered_evidence.as_ref() {
                transcript_candidates.push(challenger.clone());
            }
            let (artifact, mut canonical) =
                fuse_transcript_stage(&transcript_candidates, reference_lyrics.as_deref())?;
            attach_caller_lyric_ranges(&mut canonical, &request.lyrics);
            lifecycle.artifact("canonical_transcript");
            lifecycle.complete();
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
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "speech.align",
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let transcript = transcript.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "alignment requires canonical or transcribed text",
                )
            })?;
            let model = resolved_model(&resolved, "qwen3_forced_aligner_0_6b")?;
            let directory = create_task_dir(&output_root, "worker/alignment")?;
            let mut align_config = serde_json::json!({
                "model_path": model.model_path,
                "text": transcript.text,
                "language": transcript.language
            });
            if let Some(anchors) = line_anchors_for_lyrics(&request.lyrics) {
                align_config["line_anchors"] = anchors;
            }
            let outputs = run_native_task(
                model,
                "uta-qwen-align-worker",
                "task-align",
                "speech.align",
                &input,
                &directory,
                align_config,
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
            let lifecycle = begin_node("alignment", "fusion.alignment", None, FUSION_VERSION);
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
            lifecycle.artifact("canonical_alignment");
            lifecycle.complete();
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

        let mut pitch_evidence: Option<PitchEvidenceV03> = None;
        let mut fcpe_evidence: Option<PitchEvidenceV03> = None;
        if needs_pitch {
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "pitch.track",
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            if fusion_pitch_owner == "fcpe" {
                let model = resolved_model(&resolved, "fcpe")?;
                fcpe_evidence = run_fcpe_schedule(
                    model,
                    &ffmpeg,
                    &input,
                    &output_root,
                    source_range,
                    &ScheduledExecution::FullInput,
                    cancellation,
                )?;
                let pitch = fcpe_evidence.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "FCPE was selected as the primary F0 expert but produced no evidence",
                    )
                })?;
                if request.requested_artifacts.pitch_evidence {
                    artifacts.pitch_evidence = Some(write_json_artifact(
                        &output_root,
                        Path::new("pitch/pitch-evidence.json"),
                        PITCH_MEDIA_TYPE,
                        pitch,
                    )?);
                }
            } else {
                let model = resolved_model(&resolved, "rmvpe")?;
                let directory = create_task_dir(&output_root, "worker/rmvpe")?;
                let outputs = run_native_task(
                    model,
                    "uta-openvino-worker",
                    "task-rmvpe",
                    "pitch.track",
                    &input,
                    &directory,
                    serde_json::json!({
                        "model_path": model.model_path,
                        "backend": openvino_backend(model)?
                    }),
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
                pitch_evidence = Some(pitch);
            }
        }
        let mut basic_pitch_evidence: Option<BasicPitchEvidenceV3> = None;
        let source_end = source_start.saturating_add(source_duration);
        let mut game_known_boundaries_us = canonical_words
            .as_deref()
            .unwrap_or_default()
            .iter()
            .flat_map(|word| [word.range.start, word.range.end])
            .filter(|boundary| *boundary > source_start && *boundary < source_end)
            .map(|boundary| boundary - source_start)
            .collect::<Vec<_>>();
        game_known_boundaries_us.sort_unstable();
        game_known_boundaries_us.dedup();
        let game_conditioned_boundary_count = game_known_boundaries_us.len();
        let game_evidence = if has_capability(&plan, "notes.game") {
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                "notes.game",
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let model = resolved_model(&resolved, "game")?;
            let directory = create_task_dir(&output_root, "worker/game")?;
            let outputs = run_native_task(
                model,
                "uta-openvino-worker",
                "task-game",
                "notes.game",
                &input,
                &directory,
                serde_json::json!({
                    "model_path": model.model_path,
                    "language": request.lyrics.language,
                    "known_boundaries_us": game_known_boundaries_us,
                    "backend": openvino_backend(model)?
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
                fcpe_evidence.as_ref(),
                game,
                acoustic,
                source_start,
                source_duration,
                &fusion_pitch_owner,
            )?,
            _ => Vec::new(),
        };
        let mut baseline_review_regions = baseline_review_regions;
        baseline_review_regions.extend(
            topology_reviews
                .iter()
                .filter(|region| {
                    !region
                        .reasons
                        .contains(&SingingReviewReason::VocalTopologyUnknown)
                })
                .cloned(),
        );
        let baseline_review_regions = merge_regions(baseline_review_regions);
        let secondary_pitch = if has_capability(&plan, "pitch.secondary.rmvpe") {
            Some(("rmvpe", "pitch.secondary.rmvpe", false))
        } else if has_capability(&plan, "pitch.secondary.fcpe") {
            Some(("fcpe", "pitch.secondary.fcpe", true))
        } else {
            None
        };
        if let Some((secondary_model_id, capability, supports_windowed_input)) = secondary_pitch {
            let model = resolved
                .iter()
                .find(|model| model.model_id == secondary_model_id);
            let policy = execution_policy_for_model(
                workflow.as_ref(),
                secondary_model_id,
                WorkflowExecutionPolicyV1::DisagreementWindows,
            );
            let scheduled = schedule(ConditionalScheduleRequest {
                capability,
                policy,
                profile: request.analysis.profile,
                source_range,
                review_regions: &baseline_review_regions,
                relevant_reasons: PITCH_DISAGREEMENT_REASONS,
                optional_usable: model.is_some(),
                required: false,
                supports_windowed_input,
                full_input_on_disagreement: false,
            })
            .map_err(|error| error.for_request(&request.request_id))?;
            conditional_schedule.push(ConditionalScheduleRecordV1::new(
                capability, policy, &scheduled,
            ));
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(&mut degraded_reasons, capability, reason);
            } else if let Some(model) = model {
                let (input, _) = workflow_bound_audio(
                    plan.workflow_execution.as_ref(),
                    capability,
                    &workflow_audio,
                    &analysis_input,
                    analysis_role,
                )?;
                let result = if secondary_model_id == "fcpe" {
                    run_fcpe_schedule(
                        model,
                        &ffmpeg,
                        &input,
                        &output_root,
                        source_range,
                        &scheduled,
                        cancellation,
                    )
                } else {
                    let directory = create_task_dir(&output_root, "worker/rmvpe-secondary")?;
                    let outputs = run_native_task(
                        model,
                        "uta-openvino-worker",
                        "task-rmvpe",
                        capability,
                        &input,
                        &directory,
                        serde_json::json!({
                            "model_path": model.model_path,
                            "backend": openvino_backend(model)?
                        }),
                        cancellation,
                    );
                    outputs.and_then(|outputs| {
                        parse_rmvpe_pitch(
                            typed_worker_output(&outputs, "pitch_evidence")?,
                            source_start,
                            source_duration,
                        )
                        .map(Some)
                    })
                };
                match result {
                    Ok(evidence) if secondary_model_id == "fcpe" => fcpe_evidence = evidence,
                    Ok(evidence) => pitch_evidence = evidence,
                    Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(
                            output_root.join(format!("worker/{secondary_model_id}")),
                        );
                        let _ = std::fs::remove_dir_all(
                            output_root.join(format!("worker/conditional/{secondary_model_id}")),
                        );
                        degraded_reasons.push(format!(
                            "optional capability {capability} failed: {}",
                            error.message
                        ));
                    }
                }
            }
        }
        if has_capability(&plan, "notes.basic_pitch") {
            let basic_pitch_required = workflow.as_ref().is_some_and(|workflow| {
                workflow.policy_for_model("basic_pitch") == Some(WorkflowExecutionPolicyV1::Always)
            });
            let model = resolved
                .iter()
                .find(|model| model.model_id == "basic_pitch");
            let policy = execution_policy_for(
                workflow.as_ref(),
                "notes.basic_pitch",
                WorkflowExecutionPolicyV1::OnDisagreement,
            );
            let scheduled = schedule(ConditionalScheduleRequest {
                capability: "notes.basic_pitch",
                policy,
                profile: request.analysis.profile,
                source_range,
                review_regions: &baseline_review_regions,
                relevant_reasons: NOTE_DISAGREEMENT_REASONS,
                optional_usable: model.is_some(),
                required: basic_pitch_required,
                supports_windowed_input: true,
                full_input_on_disagreement: false,
            })
            .map_err(|error| error.for_request(&request.request_id))?;
            conditional_schedule.push(ConditionalScheduleRecordV1::new(
                "notes.basic_pitch",
                policy,
                &scheduled,
            ));
            if let ScheduledExecution::Skip(reason) = scheduled {
                record_schedule_skip(&mut degraded_reasons, "notes.basic_pitch", reason);
            } else if let Some(model) = model {
                let (input, _) = workflow_bound_audio(
                    plan.workflow_execution.as_ref(),
                    "notes.basic_pitch",
                    &workflow_audio,
                    &analysis_input,
                    analysis_role,
                )?;
                match run_basic_pitch_schedule(
                    model,
                    &ffmpeg,
                    &input,
                    &output_root,
                    source_range,
                    &scheduled,
                    cancellation,
                ) {
                    Ok(evidence) => basic_pitch_evidence = evidence,
                    Err(error)
                        if error.code == EngineErrorCode::Cancelled || basic_pitch_required =>
                    {
                        return Err(error);
                    }
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
        let mut technique_evidence = Vec::<TechniqueEvidenceV1>::new();
        for model_id in ["rosvot", "stars"] {
            let Some(model) = resolved.iter().find(|model| model.model_id == model_id) else {
                continue;
            };
            let notes_capability = format!("notes.{model_id}");
            let include_notes = has_capability(&plan, &notes_capability);
            let include_technique =
                model_id == "stars" && has_capability(&plan, "technique.analyze");
            if !include_notes && !include_technique {
                continue;
            }

            let notes_runs = if include_notes {
                let policy = execution_policy_for(
                    workflow.as_ref(),
                    &notes_capability,
                    WorkflowExecutionPolicyV1::MaximumOnly,
                );
                let scheduled = schedule(ConditionalScheduleRequest {
                    capability: &notes_capability,
                    policy,
                    profile: request.analysis.profile,
                    source_range,
                    review_regions: &baseline_review_regions,
                    relevant_reasons: NOTE_DISAGREEMENT_REASONS,
                    optional_usable: true,
                    required: false,
                    supports_windowed_input: false,
                    full_input_on_disagreement: false,
                })?;
                conditional_schedule.push(ConditionalScheduleRecordV1::new(
                    &notes_capability,
                    policy,
                    &scheduled,
                ));
                match scheduled {
                    ScheduledExecution::Skip(reason) => {
                        record_schedule_skip(&mut degraded_reasons, &notes_capability, reason);
                        false
                    }
                    ScheduledExecution::FullInput | ScheduledExecution::Windows(_) => true,
                }
            } else {
                false
            };

            let technique_runs = if include_technique {
                let capability = "technique.analyze";
                let policy = execution_policy_for(
                    workflow.as_ref(),
                    capability,
                    WorkflowExecutionPolicyV1::MaximumOnly,
                );
                let scheduled = schedule(ConditionalScheduleRequest {
                    capability,
                    policy,
                    profile: request.analysis.profile,
                    source_range,
                    review_regions: &baseline_review_regions,
                    relevant_reasons: NOTE_DISAGREEMENT_REASONS,
                    optional_usable: true,
                    required: false,
                    supports_windowed_input: false,
                    full_input_on_disagreement: false,
                })?;
                conditional_schedule.push(ConditionalScheduleRecordV1::new(
                    capability, policy, &scheduled,
                ));
                match scheduled {
                    ScheduledExecution::Skip(reason) => {
                        record_schedule_skip(&mut degraded_reasons, capability, reason);
                        false
                    }
                    ScheduledExecution::FullInput | ScheduledExecution::Windows(_) => true,
                }
            } else {
                false
            };

            if !notes_runs && !technique_runs {
                continue;
            }
            let route_capability = if notes_runs {
                notes_capability.as_str()
            } else {
                "technique.analyze"
            };
            let (input, _) = workflow_bound_audio(
                plan.workflow_execution.as_ref(),
                route_capability,
                &workflow_audio,
                &analysis_input,
                analysis_role,
            )?;
            let result = run_advanced_note_challenger(
                model,
                &input,
                &output_root,
                canonical_words.as_deref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "advanced-note challenger requires TimedTranscript word boundaries",
                    )
                })?,
                source_start,
                source_duration,
                technique_runs,
                cancellation,
            );
            match result {
                Ok(evidence) => {
                    if technique_runs {
                        match evidence.technique_artifact(source_start, source_duration) {
                            Ok(Some(technique)) => {
                                artifacts.technique_evidence = Some(write_json_artifact(
                                    &output_root,
                                    Path::new("analysis/technique-evidence.json"),
                                    TECHNIQUE_MEDIA_TYPE,
                                    &technique,
                                )?);
                                technique_evidence.push(technique);
                            }
                            Ok(None) => degraded_reasons.push(
                                "optional capability technique.analyze produced no technique evidence"
                                    .to_string(),
                            ),
                            Err(error) => degraded_reasons.push(format!(
                                "optional capability technique.analyze produced invalid evidence: {}",
                                error.message
                            )),
                        }
                    }
                    if notes_runs {
                        advanced_note_evidence.push(evidence);
                    }
                }
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(output_root.join(format!("worker/{model_id}")));
                    let requested_routes = [
                        notes_runs.then_some(notes_capability.as_str()),
                        technique_runs.then_some("technique.analyze"),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" + ");
                    degraded_reasons.push(format!(
                        "optional capability {requested_routes} failed: {}",
                        error.message
                    ));
                }
            }
        }
        let mut timed_note_evidence = Vec::<TimedNoteExpertEvidenceV1>::new();
        if has_capability(&plan, "notes.jbm555") {
            let model = resolved_model(&resolved, "jbm555_cectc_80")?;
            let separator_generation = resolved
                .iter()
                .find(|resource| resource.model_id == "bs_roformer_leap_xe90_vocals")
                .map(|resource| resource.generation.as_str())
                .unwrap_or("caller-vocal");
            let directory = create_task_dir(&output_root, "worker/jbm555")?;
            let backend = openvino_backend(model)?;
            let outputs = SupervisedWorker::run(
                &model.runtime_executable,
                &WorkerExpectation {
                    component: "uta-openvino-worker".to_string(),
                    runtime_recipe_digest: model.runtime_recipe_digest.clone(),
                },
                &NativeTask {
                    task_id: format!("{}-jbm555", request.request_id),
                    node_id: "notes.jbm555".to_string(),
                    presentation_node_id: None,
                    model_id: model.model_id.clone(),
                    input_artifacts: vec![primary.path.clone(), analysis_input.clone()],
                    output_dir: directory,
                    config: serde_json::json!({
                        "model_path": model.model_path,
                        "backend": backend,
                        "source_start": source_start,
                        "source_duration": source_duration,
                        "model_generation": model.generation,
                        "mix_audio_identity": "task-mix",
                        "vocal_audio_identity": "task-vocal",
                        "separator_model_generation": separator_generation,
                        "vocal_preparation_generation": "native-44k1"
                    }),
                    timeout: Duration::from_secs(4 * 60 * 60),
                },
                cancellation,
                |_| {},
            );
            match outputs {
                Ok(outputs) => {
                    let evidence = parse_jbm555_evidence(
                        typed_worker_output(&outputs, "jbm555_note_evidence")?,
                        Jbm555ExpectedInputsV1 {
                            source_start,
                            source_duration,
                            mix_audio_identity: "task-mix",
                            vocal_audio_identity: "task-vocal",
                            separator_model_generation: separator_generation,
                            vocal_preparation_generation: "native-44k1",
                        },
                    )?;
                    timed_note_evidence.push(evidence.timed_note_evidence(
                        Jbm555ExpectedInputsV1 {
                            source_start,
                            source_duration,
                            mix_audio_identity: "task-mix",
                            vocal_audio_identity: "task-vocal",
                            separator_model_generation: separator_generation,
                            vocal_preparation_generation: "native-44k1",
                        },
                    )?);
                }
                Err(error) if error.code == EngineErrorCode::Cancelled => return Err(error),
                Err(error) => degraded_reasons.push(format!(
                    "optional capability notes.jbm555 failed: {}",
                    error.message
                )),
            }
        }
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        let singing_fusion = if has_capability(&plan, "fusion.singing") {
            let lifecycle = begin_node("singing-fusion", "fusion.singing", None, FUSION_VERSION);
            let output = execute_singing_fusion_stage_with_timed_notes(
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
                game_evidence.as_ref(),
                acoustic_evidence.as_ref(),
                &advanced_note_evidence,
                &timed_note_evidence,
                &technique_evidence,
                &request.boundary_constraints,
                source_start,
                source_duration,
                &fusion_pitch_owner,
            )?;
            lifecycle.artifact("singing_fusion_evidence");
            lifecycle.complete();
            Some(output)
        } else {
            None
        };
        let singing = if has_capability(&plan, "fusion.candidate_graph") {
            let decision_implementation = match fusion_mode {
                FusionModeV1::Algorithm => HSMM_VERSION.to_string(),
                FusionModeV1::AiJudgment => {
                    let adapter = fusion_adapter
                        .as_ref()
                        .expect("AI mode resolved its required adapter");
                    format!("{}@{}", adapter.identity, adapter.version)
                }
            };
            let lifecycle = begin_node(
                "candidate-graph",
                "fusion.candidate_graph",
                None,
                decision_implementation,
            );
            let mut output = execute_candidate_graph_stage(
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
                match fusion_mode {
                    FusionModeV1::Algorithm => FusionDecisionModeV1::Algorithm,
                    FusionModeV1::AiJudgment => FusionDecisionModeV1::AiJudgment {
                        executable: &fusion_adapter
                            .as_ref()
                            .expect("AI mode resolved its required adapter")
                            .executable,
                        timeout: Duration::from_secs(600),
                        cancellation,
                    },
                },
            )?;
            output.review_regions.extend(topology_reviews.clone());
            output.review_regions = merge_regions(output.review_regions);
            lifecycle.artifact("candidate_graph");
            lifecycle.complete();
            Some(output)
        } else {
            None
        };
        let quantized_candidate = if has_capability(&plan, "rhythm.quantize") {
            let lifecycle = begin_node(
                "rhythm-quantize",
                "rhythm.quantize",
                None,
                QUANTIZATION_VERSION,
            );
            let singing = singing.as_ref().ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::MissingRequiredInput,
                    "rhythm.quantize requires a canonical singing track",
                )
                .with_capability("rhythm.quantize")
            })?;
            let source_end = source_start.checked_add(source_duration).ok_or_else(|| {
                EngineError::new(
                    EngineErrorCode::TimelineInvalid,
                    "quantization source timeline overflowed",
                )
                .with_capability("rhythm.quantize")
            })?;
            let source_range = TimeRange::new(source_start, source_end).map_err(|message| {
                EngineError::new(EngineErrorCode::TimelineInvalid, message)
                    .with_capability("rhythm.quantize")
            })?;
            let hard_boundaries = request
                .boundary_constraints
                .iter()
                .filter(|constraint| constraint.authority == BoundaryAuthority::Hard)
                .map(|constraint| {
                    TimeRange::new(constraint.start, constraint.end()?).map_err(|message| {
                        EngineError::new(EngineErrorCode::InvalidConstraints, message)
                            .with_capability("rhythm.quantize")
                    })
                })
                .collect::<EngineResult<Vec<_>>>()?;
            let mut track = singing.track.clone();
            let report = quantize_singing_track(
                &mut track,
                request.musical_context.as_ref().ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::MissingRequiredInput,
                        "rhythm.quantize requires musical context",
                    )
                    .with_capability("rhythm.quantize")
                })?,
                source_range,
                &hard_boundaries,
            )?;
            lifecycle.artifact("quantized_candidate_graph");
            lifecycle.complete();
            Some((track, report))
        } else {
            None
        };
        if cancellation.is_cancelled() {
            return Err(cancelled(request));
        }
        let analyzed_audio = decode_audio(&ffmpeg, analysis_role, &analysis_input)?;
        let audio_quality = evaluate_audio_quality(QualityEvaluationInput {
            profile: request.analysis.profile,
            planned_gates: &plan.quality_gates,
            evaluated_audio_role: analysis_role,
            source_start,
            expected_duration: source_duration,
            actual_duration: analyzed_audio.facts.duration,
            source: primary_decoded.metrics,
            analyzed: analyzed_audio.metrics,
            cleanup: cleanup_comparison,
            vocal_topology: vocal_topology.as_ref(),
            instrumental: instrumental_quality.as_ref(),
        })
        .map_err(|error| error.for_request(&request.request_id))?;
        enforce_required_quality(&audio_quality)
            .map_err(|error| error.for_request(&request.request_id))?;
        for reason in quality_degraded_reasons(&audio_quality) {
            if !degraded_reasons.contains(&reason) {
                degraded_reasons.push(reason);
            }
        }
        let participating_resources = resolved
            .iter()
            .filter(|resource| match resource.model_id.as_str() {
                "fcpe" => fcpe_evidence.is_some(),
                "basic_pitch" => basic_pitch_evidence.is_some(),
                "firered_asr2_aed" => firered_evidence.is_some(),
                "melband_roformer_denoise_aufr33" => denoise_participated,
                "melband_roformer_dereverb_anvuew" => dereverb_participated,
                "stars" | "rosvot" => {
                    advanced_note_evidence
                        .iter()
                        .any(|evidence| evidence.model_id == resource.model_id)
                        || technique_evidence
                            .iter()
                            .any(|evidence| evidence.model_id == resource.model_id)
                }
                _ => true,
            })
            .collect::<Vec<_>>();
        let fusion_decision = singing
            .as_ref()
            .map(|output| match &output.decision {
                CandidatePathDecisionV1::Algorithm {
                    candidate_set_digest,
                    selected_candidate_ids,
                } => Ok::<_, EngineError>(FusionDecisionProvenanceV1::Algorithm {
                    selector: HSMM_VITERBI_SELECTOR.to_string(),
                    selector_version: HSMM_VERSION.to_string(),
                    candidate_set_digest: candidate_set_digest.clone(),
                    selected_candidate_ids: selected_candidate_ids.clone(),
                    reuse_policy: AnalysisReusePolicyV1::Deterministic,
                }),
                CandidatePathDecisionV1::AiJudgment {
                    candidate_set_digest,
                    selected_candidate_ids,
                    response_digest,
                } => {
                    let adapter = fusion_adapter.as_ref().ok_or_else(|| {
                        EngineError::new(
                            EngineErrorCode::InternalError,
                            "AI fusion decision lost its resolved adapter identity",
                        )
                    })?;
                    Ok(FusionDecisionProvenanceV1::AiJudgment {
                        adapter_resource: FUSION_AGENT_ADAPTER_RESOURCE.to_string(),
                        adapter_protocol: FUSION_AGENT_PROTOCOL.to_string(),
                        adapter_protocol_version: adapter.protocol_version,
                        adapter_identity: adapter.identity.clone(),
                        adapter_version: adapter.version.clone(),
                        candidate_set_digest: candidate_set_digest.clone(),
                        selected_candidate_ids: selected_candidate_ids.clone(),
                        response_digest: response_digest.clone(),
                        reuse_policy: AnalysisReusePolicyV1::PreservedRevisionOnly,
                    })
                }
            })
            .transpose()?;
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
            audio_quality_version: AUDIO_QUALITY_VERSION,
            quality_gates: &plan.quality_gates,
            calibration_version: CALIBRATION_VERSION,
            finalize_vocal_chart_version: FINALIZE_VOCAL_CHART_VERSION,
            fusion_version: FUSION_VERSION,
            fusion_decision: fusion_decision.as_ref(),
            quantization_version: QUANTIZATION_VERSION,
            postprocess_version: POSTPROCESS_VERSION,
        })?;
        let finalization_lifecycle = has_capability(&plan, "finalize.vocal_chart").then(|| {
            begin_node(
                "vocal-chart",
                "finalize.vocal_chart",
                None,
                FINALIZE_VOCAL_CHART_VERSION,
            )
        });
        publish_candidate_artifacts(
            &output_root,
            request.requested_artifacts.singing_analysis,
            has_capability(&plan, "finalize.vocal_chart"),
            request.analysis.preserve_continuous_pitch,
            &fingerprint,
            fusion_decision.as_ref(),
            singing.as_ref(),
            quantized_candidate.as_ref().map(|(track, _)| track),
            quantized_candidate.as_ref().map(|(_, report)| report),
            &mut artifacts,
            cancellation,
        )?;
        if let Some(lifecycle) = finalization_lifecycle {
            lifecycle.artifact("candidate_vocal_chart");
            lifecycle.complete();
        }
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

        for reason in &degraded_reasons {
            emit_degraded(reason.clone());
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
            fusion_decision,
            quantization_version: QUANTIZATION_VERSION.to_string(),
            audio_quality_version: AUDIO_QUALITY_VERSION.to_string(),
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
                decoded_audio: decoded_sources
                    .into_iter()
                    .map(|decoded| decoded.facts)
                    .collect(),
                warnings: Vec::new(),
                quantization: quantized_candidate
                    .as_ref()
                    .map(|(_, report)| report.clone()),
                audio_quality: Some(audio_quality),
                evidence: serde_json::json!({
                    "acoustic": acoustic_artifact,
                    "acoustic_algorithm": ACOUSTIC_DSP_VERSION,
                    "game_note_count": game_evidence.as_ref().map(|evidence| evidence.notes.len()),
                    "game_conditioned_boundary_count": game_conditioned_boundary_count,
                    "fcpe_frame_count": fcpe_evidence.as_ref().map(|evidence| evidence.frequency_hz.len()),
                    "advanced_note_counts": advanced_note_evidence.iter().map(|evidence| {
                        (evidence.model_id.clone(), evidence.notes.len())
                    }).collect::<std::collections::BTreeMap<_, _>>(),
                    "technique_experts": technique_evidence.iter().map(|evidence| {
                        evidence.model_id.clone()
                    }).collect::<Vec<_>>(),
                    "conditional_schedule": conditional_schedule,
                    "transcript_disagreement_regions": transcript_disagreement_regions,
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
                uta_runtime_manager::ResourceKind::Model => {
                    match self.runtime_manager.resolve_model_with_backend(
                        &resource.id,
                        request.execution_policy.runtime_policy,
                        request.execution_policy.requested_backend_for(&resource.id),
                    ) {
                        Ok(model) => resolved.push(model),
                        Err(error) if !requirement.required => degraded.push(format!(
                            "optional capability {} skipped: {}",
                            requirement.reason, error
                        )),
                        Err(error) => return Err(EngineError::from(error)),
                    }
                }
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

#[cfg(test)]
mod tests;
