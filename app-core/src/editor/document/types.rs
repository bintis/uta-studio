//! The editable chart document.
//!
//! [`EditorDocument`] owns a [`VocalChartV1`] and exposes flattened, index
//! addressed views so the timeline can render and mutate notes and lyrics
//! without knowing how the format nests tracks, phrases, and lyric tokens.
//!
//! Editing never rejects a transient overlap: the format forbids overlapping
//! notes, but a drag legitimately passes through one. Violations surface as
//! chart problems and block saving instead of fighting the pointer.

use std::collections::HashSet;

use utz::{NoteBonus, NotePitch, ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalTrackRole};

/// The shortest authorable note. Matches the analyzer-era editor so existing
/// charts keep their timing behaviour.
pub const MIN_NOTE_SECONDS: f64 = 0.03;

/// Editor-facing note classification. It projects the format's independent
/// `vocal_mode`, `bonus`, and `scoring.mode` fields onto the five choices the
/// timeline and the UltraStar exporter share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteKind {
    #[default]
    Normal,
    Golden,
    Freestyle,
    Rap,
    GoldenRap,
}

impl NoteKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Golden => "golden",
            Self::Freestyle => "freestyle",
            Self::Rap => "rap",
            Self::GoldenRap => "golden_rap",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value {
            "golden" => Self::Golden,
            "freestyle" => Self::Freestyle,
            "rap" => Self::Rap,
            "golden_rap" => Self::GoldenRap,
            _ => Self::Normal,
        }
    }

    /// Cycle order used by the editor's "change note type" action.
    pub fn cycle(self) -> Self {
        match self {
            Self::Normal => Self::Golden,
            Self::Golden => Self::Freestyle,
            Self::Freestyle => Self::Rap,
            Self::Rap => Self::GoldenRap,
            Self::GoldenRap => Self::Normal,
        }
    }

    /// Classifies a note the way the timeline and the UltraStar exporter see it.
    pub fn of(note: &VocalNote) -> Self {
        match (note.vocal_mode, note.bonus) {
            (VocalMode::Rap, NoteBonus::Golden) => Self::GoldenRap,
            (VocalMode::Rap | VocalMode::Spoken, _) => Self::Rap,
            (VocalMode::Freestyle, _) => Self::Freestyle,
            (_, NoteBonus::Golden) => Self::Golden,
            _ if note.scoring.mode == ScoringMode::None => Self::Freestyle,
            _ => Self::Normal,
        }
    }

    pub(crate) fn apply(self, note: &mut VocalNote) {
        let (mode, bonus, scoring) = match self {
            Self::Normal => (VocalMode::Pitched, NoteBonus::Normal, ScoringMode::Pitch),
            Self::Golden => (VocalMode::Pitched, NoteBonus::Golden, ScoringMode::Pitch),
            Self::Freestyle => (VocalMode::Freestyle, NoteBonus::Normal, ScoringMode::None),
            Self::Rap => (VocalMode::Rap, NoteBonus::Normal, ScoringMode::Rhythm),
            Self::GoldenRap => (VocalMode::Rap, NoteBonus::Golden, ScoringMode::Rhythm),
        };
        note.vocal_mode = mode;
        note.bonus = bonus;
        note.scoring.mode = scoring;
        // Pitch scoring requires a target; keep one so the chart stays valid.
        if scoring == ScoringMode::Pitch && note.pitch.is_none() {
            note.pitch = Some(NotePitch { midi: 60, cents: 0 });
        }
    }
}

/// What one track of a chart is for. Mirrors the format's `VocalTrackRole`
/// exactly — role is purely musical. A track's UltraStar-style duet part
/// number (`VocalTrack::part`) is a separate, automatically derived concept:
/// [`EditorDocument::recompute_track_parts`] assigns contiguous parts to every
/// `Lead` track whenever there is more than one, so a second lead track is
/// what makes a chart a duet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackRole {
    #[default]
    Lead,
    Harmony,
    Backing,
    Adlib,
}

impl TrackRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Harmony => "harmony",
            Self::Backing => "backing",
            Self::Adlib => "ad-lib",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value {
            "harmony" => Self::Harmony,
            "backing" => Self::Backing,
            "ad-lib" | "adlib" => Self::Adlib,
            _ => Self::Lead,
        }
    }

    /// Cycle order used by the track strip's role button.
    pub fn cycle(self) -> Self {
        match self {
            Self::Lead => Self::Harmony,
            Self::Harmony => Self::Backing,
            Self::Backing => Self::Adlib,
            Self::Adlib => Self::Lead,
        }
    }

    /// Whether a player is expected to sing this track, as opposed to it being
    /// reference or colour material. Drives the UltraStar duet projection.
    pub fn is_sung(self) -> bool {
        matches!(self, Self::Lead)
    }

    /// Reads the role a chart stores.
    pub fn of(role: VocalTrackRole) -> Self {
        match role {
            VocalTrackRole::Lead => Self::Lead,
            VocalTrackRole::Harmony => Self::Harmony,
            VocalTrackRole::Backing => Self::Backing,
            VocalTrackRole::Adlib => Self::Adlib,
        }
    }

    pub(crate) fn to_format(self) -> VocalTrackRole {
        match self {
            Self::Lead => VocalTrackRole::Lead,
            Self::Harmony => VocalTrackRole::Harmony,
            Self::Backing => VocalTrackRole::Backing,
            Self::Adlib => VocalTrackRole::Adlib,
        }
    }
}

/// A track as the track strip sees it.
#[derive(Debug, Clone)]
pub struct TrackSummary {
    pub index: usize,
    pub id: String,
    pub role: TrackRole,
    /// UltraStar-style duet part number (1, 2, ...), when this track is one
    /// of two or more lead tracks. `None` for a solo lead or any non-lead
    /// track.
    pub part: Option<u32>,
    pub singer: Option<String>,
    pub scoring_enabled: bool,
    pub note_count: usize,
    pub phrase_count: usize,
    /// Seconds the track actually sings, for the coverage bar.
    pub sung_seconds: f64,
    /// First note start and last note end, in seconds.
    pub span: (f64, f64),
}

/// Addresses a lyric token as (phrase ordinal, text-token ordinal). Held notes
/// carry continuation tokens, which are not separately addressable: they belong
/// to the text token they continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LyricAddress {
    pub segment: usize,
    pub word: usize,
}

/// A note as the timeline sees it.
#[derive(Debug, Clone)]
pub struct ChartNote {
    pub index: usize,
    pub id: String,
    pub phrase: usize,
    pub start: f64,
    pub end: f64,
    pub midi: f64,
    pub kind: NoteKind,
    /// Whether the note carries a pitch target at all. Rhythm and spoken notes
    /// render on a neutral row rather than a pitch row.
    pub pitched: bool,
    /// An unclassified note nobody has triaged yet — the analyzer's
    /// placeholder for a lyric it couldn't match to a detected pitch, or one
    /// freed by unbinding. Distinct from an intentionally authored `Rap`
    /// note: nothing else in this module ever sets `VocalMode::Spoken`.
    pub placeholder: bool,
    /// The note contributes to the score in some way.
    pub scores: bool,
    /// The note is scored against its pitch target specifically.
    pub scores_pitch: bool,
    pub golden: bool,
    /// The note holds a syllable that started on an earlier note.
    pub continues_lyric: bool,
    pub lyric: Option<String>,
}

/// A lyric token as the lyric lane sees it.
#[derive(Debug, Clone)]
pub struct ChartLyric {
    pub address: LyricAddress,
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// A token whose owning note carries a pitch target has note guidance.
    pub guided: bool,
    /// Flattened index of the note that owns the token.
    pub note: usize,
    /// Flattened indices of the notes (if any) that hold this syllable
    /// through a pitch change — a held note whose pitch glides partway
    /// through, authored as a chain of continuation tokens. Empty for a
    /// syllable that isn't held past its own note.
    pub continuation_notes: Vec<usize>,
}

/// A note copied to the editor clipboard, with times relative to the copy origin.
#[derive(Debug, Clone)]
pub struct ClipboardNote {
    pub(crate) offset: u64,
    pub(crate) duration: u64,
    pub(crate) pitch: Option<NotePitch>,
    pub(crate) kind: NoteKind,
    pub(crate) weight: f64,
    pub(crate) text: Option<String>,
}

pub(crate) struct FlatNote {
    pub(crate) phrase: usize,
    pub(crate) note: VocalNote,
}

pub struct EditorDocument {
    pub(crate) chart: VocalChartV1,
    pub(crate) track: usize,
    pub(crate) revision: u64,
    pub(crate) used_ids: HashSet<String>,
    pub(crate) next_id: u64,
}
