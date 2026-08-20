//! `GraphViewModel`: the single source of truth the DAG canvas renders
//! from (docs/analysis-dag-redesign.md Phase 7 §7.1, phase plan
//! "AnalysisGraphSpec + AnalysisPlan + AnalysisRun + NodeAttempts +
//! ArtifactInventory -> GraphViewModel. UI 不再自行读取 cache 文件猜状态").
//! Pure: every input is a plain value or closure the caller already
//! computed, so this module never touches the filesystem, the DB, or Bevy
//! -- it can be (and is) unit-tested without any of the app's real state.
//!
//! `NodeState`'s *planned* values (`NotApplicable`/`Disabled`/`Blocked`/
//! `Frozen`) come straight from Phase 1's `AnalysisPlan` and always win.
//! Everything else is a *run-time* read derived the same way the existing
//! 7-bucket progress UI already computes per-bucket completion
//! (`stage_complete` in `analysis.rs`) -- this module doesn't invent a new
//! execution-state source, it just gives every graph node (not only the 7
//! buckets) a state by mapping each node onto its bucket via the existing
//! `analysis_node_stage_index` bridge.

use std::collections::BTreeSet;

use app_core::{AnalysisGraphSpec, AnalysisNodeId, AnalysisPlan, ArtifactKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphNodeState {
    NotApplicable,
    Disabled,
    Blocked,
    Frozen,
    Waiting,
    Running,
    Complete,
    /// §7's "GraphNodeState has no Failed/Stale variant" gap, closed:
    /// `resolve_node_state` reaches this from a real `NodeState::Failed`
    /// (`overlay_failed_node_attempts` in `desktop/src/studio/analysis.rs`,
    /// itself reading real `analysis_node_attempts` rows).
    Failed,
    /// Reached from a real `NodeState::Stale`
    /// (`overlay_stale_candidate_chart` in `desktop/src/studio/analysis.rs`,
    /// itself reading `app_core::candidate_chart_status` -- Phase 5's
    /// mtime-based staleness comparison between the Authored Chart and the
    /// analyzer outputs it was built from). Only ever applied to
    /// `chart.build_candidate` today: that's the one node whose output a
    /// Candidate/Authored distinction actually exists for.
    Stale,
    /// Phase 4 §4.5 Bypass: reached from a real `NodeState::Bypassed`
    /// (`analysis_plan::build_plan`, when the request's `bypassed_nodes`
    /// includes this node -- today only ever `stems.separate`, routed
    /// around with the Original Mix). Distinct from `Frozen`: a bypassed
    /// node never ran and has no output of its own being reused, it's
    /// substituted entirely.
    Bypassed,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphNodeView {
    pub(crate) id: AnalysisNodeId,
    pub(crate) label: String,
    pub(crate) state: GraphNodeState,
    /// Non-zero only when this is a collapsed compound node -- how many
    /// children `Expand` would reveal. A node is compound iff this can be
    /// non-zero; there's no separate `is_compound` flag on this view (the
    /// Node Context Menu's expand/collapse toggle looks that up directly
    /// from `AnalysisGraphSpec` instead, via
    /// `analysis_node_compound_toggle_action` in `desktop/src/studio/
    /// analysis.rs`, since it needs the answer independent of this node's
    /// current expand state -- `collapsed_child_count` alone can't tell an
    /// already-expanded compound node apart from a plain one, both read 0).
    pub(crate) collapsed_child_count: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphViewModel {
    pub(crate) nodes: Vec<GraphNodeView>,
}

impl GraphViewModel {
    pub(crate) fn node(&self, id: &AnalysisNodeId) -> Option<&GraphNodeView> {
        self.nodes.iter().find(|n| &n.id == id)
    }
}

fn compound_child_ids(graph: &AnalysisGraphSpec) -> BTreeSet<AnalysisNodeId> {
    graph
        .nodes
        .iter()
        .flat_map(|n| n.compound_children.iter().cloned())
        .collect()
}

/// Blends a Phase 1 plan state (if any) with the existing bucket-based
/// run-time completion signal. Plan states that mean "this node is out of
/// scope or cannot run" always take priority over a stale/default
/// run-time read.
pub(crate) fn resolve_node_state(
    planned_state: Option<app_core::NodeState>,
    bucket: Option<usize>,
    current_stage_index: usize,
    is_live_node: bool,
    stage_complete: &dyn Fn(usize) -> bool,
) -> GraphNodeState {
    match planned_state {
        Some(app_core::NodeState::NotApplicable) => return GraphNodeState::NotApplicable,
        Some(app_core::NodeState::Disabled) => return GraphNodeState::Disabled,
        Some(app_core::NodeState::Blocked) => return GraphNodeState::Blocked,
        Some(app_core::NodeState::Frozen) => return GraphNodeState::Frozen,
        Some(app_core::NodeState::Failed) => return GraphNodeState::Failed,
        Some(app_core::NodeState::Stale) => return GraphNodeState::Stale,
        Some(app_core::NodeState::Bypassed) => return GraphNodeState::Bypassed,
        _ => {}
    }
    if is_live_node {
        return GraphNodeState::Running;
    }
    match bucket {
        Some(bucket) if stage_complete(bucket) => GraphNodeState::Complete,
        Some(bucket) if bucket == current_stage_index => GraphNodeState::Running,
        _ => GraphNodeState::Waiting,
    }
}

/// Builds the full per-node view, filtering out compound children whose
/// parent is not in `expanded` (docs/analysis-dag-redesign.md Phase 7
/// §7.3's "Music Analysis 支持展开"). A child's parent is found by scanning
/// `compound_children`, not a stored back-reference, since
/// `AnalysisGraphSpec` only records the parent -> children direction.
pub(crate) fn build_graph_view_model(
    graph: &AnalysisGraphSpec,
    plan: Option<&AnalysisPlan>,
    live_node_id: Option<&str>,
    current_stage_index: usize,
    expanded: &BTreeSet<AnalysisNodeId>,
    node_bucket: &dyn Fn(&str) -> Option<usize>,
    stage_complete: &dyn Fn(usize) -> bool,
) -> GraphViewModel {
    let child_ids = compound_child_ids(graph);
    let mut nodes = Vec::new();
    for node in &graph.nodes {
        if node.id.as_str() == "stems.separate" && expanded.contains(&node.id) {
            // Children are the real work; hide the compat shell in Full view.
            continue;
        }
        if node.id.as_str() == "stems.bind_analysis_outputs" {
            // Alias only: picks which already-produced vocal/instrumental
            // file pitch and lyrics will read. Not a processing step.
            continue;
        }
        if child_ids.contains(&node.id) {
            let parent_expanded = graph.nodes.iter().any(|parent| {
                parent.compound_children.contains(&node.id) && expanded.contains(&parent.id)
            });
            if !parent_expanded {
                continue;
            }
        }
        let planned_state = plan.and_then(|p| p.node(&node.id)).map(|n| n.state);
        if matches!(planned_state, Some(app_core::NodeState::NotApplicable)) {
            continue;
        }
        let bucket = node_bucket(node.id.as_str());
        let is_live_node = live_node_id == Some(node.id.as_str());
        let state = resolve_node_state(
            planned_state,
            bucket,
            current_stage_index,
            is_live_node,
            stage_complete,
        );
        let collapsed_child_count = if node.is_compound() && !expanded.contains(&node.id) {
            node.compound_children.len()
        } else {
            0
        };
        nodes.push(GraphNodeView {
            id: node.id.clone(),
            label: node.label.clone(),
            state,
            collapsed_child_count,
        });
    }
    GraphViewModel { nodes }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderNodeKind {
    Compute,
    Artifact,
    Export,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderNode {
    pub(crate) id: AnalysisNodeId,
    pub(crate) kind: RenderNodeKind,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) state: GraphNodeState,
    pub(crate) collapsed_child_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderEdgeRole {
    ComputeDependency,
    ArtifactOutput,
    ExportTarget,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderEdge {
    pub(crate) from: AnalysisNodeId,
    pub(crate) to: AnalysisNodeId,
    pub(crate) artifact_kind: Option<ArtifactKind>,
    pub(crate) role: RenderEdgeRole,
    pub(crate) producer_node: AnalysisNodeId,
}

impl RenderEdge {
    pub(crate) fn endpoints(&self) -> (AnalysisNodeId, AnalysisNodeId) {
        (self.from.clone(), self.to.clone())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RenderGraph {
    pub(crate) nodes: Vec<RenderNode>,
    pub(crate) edges: Vec<RenderEdge>,
}

impl RenderGraph {
    pub(crate) fn node(&self, id: &AnalysisNodeId) -> Option<&RenderNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    pub(crate) fn edge_pairs(&self) -> Vec<(AnalysisNodeId, AnalysisNodeId)> {
        self.edges.iter().map(RenderEdge::endpoints).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphLineageHighlight {
    pub(crate) emphasized_nodes: BTreeSet<AnalysisNodeId>,
    pub(crate) emphasized_edges: BTreeSet<(AnalysisNodeId, AnalysisNodeId)>,
    pub(crate) missing_gaps: Vec<String>,
}

impl GraphLineageHighlight {
    pub(crate) fn emphasizes_node(&self, id: &AnalysisNodeId) -> bool {
        self.emphasized_nodes.is_empty() || self.emphasized_nodes.contains(id)
    }

    pub(crate) fn emphasizes_edge(&self, from: &AnalysisNodeId, to: &AnalysisNodeId) -> bool {
        self.emphasized_edges.is_empty()
            || self.emphasized_edges.contains(&(from.clone(), to.clone()))
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.emphasized_nodes.is_empty()
    }
}

pub(crate) fn virtual_artifact_node_id(kind: ArtifactKind) -> Option<&'static str> {
    match kind {
        ArtifactKind::RawVocalStem => Some("artifact.raw_vocal"),
        ArtifactKind::DenoisedVocalStem => Some("artifact.denoised_vocal"),
        ArtifactKind::DereverbedVocalStem => Some("artifact.dereverbed_vocal"),
        ArtifactKind::AnalysisVocalStem | ArtifactKind::VocalStem => Some("artifact.vocal_stem"),
        ArtifactKind::HighQualityInstrumentalStem => Some("artifact.hq_instrumental"),
        ArtifactKind::KaraokeInstrumentalStem => Some("artifact.karaoke_stem"),
        ArtifactKind::InstrumentalStem => Some("artifact.instrumental_stem"),
        ArtifactKind::MusicAnalysis => Some("artifact.music_analysis"),
        ArtifactKind::PitchTrack => Some("artifact.note_guide"),
        ArtifactKind::RecognizedText => Some("artifact.lyrics"),
        ArtifactKind::LyricsInput => Some("artifact.lyrics_input"),
        ArtifactKind::TimedTranscript => Some("artifact.timed_lyrics"),
        ArtifactKind::AuthoredChart | ArtifactKind::CandidateChart => Some("artifact.chart"),
        _ => None,
    }
}

pub(crate) fn filter_render_graph_for_mini_view(render: RenderGraph) -> RenderGraph {
    let compute_ids: BTreeSet<AnalysisNodeId> = render
        .nodes
        .iter()
        .filter(|node| node.kind == RenderNodeKind::Compute)
        .map(|node| node.id.clone())
        .collect();
    let mut edges: Vec<RenderEdge> = render
        .edges
        .iter()
        .filter(|edge| compute_ids.contains(&edge.from) && compute_ids.contains(&edge.to))
        .cloned()
        .collect();
    for edge in &render.edges {
        if !compute_ids.contains(&edge.to) || compute_ids.contains(&edge.from) {
            continue;
        }
        for producer in compute_sources_of(&render, &edge.from, &compute_ids) {
            if edges
                .iter()
                .any(|existing| existing.from == producer && existing.to == edge.to)
            {
                continue;
            }
            edges.push(RenderEdge {
                from: producer.clone(),
                to: edge.to.clone(),
                artifact_kind: edge.artifact_kind,
                role: RenderEdgeRole::ComputeDependency,
                producer_node: producer,
            });
        }
    }
    RenderGraph {
        nodes: render
            .nodes
            .into_iter()
            .filter(|node| node.kind == RenderNodeKind::Compute)
            .collect(),
        edges,
    }
}

fn compute_sources_of(
    render: &RenderGraph,
    start: &AnalysisNodeId,
    compute_ids: &BTreeSet<AnalysisNodeId>,
) -> Vec<AnalysisNodeId> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.clone()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if compute_ids.contains(&id) {
            found.push(id);
            continue;
        }
        for edge in &render.edges {
            if edge.to == id {
                stack.push(edge.from.clone());
            }
        }
    }
    found
}

pub(crate) fn graph_lineage_highlight(
    render: &RenderGraph,
    lineage: &app_core::ArtifactLineage,
    scope: crate::studio::LineageScope,
    selected: &app_core::ArtifactRef,
) -> GraphLineageHighlight {
    let mut emphasized_nodes = BTreeSet::new();
    let mut missing_gaps = lineage.missing_revision_ids.clone();
    let include_upstream = matches!(
        scope,
        crate::studio::LineageScope::Upstream | crate::studio::LineageScope::Full
    );
    let include_downstream = matches!(
        scope,
        crate::studio::LineageScope::Downstream | crate::studio::LineageScope::Full
    );

    let map_revision =
        |kind: ArtifactKind, producer: &AnalysisNodeId, into: &mut BTreeSet<AnalysisNodeId>| {
            let producer_id = AnalysisNodeId::new(producer.as_str());
            if render.node(&producer_id).is_some() {
                into.insert(producer_id);
            }
            if let Some(virtual_id) = virtual_artifact_node_id(kind) {
                let virtual_id = AnalysisNodeId::new(virtual_id);
                if render.node(&virtual_id).is_some() {
                    into.insert(virtual_id);
                }
            }
        };

    if include_upstream {
        for node in &lineage.nodes {
            map_revision(
                node.artifact.kind,
                &node.artifact.producer_node,
                &mut emphasized_nodes,
            );
        }
    } else {
        map_revision(
            selected.kind,
            &AnalysisNodeId::new(""),
            &mut emphasized_nodes,
        );
        if let Some(root) = lineage.nodes.first() {
            map_revision(
                root.artifact.kind,
                &root.artifact.producer_node,
                &mut emphasized_nodes,
            );
        }
    }
    if include_downstream {
        for consumer in &lineage.downstream_consumers {
            if render.node(consumer).is_some() {
                emphasized_nodes.insert(consumer.clone());
            }
        }
        if selected.kind == ArtifactKind::AuthoredChart
            || selected.kind == ArtifactKind::CandidateChart
        {
            for export_id in ["export.utz", "export.ultrastar"] {
                let export_id = AnalysisNodeId::new(export_id);
                if render.node(&export_id).is_some() {
                    emphasized_nodes.insert(export_id);
                }
            }
        }
    }

    if emphasized_nodes.is_empty() {
        missing_gaps.extend(lineage.missing_revision_ids.iter().cloned());
    }

    let emphasized_edges = render
        .edges
        .iter()
        .filter(|edge| emphasized_nodes.contains(&edge.from) && emphasized_nodes.contains(&edge.to))
        .map(RenderEdge::endpoints)
        .collect();

    GraphLineageHighlight {
        emphasized_nodes,
        emphasized_edges,
        missing_gaps,
    }
}

fn infer_compute_edge_kind(
    graph: &AnalysisGraphSpec,
    from: &AnalysisNodeId,
    to: &AnalysisNodeId,
) -> Option<ArtifactKind> {
    let from_node = graph.node(from)?;
    let to_node = graph.node(to)?;
    from_node
        .outputs
        .iter()
        .find(|kind| to_node.inputs.contains(kind))
        .copied()
}

fn compound_parent_id(graph: &AnalysisGraphSpec, id: &AnalysisNodeId) -> Option<AnalysisNodeId> {
    graph
        .nodes
        .iter()
        .find(|parent| parent.compound_children.contains(id))
        .map(|parent| parent.id.clone())
}

fn visible_or_parent(
    graph: &AnalysisGraphSpec,
    view: &GraphViewModel,
    id: &AnalysisNodeId,
) -> Option<AnalysisNodeId> {
    if view.node(id).is_some() {
        return Some(id.clone());
    }
    let parent = compound_parent_id(graph, id)?;
    view.node(&parent).map(|_| parent)
}

fn promoted_compute_edges(
    graph: &AnalysisGraphSpec,
    view: &GraphViewModel,
) -> Vec<(AnalysisNodeId, AnalysisNodeId)> {
    let mut edges = BTreeSet::new();
    for edge in &graph.edges {
        let Some(from) = visible_or_parent(graph, view, &edge.from) else {
            continue;
        };
        let Some(to) = visible_or_parent(graph, view, &edge.to) else {
            continue;
        };
        if from != to {
            edges.insert((from, to));
        }
    }
    edges.into_iter().collect()
}

fn dropped_stem_alias_edge(
    view: &GraphViewModel,
    from: &AnalysisNodeId,
    to: &AnalysisNodeId,
) -> bool {
    if from.as_str() == "stems.vocals"
        && to.as_str() == "vocals.dereverb"
        && view.node(&AnalysisNodeId::new("vocals.denoise")).is_some()
    {
        return true;
    }
    matches!(
        (from.as_str(), to.as_str()),
        ("stems.vocals", "vocals.denoise")
            | ("stems.vocals", "vocals.dereverb")
            | ("vocals.denoise", "vocals.dereverb")
            | ("stems.vocals", "stems.bind_analysis_outputs")
            | ("vocals.denoise", "stems.bind_analysis_outputs")
            | ("vocals.dereverb", "stems.bind_analysis_outputs")
            | ("stems.instrumental", "stems.bind_analysis_outputs")
            | ("stems.bind_analysis_outputs", "pitch.extract")
            | ("stems.bind_analysis_outputs", "lyrics.preprocess")
            | ("stems.vocals", "pitch.extract")
            | ("stems.vocals", "lyrics.preprocess")
            | ("vocals.denoise", "pitch.extract")
            | ("vocals.denoise", "lyrics.preprocess")
            | ("vocals.dereverb", "pitch.extract")
            | ("vocals.dereverb", "lyrics.preprocess")
            | ("pitch.extract", "chart.build_candidate")
            | ("lyrics.align", "chart.build_candidate")
            | ("lyrics.import_timed", "chart.build_candidate")
            | ("lyrics.transcribe", "chart.build_candidate")
            | ("lyrics.transcribe", "lyrics.align")
            | ("preflight", "lyrics.import_timed")
    )
}

fn last_vocal_producer(view: &GraphViewModel) -> Option<AnalysisNodeId> {
    ["vocals.dereverb", "vocals.denoise", "stems.vocals"]
        .into_iter()
        .map(AnalysisNodeId::new)
        .find(|id| view.node(id).is_some())
}

fn last_vocal_artifact_id(view: &GraphViewModel) -> Option<&'static str> {
    match last_vocal_producer(view)?.as_str() {
        "vocals.dereverb" => Some("artifact.dereverbed_vocal"),
        "vocals.denoise" => Some("artifact.denoised_vocal"),
        "stems.vocals" => Some("artifact.raw_vocal"),
        _ => None,
    }
}

struct OnPathArtifactSpec<'a> {
    id: &'a str,
    label: &'a str,
    detail: &'a str,
    producer: &'a str,
    consumer: Option<&'a str>,
    kind: ArtifactKind,
}

fn push_on_path_artifact(
    spec: OnPathArtifactSpec,
    view: &GraphViewModel,
    artifact_present: &dyn Fn(ArtifactKind) -> bool,
    nodes: &mut Vec<RenderNode>,
    edges: &mut Vec<RenderEdge>,
) {
    let OnPathArtifactSpec {
        id,
        label,
        detail,
        producer,
        consumer,
        kind,
    } = spec;
    let producer_id = AnalysisNodeId::new(producer);
    if view.node(&producer_id).is_none() {
        return;
    }
    if !nodes.iter().any(|node| node.id.as_str() == id) {
        let state =
            artifact_ready_state(upstream_state(view, &producer_id), artifact_present(kind));
        nodes.push(RenderNode {
            id: AnalysisNodeId::new(id),
            kind: RenderNodeKind::Artifact,
            label: label.to_string(),
            detail: detail.to_string(),
            state,
            collapsed_child_count: 0,
        });
        edges.push(RenderEdge {
            from: producer_id.clone(),
            to: AnalysisNodeId::new(id),
            artifact_kind: Some(kind),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: producer_id.clone(),
        });
    }
    if let Some(consumer) = consumer {
        let consumer_id = AnalysisNodeId::new(consumer);
        if view.node(&consumer_id).is_some()
            || nodes.iter().any(|node| node.id.as_str() == consumer)
        {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new(id),
                to: consumer_id,
                artifact_kind: Some(kind),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: producer_id,
            });
        }
    }
}

fn push_vocal_product_chain(
    view: &GraphViewModel,
    artifact_present: &dyn Fn(ArtifactKind) -> bool,
    nodes: &mut Vec<RenderNode>,
    edges: &mut Vec<RenderEdge>,
) {
    if view.node(&AnalysisNodeId::new("stems.vocals")).is_none() {
        return;
    }
    let next_after_extract = if view.node(&AnalysisNodeId::new("vocals.denoise")).is_some() {
        Some("vocals.denoise")
    } else if view.node(&AnalysisNodeId::new("vocals.dereverb")).is_some() {
        Some("vocals.dereverb")
    } else {
        None
    };
    let (extract_name, extract_kind) = if next_after_extract.is_some() {
        ("vocals_raw.flac", ArtifactKind::RawVocalStem)
    } else {
        ("vocals.flac", ArtifactKind::VocalStem)
    };
    push_on_path_artifact(
        OnPathArtifactSpec {
            id: "artifact.raw_vocal",
            label: extract_name,
            detail: "extracted vocal · lossless",
            producer: "stems.vocals",
            consumer: next_after_extract,
            kind: extract_kind,
        },
        view,
        artifact_present,
        nodes,
        edges,
    );
    if view.node(&AnalysisNodeId::new("vocals.denoise")).is_some() {
        let next_after_denoise = if view.node(&AnalysisNodeId::new("vocals.dereverb")).is_some() {
            Some("vocals.dereverb")
        } else {
            None
        };
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.denoised_vocal",
                label: "vocals_denoised.flac",
                detail: "denoised vocal · lossless",
                producer: "vocals.denoise",
                consumer: next_after_denoise,
                kind: ArtifactKind::DenoisedVocalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
    }
    if view.node(&AnalysisNodeId::new("vocals.dereverb")).is_some() {
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.dereverbed_vocal",
                label: "vocals_dry.flac",
                detail: "dereverbed vocal · lossless",
                producer: "vocals.dereverb",
                consumer: None,
                kind: ArtifactKind::DereverbedVocalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
    }
    if let Some(artifact_id) = last_vocal_artifact_id(view) {
        for consumer in ["pitch.extract", "lyrics.preprocess"] {
            if view.node(&AnalysisNodeId::new(consumer)).is_some() {
                let producer = last_vocal_producer(view).expect("artifact implies producer");
                edges.push(RenderEdge {
                    from: AnalysisNodeId::new(artifact_id),
                    to: AnalysisNodeId::new(consumer),
                    artifact_kind: Some(ArtifactKind::VocalStem),
                    role: RenderEdgeRole::ArtifactOutput,
                    producer_node: producer,
                });
            }
        }
    }
    if view
        .node(&AnalysisNodeId::new("stems.instrumental"))
        .is_some()
    {
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.hq_instrumental",
                label: "instrumental.flac",
                detail: "high-quality accompaniment · lossless",
                producer: "stems.instrumental",
                consumer: None,
                kind: ArtifactKind::HighQualityInstrumentalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
    }
    if view.node(&AnalysisNodeId::new("stems.karaoke")).is_some() {
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.karaoke_stem",
                label: "instrumental_karaoke.flac",
                detail: "side path · not used for charting",
                producer: "stems.karaoke",
                consumer: None,
                kind: ArtifactKind::KaraokeInstrumentalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
    }
}

/// Rolls a compound node's collapsed-vs-expanded children into one
/// worst-case state for the virtual artifact/export nodes downstream of it
/// -- e.g. "Vocal stem" (downstream of `stems.separate`) is only ready once
/// `stems.separate` itself reports `Complete`.
fn upstream_state(view: &GraphViewModel, id: &AnalysisNodeId) -> GraphNodeState {
    view.node(id)
        .map(|n| n.state)
        .unwrap_or(GraphNodeState::Waiting)
}

fn artifact_ready_state(upstream: GraphNodeState, artifact_present: bool) -> GraphNodeState {
    match upstream {
        GraphNodeState::NotApplicable => GraphNodeState::NotApplicable,
        GraphNodeState::Disabled => GraphNodeState::Disabled,
        GraphNodeState::Blocked => GraphNodeState::Blocked,
        _ if artifact_present => GraphNodeState::Complete,
        GraphNodeState::Running => GraphNodeState::Running,
        _ => GraphNodeState::Waiting,
    }
}

/// Extends the real compute-node view model with the virtual
/// artifact/export boxes docs/analysis-dag-redesign.md Phase 7 §7.3's
/// suggested main graph structure calls for ("Vocal Stem", "Export UTZ",
/// ...) -- none of these are real `AnalysisGraphSpec` nodes (a node's
/// `outputs: Vec<ArtifactKind>` is data, not a separate graph node), so the
/// UI has always had to synthesize them for display. This makes that
/// synthesis one explicit, tested function instead of the hand-placed
/// boxes it replaces. Readiness for each virtual node comes from real
/// on-disk artifact presence (`ArtifactSummary`), not a stage-progress
/// guess -- strictly more accurate than what it replaces, which only had
/// the progress heuristic to go on.
pub(crate) fn build_render_graph(
    graph: &AnalysisGraphSpec,
    view: &GraphViewModel,
    artifact_present: &dyn Fn(app_core::ArtifactKind) -> bool,
) -> RenderGraph {
    let mut nodes: Vec<RenderNode> = view
        .nodes
        .iter()
        .map(|n| RenderNode {
            id: n.id.clone(),
            kind: RenderNodeKind::Compute,
            label: n.label.clone(),
            detail: String::new(),
            state: n.state,
            collapsed_child_count: n.collapsed_child_count,
        })
        .collect();
    // The real compute-node dependency edges (preflight -> stems.separate,
    // stems.separate -> pitch.extract, ...) -- without these every compute
    // node has no incoming edge among its peers and the layout algorithm
    // correctly, but uselessly, ranks all of them into the same leftmost
    // column. Only kept when both endpoints are actually in `view` (a
    // collapsed compound child's edges to its parent are not drawn, same
    // as the child itself not being drawn).
    let mut edges: Vec<RenderEdge> = promoted_compute_edges(graph, view)
        .into_iter()
        .filter(|(from, to)| !dropped_stem_alias_edge(view, from, to))
        .map(|(from, to)| RenderEdge {
            artifact_kind: infer_compute_edge_kind(graph, &from, &to),
            role: RenderEdgeRole::ComputeDependency,
            producer_node: from.clone(),
            from,
            to,
        })
        .collect();
    push_vocal_product_chain(view, artifact_present, &mut nodes, &mut edges);

    let push_artifact = |id: &str,
                         label: &str,
                         detail: &str,
                         upstream_id: &str,
                         kind: app_core::ArtifactKind,
                         nodes: &mut Vec<RenderNode>,
                         edges: &mut Vec<RenderEdge>| {
        let upstream = AnalysisNodeId::new(upstream_id);
        if view.node(&upstream).is_none() {
            // The upstream compute node isn't in the (possibly filtered)
            // view -- nothing to attach this artifact to.
            return;
        }
        let state = artifact_ready_state(upstream_state(view, &upstream), artifact_present(kind));
        let artifact_id = AnalysisNodeId::new(id);
        nodes.push(RenderNode {
            id: artifact_id.clone(),
            kind: RenderNodeKind::Artifact,
            label: label.to_string(),
            detail: detail.to_string(),
            state,
            collapsed_child_count: 0,
        });
        edges.push(RenderEdge {
            from: upstream.clone(),
            to: artifact_id,
            artifact_kind: Some(kind),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: upstream,
        });
    };

    if view.node(&AnalysisNodeId::new("stems.separate")).is_some() {
        push_artifact(
            "artifact.vocal_stem",
            "Vocal stem",
            "vocals.flac · lossless",
            "stems.separate",
            app_core::ArtifactKind::VocalStem,
            &mut nodes,
            &mut edges,
        );
        push_artifact(
            "artifact.instrumental_stem",
            "Instrumental stem",
            "instrumental.flac · lossless",
            "stems.separate",
            app_core::ArtifactKind::InstrumentalStem,
            &mut nodes,
            &mut edges,
        );
    }
    if view.node(&AnalysisNodeId::new("music.analysis")).is_some() {
        push_artifact(
            "artifact.music_analysis",
            "Music analysis",
            "key · BPM · descriptors",
            "music.analysis",
            app_core::ArtifactKind::MusicAnalysis,
            &mut nodes,
            &mut edges,
        );
    }
    push_artifact(
        "artifact.note_guide",
        "Note guide",
        "Pitch contour + notes",
        "pitch.extract",
        app_core::ArtifactKind::PitchTrack,
        &mut nodes,
        &mut edges,
    );
    if view.node(&AnalysisNodeId::new("pitch.extract")).is_some()
        && view
            .node(&AnalysisNodeId::new("chart.build_candidate"))
            .is_some()
        && nodes
            .iter()
            .any(|node| node.id.as_str() == "artifact.note_guide")
    {
        edges.push(RenderEdge {
            from: AnalysisNodeId::new("artifact.note_guide"),
            to: AnalysisNodeId::new("chart.build_candidate"),
            artifact_kind: Some(app_core::ArtifactKind::PitchNoteCandidates),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: AnalysisNodeId::new("pitch.extract"),
        });
    }
    if view
        .node(&AnalysisNodeId::new("lyrics.transcribe"))
        .is_some()
    {
        push_artifact(
            "artifact.lyrics",
            "Lyrics",
            "text · no timing",
            "lyrics.transcribe",
            app_core::ArtifactKind::RecognizedText,
            &mut nodes,
            &mut edges,
        );
        if view.node(&AnalysisNodeId::new("lyrics.align")).is_some()
            && nodes
                .iter()
                .any(|node| node.id.as_str() == "artifact.lyrics")
        {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("artifact.lyrics"),
                to: AnalysisNodeId::new("lyrics.align"),
                artifact_kind: Some(app_core::ArtifactKind::RecognizedText),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: AnalysisNodeId::new("lyrics.transcribe"),
            });
        }
    }
    let align_needs_provided_lyrics = view.node(&AnalysisNodeId::new("lyrics.align")).is_some()
        && view
            .node(&AnalysisNodeId::new("lyrics.transcribe"))
            .is_none();
    let import_needs_provided_lyrics = view
        .node(&AnalysisNodeId::new("lyrics.import_timed"))
        .is_some();
    if (align_needs_provided_lyrics || import_needs_provided_lyrics)
        && view.node(&AnalysisNodeId::new("preflight")).is_some()
    {
        let (label, detail) = if import_needs_provided_lyrics && !align_needs_provided_lyrics {
            ("Lyrics file", "timed LRC · provided")
        } else {
            ("Lyrics file", "plain · provided")
        };
        let file_id = AnalysisNodeId::new("artifact.lyrics_input");
        if !nodes.iter().any(|node| node.id == file_id) {
            let state = if artifact_present(app_core::ArtifactKind::LyricsInput) {
                GraphNodeState::Complete
            } else {
                GraphNodeState::Waiting
            };
            nodes.push(RenderNode {
                id: file_id.clone(),
                kind: RenderNodeKind::Artifact,
                label: label.to_string(),
                detail: detail.to_string(),
                state,
                collapsed_child_count: 0,
            });
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("preflight"),
                to: file_id.clone(),
                artifact_kind: Some(app_core::ArtifactKind::LyricsInput),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: AnalysisNodeId::new("preflight"),
            });
        }
        if align_needs_provided_lyrics {
            edges.push(RenderEdge {
                from: file_id.clone(),
                to: AnalysisNodeId::new("lyrics.align"),
                artifact_kind: Some(app_core::ArtifactKind::LyricsInput),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: AnalysisNodeId::new("preflight"),
            });
        }
        if import_needs_provided_lyrics {
            edges.push(RenderEdge {
                from: file_id,
                to: AnalysisNodeId::new("lyrics.import_timed"),
                artifact_kind: Some(app_core::ArtifactKind::LyricsInput),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: AnalysisNodeId::new("preflight"),
            });
        }
    }
    // Timed lyrics only accepts the previous layer's timing producers:
    // Known Lyrics/Whisper end at lyrics.align, Timed LRC at
    // lyrics.import_timed. lyrics.transcribe is one layer further up and
    // already feeds lyrics.align, so drawing it here would skip a layer
    // and, after longest-path ranking, sit on the same skip rail as stem
    // outputs. Parakeet (no align/import in the view) is the one case
    // where transcribe is the immediate producer.
    let timing_layer = ["lyrics.align", "lyrics.import_timed"]
        .into_iter()
        .map(AnalysisNodeId::new)
        .filter_map(|source_id| view.node(&source_id).map(|n| (source_id, n.state)))
        .collect::<Vec<_>>();
    let timed_lyrics_upstreams: Vec<(AnalysisNodeId, GraphNodeState)> = if !timing_layer.is_empty()
    {
        timing_layer
    } else {
        ["lyrics.transcribe"]
            .into_iter()
            .map(AnalysisNodeId::new)
            .filter_map(|source_id| view.node(&source_id).map(|n| (source_id, n.state)))
            .collect()
    };
    if !timed_lyrics_upstreams.is_empty() {
        let best_upstream_state = timed_lyrics_upstreams
            .iter()
            .map(|(_, state)| *state)
            .max_by_key(graph_node_state_rank)
            .expect("non-empty");
        let state = artifact_ready_state(
            best_upstream_state,
            artifact_present(app_core::ArtifactKind::TimedTranscript),
        );
        let timed_lyrics_id = AnalysisNodeId::new("artifact.timed_lyrics");
        nodes.push(RenderNode {
            id: timed_lyrics_id.clone(),
            kind: RenderNodeKind::Artifact,
            label: "Timed lyrics".to_string(),
            detail: "text · with timing".to_string(),
            state,
            collapsed_child_count: 0,
        });
        for (source_id, _) in &timed_lyrics_upstreams {
            edges.push(RenderEdge {
                from: source_id.clone(),
                to: timed_lyrics_id.clone(),
                artifact_kind: Some(app_core::ArtifactKind::TimedTranscript),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: source_id.clone(),
            });
        }
        if view
            .node(&AnalysisNodeId::new("chart.build_candidate"))
            .is_some()
        {
            let producer = timed_lyrics_upstreams[0].0.clone();
            edges.push(RenderEdge {
                from: timed_lyrics_id,
                to: AnalysisNodeId::new("chart.build_candidate"),
                artifact_kind: Some(app_core::ArtifactKind::TimedTranscript),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: producer,
            });
        }
    }

    push_artifact(
        "artifact.chart",
        "Editable chart",
        "Authoring-ready assets",
        "chart.build_candidate",
        app_core::ArtifactKind::AuthoredChart,
        &mut nodes,
        &mut edges,
    );

    if view
        .node(&AnalysisNodeId::new("chart.build_candidate"))
        .is_some()
    {
        let chart_state = artifact_ready_state(
            upstream_state(view, &AnalysisNodeId::new("chart.build_candidate")),
            artifact_present(app_core::ArtifactKind::AuthoredChart),
        );
        for (export_id, label) in [
            ("export.utz", "UTZ package"),
            ("export.ultrastar", "UltraStar chart"),
        ] {
            let export_id = AnalysisNodeId::new(export_id);
            nodes.push(RenderNode {
                id: export_id.clone(),
                kind: RenderNodeKind::Export,
                label: label.to_string(),
                detail: "Explicit export target".to_string(),
                state: chart_state,
                collapsed_child_count: 0,
            });
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("artifact.chart"),
                to: export_id,
                artifact_kind: Some(app_core::ArtifactKind::AuthoredChart),
                role: RenderEdgeRole::ExportTarget,
                producer_node: AnalysisNodeId::new("chart.build_candidate"),
            });
        }
    }

    RenderGraph { nodes, edges }
}

fn graph_node_state_rank(state: &GraphNodeState) -> u8 {
    match state {
        GraphNodeState::NotApplicable => 0,
        GraphNodeState::Disabled => 1,
        GraphNodeState::Blocked => 2,
        // A failed route genuinely ran (unlike NotApplicable/Disabled/
        // Blocked, which never attempt to) and produced a definitive
        // outcome, so it outranks those -- but it's not "still might
        // succeed" the way Waiting is, so it ranks below that.
        GraphNodeState::Failed => 3,
        GraphNodeState::Waiting => 4,
        // Bypassed and Frozen are the same tier for this purpose: both mean
        // "this node's input is satisfied without running it," just via a
        // different mechanism (substitute input vs. reused own output).
        GraphNodeState::Frozen => 5,
        GraphNodeState::Bypassed => 5,
        GraphNodeState::Running => 6,
        GraphNodeState::Complete => 7,
        // Stale means the node genuinely completed *and* carries a real,
        // current signal the plain-Complete case doesn't (a newer candidate
        // differs from what's authored) -- strictly more informative than
        // Complete, so it outranks it.
        GraphNodeState::Stale => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> AnalysisNodeId {
        AnalysisNodeId::new(s)
    }

    fn no_bucket(_: &str) -> Option<usize> {
        None
    }

    fn always_incomplete(_: usize) -> bool {
        false
    }

    #[test]
    fn a_failed_planned_state_wins_over_any_bucket_based_read() {
        // §7's "GraphNodeState has no Failed variant" gap, closed:
        // `overlay_failed_node_attempts` (desktop/src/studio/analysis.rs)
        // feeds a real `NodeState::Failed` in here; it must win even when
        // the bucket-based signal would otherwise say Complete or Running.
        let state =
            resolve_node_state(Some(app_core::NodeState::Failed), Some(0), 0, true, &|_| {
                true
            });
        assert_eq!(state, GraphNodeState::Failed);
    }

    #[test]
    fn a_failed_node_ranks_above_blocked_disabled_and_not_applicable_but_below_waiting() {
        assert!(
            graph_node_state_rank(&GraphNodeState::Failed)
                > graph_node_state_rank(&GraphNodeState::Blocked)
        );
        assert!(
            graph_node_state_rank(&GraphNodeState::Failed)
                > graph_node_state_rank(&GraphNodeState::Disabled)
        );
        assert!(
            graph_node_state_rank(&GraphNodeState::Failed)
                > graph_node_state_rank(&GraphNodeState::NotApplicable)
        );
        assert!(
            graph_node_state_rank(&GraphNodeState::Failed)
                < graph_node_state_rank(&GraphNodeState::Waiting)
        );
        assert!(
            graph_node_state_rank(&GraphNodeState::Failed)
                < graph_node_state_rank(&GraphNodeState::Complete)
        );
    }

    #[test]
    fn planned_not_applicable_wins_even_if_the_bucket_looks_complete() {
        let state = resolve_node_state(
            Some(app_core::NodeState::NotApplicable),
            Some(0),
            5,
            false,
            &|_| true,
        );
        assert_eq!(state, GraphNodeState::NotApplicable);
    }

    #[test]
    fn planned_blocked_wins_over_a_matching_live_node_id() {
        // Shouldn't happen in practice (a blocked node can't be the live
        // node), but the priority order must still hold defensively.
        let state = resolve_node_state(
            Some(app_core::NodeState::Blocked),
            Some(2),
            2,
            true,
            &|_| false,
        );
        assert_eq!(state, GraphNodeState::Blocked);
    }

    #[test]
    fn live_node_id_match_reports_running_when_unplanned() {
        let state = resolve_node_state(None, Some(3), 3, true, &always_incomplete);
        assert_eq!(state, GraphNodeState::Running);
    }

    #[test]
    fn a_completed_bucket_reports_complete() {
        let state = resolve_node_state(None, Some(1), 4, false, &|bucket| bucket == 1);
        assert_eq!(state, GraphNodeState::Complete);
    }

    #[test]
    fn the_current_bucket_reports_running_even_without_a_node_id_match() {
        let state = resolve_node_state(None, Some(4), 4, false, &always_incomplete);
        assert_eq!(state, GraphNodeState::Running);
    }

    #[test]
    fn a_future_incomplete_bucket_reports_waiting() {
        let state = resolve_node_state(None, Some(6), 2, false, &always_incomplete);
        assert_eq!(state, GraphNodeState::Waiting);
    }

    #[test]
    fn a_node_with_no_known_bucket_reports_waiting_rather_than_panicking() {
        let state = resolve_node_state(None, None, 2, false, &always_incomplete);
        assert_eq!(state, GraphNodeState::Waiting);
    }

    #[test]
    fn collapsed_compound_children_are_hidden_from_the_view_model() {
        let graph = app_core::baseline_graph_spec();
        let expanded = BTreeSet::new();
        let view = build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &expanded,
            &no_bucket,
            &always_incomplete,
        );
        assert!(view.node(&id("music.analysis")).is_some());
        assert!(view.node(&id("music.key")).is_none());
        assert!(view.node(&id("music.rhythm")).is_none());
        assert!(view.node(&id("music.descriptors")).is_none());
        let parent = view.node(&id("music.analysis")).unwrap();
        assert_eq!(parent.collapsed_child_count, 3);
    }

    #[test]
    fn expanding_stems_separate_hides_the_shell_and_shows_children() {
        let graph = app_core::baseline_graph_spec();
        let mut expanded = BTreeSet::new();
        expanded.insert(id("stems.separate"));
        let view = build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &expanded,
            &no_bucket,
            &always_incomplete,
        );
        assert!(view.node(&id("stems.separate")).is_none());
        assert!(view.node(&id("stems.vocals")).is_some());
        assert!(view.node(&id("stems.bind_analysis_outputs")).is_none());
        assert!(view.node(&id("vocals.denoise")).is_some());
    }

    #[test]
    fn expanding_a_compound_node_reveals_its_children_with_zero_collapsed_count() {
        let graph = app_core::baseline_graph_spec();
        let mut expanded = BTreeSet::new();
        expanded.insert(id("music.analysis"));
        let view = build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &expanded,
            &no_bucket,
            &always_incomplete,
        );
        assert!(view.node(&id("music.key")).is_some());
        assert!(view.node(&id("music.rhythm")).is_some());
        assert!(view.node(&id("music.descriptors")).is_some());
        assert_eq!(
            view.node(&id("music.analysis"))
                .unwrap()
                .collapsed_child_count,
            0
        );
    }

    #[test]
    fn every_non_child_node_is_present_when_nothing_is_expanded() {
        let graph = app_core::baseline_graph_spec();
        let expanded = BTreeSet::new();
        let view = build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &expanded,
            &no_bucket,
            &always_incomplete,
        );
        let child_ids = compound_child_ids(&graph);
        let expected = graph.nodes.len() - child_ids.len();
        assert_eq!(view.nodes.len(), expected);
    }

    fn full_view_model(stage_complete: &dyn Fn(usize) -> bool) -> GraphViewModel {
        let graph = app_core::baseline_graph_spec();
        build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &BTreeSet::new(),
            &crate::studio::analysis_node_stage_index,
            stage_complete,
        )
    }

    fn expanded_stem_view(stage_complete: &dyn Fn(usize) -> bool) -> GraphViewModel {
        let graph = app_core::baseline_graph_spec();
        let mut expanded = BTreeSet::new();
        expanded.insert(id("stems.separate"));
        build_graph_view_model(
            &graph,
            None,
            None,
            0,
            &expanded,
            &crate::studio::analysis_node_stage_index,
            stage_complete,
        )
    }

    #[test]
    fn every_compute_node_appears_in_the_render_graph() {
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        for node in &view.nodes {
            assert!(
                render.node(&node.id).is_some(),
                "compute node {} missing from render graph",
                node.id.as_str()
            );
        }
    }

    /// Regression test for a real bug this session's own live-app
    /// screenshot verification caught: `build_render_graph` used to only
    /// ever add the *virtual* artifact/export edges, never copying over
    /// the real compute-node dependency edges from the graph spec
    /// (`preflight -> stems.separate`, `stems.separate -> pitch.extract`,
    /// ...). With no edges between them, every compute node had no
    /// incoming edge among its peers, so the (correctly-tested-in-isolation)
    /// layered-layout algorithm ranked all of them into the same leftmost
    /// column, stacked vertically -- passed every unit test, since nothing
    /// asserted the real edges survived into the render graph, and only
    /// became visible as a wall of same-column boxes in an actual
    /// screenshot.
    #[test]
    fn every_real_compute_edge_from_the_graph_spec_survives_into_the_render_graph() {
        let graph = app_core::baseline_graph_spec();
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&graph, &view, &|_| false);
        for edge in &graph.edges {
            if view.node(&edge.from).is_none() || view.node(&edge.to).is_none() {
                continue;
            }
            if dropped_stem_alias_edge(&view, &edge.from, &edge.to) {
                continue;
            }
            assert!(
                render
                    .edges
                    .iter()
                    .any(|render_edge| render_edge.from == edge.from && render_edge.to == edge.to),
                "missing real compute edge {} -> {}",
                edge.from.as_str(),
                edge.to.as_str()
            );
        }
    }

    #[test]
    fn virtual_artifact_and_export_nodes_are_present() {
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        for expected in [
            "artifact.vocal_stem",
            "artifact.instrumental_stem",
            "artifact.note_guide",
            "artifact.lyrics",
            "artifact.timed_lyrics",
            "artifact.chart",
            "export.utz",
            "export.ultrastar",
        ] {
            assert!(
                render.node(&id(expected)).is_some(),
                "missing virtual node {expected}"
            );
        }
    }

    #[test]
    fn an_artifact_actually_present_on_disk_reports_complete_even_mid_run() {
        // stems.separate itself is only Waiting (nothing completed yet in
        // this synthetic run), but the vocal stem file is already on disk
        // -- e.g. a resumed/legacy song. The artifact box must reflect
        // reality, not just extrapolate from its upstream compute node.
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|kind| {
            kind == app_core::ArtifactKind::VocalStem
        });
        let vocal_stem = render.node(&id("artifact.vocal_stem")).unwrap();
        assert_eq!(vocal_stem.state, GraphNodeState::Complete);
        let instrumental = render.node(&id("artifact.instrumental_stem")).unwrap();
        assert_ne!(instrumental.state, GraphNodeState::Complete);
    }

    #[test]
    fn an_artifact_with_no_upstream_completion_and_no_file_is_waiting() {
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        let vocal_stem = render.node(&id("artifact.vocal_stem")).unwrap();
        assert_eq!(vocal_stem.state, GraphNodeState::Waiting);
    }

    #[test]
    fn timed_lyrics_is_fed_by_the_previous_timing_layer() {
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        let sources: Vec<&AnalysisNodeId> = render
            .edges
            .iter()
            .filter(|edge| edge.to == id("artifact.timed_lyrics"))
            .map(|edge| &edge.from)
            .collect();
        assert!(sources.contains(&&id("lyrics.align")));
        assert!(sources.contains(&&id("lyrics.import_timed")));
        assert!(
            !sources.contains(&&id("lyrics.transcribe")),
            "transcribe already feeds align; it must not skip a layer into Timed lyrics"
        );
        assert!(
            !sources.contains(&&id("stems.separate")),
            "stem separation does not produce timed lyrics"
        );
        let boxes = render
            .nodes
            .iter()
            .filter(|n| n.id == id("artifact.timed_lyrics"))
            .count();
        assert_eq!(boxes, 1);
        let has = |from: &str, to: &str| {
            render
                .edges
                .iter()
                .any(|edge| edge.from == id(from) && edge.to == id(to))
        };
        assert!(has("artifact.timed_lyrics", "chart.build_candidate"));
        assert!(!has("lyrics.align", "chart.build_candidate"));
        assert!(!has("lyrics.import_timed", "chart.build_candidate"));
        assert!(!has("lyrics.transcribe", "chart.build_candidate"));
        assert!(has("lyrics.transcribe", "artifact.lyrics"));
        assert!(has("artifact.lyrics", "lyrics.align"));
        assert!(!has("lyrics.transcribe", "lyrics.align"));
        assert!(!has("artifact.lyrics_input", "lyrics.align"));
        assert_eq!(render.node(&id("artifact.lyrics")).unwrap().label, "Lyrics");
        assert_eq!(
            render.node(&id("artifact.timed_lyrics")).unwrap().label,
            "Timed lyrics"
        );
    }

    #[test]
    fn mini_view_collapses_timed_lyrics_into_compute_edges() {
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&app_core::baseline_graph_spec(), &view, &|_| false);
        let mini = filter_render_graph_for_mini_view(render);
        let has = |from: &str, to: &str| {
            mini.edges
                .iter()
                .any(|edge| edge.from == id(from) && edge.to == id(to))
        };
        assert!(has("lyrics.align", "chart.build_candidate"));
        assert!(has("lyrics.import_timed", "chart.build_candidate"));
        assert!(!has("lyrics.transcribe", "chart.build_candidate"));
        assert!(has("lyrics.transcribe", "lyrics.align"));
    }

    #[test]
    fn known_lyrics_alignment_reads_original_lyrics() {
        let graph = app_core::baseline_graph_spec();
        let plan = app_core::build_plan(
            &graph,
            &app_core::AnalysisRequest {
                file_hash: "song".into(),
                targets: [id("chart.build_candidate")].into_iter().collect(),
                disabled_nodes: BTreeSet::new(),
                frozen_artifacts: BTreeSet::new(),
                bypassed_nodes: BTreeSet::new(),
                lyrics_route: app_core::LyricsRoute::KnownLyrics,
                model_availability: Default::default(),
                profile_snapshot: app_core::AnalysisProfileSnapshot::default(),
                active_stem_nodes: BTreeSet::new(),
            },
        )
        .unwrap();
        let view = build_graph_view_model(
            &graph,
            Some(&plan),
            None,
            0,
            &BTreeSet::new(),
            &no_bucket,
            &always_incomplete,
        );
        assert!(view.node(&id("lyrics.align")).is_some());
        assert!(view.node(&id("lyrics.transcribe")).is_none());
        let render = build_render_graph(&graph, &view, &|_| false);
        let has = |from: &str, to: &str| {
            render
                .edges
                .iter()
                .any(|edge| edge.from == id(from) && edge.to == id(to))
        };
        assert_eq!(
            render.node(&id("artifact.lyrics_input")).unwrap().label,
            "Lyrics file"
        );
        assert!(has("preflight", "artifact.lyrics_input"));
        assert!(has("artifact.lyrics_input", "lyrics.align"));
        assert!(render.node(&id("artifact.lyrics")).is_none());
        assert!(!has("lyrics.transcribe", "lyrics.align"));
    }

    #[test]
    fn every_edge_endpoint_resolves_to_a_real_node_in_the_render_graph() {
        // No dangling edges: every edge's `from` and `to` -- whether a real
        // compute node or a virtual artifact/export node -- must resolve
        // to something `build_render_graph` actually emitted.
        let view = full_view_model(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        for edge in &render.edges {
            assert!(
                render.node(&edge.from).is_some(),
                "edge references {} which isn't in the render graph",
                edge.from.as_str()
            );
            assert!(
                render.node(&edge.to).is_some(),
                "edge references {} which isn't in the render graph",
                edge.to.as_str()
            );
        }
    }

    #[test]
    fn export_targets_share_the_authored_chart_readiness() {
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&app_core::baseline_graph_spec(), &view, &|kind| {
            kind == app_core::ArtifactKind::AuthoredChart
        });
        let chart = render.node(&id("artifact.chart")).unwrap();
        let utz = render.node(&id("export.utz")).unwrap();
        let ultrastar = render.node(&id("export.ultrastar")).unwrap();
        assert_eq!(chart.state, GraphNodeState::Complete);
        assert_eq!(utz.state, GraphNodeState::Complete);
        assert_eq!(ultrastar.state, GraphNodeState::Complete);
    }

    #[test]
    fn artifact_and_export_edges_carry_a_concrete_kind() {
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&app_core::baseline_graph_spec(), &view, &|_| false);
        let vocal = render
            .edges
            .iter()
            .find(|edge| edge.to == id("artifact.vocal_stem"))
            .unwrap();
        assert_eq!(vocal.artifact_kind, Some(app_core::ArtifactKind::VocalStem));
        assert_eq!(vocal.producer_node, id("stems.separate"));
        let export = render
            .edges
            .iter()
            .find(|edge| edge.to == id("export.utz"))
            .unwrap();
        assert_eq!(
            export.artifact_kind,
            Some(app_core::ArtifactKind::AuthoredChart)
        );
    }

    #[test]
    fn mini_view_keeps_only_compute_nodes_and_edges() {
        let view = full_view_model(&|_| false);
        let render = filter_render_graph_for_mini_view(build_render_graph(
            &app_core::baseline_graph_spec(),
            &view,
            &|_| false,
        ));
        assert!(
            render
                .nodes
                .iter()
                .all(|node| node.kind == RenderNodeKind::Compute)
        );
        assert!(render.node(&id("artifact.vocal_stem")).is_none());
        assert!(render.node(&id("export.utz")).is_none());
        assert!(render.node(&id("stems.separate")).is_some());
        assert!(
            render
                .edges
                .iter()
                .all(|edge| render.node(&edge.from).is_some() && render.node(&edge.to).is_some())
        );
    }

    #[test]
    fn expanded_stem_chain_connects_through_output_files() {
        let view = expanded_stem_view(&|_| false);
        let graph = app_core::baseline_graph_spec();
        let render = build_render_graph(&graph, &view, &|_| false);
        assert!(render.node(&id("stems.vocals")).is_some());
        assert!(render.node(&id("vocals.denoise")).is_some());
        assert!(render.node(&id("vocals.dereverb")).is_some());
        assert!(render.node(&id("stems.bind_analysis_outputs")).is_none());
        assert_eq!(
            render.node(&id("artifact.raw_vocal")).unwrap().label,
            "vocals_raw.flac"
        );
        assert_eq!(
            render.node(&id("artifact.denoised_vocal")).unwrap().label,
            "vocals_denoised.flac"
        );
        assert_eq!(
            render.node(&id("artifact.dereverbed_vocal")).unwrap().label,
            "vocals_dry.flac"
        );
        let has = |from: &str, to: &str| {
            render
                .edges
                .iter()
                .any(|edge| edge.from == id(from) && edge.to == id(to))
        };
        assert!(has("stems.vocals", "artifact.raw_vocal"));
        assert!(has("artifact.raw_vocal", "vocals.denoise"));
        assert!(has("vocals.denoise", "artifact.denoised_vocal"));
        assert!(has("artifact.denoised_vocal", "vocals.dereverb"));
        assert!(has("vocals.dereverb", "artifact.dereverbed_vocal"));
        assert!(has("artifact.dereverbed_vocal", "pitch.extract"));
        assert!(has("artifact.dereverbed_vocal", "lyrics.preprocess"));
        assert!(has("pitch.extract", "artifact.note_guide"));
        assert!(has("artifact.note_guide", "chart.build_candidate"));
        assert!(!has("pitch.extract", "chart.build_candidate"));
        assert!(!has("stems.vocals", "pitch.extract"));
        assert!(!has("stems.vocals", "lyrics.preprocess"));
        assert!(!has("stems.vocals", "vocals.denoise"));
    }

    #[test]
    fn note_guide_feeds_candidate_chart_instead_of_pitch_extract() {
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&app_core::baseline_graph_spec(), &view, &|_| false);
        let has = |from: &str, to: &str| {
            render
                .edges
                .iter()
                .any(|edge| edge.from == id(from) && edge.to == id(to))
        };
        assert!(has("pitch.extract", "artifact.note_guide"));
        assert!(has("artifact.note_guide", "chart.build_candidate"));
        assert!(!has("pitch.extract", "chart.build_candidate"));
        let mini = filter_render_graph_for_mini_view(render);
        assert!(
            mini.edges
                .iter()
                .any(|edge| edge.from == id("pitch.extract")
                    && edge.to == id("chart.build_candidate"))
        );
    }

    #[test]
    fn not_applicable_stem_nodes_are_omitted_from_the_view() {
        let graph = app_core::baseline_graph_spec();
        let plan = app_core::build_plan(
            &graph,
            &app_core::AnalysisRequest {
                file_hash: "song".into(),
                targets: [id("pitch.extract")].into_iter().collect(),
                disabled_nodes: BTreeSet::new(),
                frozen_artifacts: BTreeSet::new(),
                bypassed_nodes: BTreeSet::new(),
                lyrics_route: app_core::LyricsRoute::WhisperAsr,
                model_availability: Default::default(),
                profile_snapshot: app_core::AnalysisProfileSnapshot::default(),
                active_stem_nodes: BTreeSet::new(),
            },
        )
        .unwrap();
        let mut expanded = BTreeSet::new();
        expanded.insert(id("stems.separate"));
        let view = build_graph_view_model(
            &graph,
            Some(&plan),
            None,
            0,
            &expanded,
            &no_bucket,
            &always_incomplete,
        );
        assert!(view.node(&id("stems.vocals")).is_some());
        assert!(view.node(&id("stems.bind_analysis_outputs")).is_none());
        assert!(view.node(&id("vocals.denoise")).is_none());
        assert!(view.node(&id("stems.karaoke")).is_none());
        let render = build_render_graph(&graph, &view, &|_| false);
        assert_eq!(
            render.node(&id("artifact.raw_vocal")).unwrap().label,
            "vocals.flac"
        );
        assert!(render
            .edges
            .iter()
            .any(|edge| edge.from == id("artifact.raw_vocal") && edge.to == id("pitch.extract")));
        assert!(
            !render.edges.iter().any(|edge| {
                edge.from == id("stems.vocals") && edge.to == id("lyrics.preprocess")
            })
        );
    }

    #[test]
    fn lineage_highlight_emphasizes_producer_and_deemphasizes_unrelated() {
        let view = full_view_model(&|_| false);
        let render = build_render_graph(&app_core::baseline_graph_spec(), &view, &|_| false);
        let selected = app_core::ArtifactRef {
            file_hash: "song".into(),
            kind: app_core::ArtifactKind::VocalStem,
            revision_id: "rev-a".into(),
        };
        let lineage = app_core::ArtifactLineage {
            root: selected.clone(),
            nodes: vec![app_core::ArtifactLineageNode {
                artifact: app_core::ArtifactRevision {
                    id: "rev-a".into(),
                    file_hash: "song".into(),
                    kind: app_core::ArtifactKind::VocalStem,
                    path: std::path::PathBuf::from("/tmp/vocal.flac"),
                    content_hash: "abc".into(),
                    producer_node: id("stems.separate"),
                    input_revisions: Vec::new(),
                    config_hash: "cfg".into(),
                    algorithm_version: "v1".into(),
                    created_at_ms: 1,
                    byte_size: 8,
                    active: true,
                    legacy: false,
                    invalidated: false,
                },
                depth: 0,
            }],
            missing_revision_ids: vec!["legacy-missing".into()],
            downstream_consumers: vec![id("pitch.extract")],
        };
        let highlight = graph_lineage_highlight(
            &render,
            &lineage,
            crate::studio::LineageScope::Full,
            &selected,
        );
        assert!(highlight.emphasized_nodes.contains(&id("stems.separate")));
        assert!(
            highlight
                .emphasized_nodes
                .contains(&id("artifact.vocal_stem"))
        );
        assert!(highlight.emphasized_nodes.contains(&id("pitch.extract")));
        assert!(!highlight.emphasized_nodes.contains(&id("lyrics.align")));
        assert_eq!(highlight.missing_gaps, vec!["legacy-missing".to_string()]);

        let mini = filter_render_graph_for_mini_view(render);
        let mini_highlight = graph_lineage_highlight(
            &mini,
            &lineage,
            crate::studio::LineageScope::Full,
            &selected,
        );
        assert!(
            mini_highlight
                .emphasized_nodes
                .contains(&id("stems.separate"))
        );
        assert!(
            mini_highlight
                .emphasized_nodes
                .contains(&id("pitch.extract"))
        );
        assert!(
            !mini_highlight
                .emphasized_nodes
                .iter()
                .any(|id| id.as_str().starts_with("artifact."))
        );
    }
}
