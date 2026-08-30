//! Pure presentation model for the compiled Processing Studio workflow.
//!
//! Nodes and bindings come from the exact workflow snapshot used by Engine
//! Preview/Execution; Bevy rendering and layout consume only this model.

use std::collections::BTreeSet;

use app_core::AnalysisNodeId;

mod workflow;
pub(crate) use workflow::{
    build_workflow_render_graph, exact_engine_capabilities_from_engine,
    exact_workflow_plan_from_engine, overlay_workflow_runtime, workflow_graph_step,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphNodeState {
    Disabled,
    Waiting,
    Running,
    Complete,
    Failed,
    Deferred,
    ProfileSkipped,
    NotRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderNodeKind {
    Compute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphNodeCategory {
    Source,
    Audio,
    Lyrics,
    Pitch,
    Evidence,
    Fusion,
    Output,
}

#[derive(Debug, Clone)]
pub(crate) struct RenderNode {
    pub(crate) id: AnalysisNodeId,
    pub(crate) kind: RenderNodeKind,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) state: GraphNodeState,
    pub(crate) category: GraphNodeCategory,
    pub(crate) capability_id: Option<String>,
    pub(crate) terminal_outputs: Vec<RenderTerminalOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderTerminalOutput {
    pub(crate) port: String,
    pub(crate) semantic_type: String,
    pub(crate) audio_role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderEdgeRole {
    ComputeDependency,
    AnalyzerAttachment,
    InactiveBinding,
}

// The DAG view no longer draws per-edge binding lines (they made the graph
// unreadable), so nothing currently reads an edge's port/semantic/role
// detail outside of `analysis_model::workflow`'s own tests, which assert
// these are computed correctly from the exact compiled workflow bindings.
// Kept -- not display metadata, but real derived data a future bindings
// inspector could read -- with dead_code silenced rather than deleting
// tested binding-resolution correctness coverage.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct RenderEdge {
    pub(crate) from: AnalysisNodeId,
    pub(crate) from_port: String,
    pub(crate) to: AnalysisNodeId,
    pub(crate) to_port: String,
    pub(crate) semantic_type: String,
    pub(crate) audio_role: Option<String>,
    pub(crate) role: RenderEdgeRole,
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
        self.nodes.iter().find(|node| &node.id == id)
    }

    pub(crate) fn edge_pairs(&self) -> Vec<(AnalysisNodeId, AnalysisNodeId)> {
        self.edges.iter().map(RenderEdge::endpoints).collect()
    }
}

/// MINI keeps only nodes that can participate in the selected request. It
/// never invents shortcut edges; the visible topology remains a subgraph of
/// the exact compiled workflow.
pub(crate) fn filter_render_graph_for_mini_view(mut render: RenderGraph) -> RenderGraph {
    render.nodes.retain(|node| {
        !matches!(
            node.state,
            GraphNodeState::Disabled
                | GraphNodeState::ProfileSkipped
                | GraphNodeState::NotRequested
        )
    });
    let visible = render
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    render
        .edges
        .retain(|edge| visible.contains(&edge.from) && visible.contains(&edge.to));
    render
}
