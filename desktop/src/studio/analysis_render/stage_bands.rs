//! Light 01-04 step grouping for the Advanced Graph, computed entirely from
//! real node positions (`GraphLayout`) and the real `workflow_graph_step`
//! assignment already used elsewhere. No fixed coordinate, node id list, or
//! second topology participates.

use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

use crate::studio::*;

pub(crate) struct StageBand {
    pub(crate) step: u8,
    pub(crate) min_x: f32,
    // Kept for its own tested correctness and for a future band-underline
    // treatment; the current single left-anchored label chip only needs
    // `min_x` (see `spawn_analysis_stage_bands`'s dark-pillar fix).
    #[allow(dead_code)]
    pub(crate) max_x: f32,
}

/// One bounding X-range per step actually present among the laid-out nodes.
/// A step with no rendered node (for example an instrumental-only run with
/// no Step 2 node) contributes no band.
pub(crate) fn compute_analysis_stage_bands(
    layout: &GraphLayout,
    steps: &BTreeMap<AnalysisNodeId, u8>,
) -> Vec<StageBand> {
    let mut ranges: BTreeMap<u8, (f32, f32)> = BTreeMap::new();
    for (id, step) in steps {
        let Some(rect) = layout.rect(id) else {
            continue;
        };
        let entry = ranges.entry(*step).or_insert((rect.x, rect.x + rect.width));
        entry.0 = entry.0.min(rect.x);
        entry.1 = entry.1.max(rect.x + rect.width);
    }
    ranges
        .into_iter()
        .map(|(step, (min_x, max_x))| StageBand { step, min_x, max_x })
        .collect()
}

pub(crate) fn analysis_stage_band_title(step: u8) -> &'static str {
    match step {
        1 => "01 PRE-PROCESSING",
        2 => "02 LYRICS",
        3 => "03 PITCH & NOTE EXPERTS",
        4 => "04 FUSION & OUTPUT",
        _ => "STEP",
    }
}

const STAGE_BAND_PADDING: f32 = 6.0;

/// A small label per step, not a full-height colored column (§4 "非常轻的
/// 背景 band 或分隔线" -- direct user feedback was that the full-canvas-height
/// bordered surface read as a heavy dark pillar behind every card, not a
/// light grouping cue). Position, not paint, is what says "these cards
/// belong to Step N": the label sits at the real left edge of that step's
/// nodes, derived only from `GraphLayout`.
pub(crate) fn spawn_analysis_stage_bands(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bands: &[StageBand],
    zoom: f32,
) {
    let padding = STAGE_BAND_PADDING * zoom;
    for band in bands {
        let left = (band.min_x * zoom - padding).max(0.0);
        parent
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(left),
                    top: px(0.0),
                    padding: UiRect::axes(px(4.0 * zoom), px(2.0 * zoom)),
                    border_radius: BorderRadius::all(px(3.0 * zoom)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.6)),
                ZIndex(2),
                Pickable::IGNORE,
            ))
            .with_children(|chip| {
                spawn_text(
                    chip,
                    font.clone(),
                    analysis_stage_band_title(band.step),
                    (8.5 * zoom).max(7.0),
                    theme.muted_foreground.with_alpha(0.9),
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> AnalysisNodeId {
        AnalysisNodeId::new(value)
    }

    fn rect(x: f32, width: f32) -> LayoutRect {
        LayoutRect {
            x,
            y: 0.0,
            width,
            height: 80.0,
        }
    }

    #[test]
    fn bands_span_the_real_extent_of_their_step_nodes() {
        let mut layout = GraphLayout {
            rects: BTreeMap::new(),
            canvas_width: 1000.0,
            canvas_height: 400.0,
        };
        layout.rects.insert(id("a"), rect(0.0, 150.0));
        layout.rects.insert(id("b"), rect(200.0, 150.0));
        layout.rects.insert(id("c"), rect(500.0, 150.0));
        let mut steps = BTreeMap::new();
        steps.insert(id("a"), 1);
        steps.insert(id("b"), 1);
        steps.insert(id("c"), 3);

        let bands = compute_analysis_stage_bands(&layout, &steps);
        let step1 = bands.iter().find(|band| band.step == 1).unwrap();
        assert_eq!(step1.min_x, 0.0);
        assert_eq!(step1.max_x, 350.0);
        let step3 = bands.iter().find(|band| band.step == 3).unwrap();
        assert_eq!(step3.min_x, 500.0);
        assert_eq!(step3.max_x, 650.0);
        assert!(bands.iter().all(|band| band.step != 2 && band.step != 4));
    }

    #[test]
    fn a_step_with_no_laid_out_node_produces_no_band() {
        let layout = GraphLayout {
            rects: BTreeMap::new(),
            canvas_width: 100.0,
            canvas_height: 100.0,
        };
        let mut steps = BTreeMap::new();
        steps.insert(id("missing"), 2);
        assert!(compute_analysis_stage_bands(&layout, &steps).is_empty());
    }
}
