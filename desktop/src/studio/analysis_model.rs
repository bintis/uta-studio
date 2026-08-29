//! Pure presentation model for the compiled Processing Studio workflow.
//!
//! Nodes and bindings come from the exact workflow snapshot used by Engine
//! Preview/Execution; Bevy rendering and layout consume only this model.

use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderEdgeEmphasis {
    Mainline,
    Contextual,
    Secondary,
}

fn edge_mainline_priority(edge: &RenderEdge, destination_order: usize) -> (u8, u8, u8, usize) {
    let role = match edge.role {
        RenderEdgeRole::ComputeDependency => 0,
        RenderEdgeRole::AnalyzerAttachment => 1,
        RenderEdgeRole::InactiveBinding => 2,
    };
    let audio_role = match edge.audio_role.as_deref() {
        Some("lead_vocal" | "vocal" | "clean_lead_vocal") => 0,
        Some("instrumental" | "music") => 2,
        Some(_) => 1,
        None => 1,
    };
    let semantic = if edge.semantic_type.contains("audio") {
        0
    } else {
        1
    };
    (role, audio_role, semantic, destination_order)
}

/// Presentation-only edge classification. It never modifies, filters, or
/// recompiles exact workflow bindings; it only ensures a card has at most one
/// solid outgoing continuation until the user asks for its local context.
pub(crate) fn render_edge_emphasis(
    graph: &RenderGraph,
    edge_index: usize,
    selected_node_id: Option<&str>,
) -> RenderEdgeEmphasis {
    let Some(edge) = graph.edges.get(edge_index) else {
        return RenderEdgeEmphasis::Secondary;
    };
    if selected_node_id
        .is_some_and(|selected| edge.from.as_str() == selected || edge.to.as_str() == selected)
    {
        return RenderEdgeEmphasis::Contextual;
    }
    let node_order = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mainline = graph
        .edges
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.from == edge.from)
        .min_by_key(|(_, candidate)| {
            edge_mainline_priority(
                candidate,
                node_order.get(&candidate.to).copied().unwrap_or(usize::MAX),
            )
        })
        .map(|(index, _)| index);
    if mainline == Some(edge_index) {
        RenderEdgeEmphasis::Mainline
    } else {
        RenderEdgeEmphasis::Secondary
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

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(to: &str, role: RenderEdgeRole, audio_role: Option<&str>) -> RenderEdge {
        RenderEdge {
            from: AnalysisNodeId::new("separator"),
            from_port: "audio".to_string(),
            to: AnalysisNodeId::new(to),
            to_port: "audio".to_string(),
            semantic_type: "audio".to_string(),
            audio_role: audio_role.map(str::to_string),
            role,
        }
    }

    #[test]
    fn default_presentation_has_one_solid_outgoing_edge_per_card() {
        let graph = RenderGraph {
            nodes: Vec::new(),
            edges: vec![
                edge(
                    "instrumental",
                    RenderEdgeRole::ComputeDependency,
                    Some("instrumental"),
                ),
                edge(
                    "lead",
                    RenderEdgeRole::ComputeDependency,
                    Some("lead_vocal"),
                ),
                edge("analyzer", RenderEdgeRole::AnalyzerAttachment, None),
            ],
        };
        let emphases = (0..graph.edges.len())
            .map(|index| render_edge_emphasis(&graph, index, None))
            .collect::<Vec<_>>();
        assert_eq!(
            emphases
                .iter()
                .filter(|emphasis| **emphasis == RenderEdgeEmphasis::Mainline)
                .count(),
            1
        );
        assert_eq!(emphases[1], RenderEdgeEmphasis::Mainline);
        assert_eq!(emphases[0], RenderEdgeEmphasis::Secondary);
        assert_eq!(emphases[2], RenderEdgeEmphasis::Secondary);
    }

    #[test]
    fn selecting_a_card_reveals_every_exact_incident_binding() {
        let graph = RenderGraph {
            nodes: Vec::new(),
            edges: vec![
                edge(
                    "instrumental",
                    RenderEdgeRole::ComputeDependency,
                    Some("instrumental"),
                ),
                edge(
                    "lead",
                    RenderEdgeRole::ComputeDependency,
                    Some("lead_vocal"),
                ),
                edge("analyzer", RenderEdgeRole::AnalyzerAttachment, None),
            ],
        };
        assert!((0..graph.edges.len()).all(|index| {
            render_edge_emphasis(&graph, index, Some("separator")) == RenderEdgeEmphasis::Contextual
        }));
        assert_eq!(
            graph.edges.len(),
            3,
            "classification must not hide DAG truth"
        );
    }
}
