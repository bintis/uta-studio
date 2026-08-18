use super::*;
use std::collections::HashSet;

use utz::{LyricJoin, LyricToken};

pub(crate) fn parse_phrase_tokens(text: &str) -> Vec<(String, LyricJoin)> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut join = LyricJoin::None;
    let mut pending = LyricJoin::None;
    let mut started = false;
    for character in text.chars() {
        let separator = match character {
            '/' => Some(LyricJoin::None),
            character if character.is_whitespace() => Some(LyricJoin::Space),
            _ => None,
        };
        match separator {
            Some(next) => {
                if started && !current.is_empty() {
                    tokens.push((std::mem::take(&mut current), join));
                    started = false;
                }
                // Consecutive separators collapse; a space anywhere between two
                // syllables wins, because it is the stronger break.
                pending = if pending == LyricJoin::Space || next == LyricJoin::Space {
                    LyricJoin::Space
                } else {
                    LyricJoin::None
                };
            }
            None => {
                if !started {
                    join = if tokens.is_empty() {
                        LyricJoin::None
                    } else {
                        pending
                    };
                    pending = LyricJoin::None;
                    started = true;
                }
                current.push(character);
            }
        }
    }
    if !current.is_empty() {
        tokens.push((current, join));
    }
    tokens
}

pub(crate) fn orphaned_continuations(flat: &[FlatNote]) -> HashSet<String> {
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
