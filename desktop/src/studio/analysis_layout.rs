//! Layered topological auto-layout for the Analysis DAG canvas
//! (docs/analysis-dag-redesign.md Phase 7 §7.2, phase plan "首版建议实现分层拓扑布局").
//! Pure and Bevy-independent so it's unit-testable without spawning any UI:
//! given a graph, it returns a rectangle per node and an overall canvas
//! size, with no hardcoded per-node coordinates anywhere in this file.
//!
//! Algorithm: rank each node by its longest path from a source (a node with
//! no incoming edges) using the graph's own validated topological order,
//! group nodes into columns by rank, and stack same-rank nodes into lanes
//! top-to-bottom in topological order. This is the standard longest-path
//! layering step of a Sugiyama-style layout; it does not attempt
//! crossing-minimization (no barycenter/median pass) -- acceptable for a
//! first version per the phase plan's own wording, and safe to add later
//! without changing this module's public shape.

use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LayoutSpacing {
    pub(crate) node_width: f32,
    pub(crate) node_height: f32,
    pub(crate) column_gap: f32,
    pub(crate) row_gap: f32,
    pub(crate) margin: f32,
}

impl Default for LayoutSpacing {
    fn default() -> Self {
        Self {
            node_width: 150.0,
            node_height: 78.0,
            column_gap: 70.0,
            row_gap: 24.0,
            margin: 24.0,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GraphLayout {
    pub(crate) rects: BTreeMap<AnalysisNodeId, LayoutRect>,
    pub(crate) canvas_width: f32,
    pub(crate) canvas_height: f32,
}

impl GraphLayout {
    pub(crate) fn rect(&self, id: &AnalysisNodeId) -> Option<LayoutRect> {
        self.rects.get(id).copied()
    }
}

/// Deterministic Kahn's-algorithm topological sort over a flat node/edge
/// list, independent of `AnalysisGraphSpec`. Layout needs this generic form
/// because the rendered canvas includes virtual artifact/export boxes
/// (docs/analysis-dag-redesign.md Phase 7 §7.3's suggested structure --
/// "Vocal Stem", "Export UTZ", etc. -- none of which are real
/// `AnalysisGraphSpec` nodes) alongside the real graph nodes, and both need
/// to lay out together as one diagram. Same tie-breaking shape as
/// `AnalysisGraphSpec::topo_order` (sorted queue) for determinism. Returns
/// `None` on a cycle.
fn topo_order_from_edges(
    nodes: &[AnalysisNodeId],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
) -> Option<Vec<AnalysisNodeId>> {
    let mut forward: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> =
        nodes.iter().map(|n| (n, Vec::new())).collect();
    let mut in_degree: BTreeMap<&AnalysisNodeId, usize> = nodes.iter().map(|n| (n, 0)).collect();
    for (from, to) in edges {
        forward.entry(from).or_default().push(to);
        *in_degree.entry(to).or_default() += 1;
    }

    let mut queue: Vec<&AnalysisNodeId> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| *id)
        .collect();
    queue.sort();

    let mut ordered = Vec::new();
    while let Some(id) = queue.pop() {
        ordered.push(id.clone());
        if let Some(targets) = forward.get(id) {
            let mut newly_free = Vec::new();
            for target in targets {
                let degree = in_degree.get_mut(target)?;
                *degree -= 1;
                if *degree == 0 {
                    newly_free.push(*target);
                }
            }
            newly_free.sort();
            queue.extend(newly_free);
        }
    }

    (ordered.len() == nodes.len()).then_some(ordered)
}

/// Longest-path rank of every node: 0 for a source (no incoming edges),
/// otherwise `1 + max(rank of every direct predecessor)`. Processes nodes
/// in the graph's own validated topological order, so every predecessor's
/// rank is already known by the time a node is reached -- this is the same
/// guarantee `topo_order` already gives every other planner in this crate.
fn compute_ranks(
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    order: &[AnalysisNodeId],
) -> BTreeMap<AnalysisNodeId, u32> {
    let mut direct_predecessors: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> = BTreeMap::new();
    for (from, to) in edges {
        direct_predecessors.entry(to).or_default().push(from);
    }

    let mut ranks: BTreeMap<AnalysisNodeId, u32> = BTreeMap::new();
    for id in order {
        let rank = direct_predecessors
            .get(id)
            .into_iter()
            .flatten()
            .map(|pred| ranks.get(*pred).copied().unwrap_or(0) + 1)
            .max()
            .unwrap_or(0);
        ranks.insert(id.clone(), rank);
    }
    ranks
}

/// Computes a rectangle per node and the overall canvas size from a flat
/// node/edge list. Returns `None` only on a cycle -- every caller passes
/// either `baseline_graph_spec()` (validated by its own tests) or a
/// hand-built virtual graph whose edges are constructed to be acyclic by
/// design, so this is a defensive return, not an expected path.
pub(crate) fn layered_layout_from_edges(
    nodes: &[AnalysisNodeId],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> Option<GraphLayout> {
    let order = topo_order_from_edges(nodes, edges)?;
    let ranks = compute_ranks(edges, &order);

    let mut rows_used_in_column: BTreeMap<u32, usize> = BTreeMap::new();
    let mut rects = BTreeMap::new();
    let mut max_rank = 0u32;
    let mut max_row = 0usize;

    for id in &order {
        let rank = *ranks.get(id).unwrap_or(&0);
        max_rank = max_rank.max(rank);
        let row = rows_used_in_column.entry(rank).or_insert(0);
        max_row = max_row.max(*row);

        let x = spacing.margin + rank as f32 * (spacing.node_width + spacing.column_gap);
        let y = spacing.margin + *row as f32 * (spacing.node_height + spacing.row_gap);
        rects.insert(
            id.clone(),
            LayoutRect {
                x,
                y,
                width: spacing.node_width,
                height: spacing.node_height,
            },
        );
        *row += 1;
    }

    let canvas_width = spacing.margin * 2.0
        + (max_rank + 1) as f32 * spacing.node_width
        + max_rank as f32 * spacing.column_gap;
    let canvas_height = spacing.margin * 2.0
        + (max_row + 1) as f32 * spacing.node_height
        + max_row as f32 * spacing.row_gap;

    Some(GraphLayout {
        rects,
        canvas_width,
        canvas_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::AnalysisGraphSpec;

    fn id(s: &str) -> AnalysisNodeId {
        AnalysisNodeId::new(s)
    }

    /// Test-only convenience: production always lays out the extended
    /// virtual-node render graph via `layered_layout_from_edges` directly
    /// (see `analysis.rs`), but most of these tests just want "lay out
    /// this `AnalysisGraphSpec`".
    fn layered_layout(graph: &AnalysisGraphSpec, spacing: LayoutSpacing) -> Option<GraphLayout> {
        let nodes: Vec<AnalysisNodeId> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        let edges: Vec<(AnalysisNodeId, AnalysisNodeId)> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        layered_layout_from_edges(&nodes, &edges, spacing)
    }

    #[test]
    fn every_node_in_the_baseline_graph_gets_a_rectangle() {
        let graph = app_core::baseline_graph_spec();
        let layout =
            layered_layout(&graph, LayoutSpacing::default()).expect("valid graph lays out");
        for node in &graph.nodes {
            assert!(
                layout.rects.contains_key(&node.id),
                "missing rect for {}",
                node.id.as_str()
            );
        }
    }

    #[test]
    fn every_edge_points_strictly_left_to_right() {
        // The whole point of ranking by longest path: an edge's target must
        // never sit at or before its source's column, or the diagram would
        // draw a dependency arrow pointing backward or straight down.
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        for edge in &graph.edges {
            let from = layout.rect(&edge.from).unwrap();
            let to = layout.rect(&edge.to).unwrap();
            assert!(
                to.x > from.x,
                "{} (x={}) should be strictly right of {} (x={})",
                edge.to.as_str(),
                to.x,
                edge.from.as_str(),
                from.x
            );
        }
    }

    #[test]
    fn nodes_sharing_a_column_never_overlap_vertically() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let mut by_column: BTreeMap<i64, Vec<LayoutRect>> = BTreeMap::new();
        for rect in layout.rects.values() {
            by_column.entry(rect.x as i64).or_default().push(*rect);
        }
        for rects in by_column.values() {
            let mut sorted = rects.clone();
            sorted.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
            for pair in sorted.windows(2) {
                let [above, below] = pair else { unreachable!() };
                assert!(
                    below.y >= above.y + above.height,
                    "rows overlap: {above:?} vs {below:?}"
                );
            }
        }
    }

    #[test]
    fn canvas_size_bounds_every_rectangle() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        for rect in layout.rects.values() {
            assert!(rect.x + rect.width <= layout.canvas_width);
            assert!(rect.y + rect.height <= layout.canvas_height);
        }
    }

    #[test]
    fn a_source_node_with_no_dependencies_starts_at_rank_zero() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        // preflight has no upstream node in the baseline graph.
        let preflight = layout.rect(&id("preflight")).unwrap();
        assert_eq!(preflight.x, LayoutSpacing::default().margin);
    }

    #[test]
    fn a_chain_of_three_lays_out_in_three_strictly_increasing_columns() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: ["a", "b", "c"]
                .iter()
                .map(|name| app_core::AnalysisNodeSpec {
                    id: id(name),
                    label: name.to_string(),
                    inputs: vec![],
                    outputs: vec![],
                    disable_policy: app_core::DisablePolicy::AlwaysRequired,
                    cache_policy: app_core::CachePolicy::None,
                    algorithm_version: "1".to_string(),
                    compound_children: vec![],
                })
                .collect(),
            edges: vec![
                app_core::AnalysisEdge {
                    from: id("a"),
                    to: id("b"),
                },
                app_core::AnalysisEdge {
                    from: id("b"),
                    to: id("c"),
                },
            ],
        };
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let xs: Vec<f32> = ["a", "b", "c"]
            .iter()
            .map(|name| layout.rect(&id(name)).unwrap().x)
            .collect();
        assert!(xs[0] < xs[1]);
        assert!(xs[1] < xs[2]);
    }

    #[test]
    fn two_independent_siblings_share_a_column_but_not_a_row() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: ["root", "left", "right"]
                .iter()
                .map(|name| app_core::AnalysisNodeSpec {
                    id: id(name),
                    label: name.to_string(),
                    inputs: vec![],
                    outputs: vec![],
                    disable_policy: app_core::DisablePolicy::AlwaysRequired,
                    cache_policy: app_core::CachePolicy::None,
                    algorithm_version: "1".to_string(),
                    compound_children: vec![],
                })
                .collect(),
            edges: vec![
                app_core::AnalysisEdge {
                    from: id("root"),
                    to: id("left"),
                },
                app_core::AnalysisEdge {
                    from: id("root"),
                    to: id("right"),
                },
            ],
        };
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let left = layout.rect(&id("left")).unwrap();
        let right = layout.rect(&id("right")).unwrap();
        assert_eq!(left.x, right.x);
        assert_ne!(left.y, right.y);
    }
}
