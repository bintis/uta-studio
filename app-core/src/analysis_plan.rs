//! Per-request planning: turns a static `AnalysisGraphSpec` plus an
//! `AnalysisRequest` (targets, disabled nodes, frozen artifacts, lyrics
//! route) into a concrete `AnalysisPlan` describing exactly which nodes will
//! run, which will be reused/frozen, and which are blocked. This is the
//! "only-read" API surface Phase 1 ships (`get_analysis_graph`,
//! `preview_analysis_plan`) — it does not touch the filesystem, the queue,
//! or the existing Python worker.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis_graph::{
    AnalysisGraphSpec, AnalysisNodeId, ArtifactKind, DisablePolicy, GraphValidationError,
    baseline_graph_spec, default_active_stem_nodes, lyrics_route_node_ids, optional_stem_node_ids,
    stem_group_node_ids,
};
use crate::analysis_profile::AnalysisProfileSnapshot;

/// See docs/analysis-dag-redesign.md §7. Phase 1's planner only ever
/// produces `Ready | Frozen | Disabled | Blocked | NotApplicable` — the rest
/// (`Queued`, `Running`, `Cached`, `Succeeded*`, `Failed`, `Stale`,
/// `Cancelled`) are execution-time states that Phase 3's event protocol and
/// Phase 2's artifact inventory populate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum NodeState {
    Missing,
    Ready,
    Queued,
    Running,
    Cached,
    Succeeded,
    SucceededWithWarnings,
    Failed,
    Stale,
    Frozen,
    Disabled,
    Blocked,
    NotApplicable,
    Cancelled,
    /// Phase 4 §4.5 Bypass: an alternate input routes around this node
    /// entirely (e.g. `stems.separate` bypassed with the Original Mix) --
    /// distinct from `Frozen` (which reuses *this node's own* stale
    /// output) and from `Disabled` (which leaves downstream `Blocked`
    /// unless something else supplies its input). Like `Frozen`, a
    /// bypassed node stops the required-upstream closure from ascending
    /// further (nothing upstream of it is needed to produce a substitute
    /// input) and never blocks its own downstream consumers.
    Bypassed,
}

/// Which lyrics sub-graph is active for a run, per
/// docs/analysis-dag-redesign.md §5 dynamic branch rules (verified against
/// `pipeline.py::transcribe_or_align`). Every lyrics-route node not covered
/// by the selected route is excluded from the plan's universe entirely
/// (`NotApplicable`), not merely left un-targeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum LyricsRoute {
    TimedLrc,
    KnownLyrics,
    WhisperAsr,
    ParakeetAsr,
}

impl LyricsRoute {
    pub fn active_nodes(&self) -> BTreeSet<AnalysisNodeId> {
        let ids: &[&str] = match self {
            LyricsRoute::TimedLrc => &["lyrics.import_timed"],
            LyricsRoute::KnownLyrics => &["lyrics.preprocess", "lyrics.align"],
            LyricsRoute::WhisperAsr => &["lyrics.preprocess", "lyrics.transcribe", "lyrics.align"],
            LyricsRoute::ParakeetAsr => &["lyrics.preprocess", "lyrics.transcribe"],
        };
        ids.iter().map(|s| AnalysisNodeId::new(*s)).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisRequest {
    pub file_hash: String,
    pub targets: BTreeSet<AnalysisNodeId>,
    #[serde(default)]
    pub disabled_nodes: BTreeSet<AnalysisNodeId>,
    #[serde(default)]
    pub frozen_artifacts: BTreeSet<ArtifactKind>,
    /// Phase 4 §4.5 Bypass: nodes to route around using their designated
    /// alternate input rather than running or freezing them. Which node ids
    /// have a real alternate input to route through is a pipeline-execution
    /// fact (today: only `stems.separate`, bypassed with the Original Mix),
    /// not something this pure planner hardcodes -- same division of
    /// responsibility as `frozen_artifacts`, where `analyzer.rs`'s
    /// `pipeline_can_honor_bypass` gates what's actually offered before a
    /// request ever reaches here.
    #[serde(default)]
    pub bypassed_nodes: BTreeSet<AnalysisNodeId>,
    pub lyrics_route: LyricsRoute,
    /// Stand-in for real model-install status until this is wired to
    /// `vendor::model_install_statuses` in a later phase. Missing entries
    /// default to "available" so Phase 1 tests don't have to enumerate
    /// every node. This planner never installs anything regardless of what
    /// this map says — it only ever reads it to decide `Blocked`.
    #[serde(default)]
    pub model_availability: BTreeMap<AnalysisNodeId, bool>,
    #[serde(default)]
    pub profile_snapshot: AnalysisProfileSnapshot,
    /// Stem-pipeline nodes selected by the current audio settings. An empty
    /// set means "use the default chart path" (`stems.vocals` + bind).
    /// Optional stem nodes not listed here become `NotApplicable`.
    #[serde(default)]
    pub active_stem_nodes: BTreeSet<AnalysisNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlannedNode {
    pub id: AnalysisNodeId,
    pub state: NodeState,
    pub will_run: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanWarning {
    pub node: AnalysisNodeId,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisPlan {
    pub graph_schema_version: u32,
    pub file_hash: String,
    pub nodes: Vec<PlannedNode>,
    pub target_nodes: BTreeSet<AnalysisNodeId>,
    pub profile_snapshot: AnalysisProfileSnapshot,
    pub warnings: Vec<PlanWarning>,
}

impl AnalysisPlan {
    pub fn node(&self, id: &AnalysisNodeId) -> Option<&PlannedNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    InvalidGraph(GraphValidationError),
    UnknownTarget(AnalysisNodeId),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGraph(err) => write!(f, "invalid graph: {err}"),
            Self::UnknownTarget(id) => write!(f, "unknown or not-applicable target node: {id}"),
        }
    }
}

fn direct_parents(graph: &AnalysisGraphSpec, id: &AnalysisNodeId) -> Vec<AnalysisNodeId> {
    graph
        .edges
        .iter()
        .filter(|edge| &edge.to == id)
        .map(|edge| edge.from.clone())
        .collect()
}

fn effective_active_stem_nodes(request: &AnalysisRequest) -> BTreeSet<AnalysisNodeId> {
    if request.active_stem_nodes.is_empty() {
        default_active_stem_nodes()
    } else {
        let mut nodes = request.active_stem_nodes.clone();
        nodes.insert(AnalysisNodeId::new("stems.bind_analysis_outputs"));
        nodes
    }
}

fn expand_stem_group(ids: &BTreeSet<AnalysisNodeId>) -> BTreeSet<AnalysisNodeId> {
    let mut expanded = ids.clone();
    if ids.contains(&AnalysisNodeId::new("stems.separate")) {
        expanded.extend(stem_group_node_ids());
    }
    expanded
}

fn plan_parents(
    graph: &AnalysisGraphSpec,
    id: &AnalysisNodeId,
    universe: &BTreeSet<AnalysisNodeId>,
) -> Vec<AnalysisNodeId> {
    let mut parents = direct_parents(graph, id);
    if id.as_str() == "stems.bind_analysis_outputs"
        && !universe.contains(&AnalysisNodeId::new("stems.vocals"))
        && universe.contains(&AnalysisNodeId::new("stems.multistem"))
    {
        parents.push(AnalysisNodeId::new("stems.multistem"));
    }
    parents
}

/// Builds a concrete plan for one request against one graph. Pure function:
/// no filesystem, queue, or model-installer access — see module docs.
pub fn build_plan(
    graph: &AnalysisGraphSpec,
    request: &AnalysisRequest,
) -> Result<AnalysisPlan, PlanError> {
    graph.validate().map_err(PlanError::InvalidGraph)?;
    let order = graph.topo_order().map_err(PlanError::InvalidGraph)?;

    let route_nodes = lyrics_route_node_ids();
    let active_route_nodes = request.lyrics_route.active_nodes();
    let optional_stems = optional_stem_node_ids();
    let active_stems = effective_active_stem_nodes(request);
    let universe: BTreeSet<AnalysisNodeId> = graph
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .filter(|id| !route_nodes.contains(id) || active_route_nodes.contains(id))
        .filter(|id| !optional_stems.contains(id) || active_stems.contains(id))
        .collect();
    let disabled_nodes = expand_stem_group(&request.disabled_nodes);
    let request_bypassed = expand_stem_group(&request.bypassed_nodes);

    for target in &request.targets {
        if !universe.contains(target) {
            return Err(PlanError::UnknownTarget(target.clone()));
        }
    }

    // Required closure: targets plus transitive upstream, stopping ascent at
    // any node whose output is satisfied by a frozen artifact (freeze means
    // "reuse current output," so nothing further upstream is needed to
    // produce it this run — docs/analysis-dag-redesign.md §6).
    let mut required: BTreeSet<AnalysisNodeId> = BTreeSet::new();
    let mut frozen_nodes: BTreeSet<AnalysisNodeId> = BTreeSet::new();
    let mut bypassed_nodes: BTreeSet<AnalysisNodeId> = BTreeSet::new();
    let mut stack: Vec<AnalysisNodeId> = request.targets.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        if !required.insert(id.clone()) {
            continue;
        }
        let node_frozen = graph
            .node(&id)
            .map(|spec| {
                spec.outputs
                    .iter()
                    .any(|kind| request.frozen_artifacts.contains(kind))
            })
            .unwrap_or(false);
        if node_frozen {
            frozen_nodes.insert(id.clone());
            continue;
        }
        if request_bypassed.contains(&id) {
            bypassed_nodes.insert(id.clone());
            continue;
        }
        for parent in plan_parents(graph, &id, &universe) {
            if universe.contains(&parent) {
                stack.push(parent);
            }
        }
    }

    // Forward pass in topological order: every graph node gets a state, so
    // the plan is complete enough for a full-graph UI even though only
    // `required` nodes actually influence `will_run`.
    let mut states: BTreeMap<AnalysisNodeId, (NodeState, Option<String>)> = BTreeMap::new();
    for id in &order {
        if !universe.contains(id) {
            let reason = if optional_stems.contains(id) {
                "not part of the selected stem pipeline"
            } else {
                "not part of the selected lyrics route"
            };
            states.insert(
                id.clone(),
                (NodeState::NotApplicable, Some(reason.to_string())),
            );
            continue;
        }
        if request_bypassed.contains(id) || bypassed_nodes.contains(id) {
            states.insert(id.clone(), (NodeState::Bypassed, None));
            continue;
        }

        let node = graph
            .node(id)
            .expect("universe node exists in a validated graph");

        if disabled_nodes.contains(id) {
            match node.disable_policy {
                DisablePolicy::AlwaysRequired => {
                    states.insert(
                        id.clone(),
                        (
                            NodeState::Blocked,
                            Some(format!("{id} cannot be disabled: always required")),
                        ),
                    );
                }
                DisablePolicy::Optional => {
                    states.insert(id.clone(), (NodeState::Disabled, None));
                }
            }
            continue;
        }
        if !required.contains(id) {
            states.insert(id.clone(), (NodeState::Ready, None));
            continue;
        }
        if frozen_nodes.contains(id) {
            states.insert(id.clone(), (NodeState::Frozen, None));
            continue;
        }

        let mut blocking_parent: Option<AnalysisNodeId> = None;
        for parent in plan_parents(graph, id, &universe) {
            if let Some((parent_state, _)) = states.get(&parent)
                && matches!(parent_state, NodeState::Blocked | NodeState::Disabled)
            {
                blocking_parent = Some(parent);
                break;
            }
        }
        if let Some(parent) = blocking_parent {
            states.insert(
                id.clone(),
                (
                    NodeState::Blocked,
                    Some(format!("upstream node {parent} is disabled or blocked")),
                ),
            );
            continue;
        }

        let model_available = request.model_availability.get(id).copied().unwrap_or(true);
        if !model_available {
            states.insert(
                id.clone(),
                (
                    NodeState::Blocked,
                    Some(format!("required model for {id} is not installed")),
                ),
            );
            continue;
        }

        states.insert(id.clone(), (NodeState::Ready, None));
    }

    let mut nodes = Vec::with_capacity(order.len());
    let mut warnings = Vec::new();
    for id in &order {
        let (state, reason) = states.remove(id).expect("every node received a state");
        let will_run = required.contains(id) && matches!(state, NodeState::Ready);
        if matches!(state, NodeState::Blocked) {
            warnings.push(PlanWarning {
                node: id.clone(),
                message: reason.clone().unwrap_or_else(|| "blocked".to_string()),
            });
        }
        nodes.push(PlannedNode {
            id: id.clone(),
            state,
            will_run,
            reason,
        });
    }

    Ok(AnalysisPlan {
        graph_schema_version: graph.schema_version,
        file_hash: request.file_hash.clone(),
        nodes,
        target_nodes: request.targets.clone(),
        profile_snapshot: request.profile_snapshot.clone(),
        warnings,
    })
}

/// Read-only API (phase plan §1, "本阶段先实现" list). `file_hash` is
/// currently unused beyond validation-ready plumbing: every song shares the
/// same graph today. A future phase may need per-song graph variants (e.g.
/// USDX-imported songs); keeping the parameter now avoids a breaking
/// signature change later.
pub fn get_analysis_graph(_file_hash: &str) -> AnalysisGraphSpec {
    baseline_graph_spec()
}

pub fn preview_analysis_plan(
    file_hash: &str,
    mut request: AnalysisRequest,
) -> Result<AnalysisPlan, PlanError> {
    request.file_hash = file_hash.to_string();
    build_plan(&baseline_graph_spec(), &request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(targets: &[&str], route: LyricsRoute) -> AnalysisRequest {
        AnalysisRequest {
            file_hash: "songA".to_string(),
            targets: targets.iter().map(|s| AnalysisNodeId::new(*s)).collect(),
            disabled_nodes: BTreeSet::new(),
            frozen_artifacts: BTreeSet::new(),
            bypassed_nodes: BTreeSet::new(),
            lyrics_route: route,
            model_availability: BTreeMap::new(),
            profile_snapshot: AnalysisProfileSnapshot::default(),
            active_stem_nodes: BTreeSet::new(),
        }
    }

    #[test]
    fn target_node_automatically_pulls_in_required_upstream() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["pitch.extract"], LyricsRoute::WhisperAsr),
        )
        .unwrap();
        assert!(
            plan.node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .will_run
        );
        assert!(
            plan.node(&AnalysisNodeId::new("stems.vocals"))
                .unwrap()
                .will_run
        );
        assert!(
            plan.node(&AnalysisNodeId::new("stems.bind_analysis_outputs"))
                .unwrap()
                .will_run
        );
        assert!(
            plan.node(&AnalysisNodeId::new("preflight"))
                .unwrap()
                .will_run
        );
        assert!(
            !plan
                .node(&AnalysisNodeId::new("stems.separate"))
                .unwrap()
                .will_run
        );
        // music.analysis is an independent sibling, never pulled in.
        assert!(
            !plan
                .node(&AnalysisNodeId::new("music.analysis"))
                .unwrap()
                .will_run
        );
    }

    /// Phase 9 §9.2 "Music Analysis" acceptance items: a standalone
    /// Music Analysis rerun must never touch Stems, Transcript, or Pitch.
    /// The mirror of `target_node_automatically_pulls_in_required_upstream`
    /// above (which checks the other direction -- targeting pitch.extract
    /// never pulls in music.analysis).
    #[test]
    fn music_analysis_only_target_never_pulls_in_stems_pitch_or_lyrics() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["music.analysis"], LyricsRoute::WhisperAsr),
        )
        .unwrap();
        assert!(
            plan.node(&AnalysisNodeId::new("music.analysis"))
                .unwrap()
                .will_run
        );
        for sibling in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "chart.build_candidate",
        ] {
            assert!(
                !plan.node(&AnalysisNodeId::new(sibling)).unwrap().will_run,
                "{sibling} must not run for a music.analysis-only target"
            );
        }
    }

    #[test]
    fn frozen_artifact_satisfies_downstream_input_without_rerunning_upstream() {
        let graph = baseline_graph_spec();
        let mut req = request(&["pitch.extract"], LyricsRoute::WhisperAsr);
        req.frozen_artifacts.insert(ArtifactKind::AnalysisVocalStem);

        let plan = build_plan(&graph, &req).unwrap();
        let bind = plan
            .node(&AnalysisNodeId::new("stems.bind_analysis_outputs"))
            .unwrap();
        assert_eq!(bind.state, NodeState::Frozen);
        assert!(!bind.will_run);

        // Freezing bind's analysis vocal must stop the closure from
        // reaching further upstream to extract or preflight.
        assert!(
            !plan
                .node(&AnalysisNodeId::new("preflight"))
                .unwrap()
                .will_run
        );

        let pitch = plan.node(&AnalysisNodeId::new("pitch.extract")).unwrap();
        assert_eq!(pitch.state, NodeState::Ready);
        assert!(pitch.will_run);
    }

    #[test]
    fn bypassed_node_satisfies_downstream_input_without_rerunning_upstream_or_blocking() {
        // Phase 4 §4.5 Bypass: routing stems.separate around via the
        // Original Mix must behave like Freeze for closure purposes (stop
        // ascending to preflight, don't block pitch.extract) but land on a
        // distinct `Bypassed` state, not `Frozen` -- the UI needs to tell
        // "reusing this node's own stale output" apart from "using a
        // substitute input entirely."
        let graph = baseline_graph_spec();
        let mut req = request(&["pitch.extract"], LyricsRoute::WhisperAsr);
        req.bypassed_nodes
            .insert(AnalysisNodeId::new("stems.separate"));

        let plan = build_plan(&graph, &req).unwrap();
        let stems = plan.node(&AnalysisNodeId::new("stems.separate")).unwrap();
        assert_eq!(stems.state, NodeState::Bypassed);
        assert!(!stems.will_run);

        assert!(
            !plan
                .node(&AnalysisNodeId::new("preflight"))
                .unwrap()
                .will_run
        );

        let pitch = plan.node(&AnalysisNodeId::new("pitch.extract")).unwrap();
        assert_eq!(pitch.state, NodeState::Ready);
        assert!(pitch.will_run);
    }

    #[test]
    fn disabling_an_always_required_node_blocks_the_plan() {
        let graph = baseline_graph_spec();
        let mut req = request(&["preflight"], LyricsRoute::WhisperAsr);
        req.disabled_nodes.insert(AnalysisNodeId::new("preflight"));

        let plan = build_plan(&graph, &req).unwrap();
        let preflight = plan.node(&AnalysisNodeId::new("preflight")).unwrap();
        assert_eq!(preflight.state, NodeState::Blocked);
        assert!(!preflight.will_run);
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.node == AnalysisNodeId::new("preflight"))
        );
    }

    #[test]
    fn disabling_optional_node_blocks_downstream_without_bypass() {
        let graph = baseline_graph_spec();
        let mut req = request(&["chart.build_candidate"], LyricsRoute::WhisperAsr);
        req.disabled_nodes
            .insert(AnalysisNodeId::new("stems.separate"));

        let plan = build_plan(&graph, &req).unwrap();
        assert_eq!(
            plan.node(&AnalysisNodeId::new("stems.separate"))
                .unwrap()
                .state,
            NodeState::Disabled
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("stems.bind_analysis_outputs"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("pitch.extract"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("chart.build_candidate"))
                .unwrap()
                .state,
            NodeState::Blocked
        );
    }

    #[test]
    fn timed_lrc_route_excludes_asr_node() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::TimedLrc),
        )
        .unwrap();
        let transcribe = plan
            .node(&AnalysisNodeId::new("lyrics.transcribe"))
            .unwrap();
        assert_eq!(transcribe.state, NodeState::NotApplicable);
        assert!(!transcribe.will_run);
        let align = plan.node(&AnalysisNodeId::new("lyrics.align")).unwrap();
        assert_eq!(align.state, NodeState::NotApplicable);
        let import_timed = plan
            .node(&AnalysisNodeId::new("lyrics.import_timed"))
            .unwrap();
        assert!(import_timed.will_run);
    }

    #[test]
    fn parakeet_route_excludes_alignment_node() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::ParakeetAsr),
        )
        .unwrap();
        assert!(
            plan.node(&AnalysisNodeId::new("lyrics.transcribe"))
                .unwrap()
                .will_run
        );
        assert_eq!(
            plan.node(&AnalysisNodeId::new("lyrics.align"))
                .unwrap()
                .state,
            NodeState::NotApplicable
        );
    }

    #[test]
    fn whisper_route_generates_asr_and_alignment() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::WhisperAsr),
        )
        .unwrap();
        assert!(
            plan.node(&AnalysisNodeId::new("lyrics.transcribe"))
                .unwrap()
                .will_run
        );
        assert!(
            plan.node(&AnalysisNodeId::new("lyrics.align"))
                .unwrap()
                .will_run
        );
    }

    #[test]
    fn known_lyrics_route_goes_directly_to_alignment() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::KnownLyrics),
        )
        .unwrap();
        assert_eq!(
            plan.node(&AnalysisNodeId::new("lyrics.transcribe"))
                .unwrap()
                .state,
            NodeState::NotApplicable
        );
        assert!(
            plan.node(&AnalysisNodeId::new("lyrics.align"))
                .unwrap()
                .will_run
        );
    }

    #[test]
    fn missing_model_blocks_the_node_and_plan_building_never_installs_anything() {
        // "never installs anything" here means: build_plan is a pure
        // function over its inputs (no ambient installer/API to call at
        // all yet) -- verified structurally by this test compiling and
        // running without any model-setup dependency, plus the explicit
        // Blocked-state assertion below.
        let graph = baseline_graph_spec();
        let mut req = request(&["pitch.extract"], LyricsRoute::WhisperAsr);
        req.model_availability
            .insert(AnalysisNodeId::new("pitch.extract"), false);

        let plan = build_plan(&graph, &req).unwrap();
        let pitch = plan.node(&AnalysisNodeId::new("pitch.extract")).unwrap();
        assert_eq!(pitch.state, NodeState::Blocked);
        assert!(!pitch.will_run);
        assert!(
            pitch
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("not installed")
        );
    }

    #[test]
    fn unknown_target_for_the_selected_route_is_rejected() {
        let graph = baseline_graph_spec();
        // lyrics.align is not in the TimedLrc route's universe.
        let err =
            build_plan(&graph, &request(&["lyrics.align"], LyricsRoute::TimedLrc)).unwrap_err();
        assert_eq!(
            err,
            PlanError::UnknownTarget(AnalysisNodeId::new("lyrics.align"))
        );
    }

    #[test]
    fn unused_stem_nodes_are_not_applicable_and_do_not_run() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::WhisperAsr),
        )
        .unwrap();
        for unused in [
            "vocals.denoise",
            "vocals.dereverb",
            "stems.instrumental",
            "stems.karaoke",
            "stems.multistem",
        ] {
            let node = plan.node(&AnalysisNodeId::new(unused)).unwrap();
            assert_eq!(node.state, NodeState::NotApplicable, "{unused}");
            assert!(!node.will_run, "{unused}");
        }
        assert!(
            plan.node(&AnalysisNodeId::new("stems.vocals"))
                .unwrap()
                .will_run
        );
        assert!(
            plan.node(&AnalysisNodeId::new("stems.bind_analysis_outputs"))
                .unwrap()
                .will_run
        );
    }

    #[test]
    fn selected_cleanup_and_accompaniment_are_required_for_pitch() {
        let graph = baseline_graph_spec();
        let mut req = request(&["pitch.extract"], LyricsRoute::WhisperAsr);
        req.active_stem_nodes = [
            "stems.vocals",
            "vocals.denoise",
            "vocals.dereverb",
            "stems.instrumental",
            "stems.bind_analysis_outputs",
        ]
        .into_iter()
        .map(AnalysisNodeId::new)
        .collect();
        let plan = build_plan(&graph, &req).unwrap();
        for expected in [
            "stems.vocals",
            "vocals.denoise",
            "vocals.dereverb",
            "stems.instrumental",
            "stems.bind_analysis_outputs",
            "pitch.extract",
        ] {
            assert!(
                plan.node(&AnalysisNodeId::new(expected)).unwrap().will_run,
                "{expected} should run"
            );
        }
        assert_eq!(
            plan.node(&AnalysisNodeId::new("stems.karaoke"))
                .unwrap()
                .state,
            NodeState::NotApplicable
        );
    }

    #[test]
    fn plan_is_serializable_for_history_snapshots() {
        let graph = baseline_graph_spec();
        let plan = build_plan(
            &graph,
            &request(&["chart.build_candidate"], LyricsRoute::WhisperAsr),
        )
        .unwrap();
        let json = serde_json::to_string(&plan).expect("serialize plan");
        let restored: AnalysisPlan = serde_json::from_str(&json).expect("deserialize plan");
        assert_eq!(restored.nodes.len(), plan.nodes.len());
    }
}
