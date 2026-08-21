use super::*;

fn active_bgm_producer(view: &GraphViewModel) -> Option<AnalysisNodeId> {
    view.audio_processing
        .as_ref()
        .and_then(|audio| {
            audio
                .output_bindings
                .iter()
                .find(|binding| binding.artifact_role == "instrumental")
        })
        .and_then(|binding| audio_step_node_id(&binding.step_id))
        .map(AnalysisNodeId::new)
        .filter(|id| view.node(id).is_some())
        .or_else(|| {
            [
                "instrumental.dereverb",
                "instrumental.denoise",
                "stems.instrumental",
            ]
            .into_iter()
            .map(AnalysisNodeId::new)
            .find(|id| view.node(id).is_some())
        })
}

fn final_bgm_artifact_id(view: &GraphViewModel) -> Option<&'static str> {
    match active_bgm_producer(view)?.as_str() {
        "instrumental.dereverb" => Some("artifact.dereverbed_instrumental"),
        "instrumental.denoise" => Some("artifact.denoised_instrumental"),
        "stems.instrumental" => Some("artifact.raw_instrumental"),
        _ => None,
    }
}

fn bgm_step_consumer(view: &GraphViewModel, producer_step_id: &str) -> Option<&'static str> {
    view.audio_processing
        .as_ref()?
        .steps
        .iter()
        .find_map(|step| match &step.input {
            AudioInputReference::StepOutput { step_id, .. } if step_id == producer_step_id => {
                audio_step_node_id(&step.step_id).filter(|node_id| {
                    matches!(*node_id, "instrumental.denoise" | "instrumental.dereverb")
                })
            }
            _ => None,
        })
}

pub(super) fn push_accompaniment_product_chain(
    view: &GraphViewModel,
    artifact_present: &dyn Fn(ArtifactKind) -> bool,
    nodes: &mut Vec<RenderNode>,
    edges: &mut Vec<RenderEdge>,
) {
    if view
        .node(&AnalysisNodeId::new("stems.instrumental"))
        .is_none()
    {
        return;
    }
    let extract_node = "stems.instrumental";
    let next_after_extract = bgm_step_consumer(view, "extract_accompaniment");
    let raw_kind = ArtifactKind::HighQualityInstrumentalStem;
    push_on_path_artifact(
        OnPathArtifactSpec {
            id: "artifact.raw_instrumental",
            label: "Raw BGM",
            detail: "separated BGM · lossless",
            producer: extract_node,
            consumer: None,
            kind: raw_kind,
        },
        view,
        artifact_present,
        nodes,
        edges,
    );
    if let Some(consumer) = next_after_extract {
        edges.push(RenderEdge {
            from: AnalysisNodeId::new(extract_node),
            to: AnalysisNodeId::new(consumer),
            artifact_kind: Some(raw_kind),
            role: RenderEdgeRole::ComputeDependency,
            producer_node: AnalysisNodeId::new(extract_node),
        });
    }

    if view
        .node(&AnalysisNodeId::new("instrumental.denoise"))
        .is_some()
    {
        let next = bgm_step_consumer(view, "denoise_accompaniment");
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.denoised_instrumental",
                label: "Denoised BGM",
                detail: "denoised BGM · lossless",
                producer: "instrumental.denoise",
                consumer: None,
                kind: ArtifactKind::DenoisedInstrumentalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
        if let Some(consumer) = next {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("instrumental.denoise"),
                to: AnalysisNodeId::new(consumer),
                artifact_kind: Some(ArtifactKind::DenoisedInstrumentalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: AnalysisNodeId::new("instrumental.denoise"),
            });
        }
    }
    if view
        .node(&AnalysisNodeId::new("instrumental.dereverb"))
        .is_some()
    {
        let next = bgm_step_consumer(view, "dereverb_accompaniment");
        push_on_path_artifact(
            OnPathArtifactSpec {
                id: "artifact.dereverbed_instrumental",
                label: "Dry BGM",
                detail: "dereverbed BGM · lossless",
                producer: "instrumental.dereverb",
                consumer: None,
                kind: ArtifactKind::DereverbedInstrumentalStem,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
        if let Some(consumer) = next {
            edges.push(RenderEdge {
                from: AnalysisNodeId::new("instrumental.dereverb"),
                to: AnalysisNodeId::new(consumer),
                artifact_kind: Some(ArtifactKind::DereverbedInstrumentalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: AnalysisNodeId::new("instrumental.dereverb"),
            });
        }
    }

    let chart = AnalysisNodeId::new("chart.build_candidate");
    if view.node(&chart).is_some()
        && let (Some(artifact_id), Some(producer)) =
            (final_bgm_artifact_id(view), active_bgm_producer(view))
    {
        edges.push(RenderEdge {
            from: AnalysisNodeId::new(artifact_id),
            to: chart,
            artifact_kind: Some(ArtifactKind::InstrumentalStem),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: producer,
        });
    }
}
