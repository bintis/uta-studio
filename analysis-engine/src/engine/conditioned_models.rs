//! Conditioned complete-model tasks that become ready after alignment and RMVPE.

use super::*;

#[derive(Clone)]
pub(super) struct ConditionedModelContext {
    pub request: AnalyzeRequest,
    pub plan: EnginePlan,
    pub resolved: Vec<uta_runtime_manager::ResolvedModel>,
    pub workflow_audio: BTreeMap<(String, String), (PathBuf, String)>,
    pub analysis_input: PathBuf,
    pub analysis_role: String,
    pub output_root: PathBuf,
    pub source_start: u64,
    pub source_duration: u64,
    pub timed_transcript: Vec<serde_json::Value>,
    pub transcript_generation: String,
    pub shared_rmvpe_evidence_path: PathBuf,
    pub cancellation: CancellationToken,
}

pub(super) struct StarsOutput {
    pub advanced_note_evidence: Option<AdvancedNoteEvidence>,
    pub technique_evidence: Option<TechniqueEvidence>,
}

pub(super) fn run_stars(
    context: ConditionedModelContext,
    include_notes: bool,
    include_technique: bool,
) -> EngineResult<StarsOutput> {
    let capability = if include_notes {
        "notes.stars"
    } else {
        "technique.analyze"
    };
    let (input, _) = workflow_bound_audio(
        context.plan.workflow_execution.as_ref(),
        capability,
        &context.workflow_audio,
        &context.analysis_input,
        &context.analysis_role,
    )?;
    if include_notes && include_technique {
        let (technique_input, _) = workflow_bound_audio(
            context.plan.workflow_execution.as_ref(),
            "technique.analyze",
            &context.workflow_audio,
            &context.analysis_input,
            &context.analysis_role,
        )?;
        if technique_input != input {
            return Err(EngineError::new(
                EngineErrorCode::InvalidContract,
                "shared STARS note and technique execution requires one vocal input",
            ));
        }
    }
    let model = resolved_model(&context.resolved, "stars")?;
    let rmvpe_model = resolved_model(&context.resolved, "rmvpe")?;
    let directory = create_task_dir(&context.output_root, "worker/stars")?;
    let (component, mut config) =
        model_dispatch(model, &context.request, "note+technique_evidence")?;
    config["timed_transcript"] = serde_json::Value::Array(context.timed_transcript);
    config["source_start_micros"] = serde_json::json!(context.source_start);
    config["include_notes"] = serde_json::json!(include_notes);
    config["include_technique"] = serde_json::json!(include_technique);
    config["model_content_digest"] = serde_json::json!(model.model_content_digest);
    config["model_generation"] = serde_json::json!(model.generation);
    config["rmvpe_model_content_digest"] = serde_json::json!(rmvpe_model.model_content_digest);
    config["rmvpe_generation"] = serde_json::json!(rmvpe_model.generation);
    config["transcript_generation"] = serde_json::json!(context.transcript_generation);
    let outputs = run_native_task_with_inputs(
        model,
        component,
        &format!("{}-stars", context.request.request_id),
        "stars",
        &[input, context.shared_rmvpe_evidence_path],
        &directory,
        config,
        &context.cancellation,
    )?;
    let evidence =
        parse_advanced_note_evidence(typed_worker_output(&outputs, "stars_evidence")?, "stars")?;
    let technique_evidence = if include_technique {
        Some(
            evidence
                .technique_artifact(context.source_start, context.source_duration)?
                .ok_or_else(|| {
                    EngineError::new(
                        EngineErrorCode::OutputValidationFailed,
                        "STARS omitted requested technique evidence",
                    )
                })?,
        )
    } else {
        None
    };
    Ok(StarsOutput {
        advanced_note_evidence: include_notes.then_some(evidence),
        technique_evidence,
    })
}

pub(super) fn run_rosvot(context: ConditionedModelContext) -> EngineResult<AdvancedNoteEvidence> {
    let (input, _) = workflow_bound_audio(
        context.plan.workflow_execution.as_ref(),
        "notes.rosvot",
        &context.workflow_audio,
        &context.analysis_input,
        &context.analysis_role,
    )?;
    let model = resolved_model(&context.resolved, "rosvot")?;
    let rmvpe_model = resolved_model(&context.resolved, "rmvpe")?;
    let directory = create_task_dir(&context.output_root, "worker/rosvot")?;
    let (component, mut config) =
        model_dispatch(model, &context.request, "note_candidate_evidence")?;
    config["timed_transcript"] = serde_json::Value::Array(context.timed_transcript);
    config["source_start_micros"] = serde_json::json!(context.source_start);
    config["model_content_digest"] = serde_json::json!(model.model_content_digest);
    config["model_generation"] = serde_json::json!(model.generation);
    config["rmvpe_model_content_digest"] = serde_json::json!(rmvpe_model.model_content_digest);
    config["rmvpe_generation"] = serde_json::json!(rmvpe_model.generation);
    config["transcript_generation"] = serde_json::json!(context.transcript_generation);
    let outputs = run_native_task_with_inputs(
        model,
        component,
        &format!("{}-rosvot", context.request.request_id),
        "notes.rosvot",
        &[input, context.shared_rmvpe_evidence_path],
        &directory,
        config,
        &context.cancellation,
    )?;
    parse_advanced_note_evidence(typed_worker_output(&outputs, "rosvot_evidence")?, "rosvot")
}
