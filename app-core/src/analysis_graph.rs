//! Static structural definition of the Analysis DAG: nodes, artifact kinds,
//! and dependency edges. This module owns *what the graph looks like*; it
//! never touches the filesystem, a queue, or a running pipeline — that is
//! `analysis_plan` (per-request planning) and later phases (execution,
//! persistence). See `docs/analysis-dag-redesign.md` for the audited
//! rationale behind the node/artifact ID choices here.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stable, UI-text-independent identifier for an analysis node. Kept as a
/// newtype around `String` (not an enum) so historical `AnalysisPlan`/`Run`
/// snapshots deserialize across schema changes without a hard-coded variant
/// list — see docs/analysis-dag-redesign.md §11 on legacy history rows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisNodeId(pub String);

impl AnalysisNodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AnalysisNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for AnalysisNodeId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// The kind of artifact a node consumes or produces. Deliberately more
/// granular than today's cache files (docs/analysis-dag-redesign.md §4) so
/// the domain model does not have to change shape again when Phase 2/4
/// split currently-combined files (e.g. `transcript.json`) apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ArtifactKind {
    SourceMedia,
    MusicAnalysis,
    KeyAnalysis,
    RhythmAnalysis,
    AudioDescriptors,
    VocalStem,
    InstrumentalStem,
    RawVocalStem,
    DenoisedVocalStem,
    DereverbedVocalStem,
    AnalysisVocalStem,
    HighQualityInstrumentalStem,
    DenoisedInstrumentalStem,
    DereverbedInstrumentalStem,
    KaraokeInstrumentalStem,
    DrumStem,
    BassStem,
    GuitarStem,
    PianoStem,
    OtherStem,
    PitchTrack,
    PitchNoteCandidates,
    LyricsInput,
    PreprocessedAudio,
    RecognizedText,
    AsrSegments,
    TimedTranscript,
    CandidateChart,
    AuthoredChart,
}

/// Whether a node can be turned off for a single run, and what happens to
/// its dependents when it is. See docs/analysis-dag-redesign.md §6 for the
/// Freeze/Disable/Bypass/Invalidate distinction this feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum DisablePolicy {
    /// Disabling is not offered; the node always runs when targeted.
    AlwaysRequired,
    /// Can be disabled; downstream nodes become Blocked unless a Freeze or
    /// Bypass supplies their input another way.
    Optional,
}

/// How a node's cache signature should be computed. `Generalized` is the
/// stem-separation pattern (pipeline.py::_cached_separator_matches) —
/// algorithm + normalized parameters + input hashes, deliberately excluding
/// unrelated sibling outputs. Phase 2 gives this real teeth; Phase 1 only
/// records the intent per node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum CachePolicy {
    Generalized,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisNodeSpec {
    pub id: AnalysisNodeId,
    pub label: String,
    pub inputs: Vec<ArtifactKind>,
    pub outputs: Vec<ArtifactKind>,
    pub disable_policy: DisablePolicy,
    pub cache_policy: CachePolicy,
    pub algorithm_version: String,
    /// Non-empty for compound nodes (e.g. `music.analysis`); the listed
    /// children appear as their own entries in `AnalysisGraphSpec::nodes`.
    pub compound_children: Vec<AnalysisNodeId>,
}

impl AnalysisNodeSpec {
    pub fn is_compound(&self) -> bool {
        !self.compound_children.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisEdge {
    pub from: AnalysisNodeId,
    pub to: AnalysisNodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisGraphSpec {
    pub schema_version: u32,
    pub nodes: Vec<AnalysisNodeSpec>,
    pub edges: Vec<AnalysisEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphValidationError {
    DuplicateNodeId(AnalysisNodeId),
    UnknownNodeInEdge(AnalysisNodeId),
    Cycle(Vec<AnalysisNodeId>),
}

impl std::fmt::Display for GraphValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(id) => write!(f, "duplicate node id: {id}"),
            Self::UnknownNodeInEdge(id) => write!(f, "edge references unknown node: {id}"),
            Self::Cycle(path) => {
                let rendered = path
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "cycle detected: {rendered}")
            }
        }
    }
}

impl AnalysisGraphSpec {
    pub fn node(&self, id: &AnalysisNodeId) -> Option<&AnalysisNodeSpec> {
        self.nodes.iter().find(|node| &node.id == id)
    }

    fn adjacency(&self) -> BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> {
        let mut map: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> =
            self.nodes.iter().map(|n| (&n.id, Vec::new())).collect();
        for edge in &self.edges {
            map.entry(&edge.from).or_default().push(&edge.to);
        }
        map
    }

    fn reverse_adjacency(&self) -> BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> {
        let mut map: BTreeMap<&AnalysisNodeId, Vec<&AnalysisNodeId>> =
            self.nodes.iter().map(|n| (&n.id, Vec::new())).collect();
        for edge in &self.edges {
            map.entry(&edge.to).or_default().push(&edge.from);
        }
        map
    }

    /// Rejects duplicate node ids, edges pointing at nodes that don't exist,
    /// and cycles. Every graph the app hands to a planner must pass this
    /// first (phase plan §1.4 / Phase 1 test checklist).
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        let mut seen = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(&node.id) {
                return Err(GraphValidationError::DuplicateNodeId(node.id.clone()));
            }
        }
        let known: BTreeSet<&AnalysisNodeId> = self.nodes.iter().map(|n| &n.id).collect();
        for edge in &self.edges {
            if !known.contains(&edge.from) {
                return Err(GraphValidationError::UnknownNodeInEdge(edge.from.clone()));
            }
            if !known.contains(&edge.to) {
                return Err(GraphValidationError::UnknownNodeInEdge(edge.to.clone()));
            }
        }
        self.topo_order().map(|_| ())
    }

    /// Kahn's algorithm; returns the offending cycle (as encountered) if the
    /// graph isn't a DAG.
    pub fn topo_order(&self) -> Result<Vec<AnalysisNodeId>, GraphValidationError> {
        let forward = self.adjacency();
        let mut in_degree: BTreeMap<&AnalysisNodeId, usize> =
            self.nodes.iter().map(|n| (&n.id, 0)).collect();
        for targets in forward.values() {
            for target in targets {
                *in_degree.entry(target).or_default() += 1;
            }
        }

        let mut queue: Vec<&AnalysisNodeId> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| *id)
            .collect();
        queue.sort();

        let mut ordered = Vec::new();
        while let Some(id) = queue.pop() {
            ordered.push(id.clone());
            if let Some(targets) = forward.get(id) {
                let mut newly_free = Vec::new();
                for target in targets {
                    let degree = in_degree.get_mut(target).expect("known node");
                    *degree -= 1;
                    if *degree == 0 {
                        newly_free.push(*target);
                    }
                }
                newly_free.sort();
                queue.extend(newly_free);
            }
            queue.sort();
        }

        if ordered.len() != self.nodes.len() {
            let stuck: Vec<AnalysisNodeId> = in_degree
                .into_iter()
                .filter(|(_, degree)| *degree > 0)
                .map(|(id, _)| id.clone())
                .collect();
            return Err(GraphValidationError::Cycle(stuck));
        }
        Ok(ordered)
    }

    /// Transitive upstream closure of `id`, not including `id` itself.
    pub fn dependencies_of(&self, id: &AnalysisNodeId) -> BTreeSet<AnalysisNodeId> {
        let reverse = self.reverse_adjacency();
        let mut visited = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if let Some(parents) = reverse.get(current) {
                for parent in parents {
                    if visited.insert((*parent).clone()) {
                        stack.push(parent);
                    }
                }
            }
        }
        visited
    }

    /// Transitive downstream closure of `id`, not including `id` itself.
    pub fn dependents_of(&self, id: &AnalysisNodeId) -> BTreeSet<AnalysisNodeId> {
        let forward = self.adjacency();
        let mut visited = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if let Some(children) = forward.get(current) {
                for child in children {
                    if visited.insert((*child).clone()) {
                        stack.push(child);
                    }
                }
            }
        }
        visited
    }
}

// A graph node is a declarative record with eight independent schema fields;
// grouping them would only hide names at the call sites without reducing
// runtime coupling.
#[allow(clippy::too_many_arguments)]
fn spec(
    id: &str,
    label: &str,
    inputs: &[ArtifactKind],
    outputs: &[ArtifactKind],
    disable_policy: DisablePolicy,
    cache_policy: CachePolicy,
    algorithm_version: &str,
    compound_children: &[&str],
) -> AnalysisNodeSpec {
    AnalysisNodeSpec {
        id: AnalysisNodeId::new(id),
        label: label.to_string(),
        inputs: inputs.to_vec(),
        outputs: outputs.to_vec(),
        disable_policy,
        cache_policy,
        algorithm_version: algorithm_version.to_string(),
        compound_children: compound_children
            .iter()
            .map(|c| AnalysisNodeId::new(*c))
            .collect(),
    }
}

fn edge(from: &str, to: &str) -> AnalysisEdge {
    AnalysisEdge {
        from: AnalysisNodeId::new(from),
        to: AnalysisNodeId::new(to),
    }
}

/// The first-version node/edge universe, per docs/analysis-dag-redesign.md
/// §3 and §5. This is the *full possible* graph across every lyrics route;
/// `analysis_plan::build_plan` narrows it down to what actually applies for
/// one song and one request.
pub fn baseline_graph_spec() -> AnalysisGraphSpec {
    use ArtifactKind::*;
    use CachePolicy::*;
    use DisablePolicy::*;

    let nodes = vec![
        spec(
            "preflight",
            "Preflight",
            &[SourceMedia],
            &[SourceMedia],
            AlwaysRequired,
            None,
            "1",
            &[],
        ),
        spec(
            "music.analysis",
            "Music Analysis",
            &[SourceMedia],
            &[MusicAnalysis],
            Optional,
            Generalized,
            "1",
            &["music.key", "music.rhythm", "music.descriptors"],
        ),
        spec(
            "music.key",
            "Key Detection",
            &[SourceMedia],
            &[KeyAnalysis],
            AlwaysRequired,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "music.rhythm",
            "Rhythm / BPM",
            &[SourceMedia],
            &[RhythmAnalysis],
            AlwaysRequired,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "music.descriptors",
            "Audio Descriptors",
            &[SourceMedia],
            &[AudioDescriptors],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "stems.separate",
            "Stem Separation",
            &[SourceMedia],
            &[VocalStem, InstrumentalStem],
            Optional,
            Generalized,
            "1",
            &[
                "stems.vocals",
                "vocals.denoise",
                "vocals.dereverb",
                "stems.instrumental",
                "instrumental.denoise",
                "instrumental.dereverb",
                "stems.karaoke",
                "stems.multistem",
                "stems.bind_analysis_outputs",
            ],
        ),
        spec(
            "stems.vocals",
            "Vocal Extraction",
            &[SourceMedia],
            &[RawVocalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "vocals.denoise",
            "Vocal Denoise",
            &[RawVocalStem],
            &[DenoisedVocalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "vocals.dereverb",
            "Vocal Dereverb",
            &[RawVocalStem, DenoisedVocalStem],
            &[DereverbedVocalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "stems.instrumental",
            "Accompaniment Extraction",
            &[SourceMedia],
            &[HighQualityInstrumentalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "instrumental.denoise",
            "BGM Denoise",
            &[HighQualityInstrumentalStem],
            &[DenoisedInstrumentalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "instrumental.dereverb",
            "BGM Dereverb",
            &[HighQualityInstrumentalStem, DenoisedInstrumentalStem],
            &[DereverbedInstrumentalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "stems.karaoke",
            "Karaoke Accompaniment",
            &[SourceMedia],
            &[KaraokeInstrumentalStem],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "stems.multistem",
            "Six-Stem Separation",
            &[SourceMedia],
            &[
                VocalStem, DrumStem, BassStem, GuitarStem, PianoStem, OtherStem,
            ],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "stems.bind_analysis_outputs",
            "Bind Analysis Audio",
            &[
                RawVocalStem,
                DenoisedVocalStem,
                DereverbedVocalStem,
                HighQualityInstrumentalStem,
                DenoisedInstrumentalStem,
                DereverbedInstrumentalStem,
            ],
            &[AnalysisVocalStem, VocalStem, InstrumentalStem],
            AlwaysRequired,
            None,
            "1",
            &[],
        ),
        spec(
            "pitch.extract",
            "Pitch Extraction",
            &[AnalysisVocalStem],
            &[PitchTrack, PitchNoteCandidates],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "lyrics.preprocess",
            "Vocal Preprocessing",
            &[AnalysisVocalStem],
            &[PreprocessedAudio],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "lyrics.transcribe",
            "Transcription",
            &[PreprocessedAudio],
            &[RecognizedText, AsrSegments],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "lyrics.align",
            "Forced Alignment",
            &[PreprocessedAudio, LyricsInput],
            &[TimedTranscript],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "lyrics.import_timed",
            "Timed Lyrics Import",
            &[LyricsInput],
            &[TimedTranscript],
            Optional,
            Generalized,
            "1",
            &[],
        ),
        spec(
            "chart.build_candidate",
            "Build Candidate Chart",
            &[PitchNoteCandidates, TimedTranscript, InstrumentalStem],
            &[CandidateChart],
            AlwaysRequired,
            Generalized,
            "1",
            &[],
        ),
    ];

    let edges = vec![
        edge("preflight", "music.analysis"),
        edge("music.analysis", "music.key"),
        edge("music.analysis", "music.rhythm"),
        edge("music.analysis", "music.descriptors"),
        edge("preflight", "stems.separate"),
        edge("preflight", "stems.vocals"),
        edge("preflight", "stems.instrumental"),
        edge("preflight", "stems.karaoke"),
        edge("preflight", "stems.multistem"),
        edge("stems.vocals", "vocals.denoise"),
        edge("stems.vocals", "vocals.dereverb"),
        edge("stems.vocals", "stems.bind_analysis_outputs"),
        edge("vocals.denoise", "vocals.dereverb"),
        edge("vocals.denoise", "stems.bind_analysis_outputs"),
        edge("vocals.dereverb", "stems.bind_analysis_outputs"),
        edge("stems.instrumental", "instrumental.denoise"),
        edge("stems.instrumental", "instrumental.dereverb"),
        edge("stems.instrumental", "stems.bind_analysis_outputs"),
        edge("instrumental.denoise", "instrumental.dereverb"),
        edge("instrumental.denoise", "stems.bind_analysis_outputs"),
        edge("instrumental.dereverb", "stems.bind_analysis_outputs"),
        edge("stems.bind_analysis_outputs", "pitch.extract"),
        edge("stems.bind_analysis_outputs", "lyrics.preprocess"),
        edge("lyrics.preprocess", "lyrics.transcribe"),
        edge("lyrics.preprocess", "lyrics.align"),
        edge("lyrics.transcribe", "lyrics.align"),
        edge("preflight", "lyrics.import_timed"),
        edge("pitch.extract", "chart.build_candidate"),
        edge("lyrics.align", "chart.build_candidate"),
        edge("lyrics.import_timed", "chart.build_candidate"),
        edge("stems.bind_analysis_outputs", "chart.build_candidate"),
        // Parakeet's ASR step emits word timing directly (no separate
        // alignment pass) -- docs/analysis-dag-redesign.md §5 dynamic
        // branch rules. lyrics.align stays the edge used by Known
        // Lyrics/Whisper routes; this is the Parakeet-only path.
        edge("lyrics.transcribe", "chart.build_candidate"),
    ];

    AnalysisGraphSpec {
        schema_version: 1,
        nodes,
        edges,
    }
}

/// The four lyrics-route node ids that `analysis_plan::LyricsRoute` gates.
/// All other nodes in `baseline_graph_spec` are route-independent.
pub fn lyrics_route_node_ids() -> BTreeSet<AnalysisNodeId> {
    [
        "lyrics.preprocess",
        "lyrics.transcribe",
        "lyrics.align",
        "lyrics.import_timed",
    ]
    .into_iter()
    .map(AnalysisNodeId::new)
    .collect()
}

/// Optional stem-pipeline nodes that a settings snapshot can leave out of
/// the plan universe. `stems.bind_analysis_outputs` stays required for
/// charting; `stems.separate` stays as the MINI/compat shell.
pub fn optional_stem_node_ids() -> BTreeSet<AnalysisNodeId> {
    [
        "stems.vocals",
        "vocals.denoise",
        "vocals.dereverb",
        "stems.instrumental",
        "instrumental.denoise",
        "instrumental.dereverb",
        "stems.karaoke",
        "stems.multistem",
    ]
    .into_iter()
    .map(AnalysisNodeId::new)
    .collect()
}

/// Default chart path when a request does not carry a settings snapshot:
/// independent dedicated vocal and BGM extraction plus bind, with no
/// cleanup or side paths.
pub fn default_active_stem_nodes() -> BTreeSet<AnalysisNodeId> {
    [
        "stems.vocals",
        "stems.instrumental",
        "stems.bind_analysis_outputs",
    ]
    .into_iter()
    .map(AnalysisNodeId::new)
    .collect()
}

/// Compound shell plus every stem child. Disabling or bypassing
/// `stems.separate` applies to this whole group so pitch/lyrics still see
/// a single gate even though bind is the real ancestor.
pub fn stem_group_node_ids() -> BTreeSet<AnalysisNodeId> {
    let mut ids = optional_stem_node_ids();
    ids.insert(AnalysisNodeId::new("stems.separate"));
    ids.insert(AnalysisNodeId::new("stems.bind_analysis_outputs"));
    ids
}

pub fn active_stem_nodes_from_settings(
    settings: &crate::audio_processing::AudioProcessingSettings,
) -> BTreeSet<AnalysisNodeId> {
    let mut nodes = BTreeSet::new();
    nodes.insert(AnalysisNodeId::new("stems.bind_analysis_outputs"));
    if crate::audio_processing::is_demucs_chart_path(settings) {
        nodes.insert(AnalysisNodeId::new("stems.multistem"));
        return nodes;
    }
    nodes.insert(AnalysisNodeId::new("stems.vocals"));
    for model_id in settings
        .vocal_cleanup_chain
        .iter()
        .filter(|model_id| crate::audio_processing::cleanup_model_enabled(model_id))
    {
        if model_id.contains("denoise") {
            nodes.insert(AnalysisNodeId::new("vocals.denoise"));
        } else {
            nodes.insert(AnalysisNodeId::new("vocals.dereverb"));
        }
    }
    nodes.insert(AnalysisNodeId::new("stems.instrumental"));
    for model_id in settings
        .accompaniment_cleanup_chain
        .iter()
        .filter(|model_id| crate::audio_processing::cleanup_model_enabled(model_id))
    {
        if model_id.contains("denoise") {
            nodes.insert(AnalysisNodeId::new("instrumental.denoise"));
        } else {
            nodes.insert(AnalysisNodeId::new("instrumental.dereverb"));
        }
    }
    if settings.karaoke_model_id.is_some() {
        nodes.insert(AnalysisNodeId::new("stems.karaoke"));
    }
    if settings.multistem_model_id.is_some() {
        nodes.insert(AnalysisNodeId::new("stems.multistem"));
    }
    nodes
}

/// Stable bridge between catalog execution step ids and DAG node ids.
/// Unknown future steps stay represented by the compound separation shell
/// until the graph gains a dedicated node for them.
pub fn analysis_node_for_audio_step(step_id: &str) -> AnalysisNodeId {
    AnalysisNodeId::new(match step_id {
        "extract_vocals" => "stems.vocals",
        "denoise_vocals" => "vocals.denoise",
        "dereverb_vocals" => "vocals.dereverb",
        "extract_accompaniment" => "stems.instrumental",
        "denoise_accompaniment" => "instrumental.denoise",
        "dereverb_accompaniment" => "instrumental.dereverb",
        "extract_karaoke" => "stems.karaoke",
        "separate_6s" | "legacy_htdemucs" => "stems.multistem",
        _ => "stems.separate",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_graph_is_valid() {
        baseline_graph_spec()
            .validate()
            .expect("baseline graph must validate");
    }

    #[test]
    fn duplicate_node_id_is_rejected() {
        let mut graph = baseline_graph_spec();
        let dup = graph.nodes[0].clone();
        graph.nodes.push(dup.clone());
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DuplicateNodeId(dup.id))
        );
    }

    #[test]
    fn edge_to_unknown_node_is_rejected() {
        let mut graph = baseline_graph_spec();
        graph.edges.push(edge("preflight", "does.not.exist"));
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::UnknownNodeInEdge(
                AnalysisNodeId::new("does.not.exist")
            ))
        );
    }

    #[test]
    fn cycle_is_rejected() {
        let mut graph = baseline_graph_spec();
        // pitch.extract already depends on bind; force the reverse edge
        // too so the two form a 2-cycle.
        graph
            .edges
            .push(edge("pitch.extract", "stems.bind_analysis_outputs"));
        assert!(matches!(
            graph.validate(),
            Err(GraphValidationError::Cycle(_))
        ));
    }

    #[test]
    fn dependencies_of_chart_build_include_full_upstream() {
        let graph = baseline_graph_spec();
        let deps = graph.dependencies_of(&AnalysisNodeId::new("chart.build_candidate"));
        for expected in [
            "preflight",
            "stems.vocals",
            "stems.bind_analysis_outputs",
            "pitch.extract",
            "lyrics.preprocess",
            "lyrics.align",
            "lyrics.import_timed",
            "lyrics.transcribe",
        ] {
            assert!(
                deps.contains(&AnalysisNodeId::new(expected)),
                "expected {expected} in dependency closure of chart.build_candidate"
            );
        }
        // music.analysis is an independent sibling under preflight --
        // chart.build_candidate's closure via stems must not pull it in.
        assert!(!deps.contains(&AnalysisNodeId::new("music.analysis")));
        // stems.separate is only the MINI/compat shell, not an ancestor of
        // the charting path.
        assert!(!deps.contains(&AnalysisNodeId::new("stems.separate")));
    }

    #[test]
    fn music_analysis_and_stems_are_independent_siblings() {
        let graph = baseline_graph_spec();
        let music_deps = graph.dependencies_of(&AnalysisNodeId::new("music.analysis"));
        let stems_deps = graph.dependencies_of(&AnalysisNodeId::new("stems.bind_analysis_outputs"));
        assert!(!music_deps.contains(&AnalysisNodeId::new("stems.bind_analysis_outputs")));
        assert!(!stems_deps.contains(&AnalysisNodeId::new("music.analysis")));
    }

    #[test]
    fn dependents_of_bind_cover_pitch_and_lyrics_but_not_music_analysis() {
        let graph = baseline_graph_spec();
        let dependents = graph.dependents_of(&AnalysisNodeId::new("stems.bind_analysis_outputs"));
        assert!(dependents.contains(&AnalysisNodeId::new("pitch.extract")));
        assert!(dependents.contains(&AnalysisNodeId::new("lyrics.preprocess")));
        assert!(dependents.contains(&AnalysisNodeId::new("chart.build_candidate")));
        assert!(!dependents.contains(&AnalysisNodeId::new("music.analysis")));
        assert!(!dependents.contains(&AnalysisNodeId::new("music.key")));
    }
}
