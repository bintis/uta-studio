use super::*;
use std::collections::{BTreeSet, HashSet};

use crate::editor::{round_units_to_millis, seconds_to_units, units_to_seconds};
use utz::{
    DEFAULT_TIMEBASE, LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring,
    ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

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

    pub(crate) fn timebase(&self) -> u64 {
        if self.chart.timebase == 0 {
            DEFAULT_TIMEBASE
        } else {
            self.chart.timebase
        }
    }

    pub(crate) fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn to_units(&self, seconds: f64) -> u64 {
        round_units_to_millis(seconds_to_units(seconds, self.timebase()), self.timebase())
    }

    pub(crate) fn to_seconds(&self, units: u64) -> f64 {
        units_to_seconds(units, self.timebase())
    }

    pub(crate) fn min_duration(&self) -> u64 {
        self.to_units(MIN_NOTE_SECONDS).max(1)
    }

    /// Languages written without inter-word spaces join lyric tokens directly.
    pub(crate) fn compact_language(&self) -> bool {
        self.chart.language.as_deref().is_some_and(|language| {
            ["zh", "ja", "ko"]
                .iter()
                .any(|prefix| language.to_ascii_lowercase().starts_with(prefix))
        })
    }

    pub(crate) fn default_join(&self) -> LyricJoin {
        if self.compact_language() {
            LyricJoin::None
        } else {
            LyricJoin::Space
        }
    }

    pub(crate) fn allocate_id(&mut self, prefix: &str) -> String {
        loop {
            let candidate = format!("{prefix}-{}", self.next_id);
            self.next_id += 1;
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    pub(crate) fn active_track(&self) -> Option<&VocalTrack> {
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
                    part: track.part,
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
            part: None,
            singer: None,
            scoring_enabled: role.is_sung(),
            // A track with no notes has no phrases either; the first note
            // added to it creates one. Such a track is dropped on save,
            // because the format requires every track to sing something.
            phrases: Vec::new(),
        });
        self.track = self.chart.tracks.len() - 1;
        self.recompute_track_parts();
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
        self.recompute_track_parts();
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
        self.recompute_track_parts();
        self.touch();
        true
    }

    /// Assigns contiguous UltraStar-style duet part numbers (1, 2, ...) to
    /// every `Lead` track, in track order, whenever there is more than one —
    /// a second lead track is what makes a chart a duet. A solo lead track,
    /// and every non-lead track, carries no part.
    pub(crate) fn recompute_track_parts(&mut self) {
        let lead_indexes: Vec<usize> = self
            .chart
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| track.role == VocalTrackRole::Lead)
            .map(|(index, _)| index)
            .collect();
        let assign_parts = lead_indexes.len() > 1;
        for (position, index) in lead_indexes.into_iter().enumerate() {
            self.chart.tracks[index].part = assign_parts.then(|| position as u32 + 1);
        }
        for track in &mut self.chart.tracks {
            if track.role != VocalTrackRole::Lead {
                track.part = None;
            }
        }
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

    pub(crate) fn prune_orphaned_continuations(&mut self) {
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

    pub(crate) fn flat_len(&self) -> usize {
        self.active_track()
            .map(|track| track.phrases.iter().map(|phrase| phrase.notes.len()).sum())
            .unwrap_or(0)
    }

    pub(crate) fn take_flat(&mut self) -> Vec<FlatNote> {
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
    pub(crate) fn restore_flat(&mut self, flat: Vec<FlatNote>) {
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

    pub(crate) fn note_at(&self, index: usize) -> Option<(usize, &VocalNote)> {
        self.active_track()?
            .phrases
            .iter()
            .enumerate()
            .flat_map(|(phrase, entry)| entry.notes.iter().map(move |note| (phrase, note)))
            .nth(index)
    }

    pub(crate) fn note_at_mut(&mut self, index: usize) -> Option<&mut VocalNote> {
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

    pub(crate) fn view_note(&self, index: usize, phrase: usize, note: &VocalNote) -> ChartNote {
        ChartNote {
            index,
            id: note.id.clone(),
            phrase,
            start: self.to_seconds(note.start),
            end: self.to_seconds(note.start.saturating_add(note.duration)),
            midi: note.pitch.map(|pitch| pitch.midi as f64).unwrap_or(60.0),
            kind: NoteKind::of(note),
            pitched: note.pitch.is_some(),
            placeholder: note.vocal_mode == VocalMode::Spoken,
            scores: note.scoring.mode != ScoringMode::None,
            scores_pitch: note.scoring.mode == ScoringMode::Pitch,
            golden: note.bonus == NoteBonus::Golden,
            continues_lyric: note
                .lyrics
                .iter()
                .any(|token| matches!(token, LyricToken::Continuation { .. })),
            // A note usually carries one syllable, but nothing stops two
            // short ones sharing a note (this happens in practice — a
            // syllable pair with no room for its own pitch target). Joining
            // every text token here, instead of showing only the first,
            // keeps the note's label from silently dropping a syllable.
            lyric: {
                let mut joined = String::new();
                for token in &note.lyrics {
                    if let LyricToken::Text(token) = token {
                        if token.join_before == LyricJoin::Space && !joined.is_empty() {
                            joined.push(' ');
                        }
                        joined.push_str(&token.text);
                    }
                }
                (!joined.is_empty()).then_some(joined)
            },
        }
    }

    pub fn notes(&self) -> Vec<ChartNote> {
        self.track_notes(self.track)
    }

    // -- lyric views ------------------------------------------------------

    /// Text tokens of a phrase, in order, as (token ordinal, note ordinal).
    pub(crate) fn phrase_tokens(&self, phrase: usize) -> Vec<(usize, usize)> {
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

    pub(crate) fn resolve(&self, address: LyricAddress) -> Option<usize> {
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
                    let mut continuation_notes = Vec::new();
                    for (held_offset, held) in entry.notes.iter().enumerate().skip(offset + 1) {
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
                        continuation_notes.push(note_index + held_offset);
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
                        continuation_notes,
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
        let target_midi = midi.round().clamp(0.0, 127.0) as u8;
        match note.pitch.as_mut() {
            Some(pitch) => pitch.midi = target_midi,
            // A note with no pitch (a lyric split off `unbind_note`, or the
            // "analyzer found no matching pitch note" placeholder
            // `migrate_analyzer_chart` leaves behind) renders at a fixed
            // default height, so a vertical drag used to silently do
            // nothing. Dragging it is a deliberate "put this note here"
            // gesture, so it now gives the note the pitch it's dragged to
            // instead of ignoring that axis.
            None => {
                note.pitch = Some(NotePitch {
                    midi: target_midi,
                    cents: 0,
                });
                // Both of those placeholders are authored as `Spoken`,
                // scored as rhythm-only — leaving that in place would mean
                // the note now has a pitch but still isn't actually judged
                // on it. A deliberately non-pitched Rap/Freestyle note is
                // left alone; only the "nobody's decided this yet"
                // placeholder gets promoted.
                if note.vocal_mode == VocalMode::Spoken {
                    note.vocal_mode = VocalMode::Pitched;
                    note.scoring.mode = ScoringMode::Pitch;
                }
            }
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
                part: None,
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
    pub(crate) fn phrase_for(&self, units: u64) -> Option<usize> {
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
    pub(crate) fn drop_continuations(&mut self, orphans: &HashSet<String>) {
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
            // More than one lyric relationship can share a note (two short
            // syllables with no room for their own notes, or a note that
            // both continues an earlier syllable and starts a new one) —
            // only the first belongs before the split point.
            left.lyrics.truncate(1);
            selected.insert(output.len());
            output.push(FlatNote { phrase, note: left });

            let mut right = note;
            right.start = split;
            right.duration = end - split;
            right.lyrics = if right.lyrics.len() > 1 {
                // Everything past the first token already has its own
                // identity (its own syllable, or a continuation of a
                // different earlier one) — keep it as-is on the right half
                // instead of collapsing it into a continuation of the first
                // and silently losing it.
                right.lyrics.split_off(1)
            } else {
                // A single relationship (one syllable, or continuing an
                // earlier one) — splitting mid-syllable means both halves
                // keep singing it, so the tail continues it.
                let head = right.lyrics.first().map(|token| match token {
                    LyricToken::Text(token) => token.id.clone(),
                    LyricToken::Continuation { continuation_of } => continuation_of.clone(),
                });
                head.map(|continuation_of| vec![LyricToken::Continuation { continuation_of }])
                    .unwrap_or_default()
            };
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

    pub(crate) fn reassign_duplicate_note_ids(&mut self) {
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
        if grid.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
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
}
