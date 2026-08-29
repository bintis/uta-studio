//! Generic layered layout for the exact compiled Workflow DAG.
//!
//! The algorithm is pure and Bevy-independent: it derives variable-size
//! columns from topological ranks, minimizes crossings with stable metadata,
//! and assigns private orthogonal rails without interpreting node ids.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};

use app_core::AnalysisNodeId;

#[path = "analysis_layout_order.rs"]
mod order;
use order::minimize_crossings;

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
    /// Screen spacing favors scanability over fitting every node into one dense
    /// fold. Fit can still scale the complete graph down, but branches keep
    /// enough breathing room that labels, ports, and orthogonal rails remain
    /// visually separable at normal zoom.
    pub(crate) fn canvas() -> Self {
        Self {
            node_width: 148.0,
            node_height: 104.0,
            column_gap: 54.0,
            row_gap: 34.0,
            margin: 34.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutNodeVisualKind {
    Compute,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayoutNodeSpec {
    pub(crate) id: AnalysisNodeId,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) order_hint: usize,
}

impl LayoutNodeSpec {
    pub(crate) fn fixed(id: AnalysisNodeId, spacing: LayoutSpacing, order_hint: usize) -> Self {
        Self {
            id,
            width: spacing.node_width,
            height: spacing.node_height,
            order_hint,
        }
    }

    /// Every node in the Advanced Graph renders at one uniform card size
    /// (`LayoutSpacing::canvas()`'s fixed dimensions) regardless of title,
    /// detail text, or port count -- title/detail are accepted for call-site
    /// compatibility but no longer measured, and the render layer clips and
    /// wraps text to fit the fixed card instead.
    pub(crate) fn from_text(
        id: AnalysisNodeId,
        kind: LayoutNodeVisualKind,
        _title: &str,
        _detail: &str,
        order_hint: usize,
    ) -> Self {
        let spacing = LayoutSpacing::canvas();
        match kind {
            LayoutNodeVisualKind::Compute => Self::fixed(id, spacing, order_hint),
        }
    }

    fn normalized(self, spacing: LayoutSpacing) -> Self {
        Self {
            width: sanitize_dimension(self.width, spacing.node_width),
            height: sanitize_dimension(self.height, spacing.node_height),
            ..self
        }
    }

    fn cache_key(&self) -> LayoutNodeCacheKey {
        LayoutNodeCacheKey {
            id: self.id.to_string(),
            width_bits: self.width.to_bits(),
            height_bits: self.height.to_bits(),
            order_hint: self.order_hint,
        }
    }
}

fn sanitize_dimension(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value >= 16.0 {
        value
    } else {
        fallback
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutNodeCacheKey {
    id: String,
    width_bits: u32,
    height_bits: u32,
    order_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    nodes: Vec<LayoutNodeCacheKey>,
    edges: Vec<(String, String)>,
    steps: Vec<u8>,
}

#[derive(Default)]
struct LayoutCache {
    entries: HashMap<LayoutCacheKey, Option<Arc<RoutedGraph>>>,
    insertion_order: VecDeque<LayoutCacheKey>,
}

impl LayoutCache {
    fn evict_one_unused(&mut self) -> bool {
        let candidates = self.insertion_order.len();
        for _ in 0..candidates {
            let Some(key) = self.insertion_order.pop_front() else {
                return false;
            };
            let evictable = self.entries.get(&key).is_none_or(|entry| match entry {
                Some(routed) => Arc::strong_count(routed) == 1,
                None => true,
            });
            if evictable {
                self.entries.remove(&key);
                return true;
            }
            self.insertion_order.push_back(key);
        }
        false
    }

    fn trim_unused_for_insert(&mut self) {
        while self.entries.len() >= LAYOUT_CACHE_LIMIT && self.evict_one_unused() {}
    }
}

const LAYOUT_CACHE_LIMIT: usize = 32;
pub(crate) const FOUR_STEP_LABEL_GUTTER: f32 = 132.0;

/// Caches geometry-aware canvas layout. Runtime state and selection remain out
/// of the key, but node dimensions and stable order hints participate so a
/// locale or node-card shape change can never reuse stale rectangles.
pub(crate) fn cached_canvas_routed_layout_with_specs(
    nodes: &[LayoutNodeSpec],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
) -> Option<Arc<RoutedGraph>> {
    static CACHE: OnceLock<Mutex<LayoutCache>> = OnceLock::new();
    let spacing = LayoutSpacing::canvas();
    let normalized = nodes
        .iter()
        .cloned()
        .map(|node| node.normalized(spacing))
        .collect::<Vec<_>>();
    let key = LayoutCacheKey {
        nodes: normalized.iter().map(LayoutNodeSpec::cache_key).collect(),
        edges: edges
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect(),
        steps: Vec::new(),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(LayoutCache::default()));
    if let Some(cached) = cache.lock().unwrap().entries.get(&key).cloned() {
        return cached;
    }

    let routed = layered_layout_from_specs(&normalized, edges, spacing)
        .map(|layout| route_layered_edges(&layout, edges, spacing))
        .map(Arc::new);

    let mut cache = cache.lock().unwrap();
    // Another thread may have computed the same geometry after our initial
    // lookup. Reuse its Arc instead of replacing a live cache entry.
    if let Some(cached) = cache.entries.get(&key).cloned() {
        return cached;
    }
    cache.trim_unused_for_insert();
    cache.insertion_order.push_back(key.clone());
    cache.entries.insert(key, routed.clone());
    routed
}

/// Four-row Processing Studio layout: each numbered product step is one
/// horizontal execution lane and every node in that lane represents one
/// concrete Engine/model operation.
pub(crate) fn cached_four_step_horizontal_routed_layout_with_specs(
    nodes: &[(LayoutNodeSpec, u8)],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
) -> Option<Arc<RoutedGraph>> {
    static CACHE: OnceLock<Mutex<LayoutCache>> = OnceLock::new();
    let spacing = LayoutSpacing::canvas();
    let normalized = nodes
        .iter()
        .map(|(node, step)| (node.clone().normalized(spacing), (*step).clamp(1, 4)))
        .collect::<Vec<_>>();
    let key = LayoutCacheKey {
        nodes: normalized
            .iter()
            .map(|(node, _)| node.cache_key())
            .collect(),
        edges: edges
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect(),
        steps: normalized.iter().map(|(_, step)| *step).collect(),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(LayoutCache::default()));
    if let Some(cached) = cache.lock().unwrap().entries.get(&key).cloned() {
        return cached;
    }
    let routed = four_step_horizontal_layout(&normalized, edges, spacing)
        .map(|layout| route_layered_edges(&layout, edges, spacing))
        .map(Arc::new);
    let mut cache = cache.lock().unwrap();
    if let Some(cached) = cache.entries.get(&key).cloned() {
        return cached;
    }
    cache.trim_unused_for_insert();
    cache.insertion_order.push_back(key.clone());
    cache.entries.insert(key, routed.clone());
    routed
}

fn four_step_horizontal_layout(
    nodes: &[(LayoutNodeSpec, u8)],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> Option<GraphLayout> {
    let ids = nodes
        .iter()
        .map(|(node, _)| node.id.clone())
        .collect::<Vec<_>>();
    let topo = topo_order_from_edges(&ids, edges)?;
    let topo_index = topo
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let specs = nodes
        .iter()
        .map(|(node, step)| (node.id.clone(), (node, *step)))
        .collect::<BTreeMap<_, _>>();
    let mut rows: BTreeMap<u8, Vec<&LayoutNodeSpec>> = BTreeMap::new();
    for (node, step) in nodes {
        rows.entry(*step).or_default().push(node);
    }
    for row in rows.values_mut() {
        row.sort_by_key(|node| {
            (
                topo_index.get(&node.id).copied().unwrap_or(usize::MAX),
                node.order_hint,
            )
        });
    }
    let mut rects = BTreeMap::new();
    let mut y = spacing.margin;
    let row_start_x = spacing.margin + FOUR_STEP_LABEL_GUTTER;
    let mut canvas_width = row_start_x + spacing.margin;
    for step in 1..=4 {
        let row = rows.get(&step).cloned().unwrap_or_default();
        let row_height = row
            .iter()
            .map(|node| node.height)
            .fold(spacing.node_height, f32::max);
        let mut x = row_start_x;
        for node in row {
            rects.insert(
                node.id.clone(),
                LayoutRect {
                    x,
                    y: y + (row_height - node.height) * 0.5,
                    width: node.width,
                    height: node.height,
                },
            );
            x += node.width + spacing.column_gap;
        }
        canvas_width = canvas_width.max(x - spacing.column_gap + spacing.margin);
        y += row_height + spacing.row_gap * 1.7;
    }
    // Every normalized node must have exactly one row assignment.
    if rects.len() != specs.len() {
        return None;
    }
    Some(GraphLayout {
        rects,
        canvas_width,
        canvas_height: y - spacing.row_gap * 1.7 + spacing.margin,
    })
}

const RAIL_GAP: f32 = 8.0;
const PORT_STUB: f32 = 8.0;
const UNDER_RAIL_CLEARANCE: f32 = 28.0;
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

fn prefers_bottom_rail(from_rect: LayoutRect, to_rect: LayoutRect) -> bool {
    to_rect.y >= from_rect.bottom()
}

/// A long edge between nodes on the same visual row does not need a detour
/// when the horizontal corridor is empty. Keep this conservative by
/// treating the source node's complete vertical span as occupied: if any
/// intermediate card overlaps that span, the normal under-row route still
/// wins.
fn horizontal_corridor_is_clear(
    layout: &GraphLayout,
    from_id: &AnalysisNodeId,
    to_id: &AnalysisNodeId,
    from: LayoutRect,
    to: LayoutRect,
) -> bool {
    let left = from.right().min(to.right());
    let right = from.x.max(to.x);
    layout.rects.iter().all(|(id, rect)| {
        if id == from_id || id == to_id {
            return true;
        }
        let overlaps_x = rect.x < right - 0.5 && rect.right() > left + 0.5;
        !overlaps_x || !vertically_overlap(from, *rect)
    })
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
                if horizontal_corridor_is_clear(layout, from_id, to_id, from, to) {
                    side_edges.push((from_id.clone(), to_id.clone()));
                } else {
                    under_edges.push((from_id.clone(), to_id.clone()));
                }
            } else if prefers_bottom_rail(from, to) {
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

    let edge_order =
        edges
            .iter()
            .enumerate()
            .fold(BTreeMap::new(), |mut order, (index, (from, to))| {
                order.entry((from.clone(), to.clone())).or_insert(index);
                order
            });

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
    for (from, list) in &mut outgoing {
        list.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| {
                    edge_order
                        .get(&(from.clone(), a.0.clone()))
                        .cmp(&edge_order.get(&(from.clone(), b.0.clone())))
                })
                .then_with(|| a.0.cmp(&b.0))
        });
    }
    for (to, list) in &mut incoming {
        list.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| {
                    edge_order
                        .get(&(a.0.clone(), to.clone()))
                        .cmp(&edge_order.get(&(b.0.clone(), to.clone())))
                })
                .then_with(|| a.0.cmp(&b.0))
        });
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

    // Under-row detours are routed after long-span and side paths. Seed the
    // allocator with every horizontal rail already emitted so a compact
    // column gap cannot make a later detour reuse part of an existing rail.
    let mut used_under_y: Vec<f32> = paths
        .values()
        .flat_map(|path| {
            path.windows(2)
                .filter_map(|pair| ((pair[0].y - pair[1].y).abs() <= 0.5).then_some(pair[0].y))
        })
        .collect();
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
            from.bottom().max(to.bottom()) + UNDER_RAIL_CLEARANCE,
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

pub(crate) fn expand_routed_graph_to_viewport(
    routed: &RoutedGraph,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    width: f32,
    height: f32,
) -> RoutedGraph {
    let width = width.max(routed.layout.canvas_width);
    let height = height.max(routed.layout.canvas_height);
    if width <= routed.layout.canvas_width + 1.0 && height <= routed.layout.canvas_height + 1.0 {
        return routed.clone();
    }
    let spacing = LayoutSpacing::canvas();
    let mut layout = routed.layout.clone();
    let horizontal_offset = (width - layout.canvas_width).max(0.0) * 0.5;
    let vertical_offset = (height - layout.canvas_height).max(0.0) * 0.5;
    for rect in layout.rects.values_mut() {
        rect.x += horizontal_offset;
        rect.y += vertical_offset;
    }
    layout.canvas_width = width;
    layout.canvas_height = height;
    route_layered_edges(&layout, edges, spacing)
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

/// Deterministic Kahn topological sort over compiled workflow nodes and
/// bindings. Same tie-breaking shape as
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

fn row_spacing_for_neighbors(
    upper: &AnalysisNodeId,
    lower: &AnalysisNodeId,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> f32 {
    let parents = |node: &AnalysisNodeId| {
        edges
            .iter()
            .filter_map(|(from, to)| (to == node).then_some(from))
            .collect::<Vec<_>>()
    };
    let upper_parents = parents(upper);
    let lower_parents = parents(lower);
    let share_parent = upper_parents
        .iter()
        .any(|parent| lower_parents.contains(parent));
    if share_parent {
        spacing.row_gap
    } else {
        spacing.row_gap * 1.65
    }
}

/// Computes test geometry from a flat node/edge list and fails closed on a
/// cycle.
#[cfg(test)]
pub(crate) fn layered_layout_from_edges(
    nodes: &[AnalysisNodeId],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> Option<GraphLayout> {
    let specs = nodes
        .iter()
        .cloned()
        .enumerate()
        .map(|(order_hint, id)| LayoutNodeSpec::fixed(id, spacing, order_hint))
        .collect::<Vec<_>>();
    layered_layout_from_specs(&specs, edges, spacing)
}

/// Geometry-aware layered layout used by the live DAG canvas. The caller's
/// node order is a stability hint, not a hard placement constraint. Weighted
/// median sweeps may change that order only when the complete graph crossing
/// count strictly improves.
pub(crate) fn layered_layout_from_specs(
    node_specs: &[LayoutNodeSpec],
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    spacing: LayoutSpacing,
) -> Option<GraphLayout> {
    let mut specs_by_id = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut order_hints = BTreeMap::new();
    for spec in node_specs
        .iter()
        .cloned()
        .map(|spec| spec.normalized(spacing))
    {
        if specs_by_id.contains_key(&spec.id) {
            continue;
        }
        order_hints.insert(spec.id.clone(), spec.order_hint);
        nodes.push(spec.id.clone());
        specs_by_id.insert(spec.id.clone(), spec);
    }
    if nodes.is_empty() {
        return Some(GraphLayout {
            rects: BTreeMap::new(),
            canvas_width: spacing.margin * 2.0,
            canvas_height: spacing.margin * 2.0,
        });
    }

    let order = topo_order_from_edges(&nodes, edges)?;
    let ranks = compute_ranks(edges, &order);
    let mut nodes_by_rank: BTreeMap<u32, Vec<AnalysisNodeId>> = BTreeMap::new();
    for id in &order {
        let rank = *ranks.get(id).unwrap_or(&0);
        nodes_by_rank.entry(rank).or_default().push(id.clone());
    }
    minimize_crossings(&mut nodes_by_rank, edges, &order_hints);

    let mut column_widths = BTreeMap::new();
    let mut column_heights = BTreeMap::new();
    for (rank, column) in &nodes_by_rank {
        let width = column
            .iter()
            .filter_map(|id| specs_by_id.get(id).map(|spec| spec.width))
            .fold(spacing.node_width, f32::max);
        let height = column
            .iter()
            .enumerate()
            .map(|(index, id)| {
                specs_by_id
                    .get(id)
                    .map(|spec| spec.height)
                    .unwrap_or(spacing.node_height)
                    + if index == 0 {
                        0.0
                    } else {
                        row_spacing_for_neighbors(&column[index - 1], id, edges, spacing)
                    }
            })
            .sum::<f32>();
        column_widths.insert(*rank, width);
        column_heights.insert(*rank, height);
    }

    let max_rank = nodes_by_rank.keys().next_back().copied().unwrap_or(0);
    let max_column_height = column_heights
        .values()
        .copied()
        .fold(spacing.node_height, f32::max);
    let mut x_by_rank = BTreeMap::new();
    let mut next_x = spacing.margin;
    for rank in 0..=max_rank {
        x_by_rank.insert(rank, next_x);
        next_x += column_widths
            .get(&rank)
            .copied()
            .unwrap_or(spacing.node_width)
            + spacing.column_gap;
    }

    let mut rects = BTreeMap::new();
    for (rank, column) in &nodes_by_rank {
        let column_x = x_by_rank.get(rank).copied().unwrap_or(spacing.margin);
        let column_width = column_widths
            .get(rank)
            .copied()
            .unwrap_or(spacing.node_width);
        let column_height = column_heights.get(rank).copied().unwrap_or(0.0);
        let mut y = spacing.margin + (max_column_height - column_height) * 0.5;
        for (index, id) in column.iter().enumerate() {
            let spec = specs_by_id
                .get(id)
                .cloned()
                .unwrap_or_else(|| LayoutNodeSpec::fixed(id.clone(), spacing, usize::MAX));
            rects.insert(
                id.clone(),
                LayoutRect {
                    x: column_x + (column_width - spec.width) * 0.5,
                    y,
                    width: spec.width,
                    height: spec.height,
                },
            );
            y += spec.height;
            if let Some(next) = column.get(index + 1) {
                y += row_spacing_for_neighbors(id, next, edges, spacing);
            }
        }
    }

    Some(GraphLayout {
        rects,
        canvas_width: next_x - spacing.column_gap + spacing.margin,
        canvas_height: max_column_height + spacing.margin * 2.0,
    })
}

#[cfg(test)]
#[path = "analysis_layout_tests.rs"]
mod tests;
