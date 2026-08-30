use super::*;
use crate::backend_cli::{
    AnalysisCancelHandle, AnalysisPlanWireV1, AnalyzeRequestWireV1, AudioSourceWireV1,
};

static ACTIVE_ENGINE_CANCELS: LazyLock<Mutex<HashMap<String, AnalysisCancelHandle>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FORCE_STOP_REQUESTED: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

pub(crate) fn force_stop_active_engine(file_hash: &str) -> Result<(), String> {
    if ANALYZER.lock().unwrap().active_hash.as_deref() != Some(file_hash) {
        return Ok(());
    }
    FORCE_STOP_REQUESTED
        .lock()
        .unwrap()
        .insert(file_hash.to_string());
    let handle = ACTIVE_ENGINE_CANCELS
        .lock()
        .unwrap()
        .get(file_hash)
        .cloned();
    let Some(handle) = handle else {
        // The scheduler may have selected the row while the packaged Engine
        // is still connecting. `execute_exact_intent` observes this marker
        // before or immediately after it installs the process handle.
        return Ok(());
    };
    if let Err(error) = handle.force_stop() {
        FORCE_STOP_REQUESTED.lock().unwrap().remove(file_hash);
        return Err(error.to_string());
    }
    Ok(())
}

pub(super) fn register_active_engine(file_hash: &str, handle: AnalysisCancelHandle) {
    ACTIVE_ENGINE_CANCELS
        .lock()
        .unwrap()
        .insert(file_hash.to_string(), handle);
}

pub(super) fn remove_active_engine(file_hash: &str) {
    ACTIVE_ENGINE_CANCELS.lock().unwrap().remove(file_hash);
}

pub(super) fn force_stop_was_requested(file_hash: &str) -> bool {
    FORCE_STOP_REQUESTED.lock().unwrap().contains(file_hash)
}

pub(super) fn take_force_stop_request(file_hash: &str) -> bool {
    FORCE_STOP_REQUESTED.lock().unwrap().remove(file_hash)
}

pub(crate) fn clear_force_stop_request(file_hash: &str) {
    FORCE_STOP_REQUESTED.lock().unwrap().remove(file_hash);
}

pub(super) fn mark_snapshot_cancelled(snapshot: &mut AnalysisProgressSnapshot, message: &str) {
    let stopped_at_ms = unix_time_ms();
    snapshot.stage = "cancelled".to_string();
    snapshot.operation = "Analysis stopped".to_string();
    snapshot.detail = message.to_string();
    snapshot.node_event = Some("cancelled".to_string());
    let current_node = snapshot.node_id.as_deref();
    let current_engine_node = snapshot.engine_node_id.as_deref();
    if let Some(route) = snapshot.stage_routes.iter_mut().rev().find(|route| {
        route.node_id.as_deref() == current_node
            && route.engine_node_id.as_deref() == current_engine_node
            && route.finished_at_ms.is_none()
    }) {
        route.node_event = Some("node_cancelled".to_string());
        route.finished_at_ms = Some(stopped_at_ms);
        route.event_at_ms = Some(stopped_at_ms);
        route.operation = "Analysis stopped".to_string();
    }
}

pub(super) fn validated_primary_source_binding<'a>(
    request: &'a AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
) -> Result<&'a AudioSourceWireV1, String> {
    let mut primaries = request.audio_sources.iter().filter(|source| source.primary);
    let primary = primaries
        .next()
        .ok_or_else(|| "persisted Engine request has no primary source".to_string())?;
    if primaries.next().is_some()
        || primary.timeline.timebase != crate::backend_cli::CANONICAL_TIMEBASE
        || plan.source_route.primary_source_id != primary.id
        || plan.source_route.input_role != primary.role
    {
        return Err("Engine request and Plan primary-source binding is invalid".to_string());
    }
    Ok(primary)
}

pub(super) fn validate_exact_execution_source_binding(
    request: &AnalyzeRequestWireV1,
    plan: &AnalysisPlanWireV1,
    library_true_source: &Path,
    queued_true_source: &Path,
) -> Result<(), String> {
    // The Plan may intentionally route a cached GuideVocals/LeadVocal file as
    // its primary analysis input. That binding is validated against the Plan;
    // only the library-owned TrueSource is compared with the queued snapshot.
    validated_primary_source_binding(request, plan)?;
    if library_true_source != queued_true_source {
        return Err(
            "source_identity_changed: queued TrueSource no longer matches the exact preview"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_primary_input_is_distinct_from_the_queued_true_source() {
        let request: AnalyzeRequestWireV1 = serde_json::from_value(serde_json::json!({
            "contract":"uta.analysis-engine.request","version":1,"request_id":"cached-input",
            "audio_sources":[{"id":"true_source","kind":"local_file","path":"/cache/guide.flac","sha256":"library-id","role":"guide_vocals","primary":true,"timeline":{"timebase":1000000,"source_start":0}}],
            "lyrics":{"mode":"none","tokens":[]},"boundary_constraints":[],
            "analysis":{"profile":"balanced","track_target":"lead","preserve_continuous_pitch":true,"enable_quantization":false},
            "requested_artifacts":{"transcript":true},"execution_policy":{},"extensions":{}
        }))
        .unwrap();
        let plan: AnalysisPlanWireV1 = serde_json::from_value(serde_json::json!({
            "schema":"uta.analysis-engine.plan","schema_version":1,"request_id":"cached-input",
            "source_route":{"primary_source_id":"true_source","input_role":"guide_vocals","preparation":[]},
            "requested_outputs":["transcript"],"required_capabilities":[],"optional_capabilities":[],
            "requirements":{"schema":"uta.runtime.requirements","schema_version":1,"resources":[]},
            "resolved_resources":[],"execution_nodes":[],"quality_gates":[],"fallback_policy":[],
            "artifact_declarations":[]
        }))
        .unwrap();

        assert!(
            validate_exact_execution_source_binding(
                &request,
                &plan,
                Path::new("/library/original.flac"),
                Path::new("/library/original.flac"),
            )
            .is_ok()
        );
        assert!(
            validate_exact_execution_source_binding(
                &request,
                &plan,
                Path::new("/library/replaced.flac"),
                Path::new("/library/original.flac"),
            )
            .unwrap_err()
            .contains("source_identity_changed")
        );
    }
}
