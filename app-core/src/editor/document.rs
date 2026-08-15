//! The editable chart document.
//!
//! [`EditorDocument`] owns a [`VocalChartV1`] and exposes flattened, index
//! addressed views so the timeline can render and mutate notes and lyrics
//! without knowing how the format nests tracks, phrases, and lyric tokens.
//!
//! Editing never rejects a transient overlap: the format forbids overlapping
//! notes, but a drag legitimately passes through one. Violations surface as
//! chart problems and block saving instead of fighting the pointer.

use std::collections::{BTreeSet, HashSet};

use utz::{
    DEFAULT_TIMEBASE, LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring,
    ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

use super::{round_units_to_millis, seconds_to_units, units_to_seconds};

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

    fn apply(self, note: &mut VocalNote) {
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
/// with the editor-facing naming the track strip and the UltraStar exporter
/// share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackRole {
    #[default]
    Lead,
    Duet,
    Harmony,
    Backing,
    Adlib,
}

impl TrackRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Duet => "duet",
            Self::Harmony => "harmony",
            Self::Backing => "backing",
            Self::Adlib => "ad-lib",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value {
            "duet" => Self::Duet,
            "harmony" => Self::Harmony,
            "backing" => Self::Backing,
            "ad-lib" | "adlib" => Self::Adlib,
            _ => Self::Lead,
        }
    }

    /// Cycle order used by the track strip's role button.
    pub fn cycle(self) -> Self {
        match self {
            Self::Lead => Self::Duet,
            Self::Duet => Self::Harmony,
            Self::Harmony => Self::Backing,
            Self::Backing => Self::Adlib,
            Self::Adlib => Self::Lead,
        }
    }

    /// Whether a player is expected to sing this track, as opposed to it being
    /// reference or colour material. Drives the UltraStar duet projection.
    pub fn is_sung(self) -> bool {
        matches!(self, Self::Lead | Self::Duet)
    }

    /// Reads the role a chart stores.
    pub fn of(role: VocalTrackRole) -> Self {
        match role {
            VocalTrackRole::Lead => Self::Lead,
            VocalTrackRole::Duet => Self::Duet,
            VocalTrackRole::Harmony => Self::Harmony,
            VocalTrackRole::Backing => Self::Backing,
            VocalTrackRole::Adlib => Self::Adlib,
        }
    }

    fn to_format(self) -> VocalTrackRole {
        match self {
            Self::Lead => VocalTrackRole::Lead,
            Self::Duet => VocalTrackRole::Duet,
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
}

/// A note copied to the editor clipboard, with times relative to the copy origin.
#[derive(Debug, Clone)]
pub struct ClipboardNote {
    offset: u64,
    duration: u64,
    pitch: Option<NotePitch>,
    kind: NoteKind,
    weight: f64,
    text: Option<String>,
}

struct FlatNote {
    phrase: usize,
    note: VocalNote,
}

pub struct EditorDocument {
    chart: VocalChartV1,
    track: usize,
    revision: u64,
    used_ids: HashSet<String>,
    next_id: u64,
}

impl EditorDocument {
    pub fn new(chart: VocalChartV1) -> Self {
        let mut used_ids = HashSet::new();
        for track in &chart.tracks {
            used_ids.insert(track.id.clone());
            for phrase in &track.phrases {
                used_ids.insert(phrase.id.clone());
                for note in &phrase.notes {
                    used_ids.insert(note.id.clone());
                    for token in &note.lyrics {
                        if let LyricToken::Text(token) = token {
                            used_ids.insert(token.id.clone());
                        }
                    }
                }
            }
        }
        Self {
            chart,
            track: 0,
            revision: 0,
            used_ids,
            next_id: 1,
        }
    }

    pub fn chart(&self) -> &VocalChartV1 {
        &self.chart
    }

    /// Returns a save-ready chart: notes ordered inside their phrase, phrases
    /// ordered, and empty phrases dropped.
    pub fn to_chart(&self) -> VocalChartV1 {
        let mut chart = self.chart.clone();
        for track in &mut chart.tracks {
            for phrase in &mut track.phrases {
                phrase.notes.sort_by_key(|note| note.start);
            }
            track.phrases.retain(|phrase| !phrase.notes.is_empty());
            track
                .phrases
                .sort_by_key(|phrase| phrase.notes.first().map(|note| note.start).unwrap_or(0));
        }
        chart.tracks.retain(|track| !track.phrases.is_empty());
        chart
    }

    /// Increments on every accepted mutation so the UI can skip rebuilding an
    /// unchanged timeline.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn language(&self) -> Option<&str> {
        self.chart.language.as_deref()
    }

    pub fn set_language(&mut self, language: Option<String>) {
        if self.chart.language != language {
            self.chart.language = language;
            self.touch();
        }
    }

    fn timebase(&self) -> u64 {
        if self.chart.timebase == 0 {
            DEFAULT_TIMEBASE
        } else {
            self.chart.timebase
        }
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn to_units(&self, seconds: f64) -> u64 {
        round_units_to_millis(seconds_to_units(seconds, self.timebase()), self.timebase())
    }

    fn to_seconds(&self, units: u64) -> f64 {
        units_to_seconds(units, self.timebase())
    }

    fn min_duration(&self) -> u64 {
        self.to_units(MIN_NOTE_SECONDS).max(1)
    }

    /// Languages written without inter-word spaces join lyric tokens directly.
    fn compact_language(&self) -> bool {
        self.chart.language.as_deref().is_some_and(|language| {
            ["zh", "ja", "ko"]
                .iter()
                .any(|prefix| language.to_ascii_lowercase().starts_with(prefix))
        })
    }

    fn default_join(&self) -> LyricJoin {
        if self.compact_language() {
            LyricJoin::None
        } else {
            LyricJoin::Space
        }
    }

    fn allocate_id(&mut self, prefix: &str) -> String {
        loop {
            let candidate = format!("{prefix}-{}", self.next_id);
            self.next_id += 1;
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn active_track(&self) -> Option<&VocalTrack> {
        self.chart.tracks.get(self.track)
    }

    // -- tracks -----------------------------------------------------------

    pub fn track_count(&self) -> usize {
        self.chart.tracks.len()
    }

    /// Index of the track every note and lyric operation applies to.
    pub fn active_track_index(&self) -> usize {
        self.track
    }

    pub fn set_active_track(&mut self, index: usize) -> bool {
        if index >= self.chart.tracks.len() || index == self.track {
            return false;
        }
        self.track = index;
        self.touch();
        true
    }

    pub fn tracks(&self) -> Vec<TrackSummary> {
        self.chart
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let notes = || track.phrases.iter().flat_map(|phrase| phrase.notes.iter());
                let sung = notes()
                    .map(|note| self.to_seconds(note.duration))
                    .sum::<f64>();
                let start = notes().map(|note| note.start).min().unwrap_or(0);
                let end = notes()
                    .map(|note| note.start.saturating_add(note.duration))
                    .max()
                    .unwrap_or(0);
                TrackSummary {
                    index,
                    id: track.id.clone(),
                    role: TrackRole::of(track.role),
                    singer: track.singer.clone(),
                    scoring_enabled: track.scoring_enabled,
                    note_count: notes().count(),
                    phrase_count: track.phrases.len(),
                    sung_seconds: sung,
                    span: (self.to_seconds(start), self.to_seconds(end)),
                }
            })
            .collect()
    }

    /// Notes of a track other than the active one, so the timeline can show
    /// where the other voices sing without making them editable.
    pub fn track_notes(&self, index: usize) -> Vec<ChartNote> {
        let Some(track) = self.chart.tracks.get(index) else {
            return Vec::new();
        };
        track
            .phrases
            .iter()
            .enumerate()
            .flat_map(|(phrase, entry)| entry.notes.iter().map(move |note| (phrase, note)))
            .enumerate()
            .map(|(flat, (phrase, note))| self.view_note(flat, phrase, note))
            .collect()
    }

    /// Adds an empty track and makes it the active one.
    pub fn add_track(&mut self, role: TrackRole) -> usize {
        let id = self.allocate_id("track");
        self.chart.tracks.push(VocalTrack {
            id,
            role: role.to_format(),
            singer: None,
            scoring_enabled: role.is_sung(),
            // A track with no notes has no phrases either; the first note
            // added to it creates one. Such a track is dropped on save,
            // because the format requires every track to sing something.
            phrases: Vec::new(),
        });
        self.track = self.chart.tracks.len() - 1;
        self.touch();
        self.track
    }

    /// Removes a track. The chart must keep at least one.
    pub fn remove_track(&mut self, index: usize) -> bool {
        if self.chart.tracks.len() < 2 || index >= self.chart.tracks.len() {
            return false;
        }
        self.chart.tracks.remove(index);
        self.track = self.track.min(self.chart.tracks.len() - 1);
        self.touch();
        true
    }

    pub fn set_track_role(&mut self, index: usize, role: TrackRole) -> bool {
        let Some(track) = self.chart.tracks.get_mut(index) else {
            return false;
        };
        let role = role.to_format();
        if track.role == role {
            return false;
        }
        track.role = role;
        self.touch();
        true
    }

    pub fn set_track_singer(&mut self, index: usize, singer: Option<String>) -> bool {
        let Some(track) = self.chart.tracks.get_mut(index) else {
            return false;
        };
        let singer = singer.filter(|name| !name.trim().is_empty());
        if track.singer == singer {
            return false;
        }
        track.singer = singer;
        self.touch();
        true
    }

    pub fn set_track_scoring(&mut self, index: usize, enabled: bool) -> bool {
        let Some(track) = self.chart.tracks.get_mut(index) else {
            return false;
        };
        if track.scoring_enabled == enabled {
            return false;
        }
        track.scoring_enabled = enabled;
        self.touch();
        true
    }

    /// Moves notes off the active track and onto another one, the path the
    /// format recommends for material that would otherwise overlap. Lyric
    /// continuations follow the text token they refer to, because the format
    /// requires a continuation to resolve inside its own track.
    pub fn move_notes_to_track(&mut self, indices: &BTreeSet<usize>, target: usize) -> usize {
        if target == self.track || target >= self.chart.tracks.len() || indices.is_empty() {
            return 0;
        }
        let mut flat = self.take_flat();
        // Take the selection out, plus any later note holding one of its
        // syllables, so a held note never loses the token it continues.
        let mut moving_texts = HashSet::new();
        for index in indices {
            if let Some(entry) = flat.get(*index) {
                for token in &entry.note.lyrics {
                    if let LyricToken::Text(token) = token {
                        moving_texts.insert(token.id.clone());
                    }
                }
            }
        }
        let mut moved = Vec::new();
        let mut kept = Vec::new();
        for (index, entry) in flat.drain(..).enumerate() {
            let continues_moved = entry.note.lyrics.iter().any(|token| {
                matches!(token, LyricToken::Continuation { continuation_of }
                    if moving_texts.contains(continuation_of))
            });
            if indices.contains(&index) || continues_moved {
                moved.push(entry.note);
            } else {
                kept.push(entry);
            }
        }
        let count = moved.len();
        self.restore_flat(kept);
        // Strip continuations the move orphaned in either direction.
        let orphans = orphaned_continuations(
            &moved
                .iter()
                .map(|note| FlatNote {
                    phrase: 0,
                    note: note.clone(),
                })
                .collect::<Vec<_>>(),
        );
        for note in &mut moved {
            note.lyrics.retain(|token| {
                !matches!(token, LyricToken::Continuation { continuation_of }
                    if orphans.contains(continuation_of))
            });
        }
        let phrase_id = self.allocate_id("phrase");
        if let Some(track) = self.chart.tracks.get_mut(target) {
            moved.sort_by_key(|note| note.start);
            track.phrases.push(VocalPhrase {
                id: phrase_id,
                notes: moved,
            });
            track
                .phrases
                .sort_by_key(|phrase| phrase.notes.first().map(|note| note.start).unwrap_or(0));
        }
        // The source track may have lost its remaining continuations.
        self.prune_orphaned_continuations();
        self.touch();
        count
    }

    fn prune_orphaned_continuations(&mut self) {
        for track in &mut self.chart.tracks {
            let mut texts = HashSet::new();
            for phrase in &track.phrases {
                for note in &phrase.notes {
                    for token in &note.lyrics {
                        if let LyricToken::Text(token) = token {
                            texts.insert(token.id.clone());
                        }
                    }
                }
            }
            for phrase in &mut track.phrases {
                for note in &mut phrase.notes {
                    note.lyrics.retain(|token| {
                        !matches!(token, LyricToken::Continuation { continuation_of }
                            if !texts.contains(continuation_of))
                    });
                }
            }
        }
    }

    // -- flattening -------------------------------------------------------

    fn flat_len(&self) -> usize {
        self.active_track()
            .map(|track| track.phrases.iter().map(|phrase| phrase.notes.len()).sum())
            .unwrap_or(0)
    }

    fn take_flat(&mut self) -> Vec<FlatNote> {
        let Some(track) = self.chart.tracks.get_mut(self.track) else {
            return Vec::new();
        };
        track
            .phrases
            .iter_mut()
            .enumerate()
            .flat_map(|(phrase, entry)| {
                std::mem::take(&mut entry.notes)
                    .into_iter()
                    .map(move |note| FlatNote { phrase, note })
            })
            .collect()
    }

    /// Reassembles a flattened note list, preserving phrase identity and
    /// dropping phrases that no longer hold a note.
    fn restore_flat(&mut self, flat: Vec<FlatNote>) {
        let Some(track) = self.chart.tracks.get_mut(self.track) else {
            return;
        };
        for entry in &mut track.phrases {
            entry.notes.clear();
        }
        for FlatNote { phrase, note } in flat {
            let phrase = phrase.min(track.phrases.len().saturating_sub(1));
            if let Some(entry) = track.phrases.get_mut(phrase) {
                entry.notes.push(note);
            }
        }
        track.phrases.retain(|phrase| !phrase.notes.is_empty());
        self.touch();
    }

    fn note_at(&self, index: usize) -> Option<(usize, &VocalNote)> {
        self.active_track()?
            .phrases
            .iter()
            .enumerate()
            .flat_map(|(phrase, entry)| entry.notes.iter().map(move |note| (phrase, note)))
            .nth(index)
    }

    fn note_at_mut(&mut self, index: usize) -> Option<&mut VocalNote> {
        self.chart
            .tracks
            .get_mut(self.track)?
            .phrases
            .iter_mut()
            .flat_map(|entry| entry.notes.iter_mut())
            .nth(index)
    }

    // -- note views -------------------------------------------------------

    pub fn note_count(&self) -> usize {
        self.flat_len()
    }

    fn view_note(&self, index: usize, phrase: usize, note: &VocalNote) -> ChartNote {
        ChartNote {
            index,
            id: note.id.clone(),
            phrase,
            start: self.to_seconds(note.start),
            end: self.to_seconds(note.start.saturating_add(note.duration)),
            midi: note.pitch.map(|pitch| pitch.midi as f64).unwrap_or(60.0),
            kind: NoteKind::of(note),
            pitched: note.pitch.is_some(),
            scores: note.scoring.mode != ScoringMode::None,
            scores_pitch: note.scoring.mode == ScoringMode::Pitch,
            golden: note.bonus == NoteBonus::Golden,
            continues_lyric: note
                .lyrics
                .iter()
                .any(|token| matches!(token, LyricToken::Continuation { .. })),
            lyric: note.lyrics.iter().find_map(|token| match token {
                LyricToken::Text(token) => Some(token.text.clone()),
                LyricToken::Continuation { .. } => None,
            }),
        }
    }

    pub fn notes(&self) -> Vec<ChartNote> {
        self.track_notes(self.track)
    }

    // -- lyric views ------------------------------------------------------

    /// Text tokens of a phrase, in order, as (token ordinal, note ordinal).
    fn phrase_tokens(&self, phrase: usize) -> Vec<(usize, usize)> {
        let Some(track) = self.active_track() else {
            return Vec::new();
        };
        let mut base = 0usize;
        for (index, entry) in track.phrases.iter().enumerate() {
            if index == phrase {
                let mut tokens = Vec::new();
                for (offset, note) in entry.notes.iter().enumerate() {
                    for token in &note.lyrics {
                        if matches!(token, LyricToken::Text(_)) {
                            tokens.push((tokens.len(), base + offset));
                        }
                    }
                }
                return tokens;
            }
            base += entry.notes.len();
        }
        Vec::new()
    }

    fn resolve(&self, address: LyricAddress) -> Option<usize> {
        self.phrase_tokens(address.segment)
            .into_iter()
            .find(|(word, _)| *word == address.word)
            .map(|(_, note)| note)
    }

    pub fn lyrics(&self) -> Vec<ChartLyric> {
        self.track_lyrics(self.track)
    }

    /// Lyric tokens of any track, for reporting problems the active track
    /// cannot see.
    pub fn track_lyrics(&self, index: usize) -> Vec<ChartLyric> {
        let Some(track) = self.chart.tracks.get(index) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        let mut note_index = 0usize;
        for (phrase, entry) in track.phrases.iter().enumerate() {
            let mut word = 0usize;
            for (offset, note) in entry.notes.iter().enumerate() {
                for token in &note.lyrics {
                    let LyricToken::Text(token) = token else {
                        continue;
                    };
                    // A held syllable ends at the last note that continues it.
                    let mut end = note.start.saturating_add(note.duration);
                    for held in entry.notes.iter().skip(offset + 1) {
                        let continues = held.lyrics.iter().any(|candidate| {
                            matches!(
                                candidate,
                                LyricToken::Continuation { continuation_of }
                                    if *continuation_of == token.id
                            )
                        });
                        if !continues {
                            break;
                        }
                        end = held.start.saturating_add(held.duration);
                    }
                    result.push(ChartLyric {
                        address: LyricAddress {
                            segment: phrase,
                            word,
                        },
                        start: self.to_seconds(note.start),
                        end: self.to_seconds(end),
                        text: token.text.clone(),
                        guided: note.pitch.is_some(),
                        note: note_index + offset,
                    });
                    word += 1;
                }
            }
            note_index += entry.notes.len();
        }
        result
    }

    /// Display text of a phrase, honouring each token's join policy.
    pub fn phrase_text(&self, phrase: usize) -> String {
        let Some(entry) = self
            .active_track()
            .and_then(|track| track.phrases.get(phrase))
        else {
            return String::new();
        };
        let mut text = String::new();
        for note in &entry.notes {
            for token in &note.lyrics {
                let LyricToken::Text(token) = token else {
                    continue;
                };
                if token.join_before == LyricJoin::Space && !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&token.text);
            }
        }
        text
    }

    pub fn phrase_count(&self) -> usize {
        self.active_track()
            .map(|track| track.phrases.len())
            .unwrap_or(0)
    }

    // -- note edits -------------------------------------------------------

    pub fn move_note(&mut self, index: usize, start: f64, end: f64, midi: f64) -> bool {
        let minimum = self.min_duration();
        let start_units = self.to_units(start);
        let end_units = self.to_units(end);
        let Some(note) = self.note_at_mut(index) else {
            return false;
        };
        note.start = start_units;
        note.duration = end_units.saturating_sub(start_units).max(minimum);
        if let Some(pitch) = note.pitch.as_mut() {
            pitch.midi = midi.round().clamp(0.0, 127.0) as u8;
        }
        self.touch();
        true
    }

    pub fn resize_note(&mut self, index: usize, start: f64, end: f64) -> bool {
        let minimum = self.min_duration();
        let start_units = self.to_units(start);
        let end_units = self.to_units(end);
        let Some(note) = self.note_at_mut(index) else {
            return false;
        };
        note.start = start_units;
        note.duration = end_units.saturating_sub(start_units).max(minimum);
        self.touch();
        true
    }

    /// Inserts a note into the phrase that covers `start`, creating a phrase
    /// when the insertion lands outside every existing one.
    pub fn insert_note(&mut self, start: f64, end: f64, midi: f64) -> Option<usize> {
        let minimum = self.min_duration();
        let start_units = self.to_units(start);
        let duration = self.to_units(end).saturating_sub(start_units).max(minimum);
        let id = self.allocate_id("note");
        let note = VocalNote {
            id,
            start: start_units,
            duration,
            pitch: Some(NotePitch {
                midi: midi.round().clamp(0.0, 127.0) as u8,
                cents: 0,
            }),
            vocal_mode: VocalMode::Pitched,
            bonus: NoteBonus::Normal,
            scoring: NoteScoring {
                mode: ScoringMode::Pitch,
                weight: 1.0,
            },
            lyrics: Vec::new(),
        };

        if self.chart.tracks.get(self.track).is_none() {
            let phrase_id = self.allocate_id("phrase");
            let track_id = self.allocate_id("track");
            self.chart.tracks.push(VocalTrack {
                id: track_id,
                role: VocalTrackRole::Lead,
                singer: None,
                scoring_enabled: true,
                phrases: vec![VocalPhrase {
                    id: phrase_id,
                    notes: vec![note],
                }],
            });
            self.track = self.chart.tracks.len() - 1;
            self.touch();
            return Some(0);
        }

        let target = self.phrase_for(start_units);
        let phrase = match target {
            Some(phrase) => phrase,
            None => {
                let phrase_id = self.allocate_id("phrase");
                let track = self.chart.tracks.get_mut(self.track)?;
                let position = track.phrases.partition_point(|phrase| {
                    phrase.notes.first().map(|note| note.start).unwrap_or(0) <= start_units
                });
                track.phrases.insert(
                    position,
                    VocalPhrase {
                        id: phrase_id,
                        notes: Vec::new(),
                    },
                );
                position
            }
        };
        let track = self.chart.tracks.get_mut(self.track)?;
        let entry = track.phrases.get_mut(phrase)?;
        let position = entry
            .notes
            .partition_point(|existing| existing.start <= start_units);
        entry.notes.insert(position, note);
        let index = track
            .phrases
            .iter()
            .take(phrase)
            .map(|phrase| phrase.notes.len())
            .sum::<usize>()
            + position;
        self.touch();
        Some(index)
    }

    /// The phrase whose span contains `units`, else the nearest phrase within
    /// one second, else none.
    fn phrase_for(&self, units: u64) -> Option<usize> {
        let track = self.active_track()?;
        let mut nearest = None::<(usize, u64)>;
        for (index, phrase) in track.phrases.iter().enumerate() {
            let start = phrase.notes.first().map(|note| note.start)?;
            let end = phrase
                .notes
                .iter()
                .map(|note| note.start.saturating_add(note.duration))
                .max()?;
            if units >= start && units <= end {
                return Some(index);
            }
            let distance = if units < start {
                start - units
            } else {
                units - end
            };
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((index, distance));
            }
        }
        nearest
            .filter(|(_, distance)| *distance <= self.timebase())
            .map(|(index, _)| index)
    }

    pub fn remove_notes(&mut self, indices: &BTreeSet<usize>) -> usize {
        let flat = self.take_flat();
        let before = flat.len();
        let kept = flat
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !indices.contains(index))
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        let removed = before - kept.len();
        let orphans = orphaned_continuations(&kept);
        self.restore_flat(kept);
        self.drop_continuations(&orphans);
        removed
    }

    /// Continuation tokens whose text token no longer exists must not survive:
    /// the format requires every reference to resolve inside its track.
    fn drop_continuations(&mut self, orphans: &HashSet<String>) {
        if orphans.is_empty() {
            return;
        }
        let Some(track) = self.chart.tracks.get_mut(self.track) else {
            return;
        };
        for phrase in &mut track.phrases {
            for note in &mut phrase.notes {
                note.lyrics.retain(|token| match token {
                    LyricToken::Continuation { continuation_of } => {
                        !orphans.contains(continuation_of)
                    }
                    LyricToken::Text(_) => true,
                });
            }
        }
    }

    pub fn split_notes(&mut self, indices: &BTreeSet<usize>, playhead: f64) -> BTreeSet<usize> {
        let minimum = self.min_duration();
        let playhead = self.to_units(playhead);
        let flat = self.take_flat();
        let mut output = Vec::with_capacity(flat.len());
        let mut selected = BTreeSet::new();
        for (index, entry) in flat.into_iter().enumerate() {
            let FlatNote { phrase, note } = entry;
            if !indices.contains(&index) || note.duration < minimum * 2 {
                if indices.contains(&index) {
                    selected.insert(output.len());
                }
                output.push(FlatNote { phrase, note });
                continue;
            }
            let start = note.start;
            let end = note.start + note.duration;
            let split = if playhead > start + minimum && playhead < end - minimum {
                playhead
            } else {
                start + note.duration / 2
            };
            let mut left = note.clone();
            left.duration = split - start;
            selected.insert(output.len());
            output.push(FlatNote { phrase, note: left });

            let mut right = note;
            right.start = split;
            right.duration = end - split;
            // The tail keeps singing the same syllable.
            let head = right.lyrics.iter().find_map(|token| match token {
                LyricToken::Text(token) => Some(token.id.clone()),
                LyricToken::Continuation { continuation_of } => Some(continuation_of.clone()),
            });
            right.lyrics = head
                .map(|continuation_of| vec![LyricToken::Continuation { continuation_of }])
                .unwrap_or_default();
            selected.insert(output.len());
            output.push(FlatNote {
                phrase,
                note: right,
            });
        }
        self.restore_flat(output);
        // Reassign ids only after the layout settles so each half stays unique.
        self.reassign_duplicate_note_ids();
        selected
    }

    fn reassign_duplicate_note_ids(&mut self) {
        let mut seen = HashSet::new();
        let mut renames = Vec::new();
        if let Some(track) = self.chart.tracks.get(self.track) {
            for (phrase, entry) in track.phrases.iter().enumerate() {
                for (offset, note) in entry.notes.iter().enumerate() {
                    if !seen.insert(note.id.clone()) {
                        renames.push((phrase, offset));
                    }
                }
            }
        }
        for (phrase, offset) in renames {
            let id = self.allocate_id("note");
            if let Some(note) = self
                .chart
                .tracks
                .get_mut(self.track)
                .and_then(|track| track.phrases.get_mut(phrase))
                .and_then(|phrase| phrase.notes.get_mut(offset))
            {
                note.id = id;
            }
        }
    }

    pub fn merge_notes(
        &mut self,
        indices: &BTreeSet<usize>,
        primary: Option<usize>,
    ) -> Option<usize> {
        if indices.len() < 2 {
            return None;
        }
        let flat = self.take_flat();
        let ordered = indices
            .iter()
            .copied()
            .filter(|index| *index < flat.len())
            .collect::<Vec<_>>();
        if ordered.len() < 2 {
            self.restore_flat(flat);
            return None;
        }
        let first = ordered[0];
        let source = primary
            .filter(|index| indices.contains(index))
            .unwrap_or(first);
        let mut merged = flat[source].note.clone();
        let phrase = flat[first].phrase;
        let start = ordered.iter().map(|index| flat[*index].note.start).min()?;
        let end = ordered
            .iter()
            .map(|index| flat[*index].note.start + flat[*index].note.duration)
            .max()?;
        merged.start = start;
        merged.duration = end.saturating_sub(start).max(self.min_duration());
        // The merged note keeps every syllable it absorbed, in time order.
        let mut lyrics = Vec::new();
        for index in &ordered {
            for token in &flat[*index].note.lyrics {
                if let LyricToken::Text(token) = token {
                    lyrics.push(LyricToken::Text(token.clone()));
                }
            }
        }
        if !lyrics.is_empty() {
            merged.lyrics = lyrics;
        }

        let mut insertion = 0usize;
        let mut output = Vec::with_capacity(flat.len() - ordered.len() + 1);
        for (index, entry) in flat.into_iter().enumerate() {
            if index == first {
                insertion = output.len();
                output.push(FlatNote {
                    phrase,
                    note: merged.clone(),
                });
            }
            if !indices.contains(&index) {
                output.push(entry);
            }
        }
        let orphans = orphaned_continuations(&output);
        self.restore_flat(output);
        self.drop_continuations(&orphans);
        Some(insertion)
    }

    pub fn quantize_notes(&mut self, indices: Option<&BTreeSet<usize>>, grid: f64) -> usize {
        if !(grid > 0.0) {
            return 0;
        }
        let grid_units = self.to_units(grid).max(1);
        let minimum = self.min_duration().max(grid_units);
        let count = self.flat_len();
        let mut changed = 0;
        for index in 0..count {
            if indices.is_some_and(|indices| !indices.contains(&index)) {
                continue;
            }
            let Some(note) = self.note_at_mut(index) else {
                continue;
            };
            let start = ((note.start + grid_units / 2) / grid_units) * grid_units;
            let end = note.start + note.duration;
            let snapped = ((end + grid_units / 2) / grid_units) * grid_units;
            note.start = start;
            note.duration = snapped.saturating_sub(start).max(minimum);
            changed += 1;
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    pub fn shift_notes(
        &mut self,
        indices: &BTreeSet<usize>,
        seconds: f64,
        semitones: f64,
        resize_end: bool,
    ) -> usize {
        if !seconds.is_finite() || !semitones.is_finite() {
            return 0;
        }
        let minimum = self.min_duration();
        let timebase = self.timebase() as f64;
        let earliest = indices
            .iter()
            .filter_map(|index| self.note_at(*index).map(|(_, note)| note.start))
            .min()
            .unwrap_or(0);
        // Never drag a selection through zero.
        let delta_units = (seconds * timebase).round() as i64;
        let safe_delta = delta_units.max(-(earliest as i64));
        let mut changed = 0;
        for index in indices.iter().copied().collect::<Vec<_>>() {
            let Some(note) = self.note_at_mut(index) else {
                continue;
            };
            if resize_end {
                let end = note.start.saturating_add(note.duration);
                let shifted = end.saturating_add_signed(delta_units);
                note.duration = shifted.saturating_sub(note.start).max(minimum);
            } else {
                note.start = note.start.saturating_add_signed(safe_delta);
                if let Some(pitch) = note.pitch.as_mut() {
                    pitch.midi = (pitch.midi as f64 + semitones).round().clamp(0.0, 127.0) as u8;
                }
            }
            changed += 1;
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    pub fn set_note_kind(&mut self, indices: &BTreeSet<usize>, kind: NoteKind) -> usize {
        let mut changed = 0;
        for index in indices.iter().copied().collect::<Vec<_>>() {
            if let Some(note) = self.note_at_mut(index) {
                kind.apply(note);
                changed += 1;
            }
        }
        if changed > 0 {
            self.touch();
        }
        changed
    }

    /// Advances the whole selection to the kind after the first note's kind, so
    /// a mixed selection converges instead of scattering.
    pub fn cycle_note_kinds(&mut self, indices: &BTreeSet<usize>) -> usize {
        let next = indices
            .iter()
            .find_map(|index| self.note_at(*index))
            .map(|(_, note)| NoteKind::of(note).cycle())
            .unwrap_or(NoteKind::Golden);
        self.set_note_kind(indices, next)
    }

    pub fn copy_notes(&self, indices: &BTreeSet<usize>) -> Vec<ClipboardNote> {
        let notes = self
            .active_track()
            .into_iter()
            .flat_map(|track| track.phrases.iter().flat_map(|phrase| phrase.notes.iter()));
        let mut selected = notes
            .enumerate()
            .filter(|(index, _)| indices.contains(index))
            .map(|(_, note)| note)
            .collect::<Vec<_>>();
        selected.sort_by_key(|note| note.start);
        let origin = selected.first().map(|note| note.start).unwrap_or(0);
        selected
            .into_iter()
            .map(|note| ClipboardNote {
                offset: note.start.saturating_sub(origin),
                duration: note.duration,
                pitch: note.pitch,
                kind: NoteKind::of(note),
                weight: note.scoring.weight,
                text: note.lyrics.iter().find_map(|token| match token {
                    LyricToken::Text(token) => Some(token.text.clone()),
                    LyricToken::Continuation { .. } => None,
                }),
            })
            .collect()
    }

    /// Seconds from the clipboard's origin to its last note end, so a duplicate
    /// can be dropped clear of the material it came from.
    pub fn clipboard_span(&self, clipboard: &[ClipboardNote]) -> f64 {
        clipboard
            .iter()
            .map(|note| self.to_seconds(note.offset.saturating_add(note.duration)))
            .fold(0.0, f64::max)
    }

    pub fn paste_notes(&mut self, clipboard: &[ClipboardNote], at: f64) -> BTreeSet<usize> {
        if clipboard.is_empty() {
            return BTreeSet::new();
        }
        let at_units = self.to_units(at);
        let minimum = self.min_duration();
        let join = self.default_join();
        let mut pasted = Vec::with_capacity(clipboard.len());
        for entry in clipboard {
            let id = self.allocate_id("note");
            let lyrics = match &entry.text {
                Some(text) => {
                    let token = self.allocate_id("lyric");
                    vec![LyricToken::Text(LyricTextToken {
                        id: token,
                        text: text.clone(),
                        join_before: join,
                        reading: None,
                        phonemes: None,
                    })]
                }
                None => Vec::new(),
            };
            let mut note = VocalNote {
                id,
                start: at_units.saturating_add(entry.offset),
                duration: entry.duration.max(minimum),
                pitch: entry.pitch,
                vocal_mode: VocalMode::Pitched,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Pitch,
                    weight: entry.weight,
                },
                lyrics,
            };
            entry.kind.apply(&mut note);
            pasted.push(note);
        }

        let mut flat = self.take_flat();
        let phrase = self
            .phrase_for(at_units)
            .unwrap_or_else(|| flat.last().map(|entry| entry.phrase).unwrap_or(0));
        let mut combined = flat
            .drain(..)
            .map(|entry| (entry, false))
            .collect::<Vec<_>>();
        combined.extend(
            pasted
                .into_iter()
                .map(|note| (FlatNote { phrase, note }, true)),
        );
        combined.sort_by(|(left, _), (right, _)| {
            left.note
                .start
                .cmp(&right.note.start)
                .then_with(|| left.note.duration.cmp(&right.note.duration))
        });
        let selected = combined
            .iter()
            .enumerate()
            .filter_map(|(index, (_, inserted))| inserted.then_some(index))
            .collect();
        self.restore_flat(combined.into_iter().map(|(entry, _)| entry).collect());
        selected
    }

    // -- lyric edits ------------------------------------------------------

    fn token_mut(&mut self, address: LyricAddress) -> Option<&mut LyricTextToken> {
        let note = self.resolve(address)?;
        let ordinal = self
            .phrase_tokens(address.segment)
            .into_iter()
            .take_while(|(word, _)| *word < address.word)
            .filter(|(_, candidate)| *candidate == note)
            .count();
        self.note_at_mut(note)?
            .lyrics
            .iter_mut()
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token),
                LyricToken::Continuation { .. } => None,
            })
            .nth(ordinal)
    }

    pub fn set_lyric_text(&mut self, address: LyricAddress, text: &str) -> bool {
        let Some(token) = self.token_mut(address) else {
            return false;
        };
        if token.text == text {
            return false;
        }
        token.text = text.to_owned();
        // A renamed syllable no longer matches its stored pronunciation.
        token.reading = None;
        token.phonemes = None;
        self.touch();
        true
    }

    /// Adds a lyric at the playhead. When the playhead lands on a note without
    /// text the token attaches there; otherwise a rhythm note carries it, which
    /// is how the format represents a lyric that has no pitch target.
    pub fn insert_lyric(
        &mut self,
        selection: Option<LyricAddress>,
        playhead: f64,
    ) -> Option<LyricAddress> {
        let join = self.default_join();
        let playhead_units = self.to_units(playhead.max(0.0));
        let mut start = playhead_units;
        if let Some(selection) = selection
            && let Some(note) = self.resolve(selection)
            && let Some((_, note)) = self.note_at(note)
        {
            let end = note.start.saturating_add(note.duration);
            if playhead_units >= note.start && playhead_units <= end {
                start = end;
            }
        }

        let empty = (0..self.flat_len()).find(|index| {
            self.note_at(*index).is_some_and(|(_, note)| {
                note.lyrics.is_empty()
                    && note.start <= start
                    && note.start.saturating_add(note.duration) > start
            })
        });
        let note_index = match empty {
            Some(index) => index,
            None => {
                let duration = self.to_units(0.35);
                self.insert_note(
                    self.to_seconds(start),
                    self.to_seconds(start + duration),
                    60.0,
                )?;
                let index = (0..self.flat_len()).find(|index| {
                    self.note_at(*index)
                        .is_some_and(|(_, note)| note.start == start)
                })?;
                let mut selection = BTreeSet::new();
                selection.insert(index);
                // A fresh lyric has no authored pitch yet.
                self.set_note_kind(&selection, NoteKind::Rap);
                if let Some(note) = self.note_at_mut(index) {
                    note.pitch = None;
                    note.vocal_mode = VocalMode::Spoken;
                }
                index
            }
        };

        let id = self.allocate_id("lyric");
        let note = self.note_at_mut(note_index)?;
        note.lyrics.push(LyricToken::Text(LyricTextToken {
            id,
            text: "New lyric".into(),
            join_before: join,
            reading: None,
            phonemes: None,
        }));
        self.touch();
        self.address_of_note(note_index)
    }

    fn address_of_note(&self, note: usize) -> Option<LyricAddress> {
        let phrase = self.note_at(note).map(|(phrase, _)| phrase)?;
        self.phrase_tokens(phrase)
            .into_iter()
            .rev()
            .find(|(_, candidate)| *candidate == note)
            .map(|(word, _)| LyricAddress {
                segment: phrase,
                word,
            })
    }

    /// Removes a lyric token. The owning note stays: deleting a syllable must
    /// not silently delete pitch the singer still has to hit.
    pub fn delete_lyric(&mut self, address: LyricAddress) -> (bool, Option<LyricAddress>) {
        let Some(note_index) = self.resolve(address) else {
            return (false, None);
        };
        let Some(token) = self.token_mut(address).map(|token| token.id.clone()) else {
            return (false, None);
        };
        if let Some(note) = self.note_at_mut(note_index) {
            note.lyrics.retain(|candidate| match candidate {
                LyricToken::Text(candidate) => candidate.id != token,
                LyricToken::Continuation { .. } => true,
            });
        }
        let mut orphans = HashSet::new();
        orphans.insert(token);
        self.drop_continuations(&orphans);
        self.touch();

        let remaining = self.phrase_tokens(address.segment).len();
        let next = if remaining > 0 {
            Some(LyricAddress {
                segment: address.segment,
                word: address.word.min(remaining - 1),
            })
        } else {
            self.previous_phrase_token(address.segment)
        };
        (true, next)
    }

    fn previous_phrase_token(&self, segment: usize) -> Option<LyricAddress> {
        for phrase in (0..segment).rev() {
            let tokens = self.phrase_tokens(phrase).len();
            if tokens > 0 {
                return Some(LyricAddress {
                    segment: phrase,
                    word: tokens - 1,
                });
            }
        }
        let tokens = self.phrase_tokens(segment);
        (!tokens.is_empty()).then_some(LyricAddress { segment, word: 0 })
    }

    pub fn delete_lyrics(&mut self, addresses: &BTreeSet<LyricAddress>) -> usize {
        let mut ordered = addresses.iter().copied().collect::<Vec<_>>();
        ordered.sort_by(|left, right| right.cmp(left));
        ordered
            .into_iter()
            .filter(|address| self.delete_lyric(*address).0)
            .count()
    }

    pub fn merge_lyrics(&mut self, addresses: &BTreeSet<LyricAddress>) -> Option<LyricAddress> {
        if addresses.len() < 2 {
            return None;
        }
        let segment = addresses.first()?.segment;
        if addresses.iter().any(|address| address.segment != segment) {
            return None;
        }
        let notes = addresses
            .iter()
            .filter_map(|address| self.resolve(*address))
            .collect::<BTreeSet<_>>();
        let compact = self.compact_language();
        let text = addresses
            .iter()
            .filter_map(|address| self.lyric_text(*address))
            .collect::<Vec<_>>()
            .join(if compact { "" } else { " " });
        let first = *addresses.first()?;
        if notes.len() > 1 {
            self.merge_notes(&notes, None)?;
        } else {
            // One note holds them all: collapse its tokens into the first.
            let note = *notes.first()?;
            let keep = self.token_mut(first).map(|token| token.id.clone())?;
            if let Some(note) = self.note_at_mut(note) {
                note.lyrics.retain(|token| match token {
                    LyricToken::Text(token) => token.id == keep,
                    LyricToken::Continuation { .. } => true,
                });
            }
        }
        let address = LyricAddress {
            segment: first.segment.min(self.phrase_count().saturating_sub(1)),
            word: first.word,
        };
        self.set_lyric_text(address, &text);
        Some(address)
    }

    pub fn lyric_text(&self, address: LyricAddress) -> Option<String> {
        let note = self.resolve(address)?;
        let ordinal = self
            .phrase_tokens(address.segment)
            .into_iter()
            .take_while(|(word, _)| *word < address.word)
            .filter(|(_, candidate)| *candidate == note)
            .count();
        self.note_at(note)?
            .1
            .lyrics
            .iter()
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token.text.clone()),
                LyricToken::Continuation { .. } => None,
            })
            .nth(ordinal)
    }

    /// Splits each selected syllable, splitting its note so both halves keep a
    /// singable target.
    pub fn split_lyrics(
        &mut self,
        addresses: &BTreeSet<LyricAddress>,
        playhead: f64,
    ) -> BTreeSet<LyricAddress> {
        let single = (addresses.len() == 1).then_some(playhead);
        let mut result = BTreeSet::new();
        // Work back to front so earlier addresses stay valid.
        for address in addresses.iter().rev().copied() {
            let Some(note_index) = self.resolve(address) else {
                continue;
            };
            let Some(text) = self.lyric_text(address) else {
                continue;
            };
            let characters = text.chars().collect::<Vec<_>>();
            let cut = (characters.len() / 2).clamp(1, characters.len().saturating_sub(1).max(1));
            let (left_text, right_text) = if characters.len() > 1 {
                (
                    characters[..cut].iter().collect::<String>(),
                    characters[cut..].iter().collect::<String>(),
                )
            } else {
                (text.clone(), String::new())
            };

            let mut selection = BTreeSet::new();
            selection.insert(note_index);
            let split = self.split_notes(&selection, single.unwrap_or(f64::NAN));
            let mut split = split.into_iter();
            let (Some(left), Some(right)) = (split.next(), split.next()) else {
                result.insert(address);
                continue;
            };
            // Splitting a note leaves the tail continuing the head's syllable;
            // the tail must own the second half of the text instead.
            let id = self.allocate_id("lyric");
            if let Some(note) = self.note_at_mut(right) {
                note.lyrics = vec![LyricToken::Text(LyricTextToken {
                    id,
                    text: right_text,
                    // Half of a split syllable never takes a leading space.
                    join_before: LyricJoin::None,
                    reading: None,
                    phonemes: None,
                })];
            }
            if let Some(left) = self.address_of_note(left) {
                self.set_lyric_text(left, &left_text);
                result.insert(left);
            }
            if let Some(right) = self.address_of_note(right) {
                result.insert(right);
            }
        }
        result
    }

    /// The phrase and in-phrase position of a flattened note index.
    fn locate_note(&self, index: usize) -> Option<(usize, usize)> {
        let track = self.active_track()?;
        let mut seen = 0usize;
        for (phrase, entry) in track.phrases.iter().enumerate() {
            if index < seen + entry.notes.len() {
                return Some((phrase, index - seen));
            }
            seen += entry.notes.len();
        }
        None
    }

    /// Splits each selected word into the syllables its language sings, giving
    /// every syllable its own note. The word's note is divided in proportion to
    /// how much of the word each syllable spells, which is closer to how it is
    /// sung than an even split.
    ///
    /// The last syllable keeps the original token's ID so a held note that
    /// continues the word still points at the syllable it is holding.
    pub fn syllabize_lyrics(&mut self, addresses: &BTreeSet<LyricAddress>) -> Vec<LyricAddress> {
        let language = self.chart.language.clone();
        let minimum = self.min_duration();
        let mut produced_ids = Vec::new();
        // Back to front, so the addresses ahead of the cursor stay valid.
        for address in addresses.iter().rev().copied() {
            let Some(note_index) = self.resolve(address) else {
                continue;
            };
            let Some((phrase_index, offset)) = self.locate_note(note_index) else {
                continue;
            };
            let Some(note) = self
                .active_track()
                .and_then(|track| track.phrases.get(phrase_index))
                .and_then(|phrase| phrase.notes.get(offset))
            else {
                continue;
            };
            let Some(token) = note.lyrics.iter().find_map(|token| match token {
                LyricToken::Text(token) => Some(token.clone()),
                LyricToken::Continuation { .. } => None,
            }) else {
                continue;
            };
            let pieces = super::syllabize::syllables(
                &token.text,
                token.reading.as_deref(),
                language.as_deref(),
            );
            // A word already one syllable long, or one whose note is too short
            // to divide, is left exactly as it is.
            if pieces.len() < 2 || note.duration < minimum.saturating_mul(pieces.len() as u64) {
                continue;
            }

            let start = note.start;
            let duration = note.duration;
            let pitch = note.pitch;
            let vocal_mode = note.vocal_mode;
            let bonus = note.bonus;
            let scoring = note.scoring.clone();
            let weights = pieces
                .iter()
                .map(|piece| piece.text.chars().count().max(1) as u64)
                .collect::<Vec<_>>();
            let total: u64 = weights.iter().sum();

            let mut replacements = Vec::with_capacity(pieces.len());
            let mut cursor = start;
            let last = pieces.len() - 1;
            for (index, piece) in pieces.iter().enumerate() {
                let end = if index == last {
                    start.saturating_add(duration)
                } else {
                    let offset: u64 = weights[..=index].iter().sum();
                    start.saturating_add(duration * offset / total)
                };
                let piece_duration = end.saturating_sub(cursor).max(minimum);
                // The last syllable inherits the token ID so continuations
                // pointing at the word keep resolving to its tail.
                let token_id = if index == last {
                    token.id.clone()
                } else {
                    self.allocate_id("lyric")
                };
                let note_id = if index == 0 {
                    // Reuse the note ID for the head so nothing else in the
                    // chart loses track of where the word starts.
                    None
                } else {
                    Some(self.allocate_id("note"))
                };
                replacements.push((
                    note_id,
                    cursor,
                    piece_duration,
                    LyricTextToken {
                        id: token_id,
                        text: piece.text.clone(),
                        // Only the first piece can carry the word's own join;
                        // the rest are inside the word.
                        join_before: if index == 0 {
                            token.join_before
                        } else {
                            LyricJoin::None
                        },
                        reading: piece.reading.clone(),
                        phonemes: None,
                    },
                ));
                cursor = cursor.saturating_add(piece_duration);
            }

            let Some(phrase) = self
                .chart
                .tracks
                .get_mut(self.track)
                .and_then(|track| track.phrases.get_mut(phrase_index))
            else {
                continue;
            };
            let original_id = phrase.notes[offset].id.clone();
            let mut built = Vec::with_capacity(replacements.len());
            for (note_id, note_start, note_duration, lyric) in replacements {
                let id = note_id.unwrap_or_else(|| original_id.clone());
                produced_ids.push(id.clone());
                built.push(VocalNote {
                    id,
                    start: note_start,
                    duration: note_duration,
                    pitch,
                    vocal_mode,
                    bonus,
                    scoring: scoring.clone(),
                    lyrics: vec![LyricToken::Text(lyric)],
                });
            }
            phrase.notes.splice(offset..=offset, built);
            self.touch();
        }

        // Addresses are only stable once every split has landed.
        let notes = self.notes();
        produced_ids
            .into_iter()
            .filter_map(|id| {
                let index = notes.iter().position(|note| note.id == id)?;
                self.address_of_note(index)
            })
            .collect()
    }

    pub fn shift_lyric(&mut self, address: LyricAddress, delta: f64) -> bool {
        let Some(note) = self.resolve(address) else {
            return false;
        };
        let mut selection = BTreeSet::new();
        selection.insert(note);
        self.shift_notes(&selection, delta, 0.0, false) > 0
    }

    pub fn set_lyric_timing(&mut self, address: LyricAddress, start: f64, end: f64) -> bool {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return false;
        }
        let Some(note) = self.resolve(address) else {
            return false;
        };
        self.resize_note(note, start, end)
    }

    pub fn adjust_lyric_boundary(
        &mut self,
        address: LyricAddress,
        start_delta: f64,
        end_delta: f64,
    ) -> bool {
        let Some(index) = self.resolve(address) else {
            return false;
        };
        let Some((_, note)) = self.note_at(index) else {
            return false;
        };
        let start = self.to_seconds(note.start);
        let end = self.to_seconds(note.start.saturating_add(note.duration));
        let next_start = (start + start_delta).clamp(0.0, end - MIN_NOTE_SECONDS);
        let next_end = (end + end_delta).max(next_start + MIN_NOTE_SECONDS);
        self.resize_note(index, next_start, next_end)
    }

    /// Joins a syllable with the one after it inside the same phrase.
    pub fn merge_lyric_with_next(&mut self, address: LyricAddress) -> bool {
        let tokens = self.phrase_tokens(address.segment);
        if address.word + 1 >= tokens.len() {
            return false;
        }
        let mut selection = BTreeSet::new();
        selection.insert(address);
        selection.insert(LyricAddress {
            segment: address.segment,
            word: address.word + 1,
        });
        self.merge_lyrics(&selection).is_some()
    }

    /// Breaks a phrase after the given syllable, which is how the editor
    /// authors a visual lyric line break.
    pub fn split_phrase(&mut self, address: LyricAddress) -> Option<LyricAddress> {
        let tokens = self.phrase_tokens(address.segment);
        if address.word + 1 >= tokens.len() {
            return None;
        }
        let boundary = tokens.get(address.word + 1).map(|(_, note)| *note)?;
        let track = self.chart.tracks.get(self.track)?;
        let base = track
            .phrases
            .iter()
            .take(address.segment)
            .map(|phrase| phrase.notes.len())
            .sum::<usize>();
        let offset = boundary.checked_sub(base)?;
        let id = self.allocate_id("phrase");
        let track = self.chart.tracks.get_mut(self.track)?;
        let phrase = track.phrases.get_mut(address.segment)?;
        if offset == 0 || offset >= phrase.notes.len() {
            return None;
        }
        let tail = phrase.notes.split_off(offset);
        track
            .phrases
            .insert(address.segment + 1, VocalPhrase { id, notes: tail });
        self.touch();
        Some(LyricAddress {
            segment: address.segment + 1,
            word: 0,
        })
    }

    pub fn merge_phrase_with_next(&mut self, address: LyricAddress) -> Option<LyricAddress> {
        let track = self.chart.tracks.get_mut(self.track)?;
        if address.segment + 1 >= track.phrases.len() {
            return None;
        }
        let words = self.phrase_tokens(address.segment).len();
        let track = self.chart.tracks.get_mut(self.track)?;
        let tail = track.phrases.remove(address.segment + 1);
        let phrase = track.phrases.get_mut(address.segment)?;
        phrase.notes.extend(tail.notes);
        phrase.notes.sort_by_key(|note| note.start);
        self.touch();
        Some(LyricAddress {
            segment: address.segment,
            word: words.saturating_sub(1),
        })
    }

    /// Conservative automatic repair: orders notes, enforces the minimum
    /// duration, and separates overlaps so the chart can pass validation.
    /// Returns the number of notes it had to touch.
    pub fn repair(&mut self) -> usize {
        let minimum = self.min_duration();
        let gap = self.to_units(0.01).max(1);
        let mut repaired = 0usize;
        for track in &mut self.chart.tracks {
            for phrase in &mut track.phrases {
                phrase.notes.sort_by(|left, right| {
                    left.start
                        .cmp(&right.start)
                        .then_with(|| left.duration.cmp(&right.duration))
                });
                for note in &mut phrase.notes {
                    if note.duration < minimum {
                        note.duration = minimum;
                        repaired += 1;
                    }
                }
            }
            track.phrases.retain(|phrase| !phrase.notes.is_empty());
            track
                .phrases
                .sort_by_key(|phrase| phrase.notes.first().map(|note| note.start).unwrap_or(0));

            // Walk the track in time order so an overlap is resolved once, even
            // when the two notes sit in different phrases.
            let mut previous_end = None::<u64>;
            for phrase in &mut track.phrases {
                for note in &mut phrase.notes {
                    if let Some(end) = previous_end
                        && note.start < end
                    {
                        note.start = end.saturating_add(gap);
                        repaired += 1;
                    }
                    previous_end = Some(note.start.saturating_add(note.duration));
                }
            }
        }
        if repaired > 0 {
            self.touch();
        }
        repaired
    }

    /// Reports what is wrong with the chart, located so the editor can jump to
    /// it. See [`super::problems`] for why editing tolerates these instead of
    /// refusing the edit.
    pub fn problems(&self) -> super::ProblemReport {
        super::problems::report(self)
    }

    /// Continuation tokens with no text token to continue, as (id, time). The
    /// format requires every reference to resolve inside its track.
    pub(crate) fn unresolved_continuations(&self, index: usize) -> Vec<(String, f64)> {
        let Some(track) = self.chart.tracks.get(index) else {
            return Vec::new();
        };
        let mut texts = HashSet::new();
        for phrase in &track.phrases {
            for note in &phrase.notes {
                for token in &note.lyrics {
                    if let LyricToken::Text(token) = token {
                        texts.insert(token.id.as_str());
                    }
                }
            }
        }
        let mut unresolved = Vec::new();
        for phrase in &track.phrases {
            for note in &phrase.notes {
                for token in &note.lyrics {
                    if let LyricToken::Continuation { continuation_of } = token
                        && !texts.contains(continuation_of.as_str())
                    {
                        unresolved.push((continuation_of.clone(), self.to_seconds(note.start)));
                    }
                }
            }
        }
        unresolved
    }

    /// Every lyric address in the active track, in reading order.
    pub fn lyric_addresses(&self) -> BTreeSet<LyricAddress> {
        (0..self.phrase_count())
            .flat_map(|segment| {
                (0..self.phrase_tokens(segment).len())
                    .map(move |word| LyricAddress { segment, word })
            })
            .collect()
    }

    /// Text and time span of one lyric, for the inline lyric editor.
    pub fn lyric(&self, address: LyricAddress) -> Option<(String, f64, f64)> {
        self.lyrics()
            .into_iter()
            .find(|lyric| lyric.address == address)
            .map(|lyric| (lyric.text, lyric.start, lyric.end))
    }

    /// Shifts every note in the chart, used by the global gap correction.
    pub fn shift_all(&mut self, seconds: f64) -> bool {
        if !seconds.is_finite() || seconds == 0.0 {
            return false;
        }
        let timebase = self.timebase() as f64;
        let earliest = self
            .chart
            .tracks
            .iter()
            .flat_map(|track| track.phrases.iter())
            .flat_map(|phrase| phrase.notes.iter())
            .map(|note| note.start)
            .min()
            .unwrap_or(0);
        let delta = ((seconds * timebase).round() as i64).max(-(earliest as i64));
        if delta == 0 {
            return false;
        }
        for track in &mut self.chart.tracks {
            for phrase in &mut track.phrases {
                for note in &mut phrase.notes {
                    note.start = note.start.saturating_add_signed(delta);
                }
            }
        }
        self.touch();
        true
    }
}

fn orphaned_continuations(flat: &[FlatNote]) -> HashSet<String> {
    let mut texts = HashSet::new();
    let mut references = HashSet::new();
    for entry in flat {
        for token in &entry.note.lyrics {
            match token {
                LyricToken::Text(token) => {
                    texts.insert(token.id.clone());
                }
                LyricToken::Continuation { continuation_of } => {
                    references.insert(continuation_of.clone());
                }
            }
        }
    }
    references.difference(&texts).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(notes: &[(f64, f64, u8, &str)]) -> EditorDocument {
        let mut phrase = VocalPhrase {
            id: "phrase-1".into(),
            notes: Vec::new(),
        };
        for (index, (start, end, midi, text)) in notes.iter().enumerate() {
            phrase.notes.push(VocalNote {
                id: format!("note-{}", index + 1),
                start: (*start * DEFAULT_TIMEBASE as f64) as u64,
                duration: ((end - start) * DEFAULT_TIMEBASE as f64) as u64,
                pitch: Some(NotePitch {
                    midi: *midi,
                    cents: 0,
                }),
                vocal_mode: VocalMode::Pitched,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Pitch,
                    weight: 1.0,
                },
                lyrics: vec![LyricToken::Text(LyricTextToken {
                    id: format!("lyric-{}", index + 1),
                    text: (*text).into(),
                    join_before: LyricJoin::Space,
                    reading: None,
                    phonemes: None,
                })],
            });
        }
        let mut chart = VocalChartV1::new(vec![VocalTrack {
            id: "lead".into(),
            role: VocalTrackRole::Lead,
            singer: None,
            scoring_enabled: true,
            phrases: vec![phrase],
        }]);
        chart.language = Some("en".into());
        EditorDocument::new(chart)
    }

    fn selection(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    #[test]
    fn move_keeps_the_minimum_duration() {
        let mut document = document(&[(1.0, 2.0, 60, "a")]);
        assert!(document.move_note(0, 3.0, 3.0, 64.0));
        let note = &document.notes()[0];
        assert_eq!(note.start, 3.0);
        assert!((note.end - (3.0 + MIN_NOTE_SECONDS)).abs() < 1e-9);
        assert_eq!(note.midi, 64.0);
    }

    #[test]
    fn insert_places_the_note_in_time_order() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]);
        let index = document.insert_note(1.2, 1.6, 65.0).unwrap();
        assert_eq!(index, 1);
        let starts = document
            .notes()
            .iter()
            .map(|note| note.start)
            .collect::<Vec<_>>();
        assert_eq!(starts, [0.0, 1.2, 2.0]);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn split_gives_the_tail_a_continuation_and_a_unique_id() {
        let mut document = document(&[(0.0, 1.0, 60, "hold")]);
        let selected = document.split_notes(&selection(&[0]), 0.4);
        assert_eq!(selected, selection(&[0, 1]));
        let notes = document.notes();
        assert_eq!(notes.len(), 2);
        assert_ne!(notes[0].id, notes[1].id);
        assert!((notes[0].end - 0.4).abs() < 1e-9);
        // The lyric lane still shows one syllable, held across both notes.
        let lyrics = document.lyrics();
        assert_eq!(lyrics.len(), 1);
        assert_eq!(lyrics[0].text, "hold");
        assert!((lyrics[0].end - 1.0).abs() < 1e-9);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn split_refuses_a_note_that_is_too_short() {
        let mut document = document(&[(0.0, 0.05, 60, "a")]);
        document.split_notes(&selection(&[0]), 0.02);
        assert_eq!(document.note_count(), 1);
    }

    #[test]
    fn merge_spans_the_selection_and_keeps_every_syllable() {
        let mut document = document(&[(0.0, 1.0, 60, "one"), (1.0, 2.0, 62, "two")]);
        let index = document.merge_notes(&selection(&[0, 1]), None).unwrap();
        assert_eq!(index, 0);
        let notes = document.notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, 0.0);
        assert_eq!(notes[0].end, 2.0);
        let lyrics = document.lyrics();
        assert_eq!(
            lyrics
                .iter()
                .map(|lyric| lyric.text.as_str())
                .collect::<Vec<_>>(),
            ["one", "two"]
        );
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn quantize_snaps_both_edges_and_holds_the_minimum() {
        let mut document = document(&[(0.31, 0.44, 60, "a")]);
        assert_eq!(document.quantize_notes(None, 0.25), 1);
        let note = &document.notes()[0];
        assert!((note.start - 0.25).abs() < 1e-9);
        assert!((note.end - 0.5).abs() < 1e-9);
    }

    #[test]
    fn shift_never_moves_a_selection_before_zero() {
        let mut document = document(&[(0.2, 1.0, 60, "a"), (1.5, 2.0, 62, "b")]);
        document.shift_notes(&selection(&[0, 1]), -5.0, 0.0, false);
        let notes = document.notes();
        assert_eq!(notes[0].start, 0.0);
        assert!((notes[1].start - 1.3).abs() < 1e-9);
    }

    #[test]
    fn shift_transposes_within_the_midi_range() {
        let mut document = document(&[(0.0, 1.0, 126, "a")]);
        document.shift_notes(&selection(&[0]), 0.0, 4.0, false);
        assert_eq!(document.notes()[0].midi, 127.0);
    }

    #[test]
    fn cycling_a_kind_converges_a_mixed_selection() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        document.set_note_kind(&selection(&[1]), NoteKind::Rap);
        document.cycle_note_kinds(&selection(&[0, 1]));
        let kinds = document
            .notes()
            .iter()
            .map(|note| note.kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds, [NoteKind::Golden, NoteKind::Golden]);
    }

    #[test]
    fn a_rhythm_kind_keeps_the_chart_valid_without_a_pitch_target() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.set_note_kind(&selection(&[0]), NoteKind::Freestyle);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn copy_and_paste_reproduce_the_selection_at_the_playhead() {
        let mut document = document(&[(0.0, 0.5, 60, "a"), (1.0, 1.5, 64, "b")]);
        let clipboard = document.copy_notes(&selection(&[0, 1]));
        let pasted = document.paste_notes(&clipboard, 4.0);
        assert_eq!(pasted.len(), 2);
        let notes = document.notes();
        assert_eq!(notes.len(), 4);
        assert!((notes[2].start - 4.0).abs() < 1e-9);
        assert!((notes[3].start - 5.0).abs() < 1e-9);
        assert_eq!(notes[3].midi, 64.0);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn deleting_a_note_drops_the_continuation_that_referenced_it() {
        let mut document = document(&[(0.0, 1.0, 60, "hold")]);
        document.split_notes(&selection(&[0]), 0.5);
        // Remove the head; the tail's continuation can no longer resolve.
        document.remove_notes(&selection(&[0]));
        assert_eq!(document.note_count(), 1);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn editing_a_syllable_clears_its_stale_pronunciation() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let address = LyricAddress {
            segment: 0,
            word: 0,
        };
        if let Some(token) = document.token_mut(address) {
            token.reading = Some("えー".into());
        }
        assert!(document.set_lyric_text(address, "b"));
        assert!(document.token_mut(address).unwrap().reading.is_none());
    }

    #[test]
    fn deleting_a_syllable_keeps_its_note() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        let (deleted, next) = document.delete_lyric(LyricAddress {
            segment: 0,
            word: 0,
        });
        assert!(deleted);
        assert_eq!(document.note_count(), 2);
        assert_eq!(document.lyrics().len(), 1);
        assert_eq!(next.unwrap().word, 0);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn splitting_a_syllable_splits_its_note_and_its_text() {
        let mut document = document(&[(0.0, 1.0, 60, "hello")]);
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        let result = document.split_lyrics(&addresses, 0.5);
        assert_eq!(result.len(), 2);
        let lyrics = document.lyrics();
        assert_eq!(
            lyrics
                .iter()
                .map(|lyric| lyric.text.as_str())
                .collect::<Vec<_>>(),
            ["he", "llo"]
        );
        assert_eq!(document.note_count(), 2);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn merging_syllables_joins_text_and_notes() {
        let mut document = document(&[(0.0, 1.0, 60, "he"), (1.0, 2.0, 62, "llo")]);
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        addresses.insert(LyricAddress {
            segment: 0,
            word: 1,
        });
        let merged = document.merge_lyrics(&addresses).unwrap();
        assert_eq!(merged.word, 0);
        assert_eq!(document.note_count(), 1);
        assert_eq!(document.lyrics()[0].text, "he llo");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn a_compact_language_merges_without_a_space() {
        let mut document = document(&[(0.0, 1.0, 60, "歌"), (1.0, 2.0, 62, "詞")]);
        document.set_language(Some("ja".into()));
        let mut addresses = BTreeSet::new();
        addresses.insert(LyricAddress {
            segment: 0,
            word: 0,
        });
        addresses.insert(LyricAddress {
            segment: 0,
            word: 1,
        });
        document.merge_lyrics(&addresses).unwrap();
        assert_eq!(document.lyrics()[0].text, "歌詞");
    }

    #[test]
    fn splitting_a_phrase_creates_a_second_lyric_line() {
        let mut document = document(&[
            (0.0, 1.0, 60, "a"),
            (1.0, 2.0, 62, "b"),
            (2.0, 3.0, 64, "c"),
        ]);
        let next = document
            .split_phrase(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(next.segment, 1);
        assert_eq!(document.phrase_count(), 2);
        assert_eq!(document.phrase_text(0), "a");
        assert_eq!(document.phrase_text(1), "b c");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn merging_phrases_restores_one_line() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        document
            .split_phrase(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(document.phrase_count(), 2);
        let address = document
            .merge_phrase_with_next(LyricAddress {
                segment: 0,
                word: 0,
            })
            .unwrap();
        assert_eq!(address.segment, 0);
        assert_eq!(document.phrase_count(), 1);
        assert_eq!(document.phrase_text(0), "a b");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn inserting_a_lyric_creates_an_unpitched_note() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let address = document.insert_lyric(None, 5.0).unwrap();
        assert_eq!(document.lyric_text(address).as_deref(), Some("New lyric"));
        let lyric = document
            .lyrics()
            .into_iter()
            .find(|lyric| lyric.address == address)
            .unwrap();
        assert!(!lyric.guided, "a fresh lyric has no pitch guidance yet");
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn revision_advances_only_on_a_real_change() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        let before = document.revision();
        assert!(!document.set_lyric_text(
            LyricAddress {
                segment: 0,
                word: 0
            },
            "a"
        ));
        assert_eq!(document.revision(), before);
        assert!(document.set_lyric_text(
            LyricAddress {
                segment: 0,
                word: 0
            },
            "b"
        ));
        assert!(document.revision() > before);
    }

    #[test]
    fn save_normalizes_note_and_phrase_order() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]);
        // Drag the second note before the first; the stored order stays put so
        // the pointer keeps its grip, but saving reorders.
        document.move_note(1, 0.0, 0.5, 62.0);
        let chart = document.to_chart();
        let starts = chart.tracks[0].phrases[0]
            .notes
            .iter()
            .map(|note| note.start)
            .collect::<Vec<_>>();
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn repair_separates_overlaps_and_makes_the_chart_valid() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (0.5, 1.5, 62, "b")]);
        assert!(document.to_chart().validate().is_err());
        assert!(document.repair() > 0);
        let notes = document.notes();
        assert!(notes[1].start >= notes[0].end);
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn repair_lifts_a_note_to_the_minimum_duration() {
        let mut document = document(&[(0.0, 0.001, 60, "a")]);
        assert_eq!(document.repair(), 1);
        let note = &document.notes()[0];
        assert!(note.end - note.start >= MIN_NOTE_SECONDS - 1e-9);
    }

    #[test]
    fn repair_leaves_a_clean_chart_untouched() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]);
        assert_eq!(document.repair(), 0);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn shift_all_moves_every_track() {
        let mut document = document(&[(1.0, 2.0, 60, "a")]);
        assert!(document.shift_all(0.5));
        assert!((document.notes()[0].start - 1.5).abs() < 1e-9);
        assert!(document.shift_all(-10.0));
        assert_eq!(document.notes()[0].start, 0.0);
    }

    #[test]
    fn syllabizing_gives_every_syllable_its_own_note() {
        let mut document = document(&[(0.0, 2.0, 60, "wonder"), (2.0, 3.0, 62, "love")]);
        let produced = document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        let notes = document.notes();
        assert_eq!(notes.len(), 3);
        assert_eq!(
            notes
                .iter()
                .filter_map(|note| note.lyric.clone())
                .collect::<Vec<_>>(),
            ["won", "der", "love"]
        );
        assert_eq!(produced.len(), 2);
        // The word keeps its span: the pieces divide it, they do not extend it.
        assert!((notes[0].start - 0.0).abs() < 1e-9);
        assert!((notes[1].end - 2.0).abs() < 1e-9);
        assert!(notes[0].end <= notes[1].start + 1e-9);
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn a_one_syllable_word_is_left_exactly_as_it_was() {
        let mut document = document(&[(0.0, 1.0, 60, "love")]);
        assert!(
            document
                .syllabize_lyrics(&BTreeSet::from([LyricAddress {
                    segment: 0,
                    word: 0
                }]))
                .is_empty()
        );
        assert_eq!(document.note_count(), 1);
        assert_eq!(document.revision(), 0);
    }

    #[test]
    fn a_note_too_short_to_divide_is_not_syllabized() {
        let mut document = document(&[(0.0, 0.04, 60, "wonder")]);
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        assert_eq!(document.note_count(), 1);
    }

    #[test]
    fn a_held_word_still_resolves_after_it_is_syllabized() {
        let mut document = document(&[(0.0, 2.0, 60, "wonder"), (2.0, 3.0, 60, "x")]);
        let held = match &document.chart().tracks[0].phrases[0].notes[0].lyrics[0] {
            LyricToken::Text(token) => token.id.clone(),
            LyricToken::Continuation { .. } => panic!("text token"),
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held,
        }];
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        // The hold now follows the last syllable of the word, not the first.
        assert!(document.unresolved_continuations(0).is_empty());
        let notes = document.notes();
        assert!(notes.last().unwrap().continues_lyric);
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn japanese_words_syllabize_by_mora_and_keep_their_reading() {
        let mut document = document(&[(0.0, 3.0, 60, "きょうは")]);
        document.set_language(Some("ja".into()));
        document.syllabize_lyrics(&BTreeSet::from([LyricAddress {
            segment: 0,
            word: 0,
        }]));
        let notes = document.notes();
        assert_eq!(
            notes
                .iter()
                .filter_map(|note| note.lyric.clone())
                .collect::<Vec<_>>(),
            ["きょ", "う", "は"]
        );
        let readings = document.chart().tracks[0].phrases[0]
            .notes
            .iter()
            .filter_map(|note| match note.lyrics.first() {
                Some(LyricToken::Text(token)) => token.reading.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(readings, ["きょ", "う", "は"]);
    }

    #[test]
    fn a_new_track_becomes_active_and_starts_empty() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        assert_eq!(document.track_count(), 1);
        let index = document.add_track(TrackRole::Duet);
        assert_eq!(index, 1);
        assert_eq!(document.active_track_index(), 1);
        assert!(document.notes().is_empty());
        let tracks = document.tracks();
        assert_eq!(tracks[1].role, TrackRole::Duet);
        assert!(tracks[1].scoring_enabled);
        // The lead track keeps its material and its coverage.
        assert_eq!(tracks[0].note_count, 1);
        assert!((tracks[0].sung_seconds - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_last_track_cannot_be_removed() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        assert!(!document.remove_track(0));
        document.add_track(TrackRole::Duet);
        assert!(document.remove_track(1));
        assert_eq!(document.track_count(), 1);
        assert_eq!(document.active_track_index(), 0);
    }

    #[test]
    fn moving_a_selection_to_another_track_legalizes_an_overlap() {
        let mut document = document(&[(0.0, 1.0, 60, "a"), (0.5, 1.5, 62, "b")]);
        assert!(document.problems().blocks_saving());
        document.add_track(TrackRole::Duet);
        document.set_active_track(0);
        assert_eq!(document.move_notes_to_track(&selection(&[1]), 1), 1);
        assert_eq!(document.notes().len(), 1);
        assert_eq!(document.track_notes(1).len(), 1);
        // Both tracks are now single voiced, so the chart saves.
        assert!(!document.problems().blocks_saving());
        document.to_chart().validate().expect("valid chart");
    }

    #[test]
    fn a_held_syllable_follows_its_notes_to_the_new_track() {
        let mut document = document(&[(0.0, 1.0, 60, "held"), (1.0, 2.0, 60, "next")]);
        // Turn the second note into a continuation of the first syllable.
        let held = document.chart().tracks[0].phrases[0].notes[0].lyrics[0].clone();
        let LyricToken::Text(held) = held else {
            panic!("text token");
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held.id.clone(),
        }];
        document.add_track(TrackRole::Harmony);
        document.set_active_track(0);
        assert_eq!(document.move_notes_to_track(&selection(&[0]), 1), 2);
        assert!(document.notes().is_empty());
        assert_eq!(document.track_notes(1).len(), 2);
        document.set_active_track(1);
        assert!(document.unresolved_continuations(1).is_empty());
    }

    #[test]
    fn an_orphaned_continuation_is_dropped_rather_than_moved_alone() {
        let mut document = document(&[(0.0, 1.0, 60, "held"), (1.0, 2.0, 60, "next")]);
        let held = document.chart().tracks[0].phrases[0].notes[0].lyrics[0].clone();
        let LyricToken::Text(held) = held else {
            panic!("text token");
        };
        document.chart.tracks[0].phrases[0].notes[1].lyrics = vec![LyricToken::Continuation {
            continuation_of: held.id,
        }];
        document.add_track(TrackRole::Harmony);
        document.set_active_track(0);
        // Moving only the holding note would leave a reference across tracks.
        assert_eq!(document.move_notes_to_track(&selection(&[1]), 1), 1);
        assert!(document.track_notes(1)[0].lyric.is_none());
        assert!(document.unresolved_continuations(1).is_empty());
        assert!(document.unresolved_continuations(0).is_empty());
    }

    #[test]
    fn problems_cover_tracks_the_user_is_not_editing() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.add_track(TrackRole::Duet);
        document.insert_note(0.0, 1.0, 62.0);
        document.insert_note(0.5, 1.5, 64.0);
        document.set_active_track(0);
        let report = document.problems();
        assert!(report.blocks_saving());
        assert!(report.problems.iter().any(|problem| problem.track == 1
            && problem.kind == super::super::ProblemKind::OverlappingNotes));
    }

    #[test]
    fn an_empty_track_is_not_persisted() {
        let mut document = document(&[(0.0, 1.0, 60, "a")]);
        document.add_track(TrackRole::Backing);
        let chart = document.to_chart();
        assert_eq!(chart.tracks.len(), 1);
        chart.validate().expect("valid chart");
    }
}
