//! Projection of authoritative analyzer node events onto DAG node state.
//!
//! Keeping this separate from render-model construction makes the boundary
//! explicit: structured events are authoritative for current runs, while the
//! older stage-bucket projection remains a compatibility path for history.

use super::{GraphNodeState, GraphViewModel};

/// Applies authoritative structured runtime events after the plan/bucket
/// projection. Bucket progress remains a fallback only: multiple real nodes
/// share a bucket, and a stale artifact can be replaced by a running node.
pub(crate) fn overlay_runtime_node_event_states(
    view: &mut GraphViewModel,
    routes: &[app_core::AnalysisStageRoute],
) {
    for node in &mut view.nodes {
        let Some(event) = routes
            .iter()
            .rev()
            .find(|route| route.node_id.as_deref() == Some(node.id.as_str()))
            .and_then(|route| route.node_event.as_deref())
        else {
            continue;
        };
        let Some(runtime_state) = runtime_node_state(event) else {
            continue;
        };
        if matches!(
            node.state,
            GraphNodeState::NotApplicable
                | GraphNodeState::Disabled
                | GraphNodeState::Blocked
                | GraphNodeState::Frozen
                | GraphNodeState::Bypassed
        ) {
            continue;
        }
        node.state = runtime_state;
    }
    for (compound_id, state) in [
        ("music.analysis", derived_music_analysis_state(routes)),
        ("stems.separate", derived_stem_analysis_state(routes)),
    ] {
        if let Some(state) = state
            && let Some(node) = view
                .nodes
                .iter_mut()
                .find(|node| node.id.as_str() == compound_id)
        {
            node.state = state;
        }
    }
}

fn latest_runtime_event<'a>(
    routes: &'a [app_core::AnalysisStageRoute],
    node_id: &str,
) -> Option<&'a str> {
    routes
        .iter()
        .rev()
        .find(|route| route.node_id.as_deref() == Some(node_id))
        .and_then(|route| route.node_event.as_deref())
}

fn event_is_terminal(event: &str) -> bool {
    matches!(
        event,
        "completed"
            | "reused"
            | "skipped"
            | "failed"
            | "cancelled"
            | "node_completed"
            | "artifact_reused"
            | "node_skipped"
            | "node_failed"
            | "node_cancelled"
    )
}

pub(super) fn derived_music_analysis_state(
    routes: &[app_core::AnalysisStageRoute],
) -> Option<GraphNodeState> {
    let events: Vec<_> = ["music.key", "music.rhythm", "music.descriptors"]
        .into_iter()
        .map(|node_id| latest_runtime_event(routes, node_id))
        .collect();
    if events.iter().all(Option::is_none) {
        return None;
    }
    if events.iter().flatten().any(|event| {
        matches!(
            *event,
            "failed" | "cancelled" | "node_failed" | "node_cancelled"
        )
    }) {
        return Some(GraphNodeState::Failed);
    }
    if events
        .iter()
        .all(|event| event.is_some_and(event_is_terminal))
    {
        return Some(GraphNodeState::Complete);
    }
    Some(GraphNodeState::Running)
}

pub(super) fn derived_stem_analysis_state(
    routes: &[app_core::AnalysisStageRoute],
) -> Option<GraphNodeState> {
    const STEM_RUNTIME_NODES: &[&str] = &[
        "stems.vocals",
        "vocals.denoise",
        "vocals.dereverb",
        "stems.instrumental",
        "instrumental.denoise",
        "instrumental.dereverb",
        "stems.karaoke",
        "stems.multistem",
        "stems.bind_analysis_outputs",
    ];
    let events: Vec<_> = STEM_RUNTIME_NODES
        .iter()
        .filter_map(|node_id| latest_runtime_event(routes, node_id))
        .collect();
    if events.is_empty() {
        return None;
    }
    if events.iter().any(|event| {
        matches!(
            *event,
            "failed" | "cancelled" | "node_failed" | "node_cancelled"
        )
    }) {
        return Some(GraphNodeState::Failed);
    }
    match latest_runtime_event(routes, "stems.bind_analysis_outputs") {
        Some("completed" | "reused" | "node_completed" | "artifact_reused") => {
            Some(GraphNodeState::Complete)
        }
        Some("skipped" | "node_skipped") => Some(GraphNodeState::Bypassed),
        _ => Some(GraphNodeState::Running),
    }
}

pub(super) fn runtime_node_state(event: &str) -> Option<GraphNodeState> {
    match event {
        "started" | "progress" | "node_started" | "node_progress" => Some(GraphNodeState::Running),
        "completed" | "reused" | "node_completed" | "artifact_reused" => {
            Some(GraphNodeState::Complete)
        }
        "failed" | "cancelled" | "node_failed" | "node_cancelled" => Some(GraphNodeState::Failed),
        "skipped" | "node_skipped" => Some(GraphNodeState::Bypassed),
        _ => None,
    }
}
