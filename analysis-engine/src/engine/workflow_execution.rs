// Copyright 2026 Uta! Studio contributors
// Licensed under the Apache License, Version 2.0.

//! Compiled Workflow routing helpers kept separate from the core Engine stages.

use std::collections::BTreeSet;

use super::worker_tasks::{run_ggml_cleanup, typed_worker_output};
use super::*;
use crate::execution::{NativeTask, SupervisedWorker, WorkerExpectation};

pub(super) struct DenoiseTask<'a> {
    pub(super) model_path: &'a Path,
    pub(super) executable: &'a Path,
    pub(super) runtime_recipe_digest: Option<&'a str>,
    pub(super) route: RoformerRoute,
    pub(super) ffmpeg: &'a Path,
    pub(super) input: &'a Path,
    pub(super) output_root: &'a Path,
    pub(super) source_duration: u64,
    pub(super) task_id: &'a str,
}

pub(super) struct DualSeparationOutput {
    pub(super) vocals: SeparationOutput,
    pub(super) instrumental: SeparationOutput,
}

pub(super) struct LeadIsolationOutput {
    pub(super) stem: SeparationOutput,
    pub(super) lead_profile: crate::audio::SignalProfile,
    pub(super) residual_profile: crate::audio::SignalProfile,
}

pub(super) struct CleanupSpec<'a> {
    pub(super) model_id: &'a str,
    pub(super) role: crate::contract::AudioRole,
    pub(super) node_id: &'a str,
    pub(super) presentation_node_id: Option<&'a str>,
    pub(super) semantic_output: &'a str,
    pub(super) artifact: &'a str,
    pub(super) worker_directory: &'a str,
    pub(super) destination: &'a str,
}

pub(super) fn create_task_dir(root: &Path, relative: &str) -> EngineResult<PathBuf> {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn publish_candidate_artifacts(
    output_root: &Path,
    request_singing_analysis: bool,
    request_vocal_chart: bool,
    preserve_continuous_pitch: bool,
    fingerprint: &str,
    fusion_decision: Option<&FusionDecisionProvenanceV1>,
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
        let fusion_decision = fusion_decision.ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                "requested SingingAnalysis has no final fusion decision provenance",
            )
        })?;
        let analysis = SingingAnalysisV1::new(
            &singing.track,
            singing.fusion.candidates.clone(),
            singing.fusion.hard_boundaries.clone(),
            singing.review_regions.clone(),
            fingerprint,
            fusion_decision,
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

pub(super) fn optional_execution_supported(capability: &str) -> bool {
    matches!(
        capability,
        "pitch.secondary"
            | "pitch.secondary.rmvpe"
            | "pitch.secondary.fcpe"
            | "speech.transcribe.challenger"
            | "audio.denoise"
            | "audio.dereverb"
    )
}

pub(super) fn workflow_cleanup_steps(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    resolved: &[uta_runtime_manager::ResolvedModel],
) -> Vec<(String, Option<String>)> {
    if let Some(workflow) = workflow {
        let model_is_resolved = |capability: &str| {
            let model_id = match capability {
                "audio.denoise" => "melband_roformer_denoise_aufr33",
                "audio.dereverb" => "melband_roformer_dereverb_anvuew",
                _ => return false,
            };
            resolved.iter().any(|model| model.model_id == model_id)
        };
        return workflow
            .nodes
            .iter()
            .filter(|node| node.execution_state == WorkflowNodeExecutionStateV1::Ready)
            .filter_map(|node| {
                node.capabilities
                    .iter()
                    .find(|capability| model_is_resolved(capability))
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

/// Reconnect a typed cached primary source to the workflow output it already
/// embodies. These upstream nodes are intentionally `NotRequested`; their
/// downstream bindings still need the reused semantic audio artifact.
pub(super) fn record_reused_workflow_audio(
    workflow: Option<&CompiledWorkflowExecutionPlanV1>,
    role: crate::contract::AudioRole,
    artifacts: &mut BTreeMap<(String, String), (PathBuf, String)>,
    path: &Path,
) {
    let Some(workflow) = workflow else {
        return;
    };
    let satisfied_outputs: &[(&str, &str)] = match role {
        crate::contract::AudioRole::VocalStem | crate::contract::AudioRole::GuideVocals => {
            &[("audio.extract_vocals", "vocal")]
        }
        crate::contract::AudioRole::LeadVocal => &[("audio.lead_isolate", "lead")],
        crate::contract::AudioRole::CleanLeadVocal => &[
            ("audio.lead_isolate", "lead"),
            ("audio.extract_vocals", "vocal"),
        ],
        crate::contract::AudioRole::OriginalMix
        | crate::contract::AudioRole::Instrumental
        | crate::contract::AudioRole::BackingVocal
        | crate::contract::AudioRole::HarmonyVocal => &[],
    };
    for (capability, output_port) in satisfied_outputs {
        for node in workflow.nodes.iter().filter(|node| {
            node.execution_state == WorkflowNodeExecutionStateV1::NotRequested
                && node.capabilities.iter().any(|item| item == capability)
        }) {
            artifacts.insert(
                (node.analysis_node.clone(), (*output_port).to_string()),
                (path.to_path_buf(), role.as_str().to_string()),
            );
        }
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
    resolve_workflow_audio(
        workflow,
        artifacts,
        &binding.from_node,
        &binding.from_port,
        &mut BTreeSet::new(),
    )
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

fn resolve_workflow_audio(
    workflow: &CompiledWorkflowExecutionPlanV1,
    artifacts: &BTreeMap<(String, String), (PathBuf, String)>,
    from_node: &str,
    from_port: &str,
    visited: &mut BTreeSet<(String, String)>,
) -> Option<(PathBuf, String)> {
    let key = (from_node.to_string(), from_port.to_string());
    if let Some(artifact) = artifacts.get(&key) {
        return Some(artifact.clone());
    }
    if !visited.insert(key) {
        return None;
    }
    let producer = workflow
        .nodes
        .iter()
        .find(|node| node.analysis_node == from_node)?;
    let optional_cleanup = producer
        .capabilities
        .iter()
        .any(|capability| matches!(capability.as_str(), "audio.denoise" | "audio.dereverb"));
    if producer.execution_state == WorkflowNodeExecutionStateV1::Ready && !optional_cleanup {
        return None;
    }
    let input = producer.input_bindings.iter().find(|binding| {
        !binding.analyzer_attachment
            && binding.execution_active
            && binding.semantic_type == "audio"
            && binding.to_port == "audio"
    })?;
    resolve_workflow_audio(
        workflow,
        artifacts,
        &input.from_node,
        &input.from_port,
        visited,
    )
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

pub(super) fn workflow_cleanup_output_role(input_role: &str) -> crate::contract::AudioRole {
    if input_role == crate::contract::AudioRole::Instrumental.as_str() {
        crate::contract::AudioRole::Instrumental
    } else {
        crate::contract::AudioRole::CleanLeadVocal
    }
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

pub(super) fn run_ggml_dual_separation(
    task: &DenoiseTask<'_>,
    model_id: &str,
    cancellation: &CancellationToken,
) -> EngineResult<DualSeparationOutput> {
    let directory = create_task_dir(task.output_root, "worker/vocal-instrumental")?;
    let semantic_output = if model_id == "bs_roformer_leap_xe90_instrumental" {
        "instrumental+vocal_residual"
    } else {
        "vocal+instrumental_residual"
    };
    let (component, config) =
        roformer_dispatch_config(&task.route, task.model_path, semantic_output)?;
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: component.to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: "audio.separate_vocal_bgm".to_string(),
            presentation_node_id: None,
            model_id: model_id.to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config,
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    if outputs.len() != 2 {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "GGML separator must publish exactly vocal and instrumental FLAC outputs",
        ));
    }
    let vocal = typed_worker_output(&outputs, "guide_vocals")?.to_path_buf();
    let instrumental = typed_worker_output(&outputs, "instrumental")?.to_path_buf();
    for (artifact, path) in [("guide_vocals", &vocal), ("instrumental", &instrumental)] {
        if outputs
            .iter()
            .find(|output| output.artifact == artifact)
            .is_none_or(|output| output.media_type != "audio/flac")
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("GGML separator output {artifact} is not lossless FLAC"),
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
                format!("GGML separator output {artifact} did not preserve the source timeline"),
            ));
        }
    }
    let stems = task.output_root.join("stems");
    std::fs::create_dir_all(&stems).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not create separation stem directory: {error}"),
        )
    })?;
    let vocal_relative = PathBuf::from("stems/guide_vocals.flac");
    let instrumental_relative = PathBuf::from("stems/instrumental.flac");
    let vocal_destination = task.output_root.join(&vocal_relative);
    let instrumental_destination = task.output_root.join(&instrumental_relative);
    if vocal_destination.exists() || instrumental_destination.exists() {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "GGML separation stem target already exists",
        ));
    }
    std::fs::rename(&vocal, &vocal_destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish vocal stem: {error}"),
        )
    })?;
    std::fs::rename(&instrumental, &instrumental_destination).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not atomically publish instrumental stem: {error}"),
        )
    })?;
    std::fs::remove_dir_all(&directory).map_err(|error| {
        EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            format!("could not clean GGML separation worker directory: {error}"),
        )
    })?;
    Ok(DualSeparationOutput {
        vocals: SeparationOutput {
            role: crate::contract::AudioRole::GuideVocals,
            artifact: artifact_ref_for_existing(task.output_root, &vocal_relative, "audio/flac")?,
        },
        instrumental: SeparationOutput {
            role: crate::contract::AudioRole::Instrumental,
            artifact: artifact_ref_for_existing(
                task.output_root,
                &instrumental_relative,
                "audio/flac",
            )?,
        },
    })
}

pub(super) fn run_ggml_denoise(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_ggml_cleanup(
        task,
        &CleanupSpec {
            model_id: "melband_roformer_denoise_aufr33",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.denoise",
            presentation_node_id: None,
            semantic_output: "dry",
            artifact: "clean_lead_vocal",
            worker_directory: "worker/denoise",
            destination: "stems/clean_lead_vocal.flac",
        },
        cancellation,
    )
}

pub(super) fn run_ggml_dereverb(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    run_ggml_cleanup(
        task,
        &CleanupSpec {
            model_id: "melband_roformer_dereverb_anvuew",
            role: crate::contract::AudioRole::CleanLeadVocal,
            node_id: "audio.dereverb",
            presentation_node_id: None,
            semantic_output: "noreverb",
            artifact: "dereverbed_vocal",
            worker_directory: "worker/dereverb",
            destination: "stems/dereverbed_clean_lead_vocal.flac",
        },
        cancellation,
    )
}

pub(super) fn run_ggml_harmony(
    task: &DenoiseTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<LeadIsolationOutput> {
    let directory = create_task_dir(task.output_root, "worker/lead-isolate")?;
    let (component, mut config) = roformer_dispatch_config(
        &task.route,
        task.model_path,
        "lead_vocal+backing_vocal_residual",
    )?;
    config["input_semantics"] = serde_json::json!("all_vocals");
    let outputs = SupervisedWorker::run(
        task.executable,
        &WorkerExpectation {
            component: component.to_string(),
            runtime_recipe_digest: task.runtime_recipe_digest.map(str::to_string),
        },
        &NativeTask {
            task_id: task.task_id.to_string(),
            node_id: "audio.lead_isolate".to_string(),
            presentation_node_id: None,
            model_id: "melband_roformer_harmony".to_string(),
            input_artifacts: vec![task.input.to_path_buf()],
            output_dir: directory.clone(),
            config,
            timeout: Duration::from_secs(4 * 60 * 60),
        },
        cancellation,
        |_| {},
    )?;
    if outputs.len() != 2 {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "Karaoke worker must publish exactly lead_vocal and vocal_residual",
        ));
    }
    let lead = typed_worker_output(&outputs, "lead_vocal")?.to_path_buf();
    let residual = typed_worker_output(&outputs, "vocal_residual")?.to_path_buf();
    let lead_decoded = decode_audio(task.ffmpeg, "lead_vocal", &lead)?;
    let residual_decoded = decode_audio(task.ffmpeg, "vocal_residual", &residual)?;
    for (artifact, decoded) in [
        ("lead_vocal", &lead_decoded),
        ("vocal_residual", &residual_decoded),
    ] {
        if outputs
            .iter()
            .find(|output| output.artifact == artifact)
            .is_none_or(|output| output.media_type != "audio/flac")
        {
            return Err(EngineError::new(
                EngineErrorCode::OutputValidationFailed,
                format!("Karaoke worker output {artifact} is not lossless FLAC"),
            ));
        }
        let facts = &decoded.facts;
        if facts.sample_rate != 44_100
            || facts.channels != 2
            || facts.frame_count == 0
            || facts.duration.abs_diff(task.source_duration) > 2_000
        {
            return Err(EngineError::new(
                EngineErrorCode::TimelineInvalid,
                format!("Karaoke {artifact} did not preserve the vocal input timeline"),
            ));
        }
    }
    if cancellation.is_cancelled() {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "lead-isolation publication was cancelled",
        ));
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
            format!("could not clean Karaoke worker directory: {error}"),
        )
    })?;
    Ok(LeadIsolationOutput {
        stem: SeparationOutput {
            role: crate::contract::AudioRole::LeadVocal,
            artifact: artifact_ref_for_existing(task.output_root, &relative, "audio/flac")?,
        },
        lead_profile: lead_decoded.profile,
        residual_profile: residual_decoded.profile,
    })
}

pub(super) fn run_ggml_workflow_cleanup(
    task: &DenoiseTask<'_>,
    analysis_node: &str,
    denoise: bool,
    role: crate::contract::AudioRole,
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
    let operation = if denoise { "denoise" } else { "dereverb" };
    let destination = format!("workflow-audio/{safe}-{operation}.flac");
    let spec = if denoise {
        CleanupSpec {
            model_id: "melband_roformer_denoise_aufr33",
            role,
            node_id: "audio.denoise",
            presentation_node_id: Some(analysis_node),
            semantic_output: "dry",
            artifact: "clean_lead_vocal",
            worker_directory: &worker_directory,
            destination: &destination,
        }
    } else {
        CleanupSpec {
            model_id: "melband_roformer_dereverb_anvuew",
            role,
            node_id: "audio.dereverb",
            presentation_node_id: Some(analysis_node),
            semantic_output: "noreverb",
            artifact: "dereverbed_vocal",
            worker_directory: &worker_directory,
            destination: &destination,
        }
    };
    run_ggml_cleanup(task, &spec, cancellation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{
        FusionModeV1, WorkflowBindingV1, WorkflowExecutionPolicyV1, WorkflowTerminalOutputV1,
    };
    use crate::workflow_executor::{
        WorkflowExecutionNodePlanV1, WorkflowNodeExecutionStateV1, WorkflowPlanIdentityV1,
    };

    #[test]
    fn cleanup_preserves_the_instrumental_branch_role() {
        assert_eq!(
            workflow_cleanup_output_role("instrumental"),
            crate::contract::AudioRole::Instrumental
        );
        assert_eq!(
            workflow_cleanup_output_role("lead_vocal"),
            crate::contract::AudioRole::CleanLeadVocal
        );
    }

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
            parameters: serde_json::Value::Object(Default::default()),
            execution_invocations: Vec::new(),
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
                workflow_schema_version: crate::workflow::WORKFLOW_SCHEMA_VERSION,
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
            fusion_policy: None,
            fusion_mode: FusionModeV1::Algorithm,
        }
    }

    #[test]
    fn only_wired_ggml_optional_execution_is_supported() {
        assert!(optional_execution_supported("pitch.secondary.rmvpe"));
        assert!(optional_execution_supported("speech.transcribe.challenger"));
        assert!(optional_execution_supported("audio.denoise"));
        assert!(!optional_execution_supported("notes.game"));
    }

    #[test]
    fn unresolved_optional_cleanup_is_skipped_and_exact_analyzer_binding_is_kept() {
        let plan = plan();
        assert!(workflow_cleanup_steps(Some(&plan), &[]).is_empty());
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

    #[test]
    fn missing_optional_cleanup_artifact_resolves_through_its_compiled_input() {
        let mut plan = plan();
        let cleanup = plan
            .nodes
            .iter_mut()
            .find(|node| node.analysis_node == "workflow.cleanup-a")
            .unwrap();
        cleanup.depends_on = vec!["workflow.source".to_string()];
        cleanup.input_bindings = vec![WorkflowBindingV1 {
            from_node: "workflow.source".to_string(),
            from_port: "mix".to_string(),
            to_node: cleanup.analysis_node.clone(),
            to_port: "audio".to_string(),
            semantic_type: "audio".to_string(),
            audio_role: Some("vocal".to_string()),
            execution_active: true,
            analyzer_attachment: false,
        }];
        let selected = std::env::temp_dir().join("selected-source.flac");
        let artifacts = BTreeMap::from([(
            ("workflow.source".to_string(), "mix".to_string()),
            (selected.clone(), "clean_lead_vocal".to_string()),
        )]);

        let (path, role) = workflow_bound_audio(
            Some(&plan),
            "pitch.track",
            &artifacts,
            Path::new("unused-fallback.flac"),
            "unused",
        )
        .unwrap();

        assert_eq!(path, selected);
        assert_eq!(role, "clean_lead_vocal");
    }

    #[test]
    fn cached_lead_primary_satisfies_not_requested_isolation_binding() {
        let mut lead = node(
            "lead-isolate",
            "lead_isolate",
            "audio.lead_isolate",
            Some(("vocal-split", "vocal")),
        );
        lead.execution_state = WorkflowNodeExecutionStateV1::NotRequested;
        let dereverb = node(
            "dereverb",
            "vocal_dereverb_1",
            "audio.dereverb",
            Some(("lead_isolate", "lead")),
        );
        let plan = CompiledWorkflowExecutionPlanV1 {
            nodes: vec![lead, dereverb],
            ..plan()
        };
        let cached_lead = std::env::temp_dir().join("cached-lead-vocal.flac");
        let mut artifacts = BTreeMap::new();

        record_reused_workflow_audio(
            Some(&plan),
            crate::contract::AudioRole::LeadVocal,
            &mut artifacts,
            &cached_lead,
        );
        let (path, role) = workflow_transform_input(
            Some(&plan),
            Some("vocal_dereverb_1"),
            &artifacts,
            Path::new("unused-fallback.flac"),
            "unused",
        )
        .unwrap();

        assert_eq!(path, cached_lead);
        assert_eq!(role, "lead_vocal");
    }

    #[test]
    fn cached_clean_lead_does_not_claim_cleanup_nodes_on_other_audio_lanes() {
        let mut lead = node(
            "lead-isolate",
            "lead_isolate",
            "audio.lead_isolate",
            Some(("vocal-split", "vocal")),
        );
        lead.execution_state = WorkflowNodeExecutionStateV1::NotRequested;
        let mut vocal_cleanup = node(
            "vocal-cleanup",
            "vocal_cleanup",
            "audio.denoise",
            Some(("lead_isolate", "lead")),
        );
        vocal_cleanup.execution_state = WorkflowNodeExecutionStateV1::NotRequested;
        let mut bgm_cleanup = node(
            "bgm-cleanup",
            "bgm_cleanup",
            "audio.denoise",
            Some(("vocal-split", "instrumental")),
        );
        bgm_cleanup.execution_state = WorkflowNodeExecutionStateV1::NotRequested;
        let plan = CompiledWorkflowExecutionPlanV1 {
            nodes: vec![lead, vocal_cleanup, bgm_cleanup],
            ..plan()
        };
        let cached_clean_lead = std::env::temp_dir().join("cached-clean-lead-vocal.flac");
        let mut artifacts = BTreeMap::new();

        record_reused_workflow_audio(
            Some(&plan),
            crate::contract::AudioRole::CleanLeadVocal,
            &mut artifacts,
            &cached_clean_lead,
        );

        assert_eq!(
            artifacts.get(&("lead_isolate".to_string(), "lead".to_string())),
            Some(&(cached_clean_lead, "clean_lead_vocal".to_string()))
        );
        assert!(!artifacts.keys().any(|(node, _)| node == "vocal_cleanup"));
        assert!(!artifacts.keys().any(|(node, _)| node == "bgm_cleanup"));
    }
}
