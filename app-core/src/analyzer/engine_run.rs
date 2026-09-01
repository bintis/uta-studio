use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use super::*;
use crate::analysis_artifact::{
    ArtifactRevision, ArtifactStore, materialize_artifact_revision_compatibility, revision_to_row,
};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::backend_cli::{
    ANALYSIS_RESULT_CONTRACT, ANALYSIS_RESULT_VERSION, AUDIO_QUALITY_REPORT_CONTRACT,
    AUDIO_QUALITY_REPORT_VERSION, AnalysisCliClient, AnalysisLifecycleFrameWireV1,
    AnalysisPlanWireV1, AnalysisResultManifestWireV1, AnalysisReusePolicyWireV1,
    AnalysisStatusWireV1, AnalyzeRequestWireV1, ArtifactRefWireV1, AudioQualityReportWireV1,
    AudioRoleWireV1, BackendCliError, FusionDecisionProvenanceWireV1, FusionModeWireV1,
    QualityGateRequirementWireV1, QualityGateStatusWireV1, QualityRegionWireV1,
    VocalTopologyModeWireV1,
};
use crate::library_db::EngineQueueIntent;

const SOURCE_DURATION_METADATA_TOLERANCE: u64 = 100_000;
const SUPPORTED_AUDIO_QUALITY_ALGORITHMS: [&str; 2] =
    ["audio-quality-gates-v1", "audio-quality-gates-v2"];

pub(crate) fn process_engine_queue_intent(
    file_hash: &str,
    cache: &CacheDir,
    intent: EngineQueueIntent,
) {
    let started_at_ms = unix_time_ms();
    ANALYSIS_STARTED
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), started_at_ms);
    let log_path = create_analysis_log(file_hash, started_at_ms);
    let engine_projection = EngineRunHistoryProjection {
        request_id: intent.request_id.clone(),
        request_json: intent.request_json.clone(),
        request_digest: intent.request_digest.clone(),
        plan_json: intent.plan_json.clone(),
        result_json: None,
        fingerprint: None,
        source_sha256: intent.source_sha256.clone(),
    };
    LIVE_ANALYSIS.lock().unwrap().insert(
        file_hash.to_string(),
        AnalysisProgressSnapshot {
            stage: "engine".to_string(),
            overall_progress: 0,
            stage_progress: 0,
            operation: "Starting Analysis Engine".to_string(),
            detail: "Executing the exact request confirmed in Plan Preview.".to_string(),
            implementation: "uta-analyze process protocol".to_string(),
            model: "Engine plan".to_string(),
            device: "Resolved by Engine".to_string(),
            requested_device: "Production policy".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            stage_routes: Vec::new(),
            node_id: None,
            engine_node_id: None,
            capability_id: None,
            node_event: Some("started".to_string()),
            artifact_reused_reason: None,
            analysis_log_path: log_path.clone(),
            engine: Some(engine_projection),
            engine_error: None,
        },
    );
    update_queue_status(file_hash, QueuedStatus::Analyzing(0));

    let result = execute_exact_intent(file_hash, cache, &intent, log_path.as_deref());
    match result {
        Ok(manifest) => {
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                snapshot.stage = "complete".to_string();
                snapshot.overall_progress = 100;
                snapshot.stage_progress = 100;
                snapshot.operation = "Analysis complete".to_string();
                snapshot.detail =
                    "Engine outputs were validated and published atomically.".to_string();
                snapshot.node_event = Some("completed".to_string());
                if let Some(engine) = snapshot.engine.as_mut() {
                    engine.result_json = serde_json::to_string(&manifest).ok();
                    engine.fingerprint = Some(manifest.fingerprint.clone());
                }
            }
            append_analysis_log_path(log_path.as_deref(), "Engine result validated and published");
            finish_analysis_history(file_hash, "completed", None);
            remove_from_queue(file_hash);
            remove_engine_progress_plan(file_hash);
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
        Err(error) => {
            append_analysis_log_path(log_path.as_deref(), &error);
            let cancelled = error.starts_with("cancelled:");
            if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
                snapshot.detail = snapshot.engine_error.as_ref().map_or_else(
                    || error.clone(),
                    |structured| {
                        format!(
                            "{}: {}{}{}",
                            structured.code,
                            structured.message,
                            structured
                                .capability
                                .as_deref()
                                .map(|value| format!(" · capability {value}"))
                                .unwrap_or_default(),
                            structured
                                .resource
                                .as_deref()
                                .map(|value| format!(" · resource {value}"))
                                .unwrap_or_default()
                        )
                    },
                );
                if cancelled {
                    mark_snapshot_cancelled(snapshot, &error);
                } else {
                    snapshot.node_event = Some("failed".to_string());
                }
            }
            if cancelled {
                finish_analysis_history(file_hash, "cancelled", Some(&error));
                remove_from_queue(file_hash);
            } else {
                finish_analysis_history(file_hash, "failed", Some(&error));
                update_queue_status(file_hash, QueuedStatus::Failed(error));
            }
            remove_engine_progress_plan(file_hash);
            LIVE_ANALYSIS.lock().unwrap().remove(file_hash);
        }
    }
}

fn execute_exact_intent(
    file_hash: &str,
    cache: &CacheDir,
    intent: &EngineQueueIntent,
    log_path: Option<&Path>,
) -> Result<AnalysisResultManifestWireV1, String> {
    if take_force_stop_request(file_hash) {
        return Err("cancelled: analysis was force-stopped".to_string());
    }
    let request: AnalyzeRequestWireV1 = serde_json::from_str(&intent.request_json)
        .map_err(|error| format!("persisted Engine request is malformed: {error}"))?;
    let plan: AnalysisPlanWireV1 = serde_json::from_str(&intent.plan_json)
        .map_err(|error| format!("persisted Engine plan is malformed: {error}"))?;
    if intent.file_hash != file_hash
        || request.request_id != intent.request_id
        || plan.request_id != intent.request_id
    {
        return Err("persisted Engine request identity is inconsistent".to_string());
    }
    crate::analysis_engine_adapter::validate_workflow_plan_identity(&request, &plan)?;
    if !valid_request_id(&intent.request_id)
        || request.contract != crate::backend_cli::ANALYZE_REQUEST_CONTRACT
        || request.version != crate::backend_cli::ANALYZE_REQUEST_VERSION
        || plan.schema != "uta.analysis-engine.plan"
        || plan.schema_version != 1
        || plan.requirements.schema != "uta.runtime.requirements"
        || plan.requirements.schema_version != 1
    {
        return Err("persisted Engine request or plan contract is unsupported".to_string());
    }
    register_engine_progress_plan(file_hash, &plan);
    let source = crate::analysis_engine_adapter::resolve_true_source(file_hash)?;
    let expected_source_duration = app_owned_source_duration(file_hash)?;
    validate_exact_execution_source_binding(&request, &plan, &source.path, &intent.source_path)?;

    let runs_root = cache.path.join("engine-runs");
    std::fs::create_dir_all(&runs_root)
        .map_err(|error| format!("could not create Engine runs root: {error}"))?;
    let output_root = runs_root.join(&intent.request_id);
    if output_root.exists() {
        // A retry reuses the persisted request_id, so a prior attempt that
        // crashed (rather than reaching a terminal outcome) can leave this
        // directory behind. Clear it instead of failing the retry outright.
        std::fs::remove_dir_all(&output_root)
            .map_err(|error| format!("could not clear stale Engine output root: {error}"))?;
    }
    std::fs::create_dir(&output_root)
        .map_err(|error| format!("could not create unique Engine output root: {error}"))?;
    let request_value =
        serde_json::from_str(&intent.request_json).map_err(|error| error.to_string())?;
    let outcome = (|| {
        let mut client = AnalysisCliClient::connect().map_err(|error| error.to_string())?;
        let cancellation = client.cancellation_handle();
        register_active_engine(file_hash, cancellation.clone());
        let analysis = if force_stop_was_requested(file_hash) {
            cancellation.force_stop().and_then(|()| {
                Err(BackendCliError::UnexpectedExit(
                    "force-stopped before analysis execution".to_string(),
                ))
            })
        } else {
            client.analyze_with_events(&request_value, &intent.request_id, &output_root, |event| {
                apply_engine_lifecycle_event(file_hash, log_path, event)
            })
        };
        remove_active_engine(file_hash);
        let stderr = client.stderr_log();
        if !stderr.is_empty() {
            append_analysis_log_path(log_path, &format!("uta-analyze stderr: {stderr}"));
        }
        if take_force_stop_request(file_hash) {
            return Err("cancelled: analysis was force-stopped".to_string());
        }
        let manifest = analysis.map_err(|error| {
            preserve_engine_error(file_hash, &error);
            error.to_string()
        })?;
        validate_and_publish_engine_result(
            file_hash,
            cache,
            &output_root,
            expected_source_duration,
            &request,
            &plan,
            &manifest,
        )?;
        Ok(manifest)
    })();
    if outcome.is_ok() {
        let _ = std::fs::remove_dir_all(&output_root);
    }
    outcome
}

fn audio_role_name(role: AudioRoleWireV1) -> &'static str {
    match role {
        AudioRoleWireV1::OriginalMix => "original_mix",
        AudioRoleWireV1::VocalStem => "vocal_stem",
        AudioRoleWireV1::GuideVocals => "guide_vocals",
        AudioRoleWireV1::LeadVocal => "lead_vocal",
        AudioRoleWireV1::CleanLeadVocal => "clean_lead_vocal",
        AudioRoleWireV1::Instrumental => "instrumental",
        AudioRoleWireV1::BackingVocal => "backing_vocal",
        AudioRoleWireV1::HarmonyVocal => "harmony_vocal",
    }
}

fn evaluated_audio_role_matches_plan(plan: &AnalysisPlanWireV1, evaluated_role: &str) -> bool {
    let baseline_role = if plan
        .source_route
        .preparation
        .iter()
        .any(|capability| capability.as_str() == "audio.lead_isolate")
    {
        AudioRoleWireV1::LeadVocal
    } else if plan
        .source_route
        .preparation
        .iter()
        .any(|capability| capability.as_str() == "audio.extract_vocals")
    {
        AudioRoleWireV1::GuideVocals
    } else {
        plan.source_route.input_role
    };
    evaluated_role == audio_role_name(baseline_role)
        || (matches!(
            baseline_role,
            AudioRoleWireV1::VocalStem
                | AudioRoleWireV1::GuideVocals
                | AudioRoleWireV1::LeadVocal
                | AudioRoleWireV1::CleanLeadVocal
        ) && workflow_analysis_route_uses_cleanup(plan)
            && evaluated_role == audio_role_name(AudioRoleWireV1::CleanLeadVocal))
}

fn workflow_analysis_route_uses_cleanup(plan: &AnalysisPlanWireV1) -> bool {
    let Some(workflow) = plan.workflow_execution.as_ref() else {
        return plan
            .execution_nodes
            .iter()
            .any(|node| matches!(node.capability.as_str(), "audio.denoise" | "audio.dereverb"));
    };
    let analyzer_roots = workflow.nodes.iter().flat_map(|node| {
        node.input_bindings.iter().filter_map(|binding| {
            (node.execution_state == crate::backend_cli::WorkflowNodeExecutionStateWireV1::Ready
                && binding.execution_active
                && binding.analyzer_attachment
                && binding.semantic_type == "audio")
                .then_some(binding.from_node.as_str())
        })
    });
    let terminal_audio_roots = workflow
        .terminal_outputs
        .iter()
        .filter(|output| output.semantic_type == "audio")
        .map(|output| output.node.as_str());
    analyzer_roots
        .chain(terminal_audio_roots)
        .any(|node| workflow_path_uses_cleanup(workflow, node, &mut BTreeSet::new()))
}

fn workflow_path_uses_cleanup(
    workflow: &crate::backend_cli::WorkflowExecutionPlanWireV1,
    analysis_node: &str,
    visited: &mut BTreeSet<String>,
) -> bool {
    if !visited.insert(analysis_node.to_string()) {
        return false;
    }
    let Some(node) = workflow
        .nodes
        .iter()
        .find(|node| node.analysis_node == analysis_node)
    else {
        return false;
    };
    if node.execution_state != crate::backend_cli::WorkflowNodeExecutionStateWireV1::Ready {
        return false;
    }
    if node
        .capabilities
        .iter()
        .any(|capability| matches!(capability.as_str(), "audio.denoise" | "audio.dereverb"))
    {
        return true;
    }
    node.input_bindings.iter().any(|binding| {
        binding.execution_active
            && !binding.analyzer_attachment
            && binding.semantic_type == "audio"
            && workflow_path_uses_cleanup(workflow, &binding.from_node, visited)
    })
}

fn app_owned_source_duration(file_hash: &str) -> Result<u64, String> {
    let song = crate::library_db::load_song_by_hash(file_hash)
        .map_err(|error| format!("could not load app-owned source duration: {error}"))?
        .ok_or_else(|| "app-owned source duration is unavailable".to_string())?;
    let duration = song.duration_secs * f64::from(crate::backend_cli::CANONICAL_TIMEBASE);
    if !duration.is_finite() || duration < 1.0 || duration >= u64::MAX as f64 {
        return Err("app-owned source duration is invalid".to_string());
    }
    Ok(duration.round() as u64)
}

fn preserve_engine_error(file_hash: &str, error: &BackendCliError) {
    let BackendCliError::Domain {
        code,
        message,
        retryable,
        request_id,
        capability,
        resource,
    } = error
    else {
        return;
    };
    if let Some(snapshot) = LIVE_ANALYSIS.lock().unwrap().get_mut(file_hash) {
        snapshot.engine_error = Some(EngineErrorHistoryProjection {
            code: code.clone(),
            message: message.clone(),
            retryable: *retryable,
            request_id: request_id.clone(),
            capability: capability.clone(),
            resource: resource.clone(),
        });
    }
}

fn apply_engine_lifecycle_event(
    file_hash: &str,
    log_path: Option<&Path>,
    event: AnalysisLifecycleFrameWireV1,
) {
    append_analysis_lifecycle_log(log_path, &event);
    // Cache a Step 1 audio-chain stem the instant its own worker succeeds,
    // before the lock below and independent of whether this run's later
    // stages ultimately fail -- see `persist_cacheable_stem`'s doc comment
    // for why that independence is the entire point. Never touch the live
    // `LIVE_ANALYSIS` lock for this: capturing/hashing a stem file is real
    // I/O and must not block every other snapshot read/write on its
    // duration.
    if event.frame_type == "artifact"
        && let (Some(artifact), Some(path)) = (event.artifact.as_deref(), event.path.as_deref())
        && let Some(cache) = CacheDir::try_new()
    {
        crate::chain_cache::persist_cacheable_stem(&cache.path, file_hash, artifact, Path::new(path));
    }
    let weighted_overall = update_engine_overall_progress(file_hash, &event);
    let presentation_node_id = event
        .presentation_node_id
        .clone()
        .unwrap_or_else(|| event.node_id.clone());
    let message = event
        .message
        .clone()
        .unwrap_or_else(|| event.capability_id.clone());
    let model = event
        .model_id
        .clone()
        .unwrap_or_else(|| "Engine native".to_string());
    // `fraction` measures the complete worker operation, including phases
    // which do not naturally expose integer work units. Work units remain
    // useful detail, but must not suppress a model's real reported percent.
    let reported_progress = event
        .progress
        .filter(|fraction| fraction.is_finite())
        .map(|fraction| (fraction.clamp(0.0, 1.0) * 100.0).floor() as usize);
    let terminal = matches!(event.frame_type.as_str(), "node_completed" | "node_failed");
    let started = matches!(
        event.frame_type.as_str(),
        "node_started" | "node_progress" | "artifact"
    );

    let mut live = LIVE_ANALYSIS.lock().unwrap();
    let Some(snapshot) = live.get_mut(file_hash) else {
        return;
    };
    if matches!(event.frame_type.as_str(), "warning" | "degraded") {
        snapshot.detail = message;
        snapshot.node_event = Some(event.frame_type);
        return;
    }
    let previous_overall = snapshot.overall_progress;
    let previous_measured_progress = snapshot
        .stage_routes
        .iter()
        .rev()
        .find(|route| {
            route.node_id.as_deref() == Some(presentation_node_id.as_str())
                && route.engine_node_id.as_deref() == Some(event.node_id.as_str())
                && route.node_event.as_deref() == Some("node_progress")
        })
        .map(|route| route.stage_progress);
    snapshot.stage = event.capability_id.clone();
    snapshot.stage_progress = if event.frame_type == "node_completed" {
        100
    } else if event.frame_type == "node_progress" {
        reported_progress.unwrap_or(0)
    } else if event.frame_type == "node_started" {
        0
    } else {
        previous_measured_progress.unwrap_or(0)
    };
    snapshot.operation = message.clone();
    snapshot.detail = format!("{} · {}", event.capability_id, model);
    snapshot.implementation = event.implementation.clone();
    snapshot.model = model.clone();
    snapshot.device = "Engine-resolved; see Plan/Result provenance".to_string();
    snapshot.requested_device = "Production policy".to_string();
    snapshot.node_id = Some(presentation_node_id.clone());
    snapshot.engine_node_id = Some(event.node_id.clone());
    snapshot.capability_id = Some(event.capability_id.clone());
    snapshot.node_event = Some(event.frame_type.clone());

    let route_exists = snapshot.stage_routes.iter().any(|route| {
        route.node_id.as_deref() == Some(presentation_node_id.as_str())
            && route.engine_node_id.as_deref() == Some(event.node_id.as_str())
    });
    if !route_exists {
        snapshot.stage_routes.push(AnalysisStageRoute {
            stage: event.capability_id.clone(),
            node_id: Some(presentation_node_id),
            engine_node_id: Some(event.node_id),
            capability_id: Some(event.capability_id),
            node_event: None,
            binding_kind: None,
            committed_outputs: Vec::new(),
            input_revision_ids: Vec::new(),
            operation: message,
            implementation: event.implementation,
            model,
            stage_progress: 0,
            requested_device: "Production policy".to_string(),
            actual_device: "Engine-resolved".to_string(),
            fallback_from: None,
            fallback_reason: None,
            backend_fallback_from: None,
            backend_fallback_reason: None,
            started_at_ms: None,
            finished_at_ms: None,
            event_at_ms: None,
            work_units_completed: None,
            work_units_total: None,
            worker_task_id: None,
        });
    }
    let current_node_id = snapshot.node_id.clone();
    let current_engine_node_id = snapshot.engine_node_id.clone();
    let route = snapshot
        .stage_routes
        .iter_mut()
        .find(|route| {
            route.node_id == current_node_id && route.engine_node_id == current_engine_node_id
        })
        .expect("the lifecycle route was found or inserted");
    if event.frame_type != "artifact" {
        route.node_event = Some(event.frame_type.clone());
    }
    route.operation = snapshot.operation.clone();
    route.implementation = snapshot.implementation.clone();
    route.model = snapshot.model.clone();
    route.stage_progress = snapshot.stage_progress;
    route.event_at_ms = Some(event.event_at_ms);
    if event.frame_type == "node_progress" {
        route.work_units_completed = event.work_units_completed;
        route.work_units_total = event.work_units_total;
        route.worker_task_id = event.worker_task_id;
    }
    if started && route.started_at_ms.is_none() {
        route.started_at_ms = Some(event.event_at_ms);
    }
    if terminal {
        route.finished_at_ms = Some(event.event_at_ms);
    }
    if let Some(weighted_overall) = weighted_overall {
        snapshot.overall_progress = snapshot.overall_progress.max(weighted_overall);
    }
    let overall_changed = snapshot.overall_progress != previous_overall;
    let overall_progress = snapshot.overall_progress;
    drop(live);
    if overall_changed {
        update_queue_status(file_hash, QueuedStatus::Analyzing(overall_progress));
    }
}

fn validate_and_publish_engine_result(
    file_hash: &str,
    cache: &CacheDir,
    output_root: &Path,
    expected_source_duration: u64,
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    manifest: &AnalysisResultManifestWireV1,
) -> Result<(), String> {
    if manifest.contract != ANALYSIS_RESULT_CONTRACT || manifest.version != ANALYSIS_RESULT_VERSION
    {
        return Err("Engine result contract identity is invalid".to_string());
    }
    if manifest.request_id != request.request_id || plan.request_id != request.request_id {
        return Err("Engine result request_id does not match the queued snapshot".to_string());
    }
    if !matches!(
        manifest.status,
        AnalysisStatusWireV1::Ok | AnalysisStatusWireV1::OkDegraded
    ) {
        return Err(format!(
            "Engine returned non-success status {:?}",
            manifest.status
        ));
    }
    if matches!(manifest.status, AnalysisStatusWireV1::Ok) && !manifest.degraded_reasons.is_empty()
    {
        return Err("Engine ok result unexpectedly contains degraded reasons".to_string());
    }
    if matches!(manifest.status, AnalysisStatusWireV1::OkDegraded)
        && manifest.degraded_reasons.is_empty()
    {
        return Err("Engine degraded result omitted degraded reasons".to_string());
    }
    for version in [
        &manifest.provenance.calibration_version,
        &manifest.provenance.fusion_version,
        &manifest.provenance.quantization_version,
        &manifest.provenance.audio_quality_version,
        &manifest.provenance.postprocess_version,
    ] {
        if version.trim().is_empty() {
            return Err("Engine result algorithm provenance is incomplete".to_string());
        }
    }
    validate_fusion_decision_result(plan, manifest)?;
    validate_quantization_result(request, manifest)?;
    validate_audio_quality_result(request, plan, manifest, expected_source_duration)?;
    for resource in &manifest.provenance.resources {
        for field in [
            "resource",
            "generation",
            "content_digest",
            "runtime",
            "runtime_generation",
            "backend",
            "device",
        ] {
            if resource
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("Engine resource provenance omitted {field}"));
            }
        }
    }

    let declared = plan
        .artifact_declarations
        .iter()
        .map(|item| {
            (
                item.semantic_type.as_str(),
                item.media_type.as_str(),
                item.required,
            )
        })
        .collect::<Vec<_>>();
    let mut stem_roles = std::collections::BTreeSet::new();
    for stem in &manifest.artifacts.stems {
        if !stem_roles.insert(stem.role) {
            return Err("Engine result contains a duplicate stem role".to_string());
        }
        if !matches!(
            stem.role,
            AudioRoleWireV1::Instrumental
                | AudioRoleWireV1::GuideVocals
                | AudioRoleWireV1::LeadVocal
                | AudioRoleWireV1::CleanLeadVocal
                | AudioRoleWireV1::BackingVocal
                | AudioRoleWireV1::HarmonyVocal
        ) {
            return Err("Engine result contains an unsupported output stem role".to_string());
        }
    }
    let artifacts = result_artifacts(manifest);
    let mut actual_semantics = std::collections::BTreeSet::new();
    if artifacts
        .iter()
        .any(|(semantic, _, _, _)| !actual_semantics.insert(semantic.as_str()))
    {
        return Err("Engine result contains a duplicate artifact semantic".to_string());
    }
    for (semantic, _, required) in &declared {
        if *required && !artifacts.iter().any(|(actual, _, _, _)| actual == semantic) {
            return Err(format!(
                "Engine result omitted required artifact {semantic}"
            ));
        }
    }
    if artifacts
        .iter()
        .any(|(semantic, _, _, _)| !declared.iter().any(|(expected, _, _)| expected == semantic))
    {
        return Err("Engine result contains an undeclared artifact".to_string());
    }

    let output_root = output_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let chain_fingerprints: crate::chain_cache::ChainFingerprints = request
        .extensions
        .get(crate::chain_cache::CHAIN_FINGERPRINTS_EXTENSION_KEY)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let store = ArtifactStore::new(&cache.path)?;
    let mut revisions = Vec::new();
    let mut activations = Vec::new();
    let mut complete_chart_revisions = Vec::new();
    let mut published_revisions = Vec::new();
    let created_at_ms = unix_time_ms();
    for (semantic, artifact, kind, producer) in artifacts {
        let expected_media = declared
            .iter()
            .find(|(name, _, _)| *name == semantic)
            .map(|(_, media, _)| *media)
            .ok_or_else(|| format!("missing declaration for {semantic}"))?;
        if artifact.media_type != expected_media {
            return Err(format!("Engine artifact {semantic} media type mismatch"));
        }
        let path = validate_artifact(&output_root, artifact)?;
        validate_semantic_artifact(&semantic, &path)?;
        let (immutable_path, content_hash, byte_size) = store.capture(file_hash, kind, &path)?;
        // The Step 1 chain cache (`chain_cache::plan_chain_cache`) matches a
        // future run's freshly computed fingerprint against exactly this
        // value, so these specific kinds record their own per-unit
        // fingerprint instead of the whole-manifest one every other kind
        // uses -- a downstream-only change must not invalidate an unrelated
        // upstream stage's cache eligibility.
        let chain_config_hash = match kind {
            ArtifactKind::VocalStem => chain_fingerprints.separation.clone(),
            ArtifactKind::InstrumentalStem => chain_fingerprints.instrumental.clone(),
            ArtifactKind::AnalysisVocalStem => chain_fingerprints.isolate.clone(),
            ArtifactKind::DereverbedVocalStem => chain_fingerprints.cleanup.clone(),
            ArtifactKind::DenoisedInstrumentalStem | ArtifactKind::DereverbedInstrumentalStem => {
                chain_fingerprints.cleanup.clone()
            }
            _ => None,
        };
        let config_hash =
            chain_config_hash.unwrap_or_else(|| format!("engine:{}", manifest.fingerprint));
        let revision = ArtifactRevision {
            id: format!("{file_hash}:{semantic}:{content_hash}"),
            file_hash: file_hash.to_string(),
            kind,
            path: immutable_path,
            content_hash,
            producer_node: AnalysisNodeId::new(producer),
            input_revisions: Vec::new(),
            config_hash,
            algorithm_version: format!("analysis-engine-result/{}", manifest.version),
            created_at_ms,
            byte_size,
            active: false,
            legacy: false,
            invalidated: false,
        };
        activations.push((
            file_hash.to_string(),
            serde_json::to_string(&kind).unwrap_or_default(),
            revision.id.clone(),
        ));
        if kind == ArtifactKind::CandidateChart {
            complete_chart_revisions.push(revision.clone());
        }
        revisions.push(revision_to_row(&revision));
        published_revisions.push(revision);
    }
    let analyzed_file_hashes = (!complete_chart_revisions.is_empty())
        .then(|| file_hash.to_string())
        .into_iter()
        .collect::<Vec<_>>();
    crate::library_db::analysis_artifacts_publish_batch(
        &revisions,
        &activations,
        &analyzed_file_hashes,
    )
    .map_err(|error| error.to_string())?;
    // Materialize every published revision, not just CandidateChart:
    // `refresh_authoring_state` (and
    // other legacy readiness checks) key off flat `{hash}_instrumental.*` /
    // `{hash}_vocals.*` files, not the content-addressed artifact store, so
    // skipping stem kinds here leaves completed songs "Analysis incomplete".
    // `compatibility_paths` already no-ops for kinds with no legacy
    // location, so this is safe to call unconditionally.
    for revision in &published_revisions {
        if let Err(error) = materialize_artifact_revision_compatibility(&cache.path, revision) {
            warn!(
                "[analyzer] Published {:?} {} but could not refresh compatibility output: {error}",
                revision.kind, revision.id
            );
        }
    }
    Ok(())
}

fn valid_decision_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_fusion_decision_result(
    plan: &AnalysisPlanWireV1,
    manifest: &AnalysisResultManifestWireV1,
) -> Result<(), String> {
    let candidate_graph_planned = plan
        .execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == "fusion.candidate_graph");
    let Some(decision) = manifest.provenance.fusion_decision.as_ref() else {
        return if candidate_graph_planned {
            Err("Engine result omitted final fusion decision provenance".to_string())
        } else {
            Ok(())
        };
    };
    if !candidate_graph_planned {
        return Err("Engine result reported a fusion decision for an unplanned stage".to_string());
    }
    let planned_mode = plan
        .workflow_execution
        .as_ref()
        .map_or(FusionModeWireV1::Algorithm, |workflow| workflow.fusion_mode);
    let (candidate_set_digest, selected_candidate_ids) = match decision {
        FusionDecisionProvenanceWireV1::Algorithm {
            selector,
            selector_version,
            candidate_set_digest,
            selected_candidate_ids,
            reuse_policy,
        } => {
            if planned_mode != FusionModeWireV1::Algorithm
                || selector != "hsmm_viterbi"
                || selector_version != "hsmm-v15"
                || *reuse_policy != AnalysisReusePolicyWireV1::Deterministic
            {
                return Err(
                    "Engine algorithmic fusion provenance does not match the exact plan"
                        .to_string(),
                );
            }
            (candidate_set_digest, selected_candidate_ids)
        }
        FusionDecisionProvenanceWireV1::AiJudgment {
            adapter_resource,
            adapter_protocol,
            adapter_protocol_version,
            adapter_identity,
            adapter_version,
            candidate_set_digest,
            selected_candidate_ids,
            response_digest,
            reuse_policy,
        } => {
            if planned_mode != FusionModeWireV1::AiJudgment
                || adapter_resource != "tool:fusion_agent_adapter"
                || adapter_protocol != "uta.fusion_agent_request/uta.fusion_agent_response"
                || *adapter_protocol_version != 4
                || adapter_identity.trim().is_empty()
                || adapter_version.trim().is_empty()
                || !valid_decision_digest(response_digest)
                || *reuse_policy != AnalysisReusePolicyWireV1::PreservedRevisionOnly
            {
                return Err(
                    "Engine AI judgment provenance does not match the exact plan and adapter contract"
                        .to_string(),
                );
            }
            (candidate_set_digest, selected_candidate_ids)
        }
    };
    let mut unique_ids = BTreeSet::new();
    if !valid_decision_digest(candidate_set_digest)
        || selected_candidate_ids.is_empty()
        || selected_candidate_ids
            .iter()
            .any(|id| id.trim().is_empty() || !unique_ids.insert(id))
    {
        return Err("Engine fusion decision candidate identity is invalid".to_string());
    }
    Ok(())
}

fn validate_audio_quality_result(
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    manifest: &AnalysisResultManifestWireV1,
    expected_source_duration: u64,
) -> Result<(), String> {
    const GATE_ORDER: &[&str] = &[
        "timeline_valid",
        "finite_samples",
        "clipping",
        "silence_ratio",
        "energy_ratio",
        "lead_purity",
        "vocal_leakage",
        "musical_damage",
        "cleanup_consistency",
        "vocal_topology",
    ];
    let report = manifest.diagnostics.audio_quality.as_ref().ok_or_else(|| {
        "Engine omitted audio quality diagnostics for an executable Plan".to_string()
    })?;
    let primary = validated_primary_source_binding(request, plan)?;
    let source_start = primary.timeline.source_start;
    let source_end = source_start
        .checked_add(expected_source_duration)
        .ok_or_else(|| "App-owned source timeline overflows".to_string())?;
    let report_end = source_start
        .checked_add(report.duration)
        .ok_or_else(|| "Engine audio quality timeline overflows".to_string())?;
    if report.contract != AUDIO_QUALITY_REPORT_CONTRACT
        || report.version != AUDIO_QUALITY_REPORT_VERSION
        || !SUPPORTED_AUDIO_QUALITY_ALGORITHMS.contains(&report.algorithm.as_str())
        || report.algorithm != manifest.provenance.audio_quality_version
        || report.profile != request.analysis.profile
        || !evaluated_audio_role_matches_plan(plan, &report.evaluated_audio_role)
        || report.duration == 0
        || report.duration.abs_diff(expected_source_duration) > SOURCE_DURATION_METADATA_TOLERANCE
        || report.planned_gates != plan.quality_gates
        || report.outcomes.len() != plan.quality_gates.len()
    {
        return Err("Engine audio quality report identity or Plan binding is invalid".to_string());
    }
    validate_vocal_topology_result(request, plan, report, expected_source_duration)?;
    let mut previous_order = None;
    let mut degrading_uncertainty = false;
    for (planned, outcome) in plan.quality_gates.iter().zip(&report.outcomes) {
        let order = GATE_ORDER
            .iter()
            .position(|known| *known == planned)
            .ok_or_else(|| format!("Engine Plan contains unknown audio quality gate {planned}"))?;
        let expected_requirement = match planned.as_str() {
            "timeline_valid" | "finite_samples" | "silence_ratio" | "energy_ratio" => {
                QualityGateRequirementWireV1::Required
            }
            _ => QualityGateRequirementWireV1::Degrading,
        };
        if previous_order.is_some_and(|previous| order <= previous)
            || outcome.gate != *planned
            || outcome.requirement != expected_requirement
            || outcome.summary.trim().is_empty()
            || (expected_requirement == QualityGateRequirementWireV1::Required
                && outcome.status != QualityGateStatusWireV1::Passed)
        {
            return Err("Engine audio quality gate outcome is inconsistent".to_string());
        }
        previous_order = Some(order);
        degrading_uncertainty |= expected_requirement == QualityGateRequirementWireV1::Degrading
            && outcome.status != QualityGateStatusWireV1::Passed;
        for metric in &outcome.metrics {
            if metric.name.trim().is_empty()
                || metric.unit.trim().is_empty()
                || !metric.value.is_finite()
                || metric.lower_bound.is_some_and(|value| !value.is_finite())
                || metric.upper_bound.is_some_and(|value| !value.is_finite())
                || matches!((metric.lower_bound, metric.upper_bound), (Some(low), Some(high)) if low > high)
            {
                return Err("Engine audio quality metric is invalid".to_string());
            }
        }
        // Every other gate's regions describe activity within the app-owned
        // requested window (`source_start..source_end`), so they're held to
        // it exactly. `vocal_topology` is the one gate whose region can
        // legitimately span the Engine's own measured full duration -- its
        // `vocal_topology_unknown` fallback region is built from
        // `report.duration`, not the app's request -- so it is bound only by
        // `report_end`, already checked below with zero tolerance.
        let is_vocal_topology = planned == "vocal_topology";
        if let Some(region) = outcome.regions.iter().find(|region| {
            (!is_vocal_topology && (region.start < source_start || region.end > source_end))
                || region.end > report_end
                || region.start >= region.end
                || region.reason.trim().is_empty()
        }) {
            return Err(format!(
                "Engine audio quality region is invalid: gate={planned} region=[{}, {}) reason={:?} (source_start={source_start} source_end={source_end} report_end={report_end})",
                region.start, region.end, region.reason
            ));
        }
        if let Some(pair) = outcome
            .regions
            .windows(2)
            .find(|pair| pair[0].end > pair[1].start)
        {
            return Err(format!(
                "Engine audio quality region is invalid: gate={planned} overlapping regions [{}, {}) and [{}, {})",
                pair[0].start, pair[0].end, pair[1].start, pair[1].end
            ));
        }
    }
    if degrading_uncertainty && manifest.status != AnalysisStatusWireV1::OkDegraded {
        return Err("Engine audio quality uncertainty was not surfaced as degraded".to_string());
    }
    Ok(())
}

fn validate_vocal_topology_result(
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    report: &AudioQualityReportWireV1,
    expected_source_duration: u64,
) -> Result<(), String> {
    let required = plan
        .quality_gates
        .iter()
        .any(|gate| gate == "vocal_topology");
    let Some(topology) = report.vocal_topology.as_ref() else {
        return if required {
            Err("Engine omitted planned typed vocal topology evidence".to_string())
        } else {
            Ok(())
        };
    };
    let source_start = validated_primary_source_binding(request, plan)?
        .timeline
        .source_start;
    let source_end = source_start
        .checked_add(expected_source_duration)
        .ok_or_else(|| "App-owned vocal topology timeline overflows".to_string())?;
    let topology_end = source_start
        .checked_add(topology.duration)
        .ok_or_else(|| "Engine vocal topology timeline overflows".to_string())?;
    let valid_regions = |regions: &[QualityRegionWireV1]| {
        regions.iter().all(|region| {
            region.start >= source_start
                && region.end <= source_end
                && region.end <= topology_end
                && region.start < region.end
                && !region.reason.trim().is_empty()
        }) && regions.windows(2).all(|pair| pair[0].end <= pair[1].start)
    };
    let mode_shape_valid = match topology.mode {
        VocalTopologyModeWireV1::SingleLead | VocalTopologyModeWireV1::Unknown => {
            topology.overlap_regions.is_empty() && topology.support_regions.is_empty()
        }
        VocalTopologyModeWireV1::AlternatingMultiLead => topology.overlap_regions.is_empty(),
        VocalTopologyModeWireV1::OverlappingMultiLead => !topology.overlap_regions.is_empty(),
        VocalTopologyModeWireV1::LeadWithSupport => !topology.support_regions.is_empty(),
    };
    if topology.contract != "uta.analysis-engine.vocal-topology-estimate"
        || topology.version != 1
        || topology.timebase != 1_000_000
        || topology.source_start != source_start
        || topology.duration != report.duration
        || topology.evidence_sources.is_empty()
        || topology
            .evidence_sources
            .iter()
            .any(|source| source.trim().is_empty())
        || topology
            .confidence
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        || !valid_regions(&topology.overlap_regions)
        || !valid_regions(&topology.support_regions)
        || !mode_shape_valid
    {
        return Err("Engine typed vocal topology evidence is invalid".to_string());
    }
    Ok(())
}

fn validate_quantization_result(
    request: &AnalyzeRequestWireV1,
    manifest: &AnalysisResultManifestWireV1,
) -> Result<(), String> {
    match (
        request.analysis.enable_quantization,
        manifest.diagnostics.quantization.as_ref(),
    ) {
        (false, None) => Ok(()),
        (false, Some(_)) => {
            Err("Engine returned quantization diagnostics for a disabled stage".to_string())
        }
        (true, None) => {
            Err("Engine omitted quantization diagnostics for an enabled stage".to_string())
        }
        (true, Some(report)) => {
            let context = request
                .musical_context
                .as_ref()
                .ok_or_else(|| "Quantized Engine result has no musical context".to_string())?;
            if manifest.artifacts.candidate_vocal_chart.is_none()
                || report.algorithm != manifest.provenance.quantization_version
                || !report.bpm.is_finite()
                || context.bpm != Some(report.bpm)
                || context.quantization_grid != Some(report.grid)
                || report.grid_step == 0
                || report.minimum_note_duration != report.grid_step
                || report.source_end <= report.source_start
                || report.adjusted_notes > report.note_count
                || report.maximum_shift > report.grid_step
            {
                return Err("Engine quantization result contract is inconsistent".to_string());
            }
            Ok(())
        }
    }
}

fn result_artifacts(
    manifest: &AnalysisResultManifestWireV1,
) -> Vec<(String, &ArtifactRefWireV1, ArtifactKind, &'static str)> {
    let mut result = Vec::new();
    for (semantic, artifact, kind, producer) in [
        (
            "candidate_vocal_chart",
            manifest.artifacts.candidate_vocal_chart.as_ref(),
            ArtifactKind::CandidateChart,
            "vocal-chart",
        ),
        (
            "pitch_evidence",
            manifest.artifacts.pitch_evidence.as_ref(),
            ArtifactKind::PitchEvidence,
            "pitch",
        ),
        (
            "technique_evidence",
            manifest.artifacts.technique_evidence.as_ref(),
            ArtifactKind::TechniqueEvidence,
            "stars-technique",
        ),
        (
            "singing_analysis",
            manifest.artifacts.singing_analysis.as_ref(),
            ArtifactKind::EvidenceBundle,
            "singing-fusion",
        ),
        (
            "transcript",
            manifest.artifacts.transcript.as_ref(),
            ArtifactKind::TranscriptEvidence,
            "transcript",
        ),
        (
            "alignment",
            manifest.artifacts.alignment.as_ref(),
            ArtifactKind::AlignmentEvidence,
            "alignment",
        ),
    ] {
        if let Some(artifact) = artifact {
            result.push((semantic.to_string(), artifact, kind, producer));
        }
    }
    for stem in &manifest.artifacts.stems {
        let (role, kind, producer) = match stem.role {
            AudioRoleWireV1::Instrumental => {
                let (kind, producer) = instrumental_result_kind(&stem.artifact.path);
                ("instrumental", kind, producer)
            }
            AudioRoleWireV1::GuideVocals => {
                ("guide_vocals", ArtifactKind::VocalStem, "extract-vocals")
            }
            AudioRoleWireV1::LeadVocal => (
                "lead_vocal",
                ArtifactKind::AnalysisVocalStem,
                "lead-isolate",
            ),
            AudioRoleWireV1::BackingVocal => (
                "backing_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            AudioRoleWireV1::HarmonyVocal => (
                "harmony_vocal",
                ArtifactKind::RawVocalStem,
                "lead-partition",
            ),
            AudioRoleWireV1::CleanLeadVocal => (
                "clean_lead_vocal",
                ArtifactKind::DereverbedVocalStem,
                "cleanup",
            ),
            _ => continue,
        };
        result.push((format!("stem:{role}"), &stem.artifact, kind, producer));
    }
    result
}

fn instrumental_result_kind(path: &Path) -> (ArtifactKind, &'static str) {
    if !path.starts_with(Path::new("workflow-audio")) {
        return (ArtifactKind::InstrumentalStem, "extract-instrumental");
    }
    let kind = if path.to_string_lossy().contains("-dereverb.flac") {
        ArtifactKind::DereverbedInstrumentalStem
    } else {
        ArtifactKind::DenoisedInstrumentalStem
    };
    (kind, "cleanup")
}

fn validate_semantic_artifact(semantic: &str, path: &Path) -> Result<(), String> {
    if semantic != "candidate_vocal_chart" {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path).map_err(|error| format!("could not read Candidate chart: {error}"))?,
    )
    .map_err(|error| format!("Engine Candidate chart is not valid JSON: {error}"))?;
    let chart = if value.get("contract").and_then(serde_json::Value::as_str)
        == Some("uta.analysis-engine.candidate-vocal-chart")
    {
        crate::vocal_chart::migrate_engine_candidate_chart(&value)
            .map_err(|error| format!("Engine Candidate projection is invalid: {error}"))?
    } else {
        serde_json::from_value::<utz::VocalChartV1>(value)
            .map_err(|error| format!("Engine Candidate VocalChart is invalid: {error}"))?
    };
    chart
        .validate()
        .map_err(|error| format!("Engine Candidate VocalChart failed validation: {error}"))
}

fn validate_artifact(output_root: &Path, artifact: &ArtifactRefWireV1) -> Result<PathBuf, String> {
    if artifact.path.is_absolute()
        || artifact.path.as_os_str().is_empty()
        || artifact.path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || artifact.bytes == 0
    {
        return Err("Engine artifact reference is invalid or unconfined".to_string());
    }
    let path = output_root.join(&artifact.path);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("Engine artifact is missing: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Engine artifact is not a regular non-symlink file".to_string());
    }
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(output_root) {
        return Err("Engine artifact escaped its authorized output root".to_string());
    }
    if metadata.len() != artifact.bytes {
        return Err("Engine artifact byte count mismatch".to_string());
    }
    Ok(canonical)
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-engine-result-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn artifact(path: &str, bytes: &[u8]) -> ArtifactRefWireV1 {
        ArtifactRefWireV1 {
            path: PathBuf::from(path),
            media_type: "application/json".to_string(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        }
    }

    #[test]
    fn result_artifact_validation_checks_confinement_and_byte_count_without_hash_verification() {
        let root = temp_root("validation");
        std::fs::write(root.join("valid.json"), b"valid").unwrap();
        validate_artifact(&root, &artifact("valid.json", b"valid")).unwrap();

        let mut opaque_hash_metadata = artifact("valid.json", b"other");
        opaque_hash_metadata.bytes = 5;
        validate_artifact(&root, &opaque_hash_metadata).unwrap();
        let mut wrong_bytes = artifact("valid.json", b"valid");
        wrong_bytes.bytes = 99;
        assert!(
            validate_artifact(&root, &wrong_bytes)
                .unwrap_err()
                .contains("byte count")
        );
        assert!(
            validate_artifact(&root, &artifact("../escape.json", b"valid"))
                .unwrap_err()
                .contains("unconfined")
        );
        assert!(
            validate_artifact(&root, &artifact("/tmp/escape.json", b"valid"))
                .unwrap_err()
                .contains("unconfined")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn candidate_semantics_are_validated_before_artifact_capture() {
        let root = temp_root("candidate-semantics");
        let valid = serde_json::json!({
            "format":"uta.vocal-chart","format_version":"0.3.0","timebase":1000000,
            "language":"ja","tracks":[{
                "id":"lead","role":"lead","phrases":[{
                    "id":"phrase-1","notes":[{
                        "id":"note-1","start":0,"duration":500000,
                        "pitch":{"midi":69,"cents":0},"vocal_mode":"pitched",
                        "bonus":"normal","scoring":{"mode":"pitch","weight":1.0},
                        "lyrics":[{"id":"lyric-1","text":"歌","join_before":"none"}]
                    }]
                }]
            }]
        });
        let valid_path = root.join("valid.json");
        std::fs::write(&valid_path, serde_json::to_vec(&valid).unwrap()).unwrap();
        validate_semantic_artifact("candidate_vocal_chart", &valid_path).unwrap();

        let invalid_path = root.join("invalid.json");
        std::fs::write(
            &invalid_path,
            br#"{"contract":"uta.analysis-engine.candidate-vocal-chart","version":1}"#,
        )
        .unwrap();
        assert!(
            validate_semantic_artifact("candidate_vocal_chart", &invalid_path)
                .unwrap_err()
                .contains("projection is invalid")
        );
        let malformed_path = root.join("malformed.json");
        std::fs::write(&malformed_path, b"{").unwrap();
        assert!(
            validate_semantic_artifact("candidate_vocal_chart", &malformed_path)
                .unwrap_err()
                .contains("not valid JSON")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quantization_wire_result_matches_exact_request_intent() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"quantized",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "musical_context":{"bpm":120.0,"time_signature":{"beats":4,"unit":4},"quantization_grid":"sixteenth","authority":"hint"},
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":true},
            "requested_artifacts":{"vocal_chart":true},"execution_policy":{},"extensions":{}
        })).unwrap();
        let mut manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quantized","status":"ok",
            "artifacts":{"candidate_vocal_chart":{"path":"candidate/vocal-chart.json","media_type":"application/vnd.uta.vocal-chart+json;version=0.3","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","bytes":1}},
            "diagnostics":{"quantization":{"algorithm":"rhythm-grid-dp-v1","bpm":120.0,"grid":"sixteenth","grid_step":125000,"minimum_note_duration":125000,"source_start":0,"source_end":1000000,"hard_boundary_count":0,"note_count":2,"adjusted_notes":2,"maximum_shift":12000}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"rhythm-grid-dp-v1","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":[]
        })).unwrap();
        validate_quantization_result(&request, &manifest).unwrap();
        manifest
            .diagnostics
            .quantization
            .as_mut()
            .unwrap()
            .grid_step = 0;
        assert!(validate_quantization_result(&request, &manifest).is_err());
        manifest.diagnostics.quantization = None;
        assert!(validate_quantization_result(&request, &manifest).is_err());
    }

    #[test]
    fn audio_quality_wire_result_is_bound_to_plan_and_surfaces_uncertainty() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"quality",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        })).unwrap();
        let gates = vec![
            "timeline_valid",
            "finite_samples",
            "clipping",
            "silence_ratio",
            "energy_ratio",
        ];
        let outcomes = gates
            .iter()
            .map(|gate| serde_json::json!({
                "gate":gate,
                "requirement":if matches!(*gate, "timeline_valid" | "finite_samples" | "silence_ratio" | "energy_ratio") { "required" } else { "degrading" },
                "status":"passed","summary":"measured","metrics":[],"regions":[]
            }))
            .collect::<Vec<_>>();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"quality",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates.clone(),
            "fallback_policy":[],"artifact_declarations":[]
        })).unwrap();
        let mut manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"quality","status":"ok",
            "artifacts":{},
            "diagnostics":{"audio_quality":{"contract":"uta.analysis-engine.audio-quality-report","version":1,"algorithm":"audio-quality-gates-v1","profile":"fast","evaluated_audio_role":"lead_vocal","duration":1000000,"planned_gates":gates,"outcomes":outcomes}},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"q","audio_quality_version":"audio-quality-gates-v1","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":[]
        })).unwrap();
        validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();

        let mut unsupported_algorithm = manifest.clone();
        unsupported_algorithm
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .algorithm = "audio-quality-gates-v3".to_string();
        unsupported_algorithm.provenance.audio_quality_version =
            "audio-quality-gates-v3".to_string();
        assert!(
            validate_audio_quality_result(&request, &plan, &unsupported_algorithm, 1_000_000,)
                .is_err()
        );

        let mut invalid_timebase = request.clone();
        invalid_timebase.audio_sources[0].timeline.timebase = 1_000;
        assert!(
            validate_audio_quality_result(&invalid_timebase, &plan, &manifest, 1_000_000).is_err()
        );
        let mut duplicate_primary = request.clone();
        let mut duplicate = duplicate_primary.audio_sources[0].clone();
        duplicate.id = "duplicate".to_string();
        duplicate_primary.audio_sources.push(duplicate);
        assert!(
            validate_audio_quality_result(&duplicate_primary, &plan, &manifest, 1_000_000).is_err()
        );
        let mut mismatched_plan = plan.clone();
        mismatched_plan.source_route.primary_source_id = "other".to_string();
        assert!(
            validate_audio_quality_result(&request, &mismatched_plan, &manifest, 1_000_000)
                .is_err()
        );
        let mut mismatched_role = manifest.clone();
        mismatched_role
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .evaluated_audio_role = "original_mix".to_string();
        assert!(
            validate_audio_quality_result(&request, &plan, &mismatched_role, 1_000_000).is_err()
        );

        manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .planned_gates
            .pop();
        assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
        manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .planned_gates = plan.quality_gates.clone();
        let clipping = &mut manifest
            .diagnostics
            .audio_quality
            .as_mut()
            .unwrap()
            .outcomes[2];
        clipping.status = QualityGateStatusWireV1::Unknown;
        assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
        manifest.status = AnalysisStatusWireV1::OkDegraded;
        validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();
        let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
        report.duration = 1_100_000;
        report.outcomes[2].regions.push(QualityRegionWireV1 {
            start: 1_000_000,
            end: 1_050_000,
            reason: "outside_app_owned_source".to_string(),
        });
        assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
        let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
        report.outcomes[2].regions.clear();
        report.duration = 1_100_001;
        assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
        let report = manifest.diagnostics.audio_quality.as_mut().unwrap();
        report.duration = 1_000_000;
        report.outcomes[2].regions = vec![
            QualityRegionWireV1 {
                start: 100,
                end: 300,
                reason: "first".to_string(),
            },
            QualityRegionWireV1 {
                start: 200,
                end: 400,
                reason: "overlap".to_string(),
            },
        ];
        assert!(validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).is_err());
    }

    #[test]
    fn vocal_topology_region_may_span_the_engines_measured_duration_past_the_apps_expected_window()
    {
        // Regression (real repro): the Engine's own decoded duration can
        // exceed the app's independently-computed `expected_source_duration`
        // by a sub-millisecond decode-vs-metadata rounding amount --
        // report.duration=305813333 vs expected_source_duration=305813000 for
        // a real song. The `vocal_topology_unknown` fallback region
        // legitimately spans that full measured duration, so it must not be
        // rejected just for extending past the app-owned `source_end`; only
        // extending past the Engine's own `report_end` is actually invalid.
        // Every other gate keeps the strict `source_end` bound (see the
        // `outside_app_owned_source` case above).
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"topology",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"fast","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        }))
        .unwrap();
        let gates = vec!["timeline_valid", "vocal_topology"];
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"topology",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
            "fallback_policy":[],"artifact_declarations":[]
        }))
        .unwrap();
        let manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,"request_id":"topology","status":"ok_degraded",
            "artifacts":{},
            "diagnostics":{"audio_quality":{
                "contract":"uta.analysis-engine.audio-quality-report","version":1,"algorithm":"audio-quality-gates-v1","profile":"fast","evaluated_audio_role":"lead_vocal",
                "duration":1_000_333,
                "planned_gates":gates,
                "outcomes":[
                    {"gate":"timeline_valid","requirement":"required","status":"passed","summary":"measured","metrics":[],"regions":[]},
                    {"gate":"vocal_topology","requirement":"degrading","status":"unknown","summary":"vocal topology is unknown","metrics":[],"regions":[
                        {"start":0,"end":1_000_333,"reason":"vocal_topology_unknown"}
                    ]}
                ],
                "vocal_topology":{
                    "contract":"uta.analysis-engine.vocal-topology-estimate","version":1,"timebase":1000000,
                    "source_start":0,"duration":1_000_333,"mode":"unknown","confidence":null,
                    "overlap_regions":[],"support_regions":[],
                    "evidence_sources":["caller_or_unpartitioned_vocal_input"]
                }
            }},
            "provenance":{"resources":[],"calibration_version":"c","fusion_version":"f","hsmm_version":"h","quantization_version":"q","audio_quality_version":"audio-quality-gates-v1","postprocess_version":"p"},
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","degraded_reasons":["vocal_topology_ambiguous"]
        }))
        .unwrap();

        validate_audio_quality_result(&request, &plan, &manifest, 1_000_000).unwrap();

        // The same overshoot on a non-topology gate stays invalid.
        let mut mismatched = manifest.clone();
        let report = mismatched.diagnostics.audio_quality.as_mut().unwrap();
        report.outcomes[0].regions.push(QualityRegionWireV1 {
            start: 0,
            end: 1_000_333,
            reason: "timeline_valid_past_app_window".to_string(),
        });
        assert!(validate_audio_quality_result(&request, &plan, &mismatched, 1_000_000).is_err());
    }

    #[test]
    fn clean_evaluated_role_requires_a_bound_workflow_cleanup_route() {
        let mut plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"role-route",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],
            "optional_capabilities":["audio.denoise"],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":[],
            "fallback_policy":[],"artifact_declarations":[],
            "workflow_execution":{
                "identity":{"contract":"uta.workflow-execution-plan","version":1,
                    "workflow_schema_version":1,"workflow_id":"role-route","workflow_revision":1,
                    "definition_digest":"fixture"},
                "nodes":[
                    {"instance_id":"source","analysis_node":"workflow.source",
                        "capabilities":["audio.source"],"execution_policy":"always",
                        "execution_state":"ready","priority":100,"input_bindings":[]},
                    {"instance_id":"cleanup","analysis_node":"workflow.cleanup",
                        "capabilities":["audio.denoise"],"execution_policy":"always",
                        "execution_state":"ready","priority":90,"input_bindings":[{
                            "from_node":"workflow.source","from_port":"lead","to_node":"workflow.cleanup",
                            "to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal",
                            "execution_active":true,"analyzer_attachment":false}]},
                    {"instance_id":"pitch","analysis_node":"workflow.pitch",
                        "capabilities":["pitch.track"],"execution_policy":"always",
                        "execution_state":"ready","priority":80,"input_bindings":[{
                            "from_node":"workflow.source","from_port":"lead","to_node":"workflow.pitch",
                            "to_port":"audio","semantic_type":"audio","audio_role":"lead_vocal",
                            "execution_active":true,"analyzer_attachment":true}]}
                ],
                "terminal_outputs":[],
                "fusion_mode":"algorithm"
            }
        }))
        .unwrap();
        assert!(evaluated_audio_role_matches_plan(&plan, "lead_vocal"));
        assert!(!evaluated_audio_role_matches_plan(
            &plan,
            "clean_lead_vocal"
        ));

        {
            let workflow = plan.workflow_execution.as_mut().unwrap();
            let pitch = workflow
                .nodes
                .iter_mut()
                .find(|node| node.analysis_node == "workflow.pitch")
                .unwrap();
            pitch.input_bindings[0].from_node = "workflow.cleanup".to_string();
            pitch.input_bindings[0].from_port = "audio".to_string();
        }
        assert!(evaluated_audio_role_matches_plan(&plan, "clean_lead_vocal"));

        {
            let workflow = plan.workflow_execution.as_mut().unwrap();
            let pitch = workflow
                .nodes
                .iter_mut()
                .find(|node| node.analysis_node == "workflow.pitch")
                .unwrap();
            pitch.input_bindings[0].from_node = "workflow.intermediate".to_string();
            let intermediate: crate::backend_cli::WorkflowExecutionNodePlanWireV1 =
                serde_json::from_value(serde_json::json!({
                    "instance_id":"intermediate","analysis_node":"workflow.intermediate",
                    "capabilities":["audio.refine"],"execution_policy":"disabled",
                    "execution_state":"profile_skipped","priority":85,"input_bindings":[{
                        "from_node":"workflow.cleanup","from_port":"audio",
                        "to_node":"workflow.intermediate","to_port":"audio",
                        "semantic_type":"audio","audio_role":"lead_vocal",
                        "execution_active":true,"analyzer_attachment":false}]
                }))
                .unwrap();
            workflow.nodes.push(intermediate);
        }
        assert!(!evaluated_audio_role_matches_plan(
            &plan,
            "clean_lead_vocal"
        ));

        {
            let workflow = plan.workflow_execution.as_mut().unwrap();
            workflow
                .nodes
                .iter_mut()
                .find(|node| node.analysis_node == "workflow.pitch")
                .unwrap()
                .execution_state =
                crate::backend_cli::WorkflowNodeExecutionStateWireV1::NotRequested;
            workflow
                .nodes
                .iter_mut()
                .find(|node| node.analysis_node == "workflow.cleanup")
                .unwrap()
                .execution_state = crate::backend_cli::WorkflowNodeExecutionStateWireV1::Disabled;
            workflow
                .terminal_outputs
                .push(crate::workflow::WorkflowTerminalOutputWireV1 {
                    node: "workflow.cleanup".to_string(),
                    port: "audio".to_string(),
                    semantic_type: "audio".to_string(),
                    audio_role: None,
                });
        }
        assert!(!evaluated_audio_role_matches_plan(
            &plan,
            "clean_lead_vocal"
        ));
    }

    #[test]
    fn optional_technique_artifact_is_committed_when_present_and_omitted_when_absent() {
        let root = temp_root("technique-commit");
        let _db_guard = crate::library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir {
            path: root.join("cache"),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        let gates = vec!["timeline_valid"];
        for present in [false, true] {
            let request_id = if present {
                "technique-present"
            } else {
                "technique-absent"
            };
            let file_hash = format!("file-{request_id}");
            let output = cache.path.join(request_id);
            std::fs::create_dir_all(&output).unwrap();
            let request: AnalyzeRequestWireV1 =
                serde_json::from_value(serde_json::json!({
                    "contract":"uta.analysis-engine.request","version":1,
                    "request_id":request_id,
                    "audio_sources":[{
                        "id":"main","kind":"local_file","path":"song.flac",
                        "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "role":"lead_vocal","primary":true,
                        "timeline":{"timebase":1000000,"source_start":0}
                    }],
                    "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
                    "analysis":{"profile":"maximum","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
                    "requested_artifacts":{"pitch_evidence":true},
                    "execution_policy":{},"extensions":{}
                }))
                .unwrap();
            let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
                "schema":"uta.analysis-engine.plan","schema_version":1,
                "request_id":request_id,
                "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
                "requested_outputs":["pitch_evidence"],
                "required_capabilities":[],"optional_capabilities":["technique.analyze"],
                "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
                "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
                "fallback_policy":[],
                "artifact_declarations":[{
                    "semantic_type":"technique_evidence","required":false,
                    "media_type":"application/vnd.uta.technique-evidence+json;version=1"
                }]
            }))
            .unwrap();
            let technique = serde_json::to_vec(&serde_json::json!({
                "contract":"uta.analysis-engine.technique-evidence","version":1,
                "model_id":"stars","taxonomy":["breathy"],
                "calibration":"uncalibrated_source_local",
                "intervals":[],"style_scope":"global","styles":[],
                "provenance":{"expert_id":"stars","task":"technique"}
            }))
            .unwrap();
            if present {
                std::fs::write(output.join("technique.json"), &technique).unwrap();
            }
            let technique_ref = present.then(|| ArtifactRefWireV1 {
                path: PathBuf::from("technique.json"),
                media_type: "application/vnd.uta.technique-evidence+json;version=1".to_string(),
                sha256: "b".repeat(64),
                bytes: technique.len() as u64,
            });
            let manifest: AnalysisResultManifestWireV1 =
                serde_json::from_value(serde_json::json!({
                    "contract":"uta.analysis-engine.result","version":1,
                    "request_id":request_id,"status":"ok",
                    "artifacts":{"technique_evidence":technique_ref},
                    "diagnostics":{"audio_quality":{
                        "contract":"uta.analysis-engine.audio-quality-report","version":1,
                        "algorithm":"audio-quality-gates-v1","profile":"maximum",
                        "evaluated_audio_role":"lead_vocal","duration":1000000,
                        "planned_gates":gates,
                        "outcomes":[{
                            "gate":"timeline_valid","requirement":"required",
                            "status":"passed","summary":"measured","metrics":[],"regions":[]
                        }]
                    }},
                    "provenance":{
                        "resources":[],"calibration_version":"c","fusion_version":"f",
                        "hsmm_version":"h","quantization_version":"q",
                        "audio_quality_version":"audio-quality-gates-v1","postprocess_version":"p"
                    },
                    "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "degraded_reasons":[]
                }))
                .unwrap();

            if present {
                let mut wrong_media = manifest.clone();
                wrong_media
                    .artifacts
                    .technique_evidence
                    .as_mut()
                    .unwrap()
                    .media_type = "application/json".to_string();
                assert!(
                    validate_and_publish_engine_result(
                        &file_hash,
                        &cache,
                        &output,
                        1_000_000,
                        &request,
                        &plan,
                        &wrong_media,
                    )
                    .unwrap_err()
                    .contains("media type mismatch")
                );
            }
            validate_and_publish_engine_result(
                &file_hash, &cache, &output, 1_000_000, &request, &plan, &manifest,
            )
            .unwrap();
            let revisions = crate::analysis_artifact::load_artifact_revisions(
                &file_hash,
                ArtifactKind::TechniqueEvidence,
            );
            assert_eq!(revisions.len(), usize::from(present));
            assert!(revisions.iter().all(|revision| {
                revision.active && revision.producer_node.as_str() == "stars-technique"
            }));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn published_instrumental_stem_reaches_its_legacy_compatibility_path() {
        // Real repro: `Song::refresh_authoring_state` and the editor-open
        // gate read the flat `{hash}_instrumental.*` compatibility file, not
        // the content-addressed artifact store. A published stem that never
        // reaches that path leaves a fully-completed song stuck showing
        // "Analysis incomplete" with an editor that won't open.
        let root = temp_root("stem-compat");
        let _db_guard = crate::library_db::reconnect_for_test(&root.join("db"));
        let cache = CacheDir {
            path: root.join("cache"),
        };
        std::fs::create_dir_all(&cache.path).unwrap();
        let gates = vec!["timeline_valid"];
        let request_id = "stem-compat";
        let file_hash = "file-stem-compat";
        let output = cache.path.join(request_id);
        std::fs::create_dir_all(&output).unwrap();
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,
            "request_id":request_id,
            "audio_sources":[{
                "id":"main","kind":"local_file","path":"song.flac",
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "role":"lead_vocal","primary":true,
                "timeline":{"timebase":1000000,"source_start":0}
            }],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"maximum","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},
            "execution_policy":{},"extensions":{}
        }))
        .unwrap();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,
            "request_id":request_id,
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],
            "required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":gates,
            "fallback_policy":[],
            "artifact_declarations":[{
                "semantic_type":"stem:instrumental","required":true,
                "media_type":"audio/flac"
            }]
        }))
        .unwrap();
        let instrumental_bytes = b"fake-flac-bytes".to_vec();
        std::fs::write(output.join("instrumental.flac"), &instrumental_bytes).unwrap();
        let manifest: AnalysisResultManifestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.result","version":1,
            "request_id":request_id,"status":"ok",
            "artifacts":{"stems":[{
                "role":"instrumental",
                "artifact":{
                    "path":"instrumental.flac",
                    "media_type":"audio/flac",
                    "sha256":"b".repeat(64),
                    "bytes":instrumental_bytes.len() as u64
                }
            }]},
            "diagnostics":{"audio_quality":{
                "contract":"uta.analysis-engine.audio-quality-report","version":1,
                "algorithm":"audio-quality-gates-v1","profile":"maximum",
                "evaluated_audio_role":"lead_vocal","duration":1000000,
                "planned_gates":gates,
                "outcomes":[{
                    "gate":"timeline_valid","requirement":"required",
                    "status":"passed","summary":"measured","metrics":[],"regions":[]
                }]
            }},
            "provenance":{
                "resources":[],"calibration_version":"c","fusion_version":"f",
                "hsmm_version":"h","quantization_version":"q",
                "audio_quality_version":"audio-quality-gates-v1","postprocess_version":"p"
            },
            "fingerprint":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "degraded_reasons":[]
        }))
        .unwrap();

        validate_and_publish_engine_result(
            file_hash, &cache, &output, 1_000_000, &request, &plan, &manifest,
        )
        .unwrap();

        assert!(
            cache.instrumental_path(file_hash).is_file(),
            "instrumental stem should reach its legacy compatibility path"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn typed_vocal_topology_wire_is_plan_bound_and_fails_closed_on_shape_conflict() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"topology",
            "audio_sources":[{"id":"main","kind":"local_file","path":"song.flac","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"lead_vocal","primary":true,"timeline":{"timebase":1000000,"source_start":2000000}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"balanced","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"pitch_evidence":true},"execution_policy":{},"extensions":{}
        }))
        .unwrap();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"topology",
            "source_route":{"primary_source_id":"main","input_role":"lead_vocal","preparation":[]},
            "requested_outputs":["pitch_evidence"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":["vocal_topology"],
            "fallback_policy":[],"artifact_declarations":[]
        }))
        .unwrap();
        let mut report: AudioQualityReportWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.audio-quality-report","version":1,
            "algorithm":"audio-quality-gates-v2","profile":"balanced",
            "evaluated_audio_role":"lead_vocal","duration":1000000,
            "planned_gates":["vocal_topology"],"outcomes":[],
            "vocal_topology":{
                "contract":"uta.analysis-engine.vocal-topology-estimate","version":1,
                "timebase":1000000,"source_start":2000000,"duration":1000000,
                "mode":"unknown","overlap_regions":[],"support_regions":[],
                "evidence_sources":["caller_or_unpartitioned_vocal_input"]
            }
        }))
        .unwrap();
        validate_vocal_topology_result(&request, &plan, &report, 1_000_000).unwrap();

        report.vocal_topology.as_mut().unwrap().mode =
            VocalTopologyModeWireV1::OverlappingMultiLead;
        assert!(validate_vocal_topology_result(&request, &plan, &report, 1_000_000).is_err());
        report.vocal_topology = None;
        assert!(
            validate_vocal_topology_result(&request, &plan, &report, 1_000_000)
                .unwrap_err()
                .contains("omitted")
        );
    }

    #[test]
    fn artifact_event_does_not_erase_last_measured_node_progress() {
        let file_hash = "engine-lifecycle-progress-fixture";
        let snapshot: AnalysisProgressSnapshot = serde_json::from_value(serde_json::json!({
            "stage":"Preparing","overall_progress":0,"stage_progress":0,
            "operation":"Preparing","detail":"","implementation":"uta-analysis-engine",
            "model":"Engine native","device":"Engine-resolved",
            "requested_device":"Production policy","fallback_from":null,
            "fallback_reason":null,"backend_fallback_from":null,
            "backend_fallback_reason":null,"stage_routes":[]
        }))
        .unwrap();
        LIVE_ANALYSIS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), snapshot);
        let event = |frame_type: &str,
                     progress: Option<f32>,
                     work_units: Option<(u64, u64)>,
                     artifact: Option<&str>| {
            AnalysisLifecycleFrameWireV1 {
                frame_type: frame_type.to_string(),
                schema_version: 1,
                request_id: "request".to_string(),
                node_id: "pitch.track".to_string(),
                presentation_node_id: Some("workflow.f0_rmvpe".to_string()),
                capability_id: "pitch.track".to_string(),
                model_id: Some("rmvpe".to_string()),
                implementation: "openvino".to_string(),
                progress,
                work_units_completed: work_units.map(|(completed, _)| completed),
                work_units_total: work_units.map(|(_, total)| total),
                worker_task_id: work_units.map(|_| "rmvpe-task-7".to_string()),
                artifact: artifact.map(str::to_string),
                path: None,
                message: None,
                event_at_ms: 1,
            }
        };
        apply_engine_lifecycle_event(file_hash, None, event("node_started", None, None, None));
        apply_engine_lifecycle_event(
            file_hash,
            None,
            event("node_progress", Some(0.42), Some((21, 50)), None),
        );
        apply_engine_lifecycle_event(
            file_hash,
            None,
            event("artifact", None, None, Some("pitch_evidence")),
        );
        let snapshot = LIVE_ANALYSIS.lock().unwrap().remove(file_hash).unwrap();
        assert_eq!(snapshot.stage_progress, 42);
        assert_eq!(snapshot.node_event.as_deref(), Some("artifact"));
        assert_eq!(snapshot.stage_routes.len(), 1);
        assert_eq!(snapshot.stage_routes[0].stage_progress, 42);
        assert_eq!(snapshot.stage_routes[0].work_units_completed, Some(21));
        assert_eq!(snapshot.stage_routes[0].work_units_total, Some(50));
        assert_eq!(
            snapshot.stage_routes[0].worker_task_id.as_deref(),
            Some("rmvpe-task-7")
        );
        assert_eq!(
            snapshot.stage_routes[0].node_event.as_deref(),
            Some("node_progress")
        );

        LIVE_ANALYSIS
            .lock()
            .unwrap()
            .insert(file_hash.to_string(), snapshot);
        apply_engine_lifecycle_event(
            file_hash,
            None,
            event("node_progress", Some(0.9), None, None),
        );
        let unitless = LIVE_ANALYSIS.lock().unwrap().remove(file_hash).unwrap();
        assert_eq!(unitless.stage_progress, 90);
        assert_eq!(unitless.stage_routes[0].stage_progress, 90);
        assert_eq!(unitless.stage_routes[0].work_units_completed, None);
        assert_eq!(unitless.stage_routes[0].work_units_total, None);
        assert_eq!(unitless.stage_routes[0].worker_task_id, None);
    }

    #[cfg(unix)]
    #[test]
    fn result_artifact_validation_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = temp_root("symlink");
        let outside = root
            .parent()
            .unwrap()
            .join(format!("uta-studio-engine-outside-{}", std::process::id()));
        std::fs::write(&outside, b"valid").unwrap();
        symlink(&outside, root.join("link.json")).unwrap();
        assert!(
            validate_artifact(&root, &artifact("link.json", b"valid"))
                .unwrap_err()
                .contains("non-symlink")
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }
}
