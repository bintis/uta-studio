//! Phase-panel geometry for the analysis DAG reference composition.

use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

use super::{GraphLayout, LayoutLaneBand, LayoutLaneKind, LayoutRect, Swimlane, swimlane_of};

// Measured from the supplied reference image's usable DAG canvas. Keeping
// the phase boundaries at 11/25/79% preserves its visual balance at every
// fitted viewport width.
const PREPARATION_WIDTH: f32 = 0.11;
const MUSIC_WIDTH: f32 = 0.14;
const MIDDLE_WIDTH: f32 = 0.54;

fn phase_columns(canvas_width: f32) -> (f32, f32, f32, f32) {
    let preparation = canvas_width * PREPARATION_WIDTH;
    let music = canvas_width * MUSIC_WIDTH;
    let middle = canvas_width * MIDDLE_WIDTH;
    (
        preparation,
        music,
        middle,
        canvas_width - preparation - music - middle,
    )
}

fn full_reference_composition_present(rects: &BTreeMap<AnalysisNodeId, LayoutRect>) -> bool {
    [
        ("artifact.raw_vocal", "stems.vocals"),
        ("artifact.denoised_vocal", "vocals.denoise"),
        ("artifact.dereverbed_vocal", "vocals.dereverb"),
    ]
    .into_iter()
    .any(|(artifact, producer)| {
        rects.contains_key(&AnalysisNodeId::new(artifact))
            && rects.contains_key(&AnalysisNodeId::new(producer))
    })
}

fn mini_reference_composition_present(rects: &BTreeMap<AnalysisNodeId, LayoutRect>) -> bool {
    [
        "preflight",
        "music.analysis",
        "stems.separate",
        "pitch.extract",
        "lyrics.preprocess",
        "chart.build_candidate",
    ]
    .into_iter()
    .all(|id| rects.contains_key(&AnalysisNodeId::new(id)))
        && !rects.contains_key(&AnalysisNodeId::new("stems.vocals"))
}

pub(super) fn reference_composition_present(rects: &BTreeMap<AnalysisNodeId, LayoutRect>) -> bool {
    full_reference_composition_present(rects) || mini_reference_composition_present(rects)
}

pub(super) fn align_reference_nodes(
    rects: &mut BTreeMap<AnalysisNodeId, LayoutRect>,
    canvas_width: f32,
    canvas_height: f32,
) {
    if !reference_composition_present(rects) {
        return;
    }

    if mini_reference_composition_present(rects) {
        for (id, x, y) in [
            ("preflight", 0.015, 0.43),
            ("music.analysis", 0.13, 0.13),
            ("stems.separate", 0.36, 0.29),
            ("pitch.extract", 0.60, 0.29),
            ("lyrics.preprocess", 0.33, 0.72),
            ("lyrics.transcribe", 0.48, 0.72),
            ("lyrics.align", 0.63, 0.72),
        ] {
            if let Some(rect) = rects.get_mut(&AnalysisNodeId::new(id)) {
                rect.x = (canvas_width * x).clamp(0.0, canvas_width - rect.width);
                rect.y = (canvas_height * y).clamp(0.0, canvas_height - rect.height);
            }
        }
        return;
    }

    let vocal_reverse = rects
        .get(&AnalysisNodeId::new("vocals.denoise"))
        .zip(rects.get(&AnalysisNodeId::new("vocals.dereverb")))
        .is_some_and(|(denoise, dereverb)| denoise.x > dereverb.x);
    let bgm_reverse = rects
        .get(&AnalysisNodeId::new("instrumental.denoise"))
        .zip(rects.get(&AnalysisNodeId::new("instrumental.dereverb")))
        .is_some_and(|(denoise, dereverb)| denoise.x > dereverb.x);
    let (vocal_denoise_x, vocal_dereverb_x) = if vocal_reverse {
        (0.48, 0.37)
    } else {
        (0.37, 0.48)
    };
    let (bgm_denoise_x, bgm_dereverb_x) = if bgm_reverse {
        (0.48, 0.37)
    } else {
        (0.37, 0.48)
    };

    for (id, x, y) in [
        ("preflight", 0.015, 0.43),
        ("music.analysis", 0.13, 0.13),
        ("stems.karaoke", 0.13, 0.43),
        ("stems.instrumental", 0.26, 0.08),
        ("instrumental.denoise", bgm_denoise_x, 0.08),
        ("instrumental.dereverb", bgm_dereverb_x, 0.08),
        ("stems.vocals", 0.26, 0.39),
        ("vocals.denoise", vocal_denoise_x, 0.39),
        ("vocals.dereverb", vocal_dereverb_x, 0.39),
        ("pitch.extract", 0.58, 0.39),
        ("artifact.note_guide", 0.68, 0.39),
        ("lyrics.preprocess", 0.26, 0.72),
        ("lyrics.transcribe", 0.37, 0.72),
        ("artifact.lyrics", 0.48, 0.72),
        ("artifact.recognized_text", 0.48, 0.72),
        ("lyrics.align", 0.59, 0.72),
        ("lyrics.import_timed", 0.69, 0.72),
        ("artifact.timed_lyrics", 0.69, 0.72),
    ] {
        if let Some(rect) = rects.get_mut(&AnalysisNodeId::new(id)) {
            rect.x = (canvas_width * x).clamp(0.0, canvas_width - rect.width);
            rect.y = (canvas_height * y).clamp(0.0, canvas_height - rect.height);
        }
    }

    // Balance the complete five-card processing row around the purple
    // panel's own center instead of around the whole canvas. The percentage
    // anchors above preserve spacing; this only removes their left bias.
    for row in [
        &[
            "stems.instrumental",
            "instrumental.denoise",
            "instrumental.dereverb",
        ][..],
        &[
            "stems.vocals",
            "vocals.denoise",
            "vocals.dereverb",
            "pitch.extract",
            "artifact.note_guide",
        ][..],
    ] {
        let row_rects: Vec<_> = row
            .iter()
            .filter_map(|id| rects.get(&AnalysisNodeId::new(*id)).copied())
            .collect();
        if let (Some(left), Some(right)) = (
            row_rects.iter().map(|rect| rect.x).reduce(f32::min),
            row_rects.iter().map(|rect| rect.right()).reduce(f32::max),
        ) {
            let (preparation, music, middle, _) = phase_columns(canvas_width);
            let panel_center = preparation + music + middle * 0.5;
            let shift = panel_center - (left + right) * 0.5;
            for id in row {
                if let Some(rect) = rects.get_mut(&AnalysisNodeId::new(*id)) {
                    rect.x = (rect.x + shift).clamp(
                        preparation + music,
                        preparation + music + middle - rect.width,
                    );
                }
            }
        }
    }
}

pub(super) fn align_authoring_nodes(
    rects: &mut BTreeMap<AnalysisNodeId, LayoutRect>,
    canvas_width: f32,
    canvas_height: f32,
) {
    let guide_center_y = rects
        .get(&AnalysisNodeId::new("artifact.note_guide"))
        .or_else(|| rects.get(&AnalysisNodeId::new("pitch.extract")))
        .map(|rect| rect.y + rect.height * 0.5);
    if let Some(chart) = rects.get_mut(&AnalysisNodeId::new("chart.build_candidate")) {
        chart.x = (canvas_width * 0.80).min(canvas_width - chart.width);
        chart.y = guide_center_y
            .map(|center| center - chart.height * 0.5)
            .unwrap_or(canvas_height * 0.29)
            .clamp(0.0, canvas_height - chart.height);
    }
    for (id, y) in [("export.ultrastar", 0.13), ("export.utz", 0.45)] {
        if let Some(export) = rects.get_mut(&AnalysisNodeId::new(id)) {
            export.x = (canvas_width * 0.90).min(canvas_width - export.width);
            export.y = (canvas_height * y).min(canvas_height - export.height);
        }
    }
}

pub(super) fn lane_bands_for_height(
    layout: &GraphLayout,
    requested_height: f32,
) -> Vec<LayoutLaneBand> {
    let panel_gap = 8.0;
    let height = requested_height.max(layout.canvas_height);
    // Keep the lower border and its corner radius inside the scroll viewport
    // instead of clipping half the stroke at the exact content edge.
    let full_height = (height - 2.0).max(1.0);
    let (preparation, music, middle, authoring) = phase_columns(layout.canvas_width);
    let middle_x = preparation + music;
    let authoring_x = middle_x + middle;
    let first_lyrics_y = layout
        .rects
        .iter()
        .filter(|(id, _)| swimlane_of(id) == Swimlane::Lyrics)
        .map(|(_, rect)| rect.y)
        .min_by(|left, right| left.partial_cmp(right).unwrap())
        .unwrap_or(height * 0.72);
    let vocal_chip_bottom = [
        "artifact.raw_vocal",
        "artifact.denoised_vocal",
        "artifact.dereverbed_vocal",
        "artifact.raw_instrumental",
        "artifact.denoised_instrumental",
        "artifact.dereverbed_instrumental",
    ]
    .into_iter()
    .filter_map(|id| layout.rect(&AnalysisNodeId::new(id)))
    .map(|rect| rect.y + rect.height)
    .fold(0.0, f32::max);
    let lyrics_y = (first_lyrics_y - 64.0)
        .max(vocal_chip_bottom + 8.0)
        .clamp(80.0, height - 80.0);

    vec![
        LayoutLaneBand {
            kind: LayoutLaneKind::Preparation,
            rect: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: preparation,
                height: full_height,
            },
        },
        LayoutLaneBand {
            kind: LayoutLaneKind::Music,
            rect: LayoutRect {
                x: preparation,
                y: 0.0,
                width: music,
                height: full_height,
            },
        },
        LayoutLaneBand {
            kind: LayoutLaneKind::VocalsAndPitch,
            rect: LayoutRect {
                x: middle_x,
                y: 0.0,
                width: middle,
                height: (lyrics_y - panel_gap).max(1.0),
            },
        },
        LayoutLaneBand {
            kind: LayoutLaneKind::LyricsAndTiming,
            rect: LayoutRect {
                x: middle_x,
                y: lyrics_y,
                width: middle,
                height: (full_height - lyrics_y).max(1.0),
            },
        },
        LayoutLaneBand {
            kind: LayoutLaneKind::AuthoringAndOutput,
            rect: LayoutRect {
                x: authoring_x,
                y: 0.0,
                width: authoring.max(1.0),
                height: full_height,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_reference_keeps_processing_and_lyrics_inside_the_middle_panels() {
        let mut rects = BTreeMap::new();
        for id in [
            "preflight",
            "music.analysis",
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
            "chart.build_candidate",
        ] {
            rects.insert(
                AnalysisNodeId::new(id),
                LayoutRect {
                    x: 0.0,
                    y: 0.0,
                    width: 136.0,
                    height: 112.0,
                },
            );
        }

        align_reference_nodes(&mut rects, 1800.0, 520.0);
        let middle_left = 1800.0 * (PREPARATION_WIDTH + MUSIC_WIDTH);
        let middle_right = middle_left + 1800.0 * MIDDLE_WIDTH;
        for id in [
            "stems.separate",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.transcribe",
            "lyrics.align",
        ] {
            let rect = rects[&AnalysisNodeId::new(id)];
            assert!(rect.x >= middle_left, "{id} leaked left of its panel");
            assert!(
                rect.right() <= middle_right,
                "{id} leaked right of its panel"
            );
        }
        assert_eq!(
            rects[&AnalysisNodeId::new("stems.separate")].y,
            rects[&AnalysisNodeId::new("pitch.extract")].y
        );
        assert_eq!(
            rects[&AnalysisNodeId::new("lyrics.preprocess")].y,
            rects[&AnalysisNodeId::new("lyrics.align")].y
        );
    }
}
