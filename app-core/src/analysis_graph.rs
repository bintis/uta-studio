//! Studio-owned structural types for the editable outer Workflow and artifact
//! lineage. Analysis Engine owns request planning, scheduling, and execution.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stable, UI-text-independent identifier for a Studio Workflow node.
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

/// The kind of immutable artifact a Workflow node consumes or produces.
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
    /// Generic workflow audio. Semantic role and processing history live in
    /// the audio descriptor and immutable lineage rather than enum variants.
    AudioStem,
    PitchTrack,
    PitchNoteCandidates,
    PitchEvidence,
    BoundaryEvidence,
    TechniqueEvidence,
    AcousticEvidence,
    LyricsInput,
    CanonicalLyrics,
    PreprocessedAudio,
    RecognizedText,
    AsrSegments,
    TranscriptEvidence,
    AlignmentEvidence,
    TimedTranscript,
    EvidenceBundle,
    CandidateGraph,
    CanonicalSingingTrack,
    HumanCorrectionSet,
    CandidateChart,
    AuthoredChart,
}

/// Whether an editable Workflow node may be disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum DisablePolicy {
    /// Disabling is not offered; the node always runs when targeted.
    AlwaysRequired,
    /// Can be disabled; downstream nodes become Blocked unless a Freeze or
    /// Bypass supplies their input another way.
    Optional,
}

/// Studio artifact-reuse intent. Engine independently validates execution.
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

    /// Rejects duplicate node ids, unknown edge endpoints, and cycles.
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
