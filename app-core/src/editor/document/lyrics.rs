use super::{
    EditorDocument, FlatNote, LyricAddress, MIN_NOTE_SECONDS, NoteKind, parse_phrase_tokens,
};
use std::collections::{BTreeSet, HashSet};

use crate::editor::seconds_to_units;
use crate::editor::syllabize::{is_han, is_hangul, is_kana};
use utz::{
    LyricJoin, LyricTextToken, LyricToken, NoteBonus, NoteScoring, ScoringMode, VocalMode,
    VocalNote, VocalPhrase,
};

impl EditorDocument {
    pub(crate) fn token_mut(&mut self, address: LyricAddress) -> Option<&mut LyricTextToken> {
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

    /// Overrides the aligner-recognized reading for a lyric token. Stores
    /// only — never re-syllabizes. Automatically re-splitting mora on every
    /// reading edit would silently move note boundaries right after the user
    /// fixed a typo, which is exactly what "never silently overwrite
    /// authored content" forbids; re-syllabizing stays an explicit, separate
    /// user action (`syllabize_lyrics`), same as retyping the text itself.
    pub fn set_lyric_reading(&mut self, address: LyricAddress, reading: Option<String>) -> bool {
        let Some(token) = self.token_mut(address) else {
            return false;
        };
        let reading = reading.filter(|value| !value.trim().is_empty());
        if token.reading == reading {
            return false;
        }
        token.reading = reading;
        self.touch();
        true
    }

    /// Whether the lyric at `address` contains a script `syllables()` treats
    /// as CJK (Han/kana/Hangul) — i.e. the scripts whose stored `reading`
    /// actually affects mora splitting. Judged per word, not per chart
    /// language: a chart tagged as a Latin-script language can still carry
    /// individual CJK loanwords, which is exactly where this field matters
    /// most (`syllables()`'s own mixed-script handling proves the case).
    pub fn lyric_uses_cjk_script(&self, address: LyricAddress) -> bool {
        self.lyric_text(address).is_some_and(|text| {
            text.chars()
                .any(|c| is_han(c) || is_kana(c) || is_hangul(c))
        })
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

    /// Gives `note_index` its own lyric text directly, right where the pitch
    /// already is — no separate placeholder note and no follow-up "Bind"
    /// step, unlike [`Self::insert_lyric`]. Only works on a note that
    /// doesn't already carry a lyric of its own; a note with existing text
    /// or a held continuation should be edited in place instead.
    pub fn add_lyric_to_note(&mut self, note_index: usize) -> Option<LyricAddress> {
        let (_, note) = self.note_at(note_index)?;
        if !note.lyrics.is_empty() {
            return None;
        }
        let join = self.default_join();
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

    /// Advances inline lyric editing to the next (`forward`) or previous
    /// eligible slot in the same phrase; if the target note has no lyric yet,
    /// creates an empty one in place (reusing `add_lyric_to_note`'s
    /// mechanism). Returns `None` at the start/end of the phrase — Tab never
    /// crosses a phrase boundary, so moving to a new line stays a visible,
    /// deliberate step rather than a silent jump.
    pub fn advance_lyric_edit(
        &mut self,
        from: LyricAddress,
        forward: bool,
    ) -> Option<LyricAddress> {
        let note_index = self.resolve(from)?;
        let (phrase, offset) = self.locate_note(note_index)?;
        let slots = self.lyric_slots(phrase);
        let position = slots.iter().position(|candidate| *candidate == offset)?;
        let next_offset = if forward {
            slots.get(position + 1).copied()?
        } else {
            slots.get(position.checked_sub(1)?).copied()?
        };
        let range = self.phrase_flat_range(phrase)?;
        let target_note = range.start + next_offset;
        self.address_of_note(target_note)
            .or_else(|| self.add_lyric_to_note(target_note))
    }

    /// Moves the lyric at `word` onto `note_index`, the format's way of
    /// attaching text to a pitch target that was authored separately. `word`'s
    /// own note must carry no pitch (a placeholder `insert_lyric` created for
    /// text with nowhere pitched to land) and `note_index` must carry no lyric
    /// yet; the emptied placeholder is then dropped. Works from either
    /// selection direction — callers don't need to know which side is which.
    pub fn bind_lyric_to_note(&mut self, word: LyricAddress, note_index: usize) -> Option<usize> {
        self.bind_lyric_to_note_aligned(word, note_index, false)
    }

    /// Same as [`Self::bind_lyric_to_note`], but when `align_to_lyric` is
    /// true the merged note keeps the lyric's own start/end instead of the
    /// pitch note's. The format has one start/end per note, so a bind must
    /// pick a side when the two disagree; this makes that pick explicit
    /// instead of the pitch note's timing always silently winning.
    pub fn bind_lyric_to_note_aligned(
        &mut self,
        word: LyricAddress,
        note_index: usize,
        align_to_lyric: bool,
    ) -> Option<usize> {
        let source_index = self.resolve(word)?;
        if source_index == note_index {
            return None;
        }
        let (_, source) = self.note_at(source_index)?;
        if source.pitch.is_some() {
            return None;
        }
        let source_start = source.start;
        let source_duration = source.duration;
        let source_token_ids: HashSet<String> = source
            .lyrics
            .iter()
            .filter_map(|token| match token {
                LyricToken::Text(token) => Some(token.id.clone()),
                LyricToken::Continuation { .. } => None,
            })
            .collect();
        if source_token_ids.len() != source.lyrics.len() {
            // A continuation token here means the source is itself the tail
            // of a held syllable; its head note is the one to bind instead.
            return None;
        }
        let (target_phrase, target) = self.note_at(note_index)?;
        if !target.lyrics.is_empty() {
            return None;
        }
        let source_phrase = self.note_at(source_index).map(|(phrase, _)| phrase)?;
        if source_phrase != target_phrase {
            return None;
        }
        // A later note holding this syllable through a continuation would be
        // orphaned by moving its anchor away.
        let held_elsewhere = self
            .active_track()?
            .phrases
            .get(source_phrase)?
            .notes
            .iter()
            .any(|note| {
                note.lyrics.iter().any(|token| match token {
                    LyricToken::Continuation { continuation_of } => {
                        source_token_ids.contains(continuation_of)
                    }
                    LyricToken::Text(_) => false,
                })
            });
        if held_elsewhere {
            return None;
        }
        let tokens = std::mem::take(&mut self.note_at_mut(source_index)?.lyrics);
        self.note_at_mut(note_index)?.lyrics = tokens;
        if align_to_lyric {
            let target_note = self.note_at_mut(note_index)?;
            target_note.start = source_start;
            target_note.duration = source_duration;
        }
        let mut orphan = BTreeSet::new();
        orphan.insert(source_index);
        self.remove_notes(&orphan);
        self.touch();
        Some(note_index - usize::from(note_index > source_index))
    }

    /// The syllable at `word`'s own phrase, the flat index of the last note
    /// currently holding it (itself, if it isn't held past that), and its
    /// text token id — the shared groundwork for deciding what note it could
    /// extend onto next.
    pub(crate) fn lyric_chain_tail(&self, word: LyricAddress) -> Option<(usize, usize, String)> {
        let source_index = self.resolve(word)?;
        let (source_phrase, source_note) = self.note_at(source_index)?;
        let token_id = source_note.lyrics.iter().find_map(|token| match token {
            LyricToken::Text(text) => Some(text.id.clone()),
            LyricToken::Continuation { .. } => None,
        })?;
        let mut tail_index = source_index;
        while let Some(next_index) = tail_index.checked_add(1) {
            let Some((phrase, note)) = self.note_at(next_index) else {
                break;
            };
            if phrase != source_phrase {
                break;
            }
            let continues = note.lyrics.iter().any(|token| {
                matches!(token, LyricToken::Continuation { continuation_of } if *continuation_of == token_id)
            });
            if !continues {
                break;
            }
            tail_index = next_index;
        }
        Some((source_phrase, tail_index, token_id))
    }

    /// The text token `note_index` would continue if `extend_lyric_over_note`
    /// were called with the same arguments, or `None` if it isn't eligible.
    /// A pitch that glides partway through one sung syllable is authored as
    /// a chain of continuation tokens across physically adjacent notes, so
    /// `note_index` must immediately follow the syllable's current last held
    /// note (same phrase, no gap) and carry no lyric of its own.
    pub(crate) fn continuation_target(
        &self,
        word: LyricAddress,
        note_index: usize,
    ) -> Option<String> {
        let (source_phrase, tail_index, token_id) = self.lyric_chain_tail(word)?;
        if note_index != tail_index + 1 {
            return None;
        }
        let (target_phrase, target_note) = self.note_at(note_index)?;
        if target_phrase != source_phrase || !target_note.lyrics.is_empty() {
            return None;
        }
        Some(token_id)
    }

    /// Whether `extend_lyric_over_note(word, note_index)` would succeed, for
    /// deciding whether to offer it in the UI without mutating anything.
    pub fn can_extend_lyric_over_note(&self, word: LyricAddress, note_index: usize) -> bool {
        self.continuation_target(word, note_index).is_some()
    }

    /// The note `extend_lyric_over_note(word, ..)` would target next, if
    /// any — the note right after the syllable's current held chain. Lets a
    /// right-click on the syllable itself offer "extend onto the next note"
    /// without the user having to separately right-click that note.
    pub fn next_extendable_note(&self, word: LyricAddress) -> Option<usize> {
        let (_, tail_index, _) = self.lyric_chain_tail(word)?;
        let candidate = tail_index + 1;
        self.can_extend_lyric_over_note(word, candidate)
            .then_some(candidate)
    }

    /// Extends the syllable at `word` to also hold `note_index`, the format's
    /// way of authoring a pitch that changes partway through one sung
    /// syllable: the note becomes a continuation instead of a separate note,
    /// and the syllable's lyric-lane block grows to cover it.
    pub fn extend_lyric_over_note(&mut self, word: LyricAddress, note_index: usize) -> bool {
        let Some(token_id) = self.continuation_target(word, note_index) else {
            return false;
        };
        let Some(note) = self.note_at_mut(note_index) else {
            return false;
        };
        note.lyrics = vec![LyricToken::Continuation {
            continuation_of: token_id,
        }];
        self.touch();
        true
    }

    /// The flat index range a phrase's notes occupy, for searching within it
    /// without knowing how earlier phrases are sized.
    pub(crate) fn phrase_flat_range(&self, phrase: usize) -> Option<std::ops::Range<usize>> {
        let track = self.active_track()?;
        let mut base = 0usize;
        for (index, entry) in track.phrases.iter().enumerate() {
            if index == phrase {
                return Some(base..base + entry.notes.len());
            }
            base += entry.notes.len();
        }
        None
    }

    /// The note within `phrase` that owns the text token `token_id` — the
    /// reverse of walking a continuation chain forward from its head.
    pub(crate) fn note_owning_token(&self, phrase: usize, token_id: &str) -> Option<usize> {
        self.phrase_flat_range(phrase)?.find(|&index| {
            self.note_at(index).is_some_and(|(_, note)| {
                note.lyrics
                    .iter()
                    .any(|token| matches!(token, LyricToken::Text(text) if text.id == token_id))
            })
        })
    }

    /// Binds the given selection (a lyric, a note, or both — one drives the
    /// search when only one is selected) to its nearest eligible counterpart
    /// in the same phrase. The toolbar button and the bare `B` shortcut use
    /// this; a held-`B` click names the counterpart explicitly instead of
    /// searching for one.
    pub fn bind_nearest(
        &mut self,
        word: Option<LyricAddress>,
        note: Option<usize>,
        align_to_lyric: bool,
    ) -> Option<usize> {
        if let Some(word) = word {
            let source_index = self.resolve(word)?;
            let (phrase, source) = self.note_at(source_index)?;
            if source.pitch.is_some() {
                return None;
            }
            let source_start = source.start;
            let range = self.phrase_flat_range(phrase)?;
            let target_index = range
                .filter(|index| *index != source_index)
                .filter_map(|index| self.note_at(index).map(|(_, note)| (index, note)))
                .filter(|(_, note)| note.pitch.is_some() && note.lyrics.is_empty())
                .min_by_key(|(_, note)| note.start.abs_diff(source_start))
                .map(|(index, _)| index)?;
            return self.bind_lyric_to_note_aligned(word, target_index, align_to_lyric);
        }
        let note_index = note?;
        let (phrase, target) = self.note_at(note_index)?;
        if target.pitch.is_none() || !target.lyrics.is_empty() {
            return None;
        }
        let target_start = target.start;
        let range = self.phrase_flat_range(phrase)?;
        let source_index = range
            .filter(|index| *index != note_index)
            .filter_map(|index| self.note_at(index).map(|(_, note)| (index, note)))
            .filter(|(_, note)| {
                note.pitch.is_none()
                    && !note.lyrics.is_empty()
                    && note
                        .lyrics
                        .iter()
                        .all(|token| matches!(token, LyricToken::Text(_)))
            })
            .min_by_key(|(_, note)| note.start.abs_diff(target_start))
            .map(|(index, _)| index)?;
        let word = self.address_of_note(source_index)?;
        self.bind_lyric_to_note_aligned(word, note_index, align_to_lyric)
    }

    /// Unbinds whichever note the selection names — directly, or through the
    /// note a selected lyric belongs to. When `word` names a specific token
    /// on a note that carries more than one, that exact token — not just
    /// whichever one the note-position lookup happens to land on — is what
    /// comes back reselected.
    pub fn unbind_selected(
        &mut self,
        word: Option<LyricAddress>,
        note: Option<usize>,
    ) -> Option<LyricAddress> {
        let note_index = note.or_else(|| word.and_then(|word| self.resolve(word)))?;
        let preferred_token_id = word.and_then(|address| self.token_id_at(address));
        self.unbind_note_preferring(note_index, preferred_token_id.as_deref())
    }

    /// Splits `note_index`'s pitch and lyric apart into two adjacent notes:
    /// the first half keeps the pitch, the second half becomes a new
    /// unpitched note carrying the lyric text. The inverse of
    /// [`Self::bind_lyric_to_note`].
    ///
    /// When `note_index` instead holds a *continuation* — one note among
    /// several a syllable is held across (see
    /// [`Self::extend_lyric_over_note`]) — there is nothing to split; the
    /// note is detached from the chain instead, keeping its pitch and
    /// getting its own independent copy of the syllable's text (a fresh
    /// token, not shared with the head note it used to continue), so
    /// detaching it never makes that text disappear. Only the chain's
    /// current tail can be detached this way, since a syllable is only
    /// ever extended onto the immediate next note; undoing it the same way
    /// keeps the chain always contiguous instead of leaving a later note
    /// orphaned mid-chain.
    pub fn unbind_note(&mut self, note_index: usize) -> Option<LyricAddress> {
        self.unbind_note_preferring(note_index, None)
    }

    /// `preferred_token_id` names the specific lyric token the caller had
    /// selected, when the note being split carries more than one — the
    /// resulting address follows that token rather than an arbitrary one.
    /// `None` (the plain [`Self::unbind_note`] entry point) falls back to
    /// the note's first token, deterministically.
    pub(crate) fn unbind_note_preferring(
        &mut self,
        note_index: usize,
        preferred_token_id: Option<&str>,
    ) -> Option<LyricAddress> {
        let (phrase, note) = self.note_at(note_index)?;
        if let Some(LyricToken::Continuation { continuation_of }) = note.lyrics.first() {
            let token_id = continuation_of.clone();
            let is_tail = !matches!(
                self.note_at(note_index + 1),
                Some((next_phrase, next_note))
                    if next_phrase == phrase
                        && next_note.lyrics.iter().any(|token| matches!(
                            token,
                            LyricToken::Continuation { continuation_of } if *continuation_of == token_id
                        ))
            );
            if !is_tail {
                return None;
            }
            let source_text = self
                .note_owning_token(phrase, &token_id)
                .and_then(|owner| self.note_at(owner))
                .and_then(|(_, owner_note)| {
                    owner_note.lyrics.iter().find_map(|token| match token {
                        LyricToken::Text(text) if text.id == token_id => Some(text.clone()),
                        _ => None,
                    })
                });
            let new_id = self.allocate_id("lyric");
            let detached_note = self.note_at_mut(note_index)?;
            detached_note.lyrics = match source_text {
                Some(mut text) => {
                    text.id = new_id;
                    vec![LyricToken::Text(text)]
                }
                // The head's text somehow no longer carries this token
                // (only reachable from a chart a repair already had to
                // touch) — detach cleanly rather than invent placeholder
                // text.
                None => Vec::new(),
            };
            self.touch();
            return self.address_of_note(note_index);
        }

        let minimum = self.min_duration();
        let eligible = note.pitch.is_some()
            && !note.lyrics.is_empty()
            && note.duration >= minimum * 2
            && note
                .lyrics
                .iter()
                .all(|token| matches!(token, LyricToken::Text(_)));
        if !eligible {
            // Nothing has been touched yet — a plain `None` return, not a
            // take/restore round-trip through `flat`. `unbind_note`'s
            // callers checkpoint before calling and discard that checkpoint
            // on `None`; if this validation ran after a mutating
            // take_flat/restore_flat pair (as it used to), that discarded
            // checkpoint would be the only record of a structural change
            // (and a revision bump) the document had already undergone.
            return None;
        }

        let mut flat = self.take_flat();
        let FlatNote { phrase, mut note } = flat.remove(note_index);
        let lyrics = std::mem::take(&mut note.lyrics);
        let start = note.start;
        let end = note.start.saturating_add(note.duration);
        let split = start + note.duration / 2;
        note.duration = split - start;
        let text_note = VocalNote {
            id: self.allocate_id("note"),
            start: split,
            duration: end - split,
            pitch: None,
            vocal_mode: VocalMode::Spoken,
            bonus: NoteBonus::Normal,
            scoring: NoteScoring {
                mode: ScoringMode::Rhythm,
                weight: 1.0,
            },
            lyrics,
        };
        // Resolved by stable token id, not by re-deriving a note position
        // after the split: `text_note` can carry more than one lyric token
        // (the eligibility check above only requires all-Text, not
        // exactly-one), and a note-position lookup can't tell them apart —
        // it would just hand back whichever one it happens to land on.
        let target_token_id = text_note
            .lyrics
            .iter()
            .filter_map(|token| match token {
                LyricToken::Text(text) => Some(text.id.as_str()),
                LyricToken::Continuation { .. } => None,
            })
            .find(|id| Some(*id) == preferred_token_id)
            .or_else(|| {
                text_note.lyrics.iter().find_map(|token| match token {
                    LyricToken::Text(text) => Some(text.id.as_str()),
                    LyricToken::Continuation { .. } => None,
                })
            })
            .map(str::to_owned);
        flat.insert(note_index, FlatNote { phrase, note });
        flat.insert(
            note_index + 1,
            FlatNote {
                phrase,
                note: text_note,
            },
        );
        self.restore_flat(flat);
        self.reassign_duplicate_note_ids();
        target_token_id.and_then(|id| self.address_of_token(phrase, &id))
    }

    pub(crate) fn address_of_note(&self, note: usize) -> Option<LyricAddress> {
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

    /// The stable id of the specific lyric token `address` names, distinct
    /// from `resolve`'s note index — a note can carry several tokens, and
    /// the id is what survives a restructure (a note split, a reassembled
    /// flat list) that a `(segment, word)` position does not.
    pub(crate) fn token_id_at(&self, address: LyricAddress) -> Option<String> {
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
                LyricToken::Text(token) => Some(token),
                LyricToken::Continuation { .. } => None,
            })
            .nth(ordinal)
            .map(|token| token.id.clone())
    }

    /// The current `LyricAddress` of the token identified by `token_id`,
    /// wherever it now lives in `phrase` — the inverse of `token_id_at`,
    /// used to reselect a token by identity after a restructure moved it
    /// instead of guessing from a note position.
    pub(crate) fn address_of_token(&self, phrase: usize, token_id: &str) -> Option<LyricAddress> {
        let entry = self.active_track()?.phrases.get(phrase)?;
        let mut word = 0usize;
        for note in &entry.notes {
            for token in &note.lyrics {
                if let LyricToken::Text(text) = token {
                    if text.id == token_id {
                        return Some(LyricAddress {
                            segment: phrase,
                            word,
                        });
                    }
                    word += 1;
                }
            }
        }
        None
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

    pub(crate) fn previous_phrase_token(&self, segment: usize) -> Option<LyricAddress> {
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

    /// The notes of a phrase that can hold a syllable. A note carrying a
    /// continuation is holding the syllable before it and is not a slot.
    pub(crate) fn lyric_slots(&self, phrase: usize) -> Vec<usize> {
        let Some(entry) = self
            .active_track()
            .and_then(|track| track.phrases.get(phrase))
        else {
            return Vec::new();
        };
        (0..entry.notes.len())
            .filter(|offset| {
                !entry.notes[*offset]
                    .lyrics
                    .iter()
                    .any(|token| matches!(token, LyricToken::Continuation { .. }))
            })
            .collect()
    }

    /// Moves every syllable in a phrase one note along, wrapping at the ends.
    ///
    /// This is the fix for a line that is right except that it sits one note
    /// off — a very common outcome of automatic alignment, and painful to
    /// repair syllable by syllable.
    pub fn roll_lyrics(&mut self, phrase: usize, forward: bool) -> bool {
        let slots = self.lyric_slots(phrase);
        if slots.len() < 2 {
            return false;
        }
        let Some(entry) = self
            .chart
            .tracks
            .get_mut(self.track)
            .and_then(|track| track.phrases.get_mut(phrase))
        else {
            return false;
        };
        let mut carried = slots
            .iter()
            .map(|offset| std::mem::take(&mut entry.notes[*offset].lyrics))
            .collect::<Vec<_>>();
        if carried.iter().all(|tokens| tokens.is_empty()) {
            return false;
        }
        if forward {
            carried.rotate_right(1);
        } else {
            carried.rotate_left(1);
        }
        for (offset, tokens) in slots.iter().zip(carried) {
            entry.notes[*offset].lyrics = tokens;
        }
        self.touch();
        true
    }

    /// The phrase written as its own tokens: a space starts a new word, and a
    /// slash divides syllables inside one. Reading and writing this text is how
    /// a whole line gets retyped without clicking through every syllable.
    ///
    /// A literal slash in lyric text cannot be expressed here, which is the
    /// price of the boundaries being visible at all.
    pub fn phrase_token_text(&self, phrase: usize) -> String {
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
                if !text.is_empty() {
                    text.push(if token.join_before == LyricJoin::Space {
                        ' '
                    } else {
                        '/'
                    });
                }
                text.push_str(&token.text);
            }
        }
        text
    }

    /// Rewrites a phrase's syllables from the text form above, keeping them on
    /// the notes that are already there.
    pub fn set_phrase_token_text(&mut self, phrase: usize, text: &str) -> bool {
        if self.phrase_token_text(phrase) == text {
            return false;
        }
        let parsed = parse_phrase_tokens(text);
        let slots = self.lyric_slots(phrase);
        if slots.is_empty() {
            return false;
        }
        // Reuse the IDs already in the phrase so continuations keep resolving
        // wherever the syllable they hold ends up.
        let existing = {
            let Some(entry) = self
                .active_track()
                .and_then(|track| track.phrases.get(phrase))
            else {
                return false;
            };
            entry
                .notes
                .iter()
                .flat_map(|note| note.lyrics.iter())
                .filter_map(|token| match token {
                    LyricToken::Text(token) => Some(token.id.clone()),
                    LyricToken::Continuation { .. } => None,
                })
                .collect::<Vec<_>>()
        };
        let mut ids = existing.into_iter();
        let tokens = parsed
            .into_iter()
            .map(|(text, join)| {
                let id = ids.next().unwrap_or_else(|| self.allocate_id("lyric"));
                LyricToken::Text(LyricTextToken {
                    id,
                    text,
                    join_before: join,
                    reading: None,
                    phonemes: None,
                })
            })
            .collect::<Vec<_>>();

        let Some(entry) = self
            .chart
            .tracks
            .get_mut(self.track)
            .and_then(|track| track.phrases.get_mut(phrase))
        else {
            return false;
        };
        for offset in &slots {
            entry.notes[*offset].lyrics.clear();
        }
        let last = *slots.last().expect("checked non-empty");
        for (index, token) in tokens.into_iter().enumerate() {
            // Syllables past the last note pile onto it rather than vanishing,
            // so retyping a longer line never silently drops words.
            let offset = slots.get(index).copied().unwrap_or(last);
            entry.notes[offset].lyrics.push(token);
        }
        self.prune_orphaned_continuations();
        self.touch();
        true
    }

    /// The phrase and in-phrase position of a flattened note index.
    pub(crate) fn locate_note(&self, index: usize) -> Option<(usize, usize)> {
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

    pub fn phrase_index_for_note(&self, note_index: usize) -> Option<usize> {
        self.locate_note(note_index).map(|(phrase, _)| phrase)
    }

    pub fn note_range_units(&self, note_indices: &BTreeSet<usize>) -> Option<(u64, u64)> {
        let notes = self.notes();
        let mut start = u64::MAX;
        let mut end = 0u64;
        for index in note_indices {
            let note = notes.get(*index)?;
            let note_start = seconds_to_units(note.start, self.timebase());
            let note_end = seconds_to_units(note.end, self.timebase());
            start = start.min(note_start);
            end = end.max(note_end);
        }
        (end > start).then_some((start, end))
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
            let pieces = crate::editor::syllabize::syllables(
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
    pub fn problems(&self) -> crate::editor::ProblemReport {
        crate::editor::problems::report(self)
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
    /// The note a lyric address currently resolves to, so a click on a lyric
    /// can locate its owning note without knowing how the format nests them.
    pub fn note_for_word(&self, address: LyricAddress) -> Option<usize> {
        self.resolve(address)
    }

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
