//! Placement helpers for compact vocal artifact chips in the DAG layout.

use std::collections::BTreeMap;

use app_core::AnalysisNodeId;

use super::{LayoutRect, LayoutSpacing};

pub(super) fn vocal_artifact_chip_producer(id: &AnalysisNodeId) -> Option<&'static str> {
    match id.as_str() {
        "artifact.raw_vocal" => Some("stems.vocals"),
        "artifact.denoised_vocal" => Some("vocals.denoise"),
        "artifact.dereverbed_vocal" => Some("vocals.dereverb"),
        "artifact.raw_instrumental" => Some("stems.instrumental"),
        "artifact.denoised_instrumental" => Some("instrumental.denoise"),
        "artifact.dereverbed_instrumental" => Some("instrumental.dereverb"),
        _ => None,
    }
}

pub(super) fn positioned_vocal_artifact_chip(
    id: &AnalysisNodeId,
    available: &[AnalysisNodeId],
) -> Option<&'static str> {
    let producer = vocal_artifact_chip_producer(id)?;
    available
        .iter()
        .any(|candidate| candidate.as_str() == producer)
        .then_some(producer)
}

pub(super) fn position_vocal_artifact_chips(
    order: &[AnalysisNodeId],
    rects: &mut BTreeMap<AnalysisNodeId, LayoutRect>,
    spacing: LayoutSpacing,
) {
    for id in order {
        let Some(producer_id) = positioned_vocal_artifact_chip(id, order) else {
            continue;
        };
        let Some(producer) = rects.get(&AnalysisNodeId::new(producer_id)).copied() else {
            continue;
        };
        let width = spacing.node_width * 0.82;
        rects.insert(
            id.clone(),
            LayoutRect {
                x: producer.x + (producer.width - width) * 0.5,
                y: producer.bottom() + 28.0,
                width,
                height: 38.0,
            },
        );
    }
}
