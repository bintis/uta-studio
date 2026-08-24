// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

//! Compiled Workflow routing helpers kept separate from the core Engine stages.

use sha2::{Digest, Sha256};

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn publish_candidate_artifacts(
    output_root: &Path,
    request_singing_analysis: bool,
    request_vocal_chart: bool,
    preserve_continuous_pitch: bool,
    fingerprint: &str,
    singing: Option<&SingingStagesOutput>,
    quantized_candidate_track: Option<&crate::fusion::CanonicalSingingTrack>,
    quantization: Option<&crate::quantization::QuantizationReportV1>,
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
        if quantized_candidate_track.is_some() != quantization.is_some() {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "Candidate quantization track/report presence is inconsistent",
            ));
        }
        let candidate_track = quantized_candidate_track.unwrap_or(&singing.track);
        let chart = finalize_candidate_vocal_chart(
            candidate_track,
            fingerprint,
            preserve_continuous_pitch,
            quantization,
        )?;
        artifacts.candidate_vocal_chart = Some(write_json_artifact(
            output_root,
            Path::new("candidate/vocal-chart.json"),
            VOCAL_CHART_MEDIA_TYPE,
            &chart,
        )?);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_advanced_note_challenger(
    model: &uta_runtime_manager::ResolvedModel,
    analysis_input: &Path,
    output_root: &Path,
    words: &[crate::fusion::CanonicalWordBoundary],
    source_start: u64,
    source_duration: u64,
    include_technique: bool,
    cancellation: &CancellationToken,
) -> EngineResult<AdvancedNoteEvidenceV1> {
    let model_id = model.model_id.as_str();
    if !matches!(model_id, "stars" | "rosvot") || (include_technique && model_id != "stars") {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "advanced-note route rejects baseline or technique substitution",
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
    let device = match model.backend {
        uta_runtime_manager::NativeBackend::CpuReference => "cpu",
        uta_runtime_manager::NativeBackend::OpenVino => {
            match std::env::var("UTA_STUDIO_ADVANCED_NOTE_DIAGNOSTIC_DEVICE") {
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
            }
        }
        _ => {
            return Err(EngineError::new(
                EngineErrorCode::RuntimeResolutionFailed,
                "advanced-note route requires an OpenVINO IR backend",
            ));
        }
    };
    let directory = create_task_dir(output_root, &format!("worker/{model_id}"))?;
    let task_capability = if include_technique {
        "technique.analyze".to_string()
    } else {
        format!("notes.{model_id}")
    };
    let outputs = run_native_task(
        model,
        "uta-openvino-worker",
        &format!("task-{model_id}"),
        &task_capability,
        analysis_input,
        &directory,
        serde_json::json!({
            "model_path": model.model_path,
            "model_generation": model.generation,
            "source_start": source_start,
            "source_duration": source_duration,
            "timed_transcript_generation": timed_transcript_generation,
            "words": word_config,
            "device": device,
            "include_technique": include_technique
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

pub(super) fn optional_execution_supported(capability: &str) -> bool {
    matches!(
        capability,
        "pitch.secondary"
            | "notes.basic_pitch"
            | "speech.transcribe.challenger"
            | "audio.denoise"
            | "audio.dereverb"
            | "notes.rosvot"
            | "notes.stars"
            | "technique.analyze"
    )
}

pub(super) fn execution_policy_for(
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

pub(super) fn record_schedule_skip(
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

pub(super) fn workflow_cleanup_steps(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    resolved: &[uta_runtime_manager::ResolvedModel],
) -> Vec<(String, Option<String>)> {
    if let Some(workflow) = workflow {
        return workflow
            .nodes
            .iter()
            .filter(|node| node.execution_state == WorkflowNodeExecutionStateV1::Ready)
            .filter_map(|node| {
                node.capabilities
                    .iter()
                    .find(|capability| {
                        matches!(capability.as_str(), "audio.denoise" | "audio.dereverb")
                    })
                    .map(|capability| (capability.clone(), Some(node.analysis_node.clone())))
            })
            .collect();
    }
    [
        ("melband_roformer_denoise_aufr33", "audio.denoise"),
        ("melband_roformer_dereverb_anvuew", "audio.dereverb"),
    ]
    .into_iter()
    .filter(|(model, _)| resolved.iter().any(|item| item.model_id == *model))
    .map(|(_, capability)| (capability.to_string(), None))
    .collect()
}

pub(super) fn record_workflow_audio(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    capability: &str,
    output_port: &str,
    artifacts: &mut BTreeMap<(String, String), (PathBuf, String)>,
    path: &Path,
    role: &str,
) {
    let Some(workflow) = workflow else {
        return;
    };
    for node in workflow.ready_nodes_for_capability(capability) {
        artifacts.insert(
            (node.analysis_node.clone(), output_port.to_string()),
            (path.to_path_buf(), role.to_string()),
        );
    }
}

pub(super) fn workflow_bound_audio(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    capability: &str,
    artifacts: &BTreeMap<(String, String), (PathBuf, String)>,
    fallback_path: &Path,
    fallback_role: &str,
) -> EngineResult<(PathBuf, String)> {
    let Some(workflow) = workflow else {
        return Ok((fallback_path.to_path_buf(), fallback_role.to_string()));
    };
    let node = workflow.node_for_capability(capability).ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::MissingCapability,
            format!("compiled workflow omitted execution node for {capability}"),
        )
        .with_capability(capability)
    })?;
    let binding = node
        .input_bindings
        .iter()
        .find(|binding| binding.analyzer_attachment && binding.semantic_type == "audio")
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::InvalidContract,
                format!(
                    "compiled workflow node {} has no audio attachment",
                    node.instance_id
                ),
            )
        })?;
    artifacts
        .get(&(binding.from_node.clone(), binding.from_port.clone()))
        .cloned()
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::MissingRequiredInput,
                format!(
                    "compiled workflow analyzer {} selected unavailable artifact {}:{}",
                    node.instance_id, binding.from_node, binding.from_port
                ),
            )
            .with_capability(capability)
        })
}

pub(super) fn workflow_transform_input(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    analysis_node: Option<&str>,
    artifacts: &BTreeMap<(String, String), (PathBuf, String)>,
    fallback_path: &Path,
    fallback_role: &str,
) -> EngineResult<(PathBuf, String)> {
    let (Some(workflow), Some(analysis_node)) = (workflow, analysis_node) else {
        return Ok((fallback_path.to_path_buf(), fallback_role.to_string()));
    };
    let node = workflow
        .nodes
        .iter()
        .find(|node| node.analysis_node == analysis_node)
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::InvalidContract,
                "workflow cleanup execution node disappeared from its plan",
            )
        })?;
    let binding = node
        .input_bindings
        .iter()
        .find(|binding| !binding.analyzer_attachment && binding.to_port == "audio")
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::InvalidContract,
                format!(
                    "workflow cleanup {} has no audio input binding",
                    node.instance_id
                ),
            )
        })?;
    artifacts
        .get(&(binding.from_node.clone(), binding.from_port.clone()))
        .cloned()
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::MissingRequiredInput,
                format!(
                    "workflow cleanup {} selected unavailable artifact {}:{}",
                    node.instance_id, binding.from_node, binding.from_port
                ),
            )
        })
}

pub(super) fn has_capability(plan: &EnginePlan, capability: &str) -> bool {
    plan.execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == capability)
}

pub(super) fn resolved_model<'a>(
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

pub(super) fn run_openvino_workflow_cleanup(
    task: &DenoiseTask<'_>,
    analysis_node: &str,
    denoise: bool,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let safe = analysis_node
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
                char::from(byte)
            } else {
                '_'
            }
        })
        .collect::<String>();
    let worker_directory = format!("worker/workflow/{safe}");
    let destination = format!("workflow-audio/{safe}.flac");
    let spec = if denoise {
        CleanupSpec {
            model_id: "melband_roformer_denoise_aufr33",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.denoise",
            semantic_output: "dry",
            artifact: "clean_lead_vocal",
            worker_directory: &worker_directory,
            destination: &destination,
        }
    } else {
        CleanupSpec {
            model_id: "melband_roformer_dereverb_anvuew",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.dereverb",
            semantic_output: "noreverb",
            artifact: "dereverbed_vocal",
            worker_directory: &worker_directory,
            destination: &destination,
        }
    };
    run_openvino_cleanup(task, &spec, cancellation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{WorkflowBindingV1, WorkflowExecutionPolicyV1, WorkflowTerminalOutputV1};
    use crate::workflow_executor::{
        WorkflowExecutionNodePlanV1, WorkflowNodeExecutionStateV1, WorkflowPlanIdentityV1,
    };

    fn node(
        instance: &str,
        analysis_node: &str,
        capability: &str,
        input: Option<(&str, &str)>,
    ) -> WorkflowExecutionNodePlanV1 {
        WorkflowExecutionNodePlanV1 {
            instance_id: instance.to_string(),
            analysis_node: analysis_node.to_string(),
            capabilities: vec![capability.to_string()],
            execution_policy: WorkflowExecutionPolicyV1::Always,
            execution_state: WorkflowNodeExecutionStateV1::Ready,
            priority: 0,
            depends_on: input
                .map(|(from, _)| vec![from.to_string()])
                .unwrap_or_default(),
            input_bindings: input
                .map(|(from, port)| {
                    vec![WorkflowBindingV1 {
                        from_node: from.to_string(),
                        from_port: port.to_string(),
                        to_node: analysis_node.to_string(),
                        to_port: "audio".to_string(),
                        semantic_type: "audio".to_string(),
                        audio_role: Some("lead_vocal".to_string()),
                        execution_active: true,
                        analyzer_attachment: capability.starts_with("pitch."),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    fn plan() -> CompiledWorkflowExecutionPlanV1 {
        CompiledWorkflowExecutionPlanV1 {
            identity: WorkflowPlanIdentityV1 {
                contract: "uta.workflow-execution".to_string(),
                version: 1,
                workflow_schema_version: 2,
                workflow_id: "workflow:test".to_string(),
                workflow_revision: 1,
                definition_digest: "a".repeat(32),
            },
            nodes: vec![
                node("cleanup-a", "workflow.cleanup-a", "audio.denoise", None),
                node(
                    "cleanup-b",
                    "workflow.cleanup-b",
                    "audio.denoise",
                    Some(("workflow.cleanup-a", "audio")),
                ),
                node(
                    "pitch",
                    "workflow.pitch",
                    "pitch.track",
                    Some(("workflow.cleanup-a", "audio")),
                ),
            ],
            terminal_outputs: Vec::<WorkflowTerminalOutputV1>::new(),
        }
    }

    #[test]
    fn actual_route_uses_exact_analyzer_binding_and_keeps_duplicate_steps() {
        let plan = plan();
        let steps = workflow_cleanup_steps(Some(&plan), &[]);
        assert_eq!(
            steps,
            [
                (
                    "audio.denoise".to_string(),
                    Some("workflow.cleanup-a".to_string())
                ),
                (
                    "audio.denoise".to_string(),
                    Some("workflow.cleanup-b".to_string())
                ),
            ]
        );
        let selected = std::env::temp_dir().join("selected-cleanup.flac");
        let latest = std::env::temp_dir().join("latest-cleanup.flac");
        let artifacts = BTreeMap::from([(
            ("workflow.cleanup-a".to_string(), "audio".to_string()),
            (selected.clone(), "lead_vocal".to_string()),
        )]);
        let (path, role) = workflow_bound_audio(
            Some(&plan),
            "pitch.track",
            &artifacts,
            &latest,
            "clean_lead_vocal",
        )
        .unwrap();
        assert_eq!(path, selected);
        assert_eq!(role, "lead_vocal");
    }
}
