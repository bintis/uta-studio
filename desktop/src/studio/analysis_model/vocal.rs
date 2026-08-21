use super::*;

pub(super) fn push_vocal_product_chain(
    view: &GraphViewModel,
    artifact_present: &dyn Fn(ArtifactKind) -> bool,
    nodes: &mut Vec<RenderNode>,
    edges: &mut Vec<RenderEdge>,
) {
    if view.node(&AnalysisNodeId::new("stems.vocals")).is_none() {
        return;
    }
    let next_after_extract = audio_step_consumer(view, "extract_vocals").or_else(|| {
        if view.node(&AnalysisNodeId::new("vocals.denoise")).is_some() {
            Some("vocals.denoise")
        } else if view.node(&AnalysisNodeId::new("vocals.dereverb")).is_some() {
            Some("vocals.dereverb")
        } else {
            None
        }
    });
    let (extract_name, extract_kind) = if next_after_extract.is_some() {
        ("vocals_raw.flac", ArtifactKind::RawVocalStem)
    } else {
        ("vocals.flac", ArtifactKind::VocalStem)
    };
    push_on_path_artifact(
        OnPathArtifactSpec {
            id: "artifact.raw_vocal",
            label: extract_name,
            detail: "extracted vocal · lossless",
            producer: "stems.vocals",
            consumer: None,
            kind: extract_kind,
        },
        view,
        artifact_present,
        nodes,
        edges,
    );
    if let Some(consumer) = next_after_extract {
        edges.push(RenderEdge {
            from: AnalysisNodeId::new("stems.vocals"),
            to: AnalysisNodeId::new(consumer),
            artifact_kind: Some(extract_kind),
            role: RenderEdgeRole::ComputeDependency,
            producer_node: AnalysisNodeId::new("stems.vocals"),
        });
    }
    if view.node(&AnalysisNodeId::new("vocals.denoise")).is_some() {
        let next_after_denoise = audio_step_consumer(view, "denoise_vocals").or_else(|| {
            view.node(&AnalysisNodeId::new("vocals.dereverb"))
                .is_some()
                .then_some("vocals.dereverb")
        });
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.denoised_vocal",
                label: "vocals_denoised.flac",
                detail: "denoised vocal · lossless",
                producer: "vocals.denoise",
                consumer: None,
                kind: ArtifactKind::DenoisedVocalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
        if let Some(consumer) = next_after_denoise {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("vocals.denoise"),
                to: AnalysisNodeId::new(consumer),
                artifact_kind: Some(ArtifactKind::DenoisedVocalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: AnalysisNodeId::new("vocals.denoise"),
            });
        }
    }
    if view.node(&AnalysisNodeId::new("vocals.dereverb")).is_some() {
        let next_after_dereverb = audio_step_consumer(view, "dereverb_vocals");
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.dereverbed_vocal",
                label: "vocals_dry.flac",
                detail: "dereverbed vocal · lossless",
                producer: "vocals.dereverb",
                consumer: None,
                kind: ArtifactKind::DereverbedVocalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
        if let Some(consumer) = next_after_dereverb {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("vocals.dereverb"),
                to: AnalysisNodeId::new(consumer),
                artifact_kind: Some(ArtifactKind::DereverbedVocalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: AnalysisNodeId::new("vocals.dereverb"),
            });
        }
    }
    if let Some(artifact_id) = last_vocal_artifact_id(view) {
        let producer = analysis_vocal_producer(view).expect("artifact implies producer");
        if view.node(&AnalysisNodeId::new("pitch.extract")).is_some() {
            edges.push(RenderEdge {
                from: producer.clone(),
                to: AnalysisNodeId::new("pitch.extract"),
                artifact_kind: Some(ArtifactKind::VocalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: producer.clone(),
            });
        }
        if view
            .node(&AnalysisNodeId::new("lyrics.preprocess"))
            .is_some()
        {
            let kind = match artifact_id {
                "artifact.dereverbed_vocal" => ArtifactKind::DereverbedVocalStem,
                "artifact.denoised_vocal" => ArtifactKind::DenoisedVocalStem,
                "artifact.raw_vocal" => ArtifactKind::RawVocalStem,
                _ => ArtifactKind::VocalStem,
            };
            edges.push(RenderEdge {
                from: AnalysisNodeId::new(artifact_id),
                to: AnalysisNodeId::new("lyrics.preprocess"),
                artifact_kind: Some(kind),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: producer,
            });
        }
    }
}
