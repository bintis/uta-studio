//! Stable crossing minimization for the layered Analysis DAG layout.
//!
//! The implementation follows the shape of a Sugiyama ordering phase without
//! importing a second graph runtime: downward/upward weighted-median sweeps are
//! accepted only when they strictly reduce crossings, then an adjacent
//! transpose pass removes remaining local inversions. The caller-provided
//! order hint is the deterministic tie-breaker so adding an unrelated node
//! does not reshuffle the graph.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

const SWEEP_ROUNDS: usize = 4;

pub(super) fn minimize_crossings(
    layers: &mut BTreeMap<u32, Vec<AnalysisNodeId>>,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    order_hints: &BTreeMap<AnalysisNodeId, usize>,
) {
    sort_by_stable_order(layers, order_hints);
    if layers.len() < 2 || edges.len() < 2 {
        return;
    }

    let incoming = adjacency(edges, false);
    let outgoing = adjacency(edges, true);

    for _ in 0..SWEEP_ROUNDS {
        let mut changed = false;
        changed |= accept_better_sweep(layers, edges, order_hints, &incoming, false);
        changed |= accept_better_sweep(layers, edges, order_hints, &outgoing, true);
        changed |= transpose_adjacent(layers, edges);
        if !changed {
            break;
        }
    }
}

fn sort_by_stable_order(
    layers: &mut BTreeMap<u32, Vec<AnalysisNodeId>>,
    order_hints: &BTreeMap<AnalysisNodeId, usize>,
) {
    for layer in layers.values_mut() {
        layer.sort_by(|left, right| {
            stable_hint(left, order_hints)
                .cmp(&stable_hint(right, order_hints))
                .then_with(|| left.cmp(right))
        });
    }
}

fn stable_hint(id: &AnalysisNodeId, order_hints: &BTreeMap<AnalysisNodeId, usize>) -> usize {
    order_hints.get(id).copied().unwrap_or(usize::MAX)
}

fn adjacency(
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    outgoing: bool,
) -> BTreeMap<AnalysisNodeId, Vec<AnalysisNodeId>> {
    let mut result: BTreeMap<AnalysisNodeId, Vec<AnalysisNodeId>> = BTreeMap::new();
    for (from, to) in edges {
        let (key, value) = if outgoing { (from, to) } else { (to, from) };
        result.entry(key.clone()).or_default().push(value.clone());
    }
    for neighbors in result.values_mut() {
        neighbors.sort();
        neighbors.dedup();
    }
    result
}

fn accept_better_sweep(
    layers: &mut BTreeMap<u32, Vec<AnalysisNodeId>>,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    order_hints: &BTreeMap<AnalysisNodeId, usize>,
    adjacency: &BTreeMap<AnalysisNodeId, Vec<AnalysisNodeId>>,
    upward: bool,
) -> bool {
    let before = count_crossings(layers, edges);
    let mut candidate = layers.clone();
    let ranks = candidate.keys().copied().collect::<Vec<_>>();
    let traversal: Box<dyn Iterator<Item = u32>> = if upward {
        Box::new(ranks.into_iter().rev())
    } else {
        Box::new(ranks.into_iter())
    };

    for rank in traversal {
        let positions = normalized_positions(&candidate);
        if let Some(layer) = candidate.get_mut(&rank) {
            reorder_layer(layer, adjacency, &positions, order_hints);
        }
    }

    let after = count_crossings(&candidate, edges);
    if after < before {
        *layers = candidate;
        true
    } else {
        false
    }
}

fn normalized_positions(
    layers: &BTreeMap<u32, Vec<AnalysisNodeId>>,
) -> BTreeMap<AnalysisNodeId, f64> {
    let mut positions = BTreeMap::new();
    for layer in layers.values() {
        let denominator = layer.len().max(1) as f64;
        for (index, id) in layer.iter().enumerate() {
            positions.insert(id.clone(), (index as f64 + 0.5) / denominator);
        }
    }
    positions
}

fn reorder_layer(
    layer: &mut [AnalysisNodeId],
    adjacency: &BTreeMap<AnalysisNodeId, Vec<AnalysisNodeId>>,
    positions: &BTreeMap<AnalysisNodeId, f64>,
    order_hints: &BTreeMap<AnalysisNodeId, usize>,
) {
    layer.sort_by(|left, right| {
        let left_score = neighbor_median(left, adjacency, positions);
        let right_score = neighbor_median(right, adjacency, positions);
        compare_optional_scores(left_score, right_score)
            .then_with(|| stable_hint(left, order_hints).cmp(&stable_hint(right, order_hints)))
            .then_with(|| left.cmp(right))
    });
}

fn neighbor_median(
    id: &AnalysisNodeId,
    adjacency: &BTreeMap<AnalysisNodeId, Vec<AnalysisNodeId>>,
    positions: &BTreeMap<AnalysisNodeId, f64>,
) -> Option<f64> {
    let mut values = adjacency
        .get(id)?
        .iter()
        .filter_map(|neighbor| positions.get(neighbor).copied())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[middle - 1] + values[middle]) * 0.5)
    } else {
        Some(values[middle])
    }
}

fn compare_optional_scores(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

fn transpose_adjacent(
    layers: &mut BTreeMap<u32, Vec<AnalysisNodeId>>,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
) -> bool {
    let mut changed_any = false;
    loop {
        let mut changed_round = false;
        let ranks = layers.keys().copied().collect::<Vec<_>>();
        for rank in ranks {
            let layer_len = layers.get(&rank).map(Vec::len).unwrap_or(0);
            if layer_len < 2 {
                continue;
            }
            let mut index = 0;
            while index + 1 < layer_len {
                let before = count_crossings(layers, edges);
                let mut candidate = layers.clone();
                candidate
                    .get_mut(&rank)
                    .expect("known rank")
                    .swap(index, index + 1);
                let after = count_crossings(&candidate, edges);
                if after < before {
                    *layers = candidate;
                    changed_round = true;
                    changed_any = true;
                } else {
                    index += 1;
                }
            }
        }
        if !changed_round {
            break;
        }
    }
    changed_any
}

pub(super) fn count_crossings(
    layers: &BTreeMap<u32, Vec<AnalysisNodeId>>,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
) -> usize {
    let mut rank_by_node = BTreeMap::new();
    let mut position_by_node = BTreeMap::new();
    for (rank, layer) in layers {
        let denominator = layer.len().max(1) as f64;
        for (position, id) in layer.iter().enumerate() {
            rank_by_node.insert(id.clone(), *rank);
            position_by_node.insert(id.clone(), (position as f64 + 0.5) / denominator);
        }
    }

    let mut direct: BTreeMap<(u32, u32), Vec<(f64, f64)>> = BTreeMap::new();
    let mut virtual_segments: BTreeMap<(u32, u32), Vec<(f64, f64)>> = BTreeMap::new();
    for (from, to) in edges {
        let (Some(from_rank), Some(to_rank), Some(from_position), Some(to_position)) = (
            rank_by_node.get(from).copied(),
            rank_by_node.get(to).copied(),
            position_by_node.get(from).copied(),
            position_by_node.get(to).copied(),
        ) else {
            continue;
        };
        if from_rank == to_rank {
            continue;
        }
        let (start_rank, end_rank, start_position, end_position) = if from_rank < to_rank {
            (from_rank, to_rank, from_position, to_position)
        } else {
            (to_rank, from_rank, to_position, from_position)
        };
        let span_ranks = end_rank - start_rank;
        if span_ranks > 1 {
            direct
                .entry((start_rank, end_rank))
                .or_default()
                .push((start_position, end_position));
        }

        let span = span_ranks as f64;
        for rank in start_rank..end_rank {
            let start_t = (rank - start_rank) as f64 / span;
            let end_t = (rank + 1 - start_rank) as f64 / span;
            virtual_segments.entry((rank, rank + 1)).or_default().push((
                interpolate(start_position, end_position, start_t),
                interpolate(start_position, end_position, end_t),
            ));
        }
    }

    direct
        .values()
        .map(|segments| inversion_count(segments))
        .sum::<usize>()
        + virtual_segments
            .values()
            .map(|segments| inversion_count(segments))
            .sum::<usize>()
}

fn interpolate(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

fn inversion_count(segments: &[(f64, f64)]) -> usize {
    let mut crossings = 0;
    for left in 0..segments.len() {
        for right in left + 1..segments.len() {
            let (left_from, left_to) = segments[left];
            let (right_from, right_to) = segments[right];
            if approximately_equal(left_from, right_from) || approximately_equal(left_to, right_to)
            {
                continue;
            }
            if (left_from < right_from) != (left_to < right_to) {
                crossings += 1;
            }
        }
    }
    crossings
}

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON * 16.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> AnalysisNodeId {
        AnalysisNodeId::new(value)
    }

    #[test]
    fn crossed_bilayer_is_reordered_only_when_crossings_drop() {
        let mut layers = BTreeMap::from([(0, vec![id("a"), id("b")]), (1, vec![id("c"), id("d")])]);
        let edges = vec![(id("a"), id("d")), (id("b"), id("c"))];
        let hints = ["a", "b", "c", "d"]
            .into_iter()
            .enumerate()
            .map(|(index, value)| (id(value), index))
            .collect();

        assert_eq!(count_crossings(&layers, &edges), 1);
        minimize_crossings(&mut layers, &edges, &hints);
        assert_eq!(count_crossings(&layers, &edges), 0);
        assert_eq!(layers[&1], vec![id("d"), id("c")]);
    }

    #[test]
    fn equal_quality_keeps_the_stable_hint_order() {
        let mut layers = BTreeMap::from([(0, vec![id("b"), id("a")]), (1, vec![id("c")])]);
        let edges = vec![(id("a"), id("c")), (id("b"), id("c"))];
        let hints = [(id("b"), 0), (id("a"), 1), (id("c"), 2)]
            .into_iter()
            .collect();

        minimize_crossings(&mut layers, &edges, &hints);
        assert_eq!(layers[&0], vec![id("b"), id("a")]);
    }

    #[test]
    fn long_edges_participate_in_adjacent_layer_crossing_counts() {
        let layers = BTreeMap::from([
            (0, vec![id("a"), id("b")]),
            (1, vec![id("m"), id("n")]),
            (2, vec![id("c"), id("d")]),
        ]);
        let edges = vec![(id("b"), id("c")), (id("a"), id("n"))];

        assert_eq!(count_crossings(&layers, &edges), 1);
    }
}
