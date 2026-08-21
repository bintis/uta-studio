//! Presentation-only shaping for the supplied full-canvas reference design.

use super::*;

const HIDDEN_SIDE_BRANCHES: &[&str] = &["stems.karaoke"];

/// Keeps execution truth in `GraphViewModel`, while giving the overview a
/// stable visual grammar with BGM above vocals. Execution-plan edges keep
/// each branch's selected post-processing order intact.
pub(crate) fn polish_reference_overview(render: &mut RenderGraph) {
    if render.node(&AnalysisNodeId::new("stems.vocals")).is_none() {
        return;
    }

    render
        .nodes
        .retain(|node| !HIDDEN_SIDE_BRANCHES.contains(&node.id.as_str()));
    render.edges.retain(|edge| {
        !HIDDEN_SIDE_BRANCHES.contains(&edge.from.as_str())
            && !HIDDEN_SIDE_BRANCHES.contains(&edge.to.as_str())
            && !(edge.from.as_str() == "preflight"
                && matches!(edge.to.as_str(), "stems.vocals" | "stems.instrumental"))
    });
    let vocal_reverse = render.edges.iter().any(|edge| {
        edge.from.as_str() == "vocals.dereverb" && edge.to.as_str() == "vocals.denoise"
    });
    render.edges.retain(|edge| {
        !(matches!(
            edge.from.as_str(),
            "stems.vocals" | "vocals.denoise" | "vocals.dereverb"
        ) && matches!(
            edge.to.as_str(),
            "vocals.denoise" | "vocals.dereverb" | "pitch.extract"
        ))
    });
    for to in ["stems.instrumental", "stems.vocals"] {
        let from = "music.analysis";
        let from = AnalysisNodeId::new(from);
        let to = AnalysisNodeId::new(to);
        if render.node(&from).is_some() && render.node(&to).is_some() {
            render.edges.push(RenderEdge {
                from: from.clone(),
                to,
                artifact_kind: Some(ArtifactKind::MusicAnalysis),
                role: RenderEdgeRole::ArtifactOutput,
                producer_node: from,
            });
        }
    }
    let cleanup = if vocal_reverse {
        ["vocals.dereverb", "vocals.denoise"]
    } else {
        ["vocals.denoise", "vocals.dereverb"]
    };
    let mut chain = vec!["stems.vocals"];
    chain.extend(
        cleanup
            .into_iter()
            .filter(|id| render.node(&AnalysisNodeId::new(*id)).is_some()),
    );
    chain.push("pitch.extract");
    for pair in chain.windows(2) {
        let from = AnalysisNodeId::new(pair[0]);
        let to = AnalysisNodeId::new(pair[1]);
        if render.node(&from).is_some() && render.node(&to).is_some() {
            render.edges.push(RenderEdge {
                from: from.clone(),
                to,
                artifact_kind: Some(ArtifactKind::AnalysisVocalStem),
                role: RenderEdgeRole::ComputeDependency,
                producer_node: from,
            });
        }
    }
    let (last_cleanup, last_artifact, last_kind) = if vocal_reverse {
        (
            "vocals.denoise",
            "artifact.denoised_vocal",
            ArtifactKind::DenoisedVocalStem,
        )
    } else {
        (
            "vocals.dereverb",
            "artifact.dereverbed_vocal",
            ArtifactKind::DereverbedVocalStem,
        )
    };
    let artifact = AnalysisNodeId::new(last_artifact);
    let lyrics = AnalysisNodeId::new("lyrics.preprocess");
    if render.node(&artifact).is_some() && render.node(&lyrics).is_some() {
        render.edges.retain(|edge| {
            !(edge.to == lyrics
                && matches!(
                    edge.from.as_str(),
                    "artifact.raw_vocal" | "artifact.denoised_vocal" | "artifact.dereverbed_vocal"
                ))
        });
        render.edges.push(RenderEdge {
            from: artifact,
            to: lyrics,
            artifact_kind: Some(last_kind),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: AnalysisNodeId::new(last_cleanup),
        });
    }
}

/// The MINI graph keeps compound processing collapsed, but it should still
/// read as the same pipeline as the full reference composition. In
/// particular, Music Analysis is the one source feeding Stem Separation;
/// Preflight must not grow a second shortcut into the processing row.
pub(crate) fn polish_mini_reference_overview(render: &mut RenderGraph) {
    let preflight = AnalysisNodeId::new("preflight");
    let music = AnalysisNodeId::new("music.analysis");
    let stems = AnalysisNodeId::new("stems.separate");
    if render.node(&music).is_none() || render.node(&stems).is_none() {
        return;
    }

    render
        .edges
        .retain(|edge| !(edge.from == preflight && edge.to == stems));
    if !render
        .edges
        .iter()
        .any(|edge| edge.from == music && edge.to == stems)
    {
        render.edges.push(RenderEdge {
            from: music.clone(),
            to: stems,
            artifact_kind: Some(ArtifactKind::MusicAnalysis),
            role: RenderEdgeRole::ArtifactOutput,
            producer_node: music,
        });
    }
}
