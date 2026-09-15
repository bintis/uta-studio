use super::*;
use std::collections::{BTreeSet, HashSet};
use uta_studio_chart::{LyricJoin, LyricTextToken, LyricTiming, LyricToken};

impl EditorDocument {
    pub(crate) fn token_at(&self, address: LyricAddress) -> Option<&LyricTextToken> {
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
    }

    pub(crate) fn effective_lyric_timing(&self, address: LyricAddress) -> Option<LyricTiming> {
        if let Some(timing) = self.token_at(address)?.timing {
            return Some(timing);
        }
        let lyric = self
            .lyrics()
            .into_iter()
            .find(|lyric| lyric.address == address)?;
        let start = crate::editor::seconds_to_units(lyric.start, self.timebase());
        let end = crate::editor::seconds_to_units(lyric.end, self.timebase());
        Some(LyricTiming {
            start,
            duration: end.saturating_sub(start),
        })
    }

    pub(crate) fn preserve_note_lyric_timings(&mut self, indices: &BTreeSet<usize>) {
        let timings = self
            .lyrics()
            .into_iter()
            .filter(|lyric| indices.contains(&lyric.note))
            .filter_map(|lyric| {
                let timing = self.effective_lyric_timing(lyric.address)?;
                Some((lyric.address, timing))
            })
            .collect::<Vec<_>>();
        for (address, timing) in timings {
            if let Some(token) = self.token_mut(address) {
                token.timing = Some(timing);
            }
        }
    }

    pub(crate) fn replace_independent_lyric(
        &mut self,
        address: LyricAddress,
        pieces: Vec<LyricTextToken>,
    ) -> Vec<String> {
        let Some(note_index) = self.resolve(address) else {
            return Vec::new();
        };
        let Some(token_id) = self.token_id_at(address) else {
            return Vec::new();
        };
        let Some(note) = self.note_at_mut(note_index) else {
            return Vec::new();
        };
        let Some(position) = note
            .lyrics
            .iter()
            .position(|token| matches!(token, LyricToken::Text(token) if token.id == token_id))
        else {
            return Vec::new();
        };
        let ids = pieces.iter().map(|token| token.id.clone()).collect();
        note.lyrics.splice(
            position..=position,
            pieces.into_iter().map(LyricToken::Text),
        );
        self.touch();
        ids
    }

    pub(crate) fn lyric_needs_independent_edit(&self, address: LyricAddress) -> bool {
        self.token_at(address)
            .is_some_and(|token| token.timing.is_some())
            || self
                .resolve(address)
                .and_then(|index| self.note_at(index))
                .is_some_and(|(_, note)| {
                    note.lyrics
                        .iter()
                        .filter(|token| matches!(token, LyricToken::Text(_)))
                        .count()
                        > 1
                })
    }

    pub(crate) fn split_independent_lyric(
        &mut self,
        address: LyricAddress,
        left: String,
        right: String,
        playhead: Option<f64>,
    ) -> BTreeSet<LyricAddress> {
        let Some(mut tail) = self.token_at(address).cloned() else {
            return BTreeSet::new();
        };
        let Some(timing) = self.effective_lyric_timing(address) else {
            return BTreeSet::new();
        };
        if timing.duration < 2 {
            return BTreeSet::from([address]);
        }
        let end = timing.start.saturating_add(timing.duration);
        let split = playhead
            .filter(|time| time.is_finite())
            .map(|time| self.to_units(time))
            .filter(|time| *time > timing.start && *time < end)
            .unwrap_or(timing.start + timing.duration / 2);
        let mut head = tail.clone();
        head.id = self.allocate_id("lyric");
        head.text = left;
        head.timing = Some(LyricTiming {
            start: timing.start,
            duration: split - timing.start,
        });
        head.reading = None;
        head.phonemes = None;
        tail.text = right;
        tail.join_before = LyricJoin::None;
        tail.timing = Some(LyricTiming {
            start: split,
            duration: end - split,
        });
        tail.reading = None;
        tail.phonemes = None;
        self.replace_independent_lyric(address, vec![head, tail])
            .into_iter()
            .filter_map(|id| self.address_of_token(address.segment, &id))
            .collect()
    }

    pub(crate) fn merge_independent_lyrics(
        &mut self,
        addresses: &BTreeSet<LyricAddress>,
    ) -> Option<LyricAddress> {
        let first = *addresses.first()?;
        let mut tokens = Vec::new();
        for address in addresses {
            tokens.push((
                self.token_at(*address)?.clone(),
                self.effective_lyric_timing(*address)?,
            ));
        }
        let keep_id = tokens.first()?.0.id.clone();
        let removed = tokens
            .iter()
            .skip(1)
            .map(|(token, _)| token.id.clone())
            .collect::<HashSet<_>>();
        let start = tokens.iter().map(|(_, timing)| timing.start).min()?;
        let end = tokens
            .iter()
            .map(|(_, timing)| timing.start.saturating_add(timing.duration))
            .max()?;
        let mut text = String::new();
        for (token, _) in &tokens {
            if !text.is_empty() && !self.compact_language() && token.join_before == LyricJoin::Space
            {
                text.push(' ');
            }
            text.push_str(&token.text);
        }
        let unresolved = tokens.iter().any(|(token, _)| token.timing_unresolved);
        let keep = self.token_mut(first)?;
        keep.text = text;
        keep.reading = None;
        keep.phonemes = None;
        keep.timing = Some(LyricTiming {
            start,
            duration: end.saturating_sub(start),
        });
        keep.timing_unresolved = unresolved;
        let owner = self.resolve(first)?;
        let range = self.phrase_flat_range(first.segment)?;
        for index in range {
            if let Some(note) = self.note_at_mut(index) {
                let mut removed_text = false;
                note.lyrics.retain_mut(|token| match token {
                    LyricToken::Text(token) if removed.contains(&token.id) => {
                        removed_text = true;
                        false
                    }
                    LyricToken::Continuation { continuation_of }
                        if removed.contains(continuation_of) =>
                    {
                        *continuation_of = keep_id.clone();
                        index != owner
                    }
                    _ => true,
                });
                if removed_text && index != owner && !note.lyrics.iter().any(
                    |token| matches!(token, LyricToken::Continuation { continuation_of } if *continuation_of == keep_id)
                ) {
                    note.lyrics.push(LyricToken::Continuation { continuation_of: keep_id.clone() });
                }
            }
        }
        self.touch();
        self.address_of_token(first.segment, &keep_id)
    }
    pub(crate) fn syllabize_independent_lyric(&mut self, address: LyricAddress) -> Vec<String> {
        let Some(token) = self.token_at(address).cloned() else {
            return Vec::new();
        };
        let Some(timing) = self.effective_lyric_timing(address) else {
            return Vec::new();
        };
        let pieces = crate::editor::syllabize::syllables(
            &token.text,
            token.reading.as_deref(),
            self.language(),
        );
        if pieces.len() < 2 || timing.duration < pieces.len() as u64 {
            return Vec::new();
        }
        let weights = pieces
            .iter()
            .map(|piece| piece.text.chars().count().max(1) as u64)
            .collect::<Vec<_>>();
        let total = weights.iter().sum::<u64>();
        let mut cursor = timing.start;
        let mut consumed = 0;
        let mut replacements = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            consumed += weights[index];
            let remaining = (pieces.len() - index - 1) as u64;
            let end = (timing.start + timing.duration * consumed / total)
                .max(cursor + 1)
                .min(timing.start + timing.duration - remaining);
            let mut replacement = token.clone();
            replacement.id = if index + 1 == pieces.len() {
                token.id.clone()
            } else {
                self.allocate_id("lyric")
            };
            replacement.text = piece.text.clone();
            replacement.reading = piece.reading.clone();
            replacement.phonemes = None;
            replacement.timing_unresolved = true;
            replacement.join_before = if index == 0 {
                token.join_before
            } else {
                LyricJoin::None
            };
            replacement.timing = Some(LyricTiming {
                start: cursor,
                duration: end - cursor,
            });
            replacements.push(replacement);
            cursor = end;
        }
        self.replace_independent_lyric(address, replacements)
    }
}
