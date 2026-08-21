use super::*;

pub(super) fn push_multistem_product_chain(
    view: &GraphViewModel,
    artifact_present: &dyn Fn(ArtifactKind) -> bool,
    nodes: &mut Vec<RenderNode>,
    edges: &mut Vec<RenderEdge>,
) {
    let producer = AnalysisNodeId::new("stems.multistem");
    if view.node(&producer).is_none() {
        return;
    }
    let specs = [
        (
            "artifact.multistem_vocal",
            "vocals.flac",
            "six-stem vocal · lossless",
            ArtifactKind::VocalStem,
        ),
        (
            "artifact.multistem_drums",
            "drums.flac",
            "six-stem output · lossless",
            ArtifactKind::DrumStem,
        ),
        (
            "artifact.multistem_bass",
            "bass.flac",
            "six-stem output · lossless",
            ArtifactKind::BassStem,
        ),
        (
            "artifact.multistem_guitar",
            "guitar.flac",
            "six-stem output · lossless",
            ArtifactKind::GuitarStem,
        ),
        (
            "artifact.multistem_piano",
            "piano.flac",
            "six-stem output · lossless",
            ArtifactKind::PianoStem,
        ),
        (
            "artifact.multistem_other",
            "other.flac",
            "six-stem output · lossless",
            ArtifactKind::OtherStem,
        ),
        (
            "artifact.multistem_instrumental",
            "instrumental.flac",
            "summed accompaniment · lossless",
            ArtifactKind::InstrumentalStem,
        ),
    ];
    for (id, label, detail, kind) in specs {
        push_on_path_artifact(
            OnPathArtifactSpec {
                id,
                label,
                detail,
                producer: "stems.multistem",
                consumer: None,
                kind,
            },
            view,
            artifact_present,
            nodes,
            edges,
        );
    }
    if analysis_vocal_producer(view).as_ref() == Some(&producer) {
        for consumer in ["pitch.extract", "lyrics.preprocess"] {
            if view.node(&AnalysisNodeId::new(consumer)).is_some() {
                edges.push(RenderEdge {
                    from: AnalysisNodeId::new("artifact.multistem_vocal"),
                    to: AnalysisNodeId::new(consumer),
                    artifact_kind: Some(ArtifactKind::VocalStem),
                    role: RenderEdgeRole::ArtifactOutput,
                    producer_node: producer.clone(),
                });
            }
        }
    }
}
