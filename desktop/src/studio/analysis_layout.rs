//! Layered topological auto-layout for the Analysis DAG canvas
//! (docs/analysis-dag-redesign.md Phase 7 §7.2, phase plan "首版建议实现分层拓扑布局").
//! Pure and Bevy-independent so it's unit-testable without spawning any UI:
//! given a graph, it returns a rectangle per node and an overall canvas
//! size, with no hardcoded per-node coordinates anywhere in this file.
//!
//! Algorithm: rank each node by its longest path from a source (a node with
//! no incoming edges) using the graph's own validated topological order,
//! group nodes into columns by rank, then pull each node as far right as
//! its successors allow so a side path such as Timed Lyrics Import sits in
//! the lyrics column instead of the stem column. A node is only wired to
//! the layer that feeds it and the layer it feeds; stem separation never
//! gains a line to Timed Lyrics. Remaining long-span edges keep private
//! rails so two pipelines never share a collinear segment.

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

impl LayoutSpacing {
    /// Tighter canvas metrics so the full analysis flow can sit on one
    /// page at the default zoom. Tests keep using `Default` so their
    /// coordinate assertions stay independent of the on-screen density.
    pub(crate) fn canvas() -> Self {
        Self {
            node_width: 128.0,
            node_height: 70.0,
            column_gap: 36.0,
            row_gap: 16.0,
            margin: 16.0,
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

    /// Visual grouping only: the three analysis branches are rendered as
    /// quiet, labelled surfaces behind their nodes. Shared source/merge
    /// nodes deliberately remain outside these bounds so the grouping does
    /// not imply any extra dependency.
    pub(crate) fn lane_bands(&self) -> Vec<LayoutLaneBand> {
        let mut bounds: [Option<LayoutRect>; 3] = [None, None, None];
        for (id, rect) in &self.rects {
            let index = match swimlane_of(id) {
                Swimlane::Music => 0,
                Swimlane::Stems => 1,
                Swimlane::Lyrics => 2,
                Swimlane::Shared => continue,
            };
            bounds[index] = Some(match bounds[index] {
                None => *rect,
                Some(current) => LayoutRect {
                    x: current.x.min(rect.x),
                    y: current.y.min(rect.y),
                    width: current.right().max(rect.right()) - current.x.min(rect.x),
                    height: current.bottom().max(rect.bottom()) - current.y.min(rect.y),
                },
            });
        }

        bounds
            .into_iter()
            .enumerate()
            .filter_map(|(index, rect)| {
                let rect = rect?;
                let horizontal_padding = 10.0;
                let header_height = 16.0;
                let bottom_padding = 10.0;
                Some(LayoutLaneBand {
                    kind: match index {
                        0 => LayoutLaneKind::Music,
                        1 => LayoutLaneKind::VocalsAndPitch,
                        _ => LayoutLaneKind::LyricsAndTiming,
                    },
                    rect: LayoutRect {
                        x: (rect.x - horizontal_padding).max(0.0),
                        y: (rect.y - header_height).max(0.0),
                        width: rect.width + horizontal_padding * 2.0,
                        height: rect.height + header_height + bottom_padding,
                    },
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutLaneKind {
    Music,
    VocalsAndPitch,
    LyricsAndTiming,
}

impl LayoutLaneKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Music => "MUSIC INSIGHTS",
            Self::VocalsAndPitch => "VOCALS & PITCH",
            Self::LyricsAndTiming => "LYRICS & TIMING",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutLaneBand {
    pub(crate) kind: LayoutLaneKind,
    pub(crate) rect: LayoutRect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl LayoutRect {
    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn distributed_port_y(self, index: usize, count: usize) -> f32 {
        if count <= 1 {
            return self.y + self.height / 2.0;
        }
        let inset = (self.height * 0.16).clamp(6.0, 14.0);
        let usable = (self.height - inset * 2.0).max(0.0);
        self.y + inset + usable * index as f32 / (count as f32 - 1.0)
    }

    fn distributed_port_x(self, index: usize, count: usize) -> f32 {
        if count <= 1 {
            return self.x + self.width / 2.0;
        }
        let inset = (self.width * 0.16).clamp(6.0, 14.0);
        let usable = (self.width - inset * 2.0).max(0.0);
        self.x + inset + usable * index as f32 / (count as f32 - 1.0)
    }
}

/// Routed orthogonal polylines for every laid-out edge, plus a canvas that
/// has been grown (and nodes shifted) so around-the-graph rails stay inside
/// the drawable area.
#[derive(Debug, Clone)]
pub(crate) struct RoutedGraph {
    pub(crate) layout: GraphLayout,
    paths: BTreeMap<(AnalysisNodeId, AnalysisNodeId), Vec<LayoutPoint>>,
}

impl RoutedGraph {
    pub(crate) fn path(
        &self,
        from: &AnalysisNodeId,
        to: &AnalysisNodeId,
    ) -> Option<&[LayoutPoint]> {
        self.paths
            .get(&(from.clone(), to.clone()))
            .map(Vec::as_slice)
    }
}

const RAIL_GAP: f32 = 8.0;
const PORT_STUB: f32 = 8.0;
const LONG_SPAN_COLUMNS: f32 = 1.5;

fn is_long_span(from: LayoutRect, to: LayoutRect, spacing: LayoutSpacing) -> bool {
    to.x - from.x > (spacing.node_width + spacing.column_gap) * LONG_SPAN_COLUMNS
}

const KIND_SIDE: u8 = 0;
const KIND_LONG_ABOVE: u8 = 1;
const KIND_LONG_BELOW: u8 = 2;
const KIND_DOWN: u8 = 3;
const KIND_UP: u8 = 4;
const KIND_UNDER: u8 = 5;
const RIGHT_FACE_KINDS: [u8; 3] = [KIND_SIDE, KIND_LONG_ABOVE, KIND_LONG_BELOW];
const LEFT_FACE_KINDS: [u8; 5] = [
    KIND_SIDE,
    KIND_LONG_ABOVE,
    KIND_LONG_BELOW,
    KIND_DOWN,
    KIND_UP,
];
const BOTTOM_OUT_KINDS: [u8; 2] = [KIND_DOWN, KIND_UNDER];

fn vertically_overlap(left: LayoutRect, right: LayoutRect) -> bool {
    left.y < right.bottom() && right.y < left.bottom()
}

fn prefers_bottom_rail(
    from: &AnalysisNodeId,
    to: &AnalysisNodeId,
    from_rect: LayoutRect,
    to_rect: LayoutRect,
) -> bool {
    from.as_str() == "lyrics.import_timed"
        || to.as_str() == "lyrics.import_timed"
        || to_rect.y >= from_rect.bottom()
}

fn of_kinds(list: &[(AnalysisNodeId, u8)], kinds: &[u8]) -> Vec<(AnalysisNodeId, u8)> {
    list.iter()
        .filter(|(_, kind)| kinds.contains(kind))
        .cloned()
        .collect()
}

fn column_key(x: f32) -> i64 {
    (x * 100.0).round() as i64
}

/// Orthogonal connector for every edge. Adjacent same-row edges stay on
/// the side ports. A short hop that lands fully above or below its source
/// leaves from that nearer face as an L so a fan-out such as Vocal → Pitch
/// and Vocal → Vocal Preprocessing does not stack two lines on the right
/// edge. A same-row skip such as Vocal Preprocessing → Forced Alignment
/// leaves from the bottom and runs under the row so it does not sit on
/// the Transcription hop. Edges that skip columns and change lanes take
/// private rails above or below the node stack; Timed Lyrics Import wraps
/// underneath.
pub(crate) fn route_layered_edges(
    layout: &GraphLayout,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> RoutedGraph {
    let mut side_edges = Vec::new();
    let mut down_edges = Vec::new();
    let mut up_edges = Vec::new();
    let mut under_edges = Vec::new();
    let mut above_edges = Vec::new();
    let mut below_edges = Vec::new();
    for (from_id, to_id) in edges {
        let Some(from) = layout.rect(from_id) else {
            continue;
        };
        let Some(to) = layout.rect(to_id) else {
            continue;
        };
        if is_long_span(from, to, spacing) {
            if vertically_overlap(from, to) {
                under_edges.push((from_id.clone(), to_id.clone()));
            } else if prefers_bottom_rail(from_id, to_id, from, to) {
                below_edges.push((from_id.clone(), to_id.clone()));
            } else {
                above_edges.push((from_id.clone(), to_id.clone()));
            }
        } else if to.y >= from.bottom() {
            down_edges.push((from_id.clone(), to_id.clone()));
        } else if from.y >= to.bottom() {
            up_edges.push((from_id.clone(), to_id.clone()));
        } else {
            side_edges.push((from_id.clone(), to_id.clone()));
        }
    }
    above_edges.sort();
    below_edges.sort();
    down_edges.sort();
    up_edges.sort();
    under_edges.sort();
    side_edges.sort();

    let mut outgoing: BTreeMap<AnalysisNodeId, Vec<(AnalysisNodeId, u8)>> = BTreeMap::new();
    let mut incoming: BTreeMap<AnalysisNodeId, Vec<(AnalysisNodeId, u8)>> = BTreeMap::new();
    let classify = |from: &AnalysisNodeId, to: &AnalysisNodeId| -> u8 {
        if below_edges
            .iter()
            .any(|edge| edge.0 == *from && edge.1 == *to)
        {
            KIND_LONG_BELOW
        } else if above_edges
            .iter()
            .any(|edge| edge.0 == *from && edge.1 == *to)
        {
            KIND_LONG_ABOVE
        } else if down_edges
            .iter()
            .any(|edge| edge.0 == *from && edge.1 == *to)
        {
            KIND_DOWN
        } else if up_edges.iter().any(|edge| edge.0 == *from && edge.1 == *to) {
            KIND_UP
        } else if under_edges
            .iter()
            .any(|edge| edge.0 == *from && edge.1 == *to)
        {
            KIND_UNDER
        } else {
            KIND_SIDE
        }
    };
    for (from_id, to_id) in side_edges
        .iter()
        .chain(down_edges.iter())
        .chain(up_edges.iter())
        .chain(under_edges.iter())
        .chain(above_edges.iter())
        .chain(below_edges.iter())
    {
        let kind = classify(from_id, to_id);
        outgoing
            .entry(from_id.clone())
            .or_default()
            .push((to_id.clone(), kind));
        incoming
            .entry(to_id.clone())
            .or_default()
            .push((from_id.clone(), kind));
    }
    for list in outgoing.values_mut() {
        list.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    }
    for list in incoming.values_mut() {
        list.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    }

    let port_index = |list: &[(AnalysisNodeId, u8)], id: &AnalysisNodeId| -> usize {
        list.iter().position(|(other, _)| other == id).unwrap_or(0)
    };

    let mut used_x_by_gap: BTreeMap<(i64, i64), Vec<f32>> = BTreeMap::new();
    let gap_after = |rect: LayoutRect| -> (i64, i64) {
        (
            column_key(rect.x),
            column_key(rect.x + rect.width + spacing.column_gap),
        )
    };
    let gap_before = |rect: LayoutRect| -> (i64, i64) {
        (
            column_key(rect.x - spacing.column_gap - spacing.node_width),
            column_key(rect.x),
        )
    };

    let mut long_exit_x: BTreeMap<(AnalysisNodeId, AnalysisNodeId), f32> = BTreeMap::new();
    let mut long_enter_x: BTreeMap<(AnalysisNodeId, AnalysisNodeId), f32> = BTreeMap::new();
    for (from_id, to_id) in above_edges.iter().chain(below_edges.iter()) {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        let exit_x = reserve_unique_x(
            from.x + from.width + PORT_STUB,
            used_x_by_gap.entry(gap_after(from)).or_default(),
        );
        let enter_x = reserve_unique_x(
            to.x - PORT_STUB,
            used_x_by_gap.entry(gap_before(to)).or_default(),
        );
        long_exit_x.insert((from_id.clone(), to_id.clone()), exit_x);
        long_enter_x.insert((from_id.clone(), to_id.clone()), enter_x);
    }

    let mut short_channel: BTreeMap<(AnalysisNodeId, AnalysisNodeId), f32> = BTreeMap::new();
    let mut short_by_gap: BTreeMap<(i64, i64), Vec<(AnalysisNodeId, AnalysisNodeId)>> =
        BTreeMap::new();
    for (from_id, to_id) in &side_edges {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        short_by_gap
            .entry((column_key(from.x), column_key(to.x)))
            .or_default()
            .push((from_id.clone(), to_id.clone()));
    }
    for group in short_by_gap.values() {
        let n = group.len();
        for (index, (from_id, to_id)) in group.iter().enumerate() {
            let from = layout.rect(from_id).expect("filtered");
            let to = layout.rect(to_id).expect("filtered");
            let gap_left = from.x + from.width;
            let gap_right = to.x;
            let t = (index + 1) as f32 / (n + 1) as f32;
            let preferred = gap_left + (gap_right - gap_left) * t;
            short_channel.insert(
                (from_id.clone(), to_id.clone()),
                reserve_unique_x(
                    preferred,
                    used_x_by_gap
                        .entry((column_key(from.x), column_key(to.x)))
                        .or_default(),
                ),
            );
        }
    }

    let mut paths = BTreeMap::new();
    for (index, (from_id, to_id)) in above_edges.iter().enumerate() {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        let rail_y = spacing.margin * 0.5 - index as f32 * RAIL_GAP;
        let outs = of_kinds(&outgoing[from_id], &RIGHT_FACE_KINDS);
        let ins = of_kinds(&incoming[to_id], &LEFT_FACE_KINDS);
        paths.insert(
            (from_id.clone(), to_id.clone()),
            long_span_path(
                from,
                to,
                rail_y,
                from.distributed_port_y(port_index(&outs, to_id), outs.len()),
                to.distributed_port_y(port_index(&ins, from_id), ins.len()),
                long_exit_x[&(from_id.clone(), to_id.clone())],
                long_enter_x[&(from_id.clone(), to_id.clone())],
            ),
        );
    }
    for (index, (from_id, to_id)) in below_edges.iter().enumerate() {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        let rail_y = layout.canvas_height - spacing.margin * 0.5 + index as f32 * RAIL_GAP;
        let outs = of_kinds(&outgoing[from_id], &RIGHT_FACE_KINDS);
        let ins = of_kinds(&incoming[to_id], &LEFT_FACE_KINDS);
        paths.insert(
            (from_id.clone(), to_id.clone()),
            long_span_path(
                from,
                to,
                rail_y,
                from.distributed_port_y(port_index(&outs, to_id), outs.len()),
                to.distributed_port_y(port_index(&ins, from_id), ins.len()),
                long_exit_x[&(from_id.clone(), to_id.clone())],
                long_enter_x[&(from_id.clone(), to_id.clone())],
            ),
        );
    }
    let mut used_y_by_gap: BTreeMap<(i64, i64), Vec<f32>> = BTreeMap::new();
    for (from_id, to_id) in &side_edges {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        let outs = of_kinds(&outgoing[from_id], &RIGHT_FACE_KINDS);
        let ins = of_kinds(&incoming[to_id], &LEFT_FACE_KINDS);
        let actual_exit_y = from.distributed_port_y(port_index(&outs, to_id), outs.len());
        let actual_enter_y = to.distributed_port_y(port_index(&ins, from_id), ins.len());
        let used = used_y_by_gap
            .entry((column_key(from.x), column_key(to.x)))
            .or_default();
        let route_exit_y = reserve_unique_y(actual_exit_y, used);
        let route_enter_y = if (actual_enter_y - actual_exit_y).abs() <= 0.5 {
            route_exit_y
        } else {
            reserve_unique_y(actual_enter_y, used)
        };
        let mid_x = short_channel[&(from_id.clone(), to_id.clone())];
        let mut path = vec![LayoutPoint {
            x: from.x + from.width,
            y: actual_exit_y,
        }];
        if (route_exit_y - actual_exit_y).abs() > 0.5 {
            path.push(LayoutPoint {
                x: from.x + from.width,
                y: route_exit_y,
            });
        }
        path.push(LayoutPoint {
            x: mid_x,
            y: route_exit_y,
        });
        path.push(LayoutPoint {
            x: mid_x,
            y: route_enter_y,
        });
        if (route_enter_y - actual_enter_y).abs() > 0.5 {
            path.push(LayoutPoint {
                x: to.x,
                y: route_enter_y,
            });
        }
        path.push(LayoutPoint {
            x: to.x,
            y: actual_enter_y,
        });
        paths.insert((from_id.clone(), to_id.clone()), path);
    }

    let mut used_bottom_x: BTreeMap<AnalysisNodeId, Vec<f32>> = BTreeMap::new();
    for (downward, face_edges) in [(true, &down_edges), (false, &up_edges)] {
        for (from_id, to_id) in face_edges {
            let from = layout.rect(from_id).expect("filtered");
            let to = layout.rect(to_id).expect("filtered");
            let face_outs = of_kinds(
                &outgoing[from_id],
                if downward {
                    &BOTTOM_OUT_KINDS
                } else {
                    &[KIND_UP]
                },
            );
            let ins = of_kinds(&incoming[to_id], &LEFT_FACE_KINDS);
            let preferred_exit_x =
                from.distributed_port_x(port_index(&face_outs, to_id), face_outs.len());
            let actual_enter_y = to.distributed_port_y(port_index(&ins, from_id), ins.len());
            let used = used_y_by_gap
                .entry((column_key(from.x), column_key(to.x)))
                .or_default();
            let route_y = reserve_unique_y(actual_enter_y, used);
            let interior_key = (column_key(from.x), column_key(from.x));
            let start_y = if downward { from.bottom() } else { from.y };
            let blocked = vertical_hits_other_node(
                layout,
                preferred_exit_x,
                start_y,
                route_y,
                from_id,
                to_id,
            ) || used_x_by_gap
                .get(&interior_key)
                .into_iter()
                .flatten()
                .chain(used_x_by_gap.get(&gap_after(from)).into_iter().flatten())
                .any(|taken| (taken - preferred_exit_x).abs() < RAIL_GAP - 0.5);
            let exit_x = if blocked {
                reserve_unique_x(
                    from.right() + PORT_STUB,
                    used_x_by_gap.entry(gap_after(from)).or_default(),
                )
            } else {
                reserve_unique_x(
                    preferred_exit_x,
                    used_x_by_gap.entry(interior_key).or_default(),
                )
            };
            if downward {
                used_bottom_x.entry(from_id.clone()).or_default().push(
                    if (exit_x - preferred_exit_x).abs() > 0.5 {
                        preferred_exit_x
                    } else {
                        exit_x
                    },
                );
            }
            paths.insert(
                (from_id.clone(), to_id.clone()),
                face_l_path(
                    from,
                    to,
                    downward,
                    preferred_exit_x,
                    exit_x,
                    actual_enter_y,
                    route_y,
                ),
            );
        }
    }

    let mut used_under_y: Vec<f32> = Vec::new();
    for (from_id, to_id) in &under_edges {
        let from = layout.rect(from_id).expect("filtered");
        let to = layout.rect(to_id).expect("filtered");
        let bottom_outs = of_kinds(&outgoing[from_id], &BOTTOM_OUT_KINDS);
        let bottom_ins = of_kinds(&incoming[to_id], &[KIND_UNDER]);
        let exit_x = reserve_unique_x(
            from.distributed_port_x(port_index(&bottom_outs, to_id), bottom_outs.len()),
            used_bottom_x.entry(from_id.clone()).or_default(),
        );
        let enter_x = reserve_unique_x(
            to.distributed_port_x(port_index(&bottom_ins, from_id), bottom_ins.len()),
            used_bottom_x.entry(to_id.clone()).or_default(),
        );
        let rail_y = reserve_unique_y(
            from.bottom().max(to.bottom()) + PORT_STUB,
            &mut used_under_y,
        );
        paths.insert(
            (from_id.clone(), to_id.clone()),
            under_row_path(from, to, exit_x, enter_x, rail_y),
        );
    }

    let mut min_y = 0.0f32;
    let mut max_y = layout.canvas_height;
    for rect in layout.rects.values() {
        min_y = min_y.min(rect.y);
        max_y = max_y.max(rect.y + rect.height);
    }
    for path in paths.values() {
        for point in path {
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
    }
    let extra_top = (spacing.margin * 0.25 - min_y).max(0.0);
    let extra_bottom = (max_y + spacing.margin * 0.25 - layout.canvas_height).max(0.0);

    let mut shifted = layout.clone();
    if extra_top > 0.0 {
        for rect in shifted.rects.values_mut() {
            rect.y += extra_top;
        }
        for path in paths.values_mut() {
            for point in path {
                point.y += extra_top;
            }
        }
    }
    shifted.canvas_height += extra_top + extra_bottom;

    RoutedGraph {
        layout: shifted,
        paths,
    }
}

fn vertical_hits_other_node(
    layout: &GraphLayout,
    x: f32,
    y0: f32,
    y1: f32,
    from_id: &AnalysisNodeId,
    to_id: &AnalysisNodeId,
) -> bool {
    let lo = y0.min(y1);
    let hi = y0.max(y1);
    layout.rects.iter().any(|(id, rect)| {
        if id == from_id || id == to_id {
            return false;
        }
        let inside_x = x >= rect.x - 0.5 && x <= rect.right() + 0.5;
        let overlap_y = lo < rect.bottom() - 0.5 && hi > rect.y + 0.5;
        inside_x && overlap_y
    })
}

fn under_row_path(
    from: LayoutRect,
    to: LayoutRect,
    exit_x: f32,
    enter_x: f32,
    rail_y: f32,
) -> Vec<LayoutPoint> {
    vec![
        LayoutPoint {
            x: exit_x,
            y: from.bottom(),
        },
        LayoutPoint {
            x: exit_x,
            y: rail_y,
        },
        LayoutPoint {
            x: enter_x,
            y: rail_y,
        },
        LayoutPoint {
            x: enter_x,
            y: to.bottom(),
        },
    ]
}

fn face_l_path(
    from: LayoutRect,
    to: LayoutRect,
    downward: bool,
    preferred_exit_x: f32,
    exit_x: f32,
    enter_y: f32,
    route_y: f32,
) -> Vec<LayoutPoint> {
    let start_y = if downward { from.bottom() } else { from.y };
    let mut path = Vec::new();
    if (exit_x - preferred_exit_x).abs() > 0.5 {
        path.push(LayoutPoint {
            x: preferred_exit_x,
            y: start_y,
        });
    }
    path.push(LayoutPoint {
        x: exit_x,
        y: start_y,
    });
    if (route_y - start_y).abs() > 0.5 {
        path.push(LayoutPoint {
            x: exit_x,
            y: route_y,
        });
    }
    path.push(LayoutPoint {
        x: to.x,
        y: route_y,
    });
    if (route_y - enter_y).abs() > 0.5 {
        path.push(LayoutPoint {
            x: to.x,
            y: enter_y,
        });
    }
    path
}

fn long_span_path(
    from: LayoutRect,
    to: LayoutRect,
    rail_y: f32,
    exit_y: f32,
    enter_y: f32,
    from_exit_x: f32,
    to_enter_x: f32,
) -> Vec<LayoutPoint> {
    let to_enter_x = to_enter_x.max(from_exit_x + RAIL_GAP);
    vec![
        LayoutPoint {
            x: from.x + from.width,
            y: exit_y,
        },
        LayoutPoint {
            x: from_exit_x,
            y: exit_y,
        },
        LayoutPoint {
            x: from_exit_x,
            y: rail_y,
        },
        LayoutPoint {
            x: to_enter_x,
            y: rail_y,
        },
        LayoutPoint {
            x: to_enter_x,
            y: enter_y,
        },
        LayoutPoint {
            x: to.x,
            y: enter_y,
        },
    ]
}

fn reserve_unique_x(preferred: f32, used: &mut Vec<f32>) -> f32 {
    reserve_unique_axis(preferred, used)
}

fn reserve_unique_y(preferred: f32, used: &mut Vec<f32>) -> f32 {
    reserve_unique_axis(preferred, used)
}

fn reserve_unique_axis(preferred: f32, used: &mut Vec<f32>) -> f32 {
    let mut value = preferred;
    let mut delta = 0.0f32;
    let mut sign = 1.0f32;
    while used
        .iter()
        .any(|taken| (taken - value).abs() < RAIL_GAP - 0.5)
    {
        delta += RAIL_GAP;
        value = preferred + sign * delta;
        sign = -sign;
    }
    used.push(value);
    value
}

#[cfg(test)]
fn longest_horizontal_y(path: &[LayoutPoint]) -> Option<f32> {
    path.windows(2)
        .filter(|pair| (pair[0].y - pair[1].y).abs() <= 0.5)
        .max_by(|a, b| {
            (a[0].x - a[1].x)
                .abs()
                .partial_cmp(&(b[0].x - b[1].x).abs())
                .unwrap()
        })
        .map(|pair| pair[0].y)
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
    tighten_ranks(&mut ranks, edges, order);
    ranks
}

/// Pull orphaned side-path nodes as far right as their successors allow.
/// Longest-path ranking otherwise parks a node such as `lyrics.import_timed`
/// in the stem column (it only depends on `preflight`). Nodes that already
/// have a same-lane producer — Pitch after the vocal file, Transcription
/// after Vocal Preprocessing — stay put so a later merge at Build Candidate
/// Chart does not stretch a long skip across the other lanes.
fn tighten_ranks(
    ranks: &mut BTreeMap<AnalysisNodeId, u32>,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    order: &[AnalysisNodeId],
) {
    let mut successors: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> = BTreeMap::new();
    let mut predecessors: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> = BTreeMap::new();
    for (from, to) in edges {
        successors.entry(from).or_default().push(to);
        predecessors.entry(to).or_default().push(from);
    }
    for id in order.iter().rev() {
        if has_same_lane_predecessor(id, predecessors.get(id).into_iter().flatten()) {
            continue;
        }
        let Some(targets) = successors.get(id) else {
            continue;
        };
        let Some(min_succ) = targets
            .iter()
            .filter_map(|target| ranks.get(*target).copied())
            .min()
        else {
            continue;
        };
        if min_succ == 0 {
            continue;
        }
        if let Some(rank) = ranks.get_mut(id) {
            *rank = (*rank).max(min_succ - 1);
        }
    }
}

fn has_same_lane_predecessor<'a>(
    id: &AnalysisNodeId,
    mut predecessors: impl Iterator<Item = &'a &'a AnalysisNodeId>,
) -> bool {
    let lane = swimlane_of(id);
    if matches!(lane, Swimlane::Shared) {
        return false;
    }
    predecessors.any(|pred| {
        let pred_lane = swimlane_of(pred);
        pred_lane == lane
    })
}

fn lane_sort_key(id: &AnalysisNodeId) -> u8 {
    node_swimlane(id)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Swimlane {
    Music = 0,
    Stems = 1,
    Lyrics = 2,
    Shared = 3,
}

fn node_swimlane(id: &AnalysisNodeId) -> u8 {
    match swimlane_of(id) {
        Swimlane::Music => 0,
        Swimlane::Stems => 1,
        Swimlane::Lyrics => 2,
        Swimlane::Shared => 1,
    }
}

fn swimlane_of(id: &AnalysisNodeId) -> Swimlane {
    match id.as_str() {
        "preflight"
        | "chart.build_candidate"
        | "artifact.chart"
        | "export.utz"
        | "export.ultrastar" => Swimlane::Shared,
        value if value.starts_with("music.") || value == "artifact.music_analysis" => {
            Swimlane::Music
        }
        value
            if value.starts_with("lyrics.")
                || matches!(
                    value,
                    "artifact.timed_lyrics"
                        | "artifact.lyrics"
                        | "artifact.recognized_text"
                        | "artifact.lyrics_input"
                ) =>
        {
            Swimlane::Lyrics
        }
        _ => Swimlane::Stems,
    }
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

    let mut nodes_by_rank: BTreeMap<u32, Vec<AnalysisNodeId>> = BTreeMap::new();
    for id in &order {
        let rank = *ranks.get(id).unwrap_or(&0);
        nodes_by_rank.entry(rank).or_default().push(id.clone());
    }
    for column in nodes_by_rank.values_mut() {
        column.sort_by(|left, right| {
            lane_sort_key(left)
                .cmp(&lane_sort_key(right))
                .then_with(|| left.cmp(right))
        });
    }

    let row_step = spacing.node_height + spacing.row_gap;
    let mut max_rows = [1usize; 3];
    for column in nodes_by_rank.values() {
        let mut counts = [0usize; 3];
        for id in column {
            if let Some(lane) = match swimlane_of(id) {
                Swimlane::Music => Some(0),
                Swimlane::Stems => Some(1),
                Swimlane::Lyrics => Some(2),
                Swimlane::Shared => None,
            } {
                counts[lane] += 1;
            }
        }
        for (lane, count) in counts.iter().enumerate() {
            max_rows[lane] = max_rows[lane].max((*count).max(1));
        }
    }
    let lane_gap = spacing.row_gap * 2.0;
    let music_height = max_rows[0] as f32 * row_step - spacing.row_gap;
    let stems_height = max_rows[1] as f32 * row_step - spacing.row_gap;
    let lyrics_height = max_rows[2] as f32 * row_step - spacing.row_gap;
    let music_top = spacing.margin;
    let stems_top = music_top + music_height + lane_gap;
    let lyrics_top = stems_top + stems_height + lane_gap;
    let shared_center = (music_top + lyrics_top + lyrics_height) / 2.0;

    let mut rects = BTreeMap::new();
    let mut max_rank = 0u32;

    for (rank, column) in &nodes_by_rank {
        max_rank = max_rank.max(*rank);
        let x = spacing.margin + *rank as f32 * (spacing.node_width + spacing.column_gap);
        let mut lane_row = [0usize; 3];
        let shared: Vec<&AnalysisNodeId> = column
            .iter()
            .filter(|id| swimlane_of(id) == Swimlane::Shared)
            .collect();
        let shared_start =
            shared_center - (shared.len().max(1) as f32 * row_step - spacing.row_gap) / 2.0;
        let mut shared_row = 0usize;
        for id in column {
            let y = match swimlane_of(id) {
                Swimlane::Shared => {
                    let y = shared_start + shared_row as f32 * row_step;
                    shared_row += 1;
                    y
                }
                lane => {
                    let index = lane as usize;
                    let top = match lane {
                        Swimlane::Music => music_top,
                        Swimlane::Stems => stems_top,
                        Swimlane::Lyrics => lyrics_top,
                        Swimlane::Shared => unreachable!(),
                    };
                    let y = top + lane_row[index] as f32 * row_step;
                    lane_row[index] += 1;
                    y
                }
            };
            rects.insert(
                id.clone(),
                LayoutRect {
                    x,
                    y,
                    width: spacing.node_width,
                    height: spacing.node_height,
                },
            );
        }
    }

    let canvas_width = spacing.margin * 2.0
        + (max_rank + 1) as f32 * spacing.node_width
        + max_rank as f32 * spacing.column_gap;
    let canvas_height = lyrics_top + lyrics_height + spacing.margin;

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

    fn spec_nodes(names: &[&str]) -> Vec<app_core::AnalysisNodeSpec> {
        names
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
            .collect()
    }

    fn spec_edge(from: &str, to: &str) -> app_core::AnalysisEdge {
        app_core::AnalysisEdge {
            from: id(from),
            to: id(to),
        }
    }

    fn collinear_overlap(
        a0: LayoutPoint,
        a1: LayoutPoint,
        b0: LayoutPoint,
        b1: LayoutPoint,
    ) -> bool {
        let a_horizontal = (a0.y - a1.y).abs() <= 0.5;
        let b_horizontal = (b0.y - b1.y).abs() <= 0.5;
        if a_horizontal != b_horizontal {
            return false;
        }
        if a_horizontal {
            if (a0.y - b0.y).abs() >= 1.0 {
                return false;
            }
            let a_lo = a0.x.min(a1.x);
            let a_hi = a0.x.max(a1.x);
            let b_lo = b0.x.min(b1.x);
            let b_hi = b0.x.max(b1.x);
            a_lo < b_hi - 1.0 && b_lo < a_hi - 1.0
        } else {
            if (a0.x - b0.x).abs() >= 1.0 {
                return false;
            }
            let a_lo = a0.y.min(a1.y);
            let a_hi = a0.y.max(a1.y);
            let b_lo = b0.y.min(b1.y);
            let b_hi = b0.y.max(b1.y);
            a_lo < b_hi - 1.0 && b_lo < a_hi - 1.0
        }
    }

    #[test]
    fn pitch_stays_beside_the_vocal_file_instead_of_sliding_to_alignment() {
        let nodes = spec_nodes(&[
            "preflight",
            "stems.vocals",
            "artifact.raw_vocal",
            "pitch.extract",
            "artifact.note_guide",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "artifact.lyrics",
            "lyrics.align",
            "artifact.timed_lyrics",
            "chart.build_candidate",
        ]);
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes,
            edges: vec![
                spec_edge("preflight", "stems.vocals"),
                spec_edge("stems.vocals", "artifact.raw_vocal"),
                spec_edge("artifact.raw_vocal", "pitch.extract"),
                spec_edge("pitch.extract", "artifact.note_guide"),
                spec_edge("artifact.note_guide", "chart.build_candidate"),
                spec_edge("artifact.raw_vocal", "lyrics.preprocess"),
                spec_edge("lyrics.preprocess", "lyrics.transcribe"),
                spec_edge("lyrics.transcribe", "artifact.lyrics"),
                spec_edge("artifact.lyrics", "lyrics.align"),
                spec_edge("lyrics.align", "artifact.timed_lyrics"),
                spec_edge("artifact.timed_lyrics", "chart.build_candidate"),
            ],
        };
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let vocal = layout.rect(&id("artifact.raw_vocal")).unwrap();
        let pitch = layout.rect(&id("pitch.extract")).unwrap();
        let align = layout.rect(&id("lyrics.align")).unwrap();
        assert!(
            (pitch.x - vocal.x).abs()
                <= LayoutSpacing::default().node_width + LayoutSpacing::default().column_gap + 1.0,
            "pitch should sit in the next column after the vocal file, got vocal.x={} pitch.x={}",
            vocal.x,
            pitch.x
        );
        assert!(
            pitch.x < align.x,
            "pitch must not be pulled into the alignment column (pitch.x={} align.x={})",
            pitch.x,
            align.x
        );
    }

    #[test]
    fn music_stems_and_lyrics_occupy_distinct_vertical_lanes() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let music = layout.rect(&id("music.analysis")).unwrap();
        let stems = layout.rect(&id("stems.separate")).unwrap();
        let lyrics = layout.rect(&id("lyrics.preprocess")).unwrap();
        assert!(
            music.y + music.height <= stems.y,
            "music lane should sit above stems"
        );
        assert!(
            stems.y + stems.height <= lyrics.y,
            "stems lane should sit above lyrics"
        );
    }

    #[test]
    fn lane_bands_group_branch_nodes_without_absorbing_shared_nodes() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let bands = layout.lane_bands();
        assert_eq!(bands.len(), 3);
        assert_eq!(bands[0].kind, LayoutLaneKind::Music);
        assert_eq!(bands[1].kind, LayoutLaneKind::VocalsAndPitch);
        assert_eq!(bands[2].kind, LayoutLaneKind::LyricsAndTiming);

        for (band, node_id) in [
            (&bands[0], "music.analysis"),
            (&bands[1], "pitch.extract"),
            (&bands[2], "lyrics.align"),
        ] {
            let node = layout.rect(&id(node_id)).unwrap();
            assert!(band.rect.x <= node.x);
            assert!(band.rect.y <= node.y);
            assert!(band.rect.right() >= node.right());
            assert!(band.rect.bottom() >= node.bottom());
        }

        let preflight = layout.rect(&id("preflight")).unwrap();
        assert!(
            bands
                .iter()
                .all(|band| { preflight.x < band.rect.x || preflight.x > band.rect.right() })
        );
    }

    #[test]
    fn timed_lyrics_import_sits_in_the_lyrics_column_not_with_stems() {
        let graph = app_core::baseline_graph_spec();
        let layout = layered_layout(&graph, LayoutSpacing::default()).unwrap();
        let import = layout.rect(&id("lyrics.import_timed")).unwrap();
        let align = layout.rect(&id("lyrics.align")).unwrap();
        let stems = layout.rect(&id("stems.separate")).unwrap();
        assert_eq!(
            import.x, align.x,
            "import must share the lyrics layer with alignment"
        );
        assert!(
            import.x > stems.x,
            "import must sit to the right of stem separation, not in its column"
        );
    }

    #[test]
    fn skip_into_timed_lyrics_import_wraps_below_the_node_stack() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&[
                "preflight",
                "stems.separate",
                "lyrics.import_timed",
                "lyrics.preprocess",
                "lyrics.align",
                "artifact.timed_lyrics",
            ]),
            edges: vec![
                spec_edge("preflight", "stems.separate"),
                spec_edge("preflight", "lyrics.import_timed"),
                spec_edge("stems.separate", "lyrics.preprocess"),
                spec_edge("lyrics.preprocess", "lyrics.align"),
                spec_edge("lyrics.align", "artifact.timed_lyrics"),
                spec_edge("lyrics.import_timed", "artifact.timed_lyrics"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        let import_box = routed.layout.rect(&id("lyrics.import_timed")).unwrap();
        let stems_box = routed.layout.rect(&id("stems.separate")).unwrap();
        assert!(import_box.x > stems_box.x);
        let from = routed.layout.rect(&id("preflight")).unwrap();
        let to = import_box;
        assert!(
            is_long_span(from, to, spacing),
            "preflight to import is the remaining skip and must not cross the stem column on the top rail"
        );
        let path = routed
            .path(&id("preflight"), &id("lyrics.import_timed"))
            .expect("import incoming path");
        let rail_y = longest_horizontal_y(path).expect("horizontal rail");
        assert!(
            rail_y > import_box.y + import_box.height,
            "import skip rail {rail_y} should sit below the import node at y={}",
            import_box.y
        );
    }

    #[test]
    fn long_span_rails_do_not_share_a_horizontal_line() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&[
                "preflight",
                "stems.separate",
                "lyrics.import_timed",
                "pitch.extract",
                "lyrics.preprocess",
                "lyrics.transcribe",
                "chart.build_candidate",
                "artifact.timed_lyrics",
            ]),
            edges: vec![
                spec_edge("preflight", "stems.separate"),
                spec_edge("preflight", "lyrics.import_timed"),
                spec_edge("stems.separate", "pitch.extract"),
                spec_edge("stems.separate", "lyrics.preprocess"),
                spec_edge("lyrics.preprocess", "lyrics.transcribe"),
                spec_edge("pitch.extract", "chart.build_candidate"),
                spec_edge("lyrics.transcribe", "chart.build_candidate"),
                spec_edge("lyrics.transcribe", "artifact.timed_lyrics"),
                spec_edge("lyrics.import_timed", "chart.build_candidate"),
                spec_edge("lyrics.import_timed", "artifact.timed_lyrics"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        let mut rails = Vec::new();
        for (from, to) in &pairs {
            let from_rect = routed.layout.rect(from).unwrap();
            let to_rect = routed.layout.rect(to).unwrap();
            if !is_long_span(from_rect, to_rect, spacing) {
                continue;
            }
            let path = routed.path(from, to).unwrap();
            rails.push(longest_horizontal_y(path).unwrap());
        }
        rails.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in rails.windows(2) {
            assert!(
                (pair[1] - pair[0]).abs() >= RAIL_GAP - 0.1,
                "long-span rails coincide: {:?}",
                rails
            );
        }
    }

    #[test]
    fn routed_edges_never_share_a_collinear_segment() {
        let graph = app_core::baseline_graph_spec();
        let mut nodes: Vec<AnalysisNodeId> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        nodes.push(id("artifact.timed_lyrics"));
        let mut edges: Vec<(AnalysisNodeId, AnalysisNodeId)> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        edges.push((id("lyrics.import_timed"), id("artifact.timed_lyrics")));
        edges.push((id("lyrics.align"), id("artifact.timed_lyrics")));
        edges.push((id("lyrics.transcribe"), id("artifact.timed_lyrics")));
        let spacing = LayoutSpacing::canvas();
        let layout = layered_layout_from_edges(&nodes, &edges, spacing).unwrap();
        let routed = route_layered_edges(&layout, &edges, spacing);
        let on_node_face = |p0: LayoutPoint, p1: LayoutPoint| -> bool {
            routed.layout.rects.values().any(|rect| {
                let vertical = (p0.x - p1.x).abs() <= 0.5;
                if vertical {
                    let on_left = (p0.x - rect.x).abs() < 0.5;
                    let on_right = (p0.x - rect.right()).abs() < 0.5;
                    return on_left || on_right;
                }
                let on_top = (p0.y - rect.y).abs() < 0.5;
                let on_bottom = (p0.y - rect.bottom()).abs() < 0.5;
                on_top || on_bottom
            })
        };
        let paths: Vec<_> = edges
            .iter()
            .filter_map(|(from, to)| routed.path(from, to).map(|path| path.to_vec()))
            .collect();
        for (left_index, left) in paths.iter().enumerate() {
            for right in paths.iter().skip(left_index + 1) {
                for a in left.windows(2) {
                    if on_node_face(a[0], a[1]) {
                        continue;
                    }
                    for b in right.windows(2) {
                        if on_node_face(b[0], b[1]) {
                            continue;
                        }
                        assert!(
                            !collinear_overlap(a[0], a[1], b[0], b[1]),
                            "edges share a segment at {:?} {:?} vs {:?} {:?}",
                            a[0],
                            a[1],
                            b[0],
                            b[1]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bottom_rails_stay_inside_the_expanded_canvas() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&[
                "preflight",
                "lyrics.import_timed",
                "a",
                "b",
                "c",
                "artifact.timed_lyrics",
            ]),
            edges: vec![
                spec_edge("preflight", "lyrics.import_timed"),
                spec_edge("preflight", "a"),
                spec_edge("a", "b"),
                spec_edge("b", "c"),
                spec_edge("c", "artifact.timed_lyrics"),
                spec_edge("lyrics.import_timed", "artifact.timed_lyrics"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        for path in routed.paths.values() {
            for point in path {
                assert!(point.y >= 0.0);
                assert!(point.y <= routed.layout.canvas_height);
            }
        }
        for rect in routed.layout.rects.values() {
            assert!(rect.y + rect.height <= routed.layout.canvas_height);
        }
    }

    #[test]
    fn fan_out_to_a_lower_lane_leaves_from_the_bottom() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&[
                "artifact.dereverbed_vocal",
                "pitch.extract",
                "lyrics.preprocess",
            ]),
            edges: vec![
                spec_edge("artifact.dereverbed_vocal", "pitch.extract"),
                spec_edge("artifact.dereverbed_vocal", "lyrics.preprocess"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        let vocal = routed
            .layout
            .rect(&id("artifact.dereverbed_vocal"))
            .unwrap();
        let pitch = routed.layout.rect(&id("pitch.extract")).unwrap();
        let lyrics = routed.layout.rect(&id("lyrics.preprocess")).unwrap();
        assert!(lyrics.y >= vocal.bottom());
        assert!((pitch.y - vocal.y).abs() < vocal.height);

        let to_pitch = routed
            .path(&id("artifact.dereverbed_vocal"), &id("pitch.extract"))
            .expect("side hop to pitch");
        assert!(
            (to_pitch[0].x - vocal.right()).abs() < 0.5,
            "pitch should keep the right-side exit, got {:?}",
            to_pitch
        );

        let to_lyrics = routed
            .path(&id("artifact.dereverbed_vocal"), &id("lyrics.preprocess"))
            .expect("L hop to lyrics");
        assert!(
            (to_lyrics[0].y - vocal.bottom()).abs() < 0.5,
            "lyrics should leave from the bottom face, got {:?}",
            to_lyrics
        );
        assert!(
            to_lyrics[0].x >= vocal.x && to_lyrics[0].x <= vocal.right(),
            "bottom exit should sit on the vocal box, got {:?}",
            to_lyrics
        );
        let has_down_then_right = to_lyrics
            .windows(2)
            .any(|pair| pair[1].y > pair[0].y + 0.5 && (pair[0].x - pair[1].x).abs() <= 0.5)
            && to_lyrics
                .windows(2)
                .any(|pair| pair[1].x > pair[0].x + 0.5 && (pair[0].y - pair[1].y).abs() <= 0.5);
        assert!(
            has_down_then_right,
            "lyrics path should be an L (down, then right): {:?}",
            to_lyrics
        );
        assert!(
            to_lyrics
                .iter()
                .all(|point| (point.x - vocal.right()).abs() > 0.5
                    || point.y >= vocal.bottom() - 0.5),
            "lyrics L must not travel along the vocal right face: {:?}",
            to_lyrics
        );
    }

    #[test]
    fn fan_out_to_an_upper_lane_leaves_from_the_top() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&["preflight", "music.analysis", "stems.separate"]),
            edges: vec![
                spec_edge("preflight", "music.analysis"),
                spec_edge("preflight", "stems.separate"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        let preflight = routed.layout.rect(&id("preflight")).unwrap();
        let music = routed.layout.rect(&id("music.analysis")).unwrap();
        assert!(preflight.y >= music.bottom());

        let to_music = routed
            .path(&id("preflight"), &id("music.analysis"))
            .expect("L hop to music");
        assert!(
            (to_music[0].y - preflight.y).abs() < 0.5,
            "music should leave from the top face, got {:?}",
            to_music
        );

        let to_stems = routed
            .path(&id("preflight"), &id("stems.separate"))
            .expect("side hop to stems");
        assert!(
            (to_stems[0].x - preflight.right()).abs() < 0.5,
            "overlapping stems hop should keep the side exit, got {:?}",
            to_stems
        );
    }

    #[test]
    fn same_row_skip_to_alignment_leaves_from_the_bottom() {
        let graph = AnalysisGraphSpec {
            schema_version: 1,
            nodes: spec_nodes(&["lyrics.preprocess", "lyrics.transcribe", "lyrics.align"]),
            edges: vec![
                spec_edge("lyrics.preprocess", "lyrics.transcribe"),
                spec_edge("lyrics.preprocess", "lyrics.align"),
                spec_edge("lyrics.transcribe", "lyrics.align"),
            ],
        };
        let spacing = LayoutSpacing::default();
        let layout = layered_layout(&graph, spacing).unwrap();
        let pairs: Vec<_> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let routed = route_layered_edges(&layout, &pairs, spacing);
        let preprocess = routed.layout.rect(&id("lyrics.preprocess")).unwrap();
        let transcribe = routed.layout.rect(&id("lyrics.transcribe")).unwrap();
        let align = routed.layout.rect(&id("lyrics.align")).unwrap();
        assert!(
            (preprocess.y - transcribe.y).abs() < 1.0 && (transcribe.y - align.y).abs() < 1.0,
            "the three lyrics nodes should share a row"
        );
        assert!(
            is_long_span(preprocess, align, spacing),
            "preprocess to align skips the transcribe column"
        );

        let to_transcribe = routed
            .path(&id("lyrics.preprocess"), &id("lyrics.transcribe"))
            .expect("side hop to transcribe");
        assert!(
            (to_transcribe[0].x - preprocess.right()).abs() < 0.5,
            "transcription should keep the right-side exit, got {:?}",
            to_transcribe
        );

        let to_align = routed
            .path(&id("lyrics.preprocess"), &id("lyrics.align"))
            .expect("under-row hop to align");
        assert!(
            (to_align[0].y - preprocess.bottom()).abs() < 0.5,
            "alignment skip should leave from the bottom face, got {:?}",
            to_align
        );
        assert!(
            to_align[0].x >= preprocess.x && to_align[0].x <= preprocess.right(),
            "bottom exit should sit on the preprocess box, got {:?}",
            to_align
        );
        let rail_y = longest_horizontal_y(to_align).expect("under-row rail");
        assert!(
            rail_y >= preprocess.bottom() && rail_y >= align.bottom(),
            "alignment skip should run under the row at {rail_y}, boxes end at {} / {}",
            preprocess.bottom(),
            align.bottom()
        );
        assert!(
            to_align
                .iter()
                .all(|point| (point.x - preprocess.right()).abs() > 0.5
                    || point.y >= preprocess.bottom() - 0.5),
            "alignment skip must not travel along the preprocess right face: {:?}",
            to_align
        );
    }
}
