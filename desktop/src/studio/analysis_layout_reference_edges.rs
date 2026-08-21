//! Small route overrides that are part of the supplied reference composition.

use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

use super::{GraphLayout, LayoutPoint};

pub(super) fn override_reference_paths(
    layout: &GraphLayout,
    edges: &[(AnalysisNodeId, AnalysisNodeId)],
    paths: &mut BTreeMap<(AnalysisNodeId, AnalysisNodeId), Vec<LayoutPoint>>,
) {
    let preflight_id = AnalysisNodeId::new("preflight");
    for target_id in ["music.analysis"] {
        let target_id = AnalysisNodeId::new(target_id);
        if !edges
            .iter()
            .any(|edge| edge == &(preflight_id.clone(), target_id.clone()))
        {
            continue;
        }
        let (Some(preflight), Some(target)) = (layout.rect(&preflight_id), layout.rect(&target_id))
        else {
            continue;
        };
        let end_y = target.y + target.height * 0.5;
        let (start_x, start_y) = (preflight.x + preflight.width * 0.62, preflight.y);
        let elbow_x = start_x;
        paths.insert(
            (preflight_id.clone(), target_id),
            vec![
                LayoutPoint {
                    x: start_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: elbow_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: elbow_x,
                    y: end_y,
                },
                LayoutPoint {
                    x: target.x,
                    y: end_y,
                },
            ],
        );
    }

    // Music Analysis hands the checked source into the vocal chain. This is
    // the sole line entering the purple processing row from the left.
    let music_id = AnalysisNodeId::new("music.analysis");
    let extract_id = if layout.rect(&AnalysisNodeId::new("stems.vocals")).is_some() {
        AnalysisNodeId::new("stems.vocals")
    } else {
        AnalysisNodeId::new("stems.separate")
    };
    if edges
        .iter()
        .any(|edge| edge == &(music_id.clone(), extract_id.clone()))
        && let (Some(music), Some(extract)) = (layout.rect(&music_id), layout.rect(&extract_id))
    {
        let start_x = music.x + music.width;
        let start_y = music.y + music.height * 0.5;
        let end_y = extract.y + extract.height * 0.5;
        paths.insert(
            (music_id, extract_id),
            vec![
                LayoutPoint {
                    x: start_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: extract.x,
                    y: start_y,
                },
                LayoutPoint {
                    x: extract.x,
                    y: end_y,
                },
            ],
        );
    }

    let vocal_row = [
        "stems.vocals",
        "vocals.denoise",
        "vocals.dereverb",
        "pitch.extract",
        "artifact.note_guide",
    ];
    for (from_id, to_id) in edges {
        let full_row = vocal_row.contains(&from_id.as_str()) && vocal_row.contains(&to_id.as_str());
        let mini_row = from_id.as_str() == "stems.separate" && to_id.as_str() == "pitch.extract";
        if !full_row && !mini_row {
            continue;
        }
        let (Some(from), Some(to)) = (layout.rect(from_id), layout.rect(to_id)) else {
            continue;
        };
        let y = from.y + from.height * 0.5;
        paths.insert(
            (from_id.clone(), to_id.clone()),
            vec![
                LayoutPoint {
                    x: from.x + from.width,
                    y,
                },
                LayoutPoint { x: to.x, y },
            ],
        );
    }

    // The collapsed MINI composition has no Pitch Guide artifact card, so
    // Pitch itself occupies the guide's role and connects straight to Chart.
    let pitch_id = AnalysisNodeId::new("pitch.extract");
    let chart_id = AnalysisNodeId::new("chart.build_candidate");
    if layout
        .rect(&AnalysisNodeId::new("stems.separate"))
        .is_some()
        && edges
            .iter()
            .any(|edge| edge == &(pitch_id.clone(), chart_id.clone()))
        && let (Some(pitch), Some(chart)) = (layout.rect(&pitch_id), layout.rect(&chart_id))
    {
        let y = pitch.y + pitch.height * 0.5;
        paths.insert(
            (pitch_id, chart_id),
            vec![
                LayoutPoint {
                    x: pitch.x + pitch.width,
                    y,
                },
                LayoutPoint { x: chart.x, y },
            ],
        );
    }

    // Pitch Guide and Chart share one row, so their dependency is a single
    // horizontal stroke rather than a decorative elbow.
    let guide_id = AnalysisNodeId::new("artifact.note_guide");
    let chart_id = AnalysisNodeId::new("chart.build_candidate");
    if edges
        .iter()
        .any(|edge| edge == &(guide_id.clone(), chart_id.clone()))
        && let (Some(guide), Some(chart)) = (layout.rect(&guide_id), layout.rect(&chart_id))
    {
        let start_x = guide.x + guide.width;
        let start_y = guide.y + guide.height * 0.5;
        paths.insert(
            (guide_id, chart_id),
            vec![
                LayoutPoint {
                    x: start_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: chart.x,
                    y: start_y,
                },
            ],
        );
    }

    // The selected BGM product enters Chart from above. Keeping this route
    // in the upper half avoids the generic long-span bottom rail crossing
    // Pitch and the lyrics panel when BGM has its own processing chain.
    for artifact in [
        "artifact.dereverbed_instrumental",
        "artifact.denoised_instrumental",
        "artifact.raw_instrumental",
    ] {
        let bgm_id = AnalysisNodeId::new(artifact);
        let chart_id = AnalysisNodeId::new("chart.build_candidate");
        if !edges
            .iter()
            .any(|edge| edge == &(bgm_id.clone(), chart_id.clone()))
        {
            continue;
        }
        let (Some(bgm), Some(chart)) = (layout.rect(&bgm_id), layout.rect(&chart_id)) else {
            continue;
        };
        let start_y = bgm.y + bgm.height * 0.5;
        let enter_x = chart.x + chart.width * 0.28;
        paths.insert(
            (bgm_id, chart_id),
            vec![
                LayoutPoint {
                    x: bgm.x + bgm.width,
                    y: start_y,
                },
                LayoutPoint {
                    x: enter_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: enter_x,
                    y: chart.y,
                },
            ],
        );
        break;
    }

    // Timed lyrics leave from the right and rise into the center of Chart's
    // bottom edge, keeping the lower orange route outside both cards.
    let timed_id = AnalysisNodeId::new("artifact.timed_lyrics");
    let chart_id = AnalysisNodeId::new("chart.build_candidate");
    if edges
        .iter()
        .any(|edge| edge == &(timed_id.clone(), chart_id.clone()))
        && let (Some(timed), Some(chart)) = (layout.rect(&timed_id), layout.rect(&chart_id))
    {
        let start_x = timed.x + timed.width;
        let start_y = timed.y + timed.height * 0.5;
        let enter_x = chart.x + chart.width * 0.5;
        paths.insert(
            (timed_id, chart_id),
            vec![
                LayoutPoint {
                    x: start_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: enter_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: enter_x,
                    y: chart.y + chart.height,
                },
            ],
        );
    }

    // The final processed vocal drops cleanly into Prep from the artifact
    // chip, without falling back to the generic bottom rail.
    let dry_id = AnalysisNodeId::new("artifact.dereverbed_vocal");
    let prep_id = AnalysisNodeId::new("lyrics.preprocess");
    if edges
        .iter()
        .any(|edge| edge == &(dry_id.clone(), prep_id.clone()))
        && let (Some(dry), Some(prep)) = (layout.rect(&dry_id), layout.rect(&prep_id))
    {
        let exit_x = dry.x + dry.width * 0.5;
        let enter_x = prep.x + prep.width * 0.5;
        // Lyrics starts 64 units above its first card and the panels keep an
        // 8-unit gap. Bias the mathematical center one unit upward so the
        // fitted stroke rasterizes clear of the orange top border.
        let rail_y = prep.y - 69.0;
        paths.insert(
            (dry_id, prep_id),
            vec![
                LayoutPoint {
                    x: exit_x,
                    y: dry.y + dry.height,
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
                    y: prep.y,
                },
            ],
        );
    }

    // MINI collapses the three vocal cards into Stem Separation. Preserve
    // the same black inter-panel corridor before entering Prep from above.
    let stems_id = AnalysisNodeId::new("stems.separate");
    let prep_id = AnalysisNodeId::new("lyrics.preprocess");
    if edges
        .iter()
        .any(|edge| edge == &(stems_id.clone(), prep_id.clone()))
        && let (Some(stems), Some(prep)) = (layout.rect(&stems_id), layout.rect(&prep_id))
    {
        let exit_x = stems.x + stems.width * 0.5;
        let enter_x = prep.x + prep.width * 0.5;
        let rail_y = prep.y - 69.0;
        paths.insert(
            (stems_id, prep_id),
            vec![
                LayoutPoint {
                    x: exit_x,
                    y: stems.y + stems.height,
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
                    y: prep.y,
                },
            ],
        );
    }

    for (from, to) in [
        ("lyrics.preprocess", "lyrics.transcribe"),
        ("lyrics.transcribe", "lyrics.align"),
    ] {
        let from_id = AnalysisNodeId::new(from);
        let to_id = AnalysisNodeId::new(to);
        if !edges
            .iter()
            .any(|edge| edge == &(from_id.clone(), to_id.clone()))
        {
            continue;
        }
        let (Some(from), Some(to)) = (layout.rect(&from_id), layout.rect(&to_id)) else {
            continue;
        };
        let y = from.y + from.height * 0.5;
        paths.insert(
            (from_id, to_id),
            vec![
                LayoutPoint {
                    x: from.x + from.width,
                    y,
                },
                LayoutPoint { x: to.x, y },
            ],
        );
    }

    let align_id = AnalysisNodeId::new("lyrics.align");
    let chart_id = AnalysisNodeId::new("chart.build_candidate");
    if layout
        .rect(&AnalysisNodeId::new("stems.separate"))
        .is_some()
        && edges
            .iter()
            .any(|edge| edge == &(align_id.clone(), chart_id.clone()))
        && let (Some(align), Some(chart)) = (layout.rect(&align_id), layout.rect(&chart_id))
    {
        let start_y = align.y + align.height * 0.5;
        let end_y = chart.y + chart.height * 0.5;
        paths.insert(
            (align_id, chart_id),
            vec![
                LayoutPoint {
                    x: align.x + align.width,
                    y: start_y,
                },
                LayoutPoint {
                    x: chart.x,
                    y: start_y,
                },
                LayoutPoint {
                    x: chart.x,
                    y: end_y,
                },
            ],
        );
    }

    for (export_id, fraction) in [("export.ultrastar", 0.32), ("export.utz", 0.68)] {
        let from_id = AnalysisNodeId::new("chart.build_candidate");
        let to_id = AnalysisNodeId::new(export_id);
        if !edges
            .iter()
            .any(|edge| edge == &(from_id.clone(), to_id.clone()))
        {
            continue;
        }
        let (Some(chart), Some(export)) = (layout.rect(&from_id), layout.rect(&to_id)) else {
            continue;
        };
        let chart_right = chart.x + chart.width;
        let start_y = chart.y + chart.height * fraction;
        let end_y = export.y + export.height * 0.5;
        let fork_x = chart_right + ((export.x - chart_right) * 0.28).max(8.0);
        paths.insert(
            (from_id, to_id),
            vec![
                LayoutPoint {
                    x: chart_right,
                    y: start_y,
                },
                LayoutPoint {
                    x: fork_x,
                    y: start_y,
                },
                LayoutPoint {
                    x: fork_x,
                    y: end_y,
                },
                LayoutPoint {
                    x: export.x,
                    y: end_y,
                },
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::analysis_layout::LayoutRect;

    fn rect(x: f32, y: f32) -> LayoutRect {
        LayoutRect {
            x,
            y,
            width: 136.0,
            height: 112.0,
        }
    }

    #[test]
    fn reference_routes_use_one_music_elbow_and_the_inter_panel_gap() {
        let music = AnalysisNodeId::new("music.analysis");
        let extract = AnalysisNodeId::new("stems.vocals");
        let dry = AnalysisNodeId::new("artifact.dereverbed_vocal");
        let prep = AnalysisNodeId::new("lyrics.preprocess");
        let layout = GraphLayout {
            rects: BTreeMap::from([
                (music.clone(), rect(230.0, 68.0)),
                (extract.clone(), rect(520.0, 151.0)),
                (dry.clone(), rect(760.0, 276.0)),
                (prep.clone(), rect(600.0, 374.0)),
            ]),
            canvas_width: 1800.0,
            canvas_height: 520.0,
        };
        let edges = vec![
            (music.clone(), extract.clone()),
            (dry.clone(), prep.clone()),
        ];
        let mut paths = BTreeMap::new();

        override_reference_paths(&layout, &edges, &mut paths);

        let music_path = &paths[&(music, extract)];
        assert_eq!(music_path.len(), 3);
        assert_eq!(
            music_path[1].x,
            layout.rects[&AnalysisNodeId::new("stems.vocals")].x
        );
        let dry_path = &paths[&(dry, prep.clone())];
        assert_eq!(dry_path[1].y, layout.rects[&prep].y - 69.0);
        assert_eq!(dry_path[2].y, layout.rects[&prep].y - 69.0);
    }
}
