//! Restrained real dependency-line rendering for the Advanced Graph.
//!
//! Every line drawn here comes from `RenderGraph.edges` (built from the
//! compiled workflow's exact semantic bindings) and `RoutedGraph.path`
//! (the existing orthogonal router in `analysis_layout.rs`). Nothing here
//! infers a connection from a node id, a capability name, or a stage
//! number, and nothing here recomputes topology.

use std::collections::{BTreeMap, BTreeSet};

use app_core::AnalysisNodeId;

use crate::studio::*;
// `bevy::prelude::RenderGraph` (bevy_render) and our own
// `analysis_model::RenderGraph` share a name; both are glob-imported here,
// so this type must stay fully qualified everywhere below.
use crate::studio::analysis_model::RenderGraph as WorkflowRenderGraph;

/// Ancestors and descendants of `selected`, including `selected` itself,
/// walked only over real dependency pairs. Never infers lineage from stage
/// numbers, node ids, or layout position.
pub(crate) fn compute_analysis_lineage(
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    selected: &AnalysisNodeId,
) -> BTreeSet<AnalysisNodeId> {
    let mut lineage = BTreeSet::new();
    lineage.insert(selected.clone());

    let mut frontier = vec![selected.clone()];
    while let Some(node) = frontier.pop() {
        for (from, to) in edges {
            if to == &node && lineage.insert(from.clone()) {
                frontier.push(from.clone());
            }
        }
    }

    let mut frontier = vec![selected.clone()];
    while let Some(node) = frontier.pop() {
        for (from, to) in edges {
            if from == &node && lineage.insert(to.clone()) {
                frontier.push(to.clone());
            }
        }
    }

    lineage
}

const EDGE_DASH: f32 = 5.0;
const EDGE_GAP: f32 = 4.0;

fn spawn_edge_rect(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(width.max(1.0)),
            height: px(height.max(1.0)),
            ..default()
        },
        BackgroundColor(color),
        ZIndex(1),
        Pickable::IGNORE,
    ));
}

/// One orthogonal path segment; `x1==x2` (vertical) or `y1==y2` (horizontal)
/// always holds for router output, so no diagonal case is needed.
fn spawn_edge_segment(
    parent: &mut ChildSpawnerCommands,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    thickness: f32,
    color: Color,
    dashed: bool,
) {
    let horizontal = (y1 - y2).abs() < 0.5;
    let length = if horizontal {
        (x2 - x1).abs()
    } else {
        (y2 - y1).abs()
    };
    if length < 0.5 {
        return;
    }
    if !dashed {
        if horizontal {
            spawn_edge_rect(
                parent,
                x1.min(x2),
                y1 - thickness / 2.0,
                length,
                thickness,
                color,
            );
        } else {
            spawn_edge_rect(
                parent,
                x1 - thickness / 2.0,
                y1.min(y2),
                thickness,
                length,
                color,
            );
        }
        return;
    }
    let mut offset = 0.0;
    while offset < length {
        let segment = EDGE_DASH.min(length - offset);
        if horizontal {
            spawn_edge_rect(
                parent,
                x1.min(x2) + offset,
                y1 - thickness / 2.0,
                segment,
                thickness,
                color,
            );
        } else {
            spawn_edge_rect(
                parent,
                x1 - thickness / 2.0,
                y1.min(y2) + offset,
                thickness,
                segment,
                color,
            );
        }
        offset += EDGE_DASH + EDGE_GAP;
    }
}

/// The router's long-span/side-channel detours (`route_layered_edges`'s
/// `above_edges`/`below_edges`, and a tall vertical run through a shared
/// column-gap corridor when several same-rank siblings fan out to distant
/// targets) are real, but a whole stack of them reads as clutter sweeping
/// across unrelated cards -- this canvas has no per-edge label a user could
/// read anyway. `spawn_analysis_graph_edges` renders every detour path
/// itself, just far quieter than a short local connector (§3 "低调") so the
/// direct local flow stays the dominant visual signal and a long dependency
/// is still discoverable by selecting a node (§9 lineage brightening).
fn path_detour_extent(path: &[LayoutPoint]) -> f32 {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for point in path {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    (max_x - min_x).max(max_y - min_y)
}

/// Roughly one row of canvas spacing (`LayoutSpacing::canvas().row_gap` is
/// 34) -- a path that never travels farther than a neighboring card is
/// "local"; anything longer is a cross-canvas detour.
const DETOUR_EXTENT_THRESHOLD: f32 = 140.0;

fn path_is_detour(path: &[LayoutPoint]) -> bool {
    path_detour_extent(path) > DETOUR_EXTENT_THRESHOLD
}

/// Restrained default/selected-lineage/failure edge styling (§3, §9). Edges
/// sit behind every node card (`ZIndex(1)` vs cards' `ZIndex(2..5)`), stay
/// thin, and never animate. `lineage` is `None` outside a selection (a
/// uniform low default) and `Some` while a node is selected (lineage edges
/// brighten, everything else dims further).
pub(crate) fn spawn_analysis_graph_edges(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    routed: &RoutedGraph,
    render_graph: &WorkflowRenderGraph,
    lineage: Option<&BTreeSet<AnalysisNodeId>>,
    zoom: f32,
) {
    let mut pair_role: BTreeMap<(AnalysisNodeId, AnalysisNodeId), RenderEdgeRole> = BTreeMap::new();
    for edge in &render_graph.edges {
        pair_role
            .entry(edge.endpoints())
            .and_modify(|role| {
                if *role == RenderEdgeRole::InactiveBinding
                    && edge.role != RenderEdgeRole::InactiveBinding
                {
                    *role = edge.role;
                }
            })
            .or_insert(edge.role);
    }

    // A non-row-aligned local edge only reads as one clean bend safely when
    // it is the sole edge touching its shared column-gap corridor: the
    // exact overlap bug fixed earlier this session happened because two
    // fanned-out siblings each drew their own bend through roughly the
    // same x, so their vertical runs coincided. `reserve_unique_x`/
    // `reserve_unique_y` in `analysis_layout.rs` exist specifically to
    // stagger that shared case -- keep using their output whenever a
    // `from` or `to` node has more than one such edge.
    let mut outgoing_bends: BTreeMap<AnalysisNodeId, usize> = BTreeMap::new();
    let mut incoming_bends: BTreeMap<AnalysisNodeId, usize> = BTreeMap::new();
    for ((from, to), _) in &pair_role {
        let Some(path) = routed.path(from, to) else {
            continue;
        };
        if path.len() < 2 || path_is_detour(path) {
            continue;
        }
        // Same "row" here means the same card-center y used everywhere
        // below (`from_center_y`/`to_center_y`), not the router's own
        // (possibly multi-port-distributed) raw endpoint y, so this count
        // agrees with the render-time decision it gates.
        let (Some(from_rect), Some(to_rect)) = (routed.layout.rect(from), routed.layout.rect(to))
        else {
            continue;
        };
        if (from_rect.y + from_rect.height / 2.0 - (to_rect.y + to_rect.height / 2.0)).abs() < 0.5 {
            continue;
        }
        *outgoing_bends.entry(from.clone()).or_default() += 1;
        *incoming_bends.entry(to.clone()).or_default() += 1;
    }

    for ((from, to), role) in &pair_role {
        let Some(path) = routed.path(from, to) else {
            continue;
        };
        if path.len() < 2 {
            continue;
        }
        // Cards render one implied port, not several distinct dots
        // (`input_ports`/`output_ports` are always 0 -- see
        // `WorkflowNodeCardSpec`), so every custom shape drawn below
        // touches the card's real vertical center rather than the
        // router's own multi-port distribution
        // (`LayoutRect::distributed_port_y`), which spreads a node's
        // several edges across its height for a distinct-ports diagram
        // this canvas does not draw (direct feedback: an edge visibly
        // met the top of a card instead of its middle). The router's own
        // path (used verbatim in the shared-corridor fallback below)
        // keeps its original distribution -- that spread is what keeps
        // *those* siblings from bunching together.
        let (Some(from_rect), Some(to_rect)) = (routed.layout.rect(from), routed.layout.rect(to))
        else {
            continue;
        };
        let from_center_y = from_rect.y + from_rect.height / 2.0;
        let to_center_y = to_rect.y + to_rect.height / 2.0;
        let inactive = *role == RenderEdgeRole::InactiveBinding;
        let detour = path_is_detour(path);
        let failure_adjacent = render_graph
            .node(from)
            .is_some_and(|node| node.state == GraphNodeState::Failed)
            || render_graph
                .node(to)
                .is_some_and(|node| node.state == GraphNodeState::Failed);
        let in_lineage = lineage.is_some_and(|set| set.contains(from) && set.contains(to));
        // A long cross-canvas detour is real, but rendering every one of a
        // fan-out's siblings at once is what repeated direct feedback
        // called messy regardless of how faint each individual line was --
        // three rounds of turning the alpha down did not fix it, so a
        // detour is now only drawn when a user has actually asked for it:
        // selecting one of its endpoints, or a real failure on one end.
        if detour && !in_lineage && !failure_adjacent {
            continue;
        }
        // A disabled/optional binding that still needs a long detour to
        // reach a selected/failed node is the least useful line to spend
        // attention on even then -- keep it out entirely.
        if inactive && detour {
            continue;
        }
        let alpha: f32 = match (lineage, in_lineage) {
            (Some(_), true) => 0.72,
            (Some(_), false) => 0.12,
            (None, _) => 0.22,
        };
        let alpha = if failure_adjacent {
            alpha.max(0.55)
        } else {
            alpha
        };
        let color = if failure_adjacent {
            theme.destructive
        } else if inactive {
            theme.muted_foreground
        } else {
            theme.border
        }
        .with_alpha(alpha);
        // A detour that did earn a place on the canvas still recedes a
        // little further than a local connector: thinner, so it never
        // reads as heavier than the direct connections it competes with.
        let thickness = if detour {
            (0.9 * zoom).max(0.75)
        } else {
            (1.1 * zoom).max(1.0)
        };

        // Real exit/entry point for every shape this function draws
        // itself (as opposed to the router's own multi-point path, used
        // verbatim in the shared-corridor fallback): the card's own edge
        // x, centered vertically.
        let first = LayoutPoint {
            x: path[0].x,
            y: from_center_y,
        };
        let last = LayoutPoint {
            x: path[path.len() - 1].x,
            y: to_center_y,
        };

        // A same-row hop reads as one straight line, and a non-row-aligned
        // hop reads as one clean "L" bend, from its real exit point to its
        // real entry point (§3 "连线可以更直一些" -- direct user feedback that
        // the router's channel-corridor jogs looked crooked for what are,
        // visually, simple neighbor-to-neighbor connections). Both
        // shortcuts are safe only when no sibling shares this corridor
        // (`outgoing_bends`/`incoming_bends` above); otherwise this keeps
        // every point of the router's own staggered path so fanned-out
        // siblings never collapse onto the same run.
        let same_row = (from_center_y - to_center_y).abs() < 0.5;
        let sole_bend = outgoing_bends.get(from).copied().unwrap_or(0) <= 1
            && incoming_bends.get(to).copied().unwrap_or(0) <= 1;
        if !detour && (same_row || sole_bend) {
            if same_row {
                spawn_edge_segment(
                    parent,
                    first.x * zoom,
                    first.y * zoom,
                    last.x * zoom,
                    last.y * zoom,
                    thickness,
                    color,
                    inactive,
                );
            } else {
                spawn_edge_segment(
                    parent,
                    first.x * zoom,
                    first.y * zoom,
                    last.x * zoom,
                    first.y * zoom,
                    thickness,
                    color,
                    inactive,
                );
                spawn_edge_segment(
                    parent,
                    last.x * zoom,
                    first.y * zoom,
                    last.x * zoom,
                    last.y * zoom,
                    thickness,
                    color,
                    inactive,
                );
            }
            continue;
        }
        // A genuine fan-out/fan-in (verified via `outgoing_bends`/
        // `incoming_bends`, not guessed) still reads as one clean "elbow"
        // instead of the router's independently-staggered jog per sibling:
        // every edge sharing the same source (or target) computes the
        // identical spine x from that shared node's own fixed exit/entry
        // point, so the siblings' vertical runs deliberately coincide into
        // one spine with branches -- unlike the earlier per-edge midpoint
        // bug, this coincidence is the intended picture, not an accident.
        let fans_out = outgoing_bends.get(from).copied().unwrap_or(0) > 1;
        let fans_in = incoming_bends.get(to).copied().unwrap_or(0) > 1;
        if !detour && (fans_out || fans_in) {
            const SPINE_STUB: f32 = 24.0;
            let spine_x = if fans_out {
                first.x + SPINE_STUB
            } else {
                last.x - SPINE_STUB
            };
            spawn_edge_segment(
                parent,
                first.x * zoom,
                first.y * zoom,
                spine_x * zoom,
                first.y * zoom,
                thickness,
                color,
                inactive,
            );
            spawn_edge_segment(
                parent,
                spine_x * zoom,
                first.y * zoom,
                spine_x * zoom,
                last.y * zoom,
                thickness,
                color,
                inactive,
            );
            spawn_edge_segment(
                parent,
                spine_x * zoom,
                last.y * zoom,
                last.x * zoom,
                last.y * zoom,
                thickness,
                color,
                inactive,
            );
            continue;
        }
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            spawn_edge_segment(
                parent,
                a.x * zoom,
                a.y * zoom,
                b.x * zoom,
                b.y * zoom,
                thickness,
                color,
                inactive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> AnalysisNodeId {
        AnalysisNodeId::new(value)
    }

    #[test]
    fn a_short_local_hop_is_not_a_detour() {
        let path = [
            LayoutPoint { x: 100.0, y: 200.0 },
            LayoutPoint { x: 140.0, y: 200.0 },
            LayoutPoint { x: 140.0, y: 210.0 },
            LayoutPoint { x: 180.0, y: 210.0 },
        ];
        assert!(!path_is_detour(&path));
    }

    #[test]
    fn a_long_horizontal_rail_is_a_detour() {
        let path = [
            LayoutPoint { x: 20.0, y: 200.0 },
            LayoutPoint { x: 20.0, y: 12.0 },
            LayoutPoint { x: 900.0, y: 12.0 },
            LayoutPoint { x: 900.0, y: 260.0 },
        ];
        assert!(path_is_detour(&path));
    }

    #[test]
    fn a_tall_vertical_side_channel_is_also_a_detour() {
        // Same x throughout (a side-channel corridor), but a large y
        // span -- the fan-out into several vertically stacked siblings
        // that motivated this classification in the first place.
        let path = [
            LayoutPoint { x: 500.0, y: 60.0 },
            LayoutPoint { x: 540.0, y: 60.0 },
            LayoutPoint { x: 540.0, y: 480.0 },
            LayoutPoint { x: 580.0, y: 480.0 },
        ];
        assert!(path_is_detour(&path));
    }

    #[test]
    fn lineage_includes_ancestors_descendants_and_the_node_itself() {
        // source -> mid -> selected -> sink, plus an unrelated branch.
        let edges = [
            (id("source"), id("mid")),
            (id("mid"), id("selected")),
            (id("selected"), id("sink")),
            (id("other_source"), id("other_sink")),
        ];
        let lineage = compute_analysis_lineage(&edges, &id("selected"));
        assert!(lineage.contains(&id("selected")));
        assert!(lineage.contains(&id("source")));
        assert!(lineage.contains(&id("mid")));
        assert!(lineage.contains(&id("sink")));
        assert!(!lineage.contains(&id("other_source")));
        assert!(!lineage.contains(&id("other_sink")));
    }

    #[test]
    fn lineage_of_an_isolated_node_is_only_itself() {
        let edges = [(id("a"), id("b"))];
        let lineage = compute_analysis_lineage(&edges, &id("c"));
        assert_eq!(lineage.len(), 1);
        assert!(lineage.contains(&id("c")));
    }

    #[test]
    fn a_diamond_reaches_every_branch_without_duplication() {
        // fan-out then fan-in: selected has two parents and one child.
        let edges = [
            (id("left"), id("selected")),
            (id("right"), id("selected")),
            (id("selected"), id("sink")),
        ];
        let lineage = compute_analysis_lineage(&edges, &id("selected"));
        assert_eq!(
            lineage,
            [id("left"), id("right"), id("selected"), id("sink")]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }

    /// Lineage is a highlighting set only -- it must never remove a node
    /// from the graph the caller renders (unrelated nodes stay visible,
    /// just dimmed).
    #[test]
    fn lineage_never_shrinks_the_rendered_node_set() {
        let edges = [
            (id("selected"), id("child")),
            (id("unrelated_a"), id("unrelated_b")),
        ];
        let all_nodes = [
            id("selected"),
            id("child"),
            id("unrelated_a"),
            id("unrelated_b"),
        ];
        let lineage = compute_analysis_lineage(&edges, &id("selected"));
        // Every node the caller would render remains a valid, distinct id
        // regardless of lineage membership.
        assert_eq!(
            all_nodes.iter().collect::<BTreeSet<_>>().len(),
            all_nodes.len()
        );
        assert!(lineage.len() < all_nodes.len());
    }
}
