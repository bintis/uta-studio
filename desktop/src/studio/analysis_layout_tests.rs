use super::*;

fn id(value: &str) -> AnalysisNodeId {
    AnalysisNodeId::new(value)
}

#[test]
fn arbitrary_workflow_ids_follow_topological_layers() {
    let nodes = [
        id("workflow.source"),
        id("workflow.split"),
        id("workflow.pitch"),
        id("workflow.final"),
    ];
    let edges = [
        (id("workflow.source"), id("workflow.split")),
        (id("workflow.split"), id("workflow.pitch")),
        (id("workflow.pitch"), id("workflow.final")),
    ];
    let layout = layered_layout_from_edges(&nodes, &edges, LayoutSpacing::default()).unwrap();
    assert!(layout.rect(&nodes[0]).unwrap().x < layout.rect(&nodes[1]).unwrap().x);
    assert!(layout.rect(&nodes[1]).unwrap().x < layout.rect(&nodes[2]).unwrap().x);
    assert!(layout.rect(&nodes[2]).unwrap().x < layout.rect(&nodes[3]).unwrap().x);
}

#[test]
fn independent_nodes_stack_without_overlap() {
    let spacing = LayoutSpacing::default();
    let specs = vec![
        LayoutNodeSpec {
            id: id("workflow.a"),
            width: 180.0,
            height: 90.0,
            order_hint: 0,
        },
        LayoutNodeSpec {
            id: id("workflow.b"),
            width: 130.0,
            height: 150.0,
            order_hint: 1,
        },
    ];
    let layout = layered_layout_from_specs(&specs, &[], spacing).unwrap();
    let a = layout.rect(&id("workflow.a")).unwrap();
    let b = layout.rect(&id("workflow.b")).unwrap();
    assert!(a.bottom() + spacing.row_gap <= b.y || b.bottom() + spacing.row_gap <= a.y);
}

#[test]
fn unrelated_branches_receive_extra_vertical_separation() {
    let spacing = LayoutSpacing::default();
    let nodes = [id("source.a"), id("source.b"), id("left"), id("right")];
    let edges = [(id("source.a"), id("left")), (id("source.b"), id("right"))];
    let layout = layered_layout_from_edges(&nodes, &edges, spacing).unwrap();
    let left = layout.rect(&id("left")).unwrap();
    let right = layout.rect(&id("right")).unwrap();
    let gap = if left.y < right.y {
        right.y - left.bottom()
    } else {
        left.y - right.bottom()
    };
    assert!(gap >= spacing.row_gap * 1.6);
}

#[test]
fn sibling_branches_keep_the_normal_row_gap() {
    let spacing = LayoutSpacing::default();
    let nodes = [id("source"), id("left"), id("right")];
    let edges = [(id("source"), id("left")), (id("source"), id("right"))];
    let layout = layered_layout_from_edges(&nodes, &edges, spacing).unwrap();
    let left = layout.rect(&id("left")).unwrap();
    let right = layout.rect(&id("right")).unwrap();
    let gap = if left.y < right.y {
        right.y - left.bottom()
    } else {
        left.y - right.bottom()
    };
    assert!((gap - spacing.row_gap).abs() < 0.6);
}

#[test]
fn cycle_fails_closed() {
    let nodes = [id("workflow.a"), id("workflow.b")];
    let edges = [
        (id("workflow.a"), id("workflow.b")),
        (id("workflow.b"), id("workflow.a")),
    ];
    assert!(layered_layout_from_edges(&nodes, &edges, LayoutSpacing::default()).is_none());
}

#[test]
fn routed_edges_end_on_their_real_nodes() {
    let nodes = [
        id("workflow.source"),
        id("workflow.left"),
        id("workflow.right"),
    ];
    let edges = [
        (id("workflow.source"), id("workflow.left")),
        (id("workflow.source"), id("workflow.right")),
    ];
    let layout = layered_layout_from_edges(&nodes, &edges, LayoutSpacing::default()).unwrap();
    let routed = route_layered_edges(&layout, &edges, LayoutSpacing::default());
    for (from, to) in edges {
        let path = routed.path(&from, &to).unwrap();
        assert!(path.len() >= 2);
        let source = routed.layout.rect(&from).unwrap();
        let target = routed.layout.rect(&to).unwrap();
        assert!(
            (path.first().unwrap().x - source.right()).abs() < 0.6
                || path.first().unwrap().y >= source.y
        );
        assert!(
            (path.last().unwrap().x - target.x).abs() < 0.6 || path.last().unwrap().y >= target.y
        );
    }
}

#[test]
fn metro_layout_fills_the_viewport_and_gives_heavy_work_more_area() {
    let spacing = LayoutSpacing::canvas();
    let mut heavy = LayoutNodeSpec::fixed(id("heavy"), spacing, 1);
    heavy.width *= 2.0;
    heavy.height *= 2.0;
    let specs = [
        (LayoutNodeSpec::fixed(id("source"), spacing, 0), 1),
        (heavy, 1),
        (LayoutNodeSpec::fixed(id("result"), spacing, 2), 4),
    ];
    let edges = [(id("source"), id("heavy")), (id("heavy"), id("result"))];
    let layout = metro_tile_layout_with_specs(&specs, &edges, 1_200.0, 640.0).unwrap();
    let source = layout.rect(&id("source")).unwrap();
    let heavy = layout.rect(&id("heavy")).unwrap();

    assert!(layout.canvas_width >= 1_200.0);
    assert!(layout.canvas_height >= 640.0);
    assert!(heavy.width > source.width * 1.9);
    assert!(heavy.height > source.height * 1.9);
}

/// Every node card renders at one uniform size now, so `from_text` must
/// ignore title/detail length entirely -- a short ASCII label and a long
/// CJK label plus a long detail string must yield identical geometry.
#[test]
fn uniform_node_geometry_ignores_title_length() {
    let spacing = LayoutSpacing::canvas();
    let short = LayoutNodeSpec::from_text(
        id("workflow.a"),
        LayoutNodeVisualKind::Compute,
        "ASR",
        "",
        0,
    );
    let long = LayoutNodeSpec::from_text(
        id("workflow.a"),
        LayoutNodeVisualKind::Compute,
        "歌唱语音识别与歌词转写",
        "a detail string long enough to have wrapped across several lines under the old text-measured sizing",
        0,
    );
    assert_eq!(short.width, spacing.node_width);
    assert_eq!(short.height, spacing.node_height);
    assert_eq!(short.width, long.width);
    assert_eq!(short.height, long.height);
}

/// Since geometry no longer varies with text, the layout cache no longer
/// needs a locale/label change to invalidate it -- instead this proves the
/// cache still distinguishes real topology changes: an identical
/// node/edge request reuses the exact cached `Arc`, while reversing an
/// edge's direction (a genuine topology change) produces different node
/// positions and therefore a different cache entry.
#[test]
fn cache_identity_follows_topology_not_text() {
    let nodes = vec![
        LayoutNodeSpec::from_text(
            id("workflow.a"),
            LayoutNodeVisualKind::Compute,
            "Short",
            "",
            0,
        ),
        LayoutNodeSpec::from_text(
            id("workflow.b"),
            LayoutNodeVisualKind::Compute,
            "Also short",
            "",
            1,
        ),
    ];
    let edges = [(id("workflow.a"), id("workflow.b"))];
    let reversed_edges = [(id("workflow.b"), id("workflow.a"))];

    let first = cached_canvas_routed_layout_with_specs(&nodes, &edges).unwrap();
    let first_again = cached_canvas_routed_layout_with_specs(&nodes, &edges).unwrap();
    assert!(Arc::ptr_eq(&first, &first_again));

    let reversed = cached_canvas_routed_layout_with_specs(&nodes, &reversed_edges).unwrap();
    assert_ne!(
        first.layout.rect(&id("workflow.a")).unwrap().x,
        reversed.layout.rect(&id("workflow.a")).unwrap().x
    );
}
