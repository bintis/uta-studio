//! Textual lyric lines for generated transcripts. These are display groupings,
//! not measured audio boundaries or model confidence.

use crate::artifact::TranscriptToken;

pub(crate) fn generated_lyric_lines(text: &str) -> Vec<TranscriptToken> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut sentence_end = false;
    let mut delimiters = Vec::new();
    let mut characters = text.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if matches!(
            character,
            '\n' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}'
        ) {
            push_line(&mut lines, &text[start..offset]);
            start = offset + character.len_utf8();
            sentence_end = false;
            continue;
        }

        let previous = text[..offset].chars().next_back();
        let next = characters.peek().map(|(_, next)| *next);
        let closes_delimiter = delimiters.last() == Some(&character);
        let apostrophe = matches!(character, '\'' | '’')
            && previous.is_some_and(char::is_alphanumeric)
            && (next.is_some_and(char::is_alphanumeric) || !closes_delimiter);
        if sentence_end {
            if matches!(character, ',' | '，' | '、' | ';' | '；' | ':' | '：') {
                // A quoted exclamation can continue its enclosing sentence:
                // "Go!", she said. Keep that explicit continuation together.
                sentence_end = false;
            } else if !character.is_whitespace()
                && !sentence_suffix(character, closes_delimiter && !apostrophe)
            {
                push_line(&mut lines, &text[start..offset]);
                start = offset;
                sentence_end = false;
            }
        }
        if !apostrophe {
            if closes_delimiter {
                delimiters.pop();
            } else if let Some(closing) = opening_delimiter(character) {
                delimiters.push(closing);
            }
        }
        sentence_end |= match character {
            '。' | '！' | '？' | '!' | '?' => true,
            '.' | '．' => period_ends_sentence(&text[start..], offset - start, character),
            '…' | '⋯' => ellipsis_ends_sentence(&text[offset + character.len_utf8()..]),
            _ => false,
        };
    }
    push_line(&mut lines, &text[start..]);
    lines
}

fn sentence_suffix(character: char, closes_delimiter: bool) -> bool {
    closes_delimiter
        || matches!(
            character,
            '。' | '！' | '？' | '!' | '?' | '.' | '．' | '…' | '⋯'
        )
        || closing_delimiter(character)
}

fn closing_delimiter(character: char) -> bool {
    matches!(
        character,
        '」' | '』' | '”' | '’' | ')' | '）' | ']' | '}' | '】' | '》' | '〉' | '»' | '›'
    )
}

fn opening_delimiter(character: char) -> Option<char> {
    Some(match character {
        '「' => '」',
        '『' => '』',
        '“' => '”',
        '‘' => '’',
        '(' => ')',
        '（' => '）',
        '[' => ']',
        '{' => '}',
        '【' => '】',
        '《' => '》',
        '〈' => '〉',
        '«' => '»',
        '‹' => '›',
        '"' | '\'' => character,
        _ => return None,
    })
}

fn period_ends_sentence(text: &str, offset: usize, period: char) -> bool {
    let before = &text[..offset];
    let after = &text[offset + period.len_utf8()..];
    let previous = before.chars().next_back();
    let next = after.chars().next();
    // Keep both ordinary and full-width decimals, including a leading .5.
    if next.is_some_and(char::is_numeric)
        && previous.is_none_or(|character| {
            character.is_numeric()
                || character.is_whitespace()
                || opening_delimiter(character).is_some()
                || matches!(character, '+' | '-' | '−')
        })
    {
        return false;
    }
    if next.is_some_and(|character| matches!(character, '.' | '．' | '…' | '⋯')) {
        return false;
    }
    if previous.is_some_and(|character| matches!(character, '.' | '．' | '…' | '⋯')) {
        return ellipsis_ends_sentence(after);
    }

    let word = before
        .rsplit(|character: char| !character.is_ascii_alphabetic() && character != '.')
        .next()
        .unwrap_or_default();
    let following = after.trim_start();
    let initial_parts = |word: &str| {
        !word.is_empty()
            && word.split('.').all(|part| {
                part.len() == 1
                    && part
                        .chars()
                        .all(|character| character.is_ascii_alphabetic())
            })
    };
    // An internal initialism dot (e.g. the first dot of U.S. or e.g.)
    // differs from an unspaced sentence boundary such as Home.Come back.
    if initial_parts(word)
        && next.is_some_and(|character| character.is_ascii_alphabetic())
        && after.chars().nth(1) == Some('.')
    {
        return false;
    }
    let next_character = following.chars().next();
    let lexical_continuation = next_character.is_some_and(char::is_alphanumeric);
    let abbreviation = word.to_ascii_lowercase();
    if (lexical_continuation
        || next_character.is_some_and(|character| matches!(character, ',' | ':' | ';')))
        && matches!(
            abbreviation.as_str(),
            "mr" | "mrs" | "ms" | "dr" | "prof" | "sr" | "jr" | "st" | "mt" | "e.g" | "i.e" | "vs"
        )
    {
        return false;
    }
    if next_character.is_some_and(|character| character.is_lowercase() || character.is_numeric())
        && matches!(
            abbreviation.as_str(),
            "etc" | "approx" | "fig" | "no" | "vol" | "a.m" | "p.m"
        )
    {
        return false;
    }
    if lexical_continuation && initial_parts(word) {
        let next_word = following
            .split(|character: char| !character.is_alphabetic())
            .next()
            .unwrap_or_default();
        // Preserve initials before names, while allowing an acronym at the
        // end of a sentence before a clear pronoun/demonstrative sentence.
        let sentence_starter = matches!(
            next_word,
            "I" | "We" | "You" | "He" | "She" | "It" | "They" | "This" | "That" | "These" | "Those"
        );
        if !sentence_starter
            && (word.contains('.')
                || (word != "I"
                    && word.chars().all(|character| character.is_ascii_uppercase())
                    && next_character.is_some_and(char::is_uppercase)))
        {
            return false;
        }
    }
    true
}

fn ellipsis_ends_sentence(after: &str) -> bool {
    let following = after.trim_start();
    if following.starts_with(['.', '．', '…', '⋯']) {
        return false;
    }
    // Ellipses also mark a pause within a line. Without an explicit newline,
    // retain a lowercase/CJK continuation; a capitalized next sentence or a
    // new directional quotation gives a useful textual boundary.
    following
        .trim_start_matches(|character: char| {
            character.is_whitespace()
                || closing_delimiter(character)
                || matches!(character, '"' | '\'')
        })
        .chars()
        .next()
        .is_none_or(|character| character.is_uppercase() || opening_delimiter(character).is_some())
}

fn push_line(lines: &mut Vec<TranscriptToken>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        lines.push(TranscriptToken {
            id: format!("lyric-line-{}", lines.len()),
            text: text.to_string(),
            confidence: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_lines(text: &str, expected: &[&str]) {
        let lines = generated_lyric_lines(text);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            expected,
            "input: {text:?}"
        );
        // Line-edge whitespace is the only permitted textual loss. This also
        // checks multibyte text, combining marks and punctuation byte-for-byte.
        let compact = |text: &str| {
            text.chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
        };
        let joined = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>();
        assert_eq!(compact(&joined), compact(text));
        for (index, line) in lines.iter().enumerate() {
            assert_eq!(line.id, format!("lyric-line-{index}"));
            assert!(line.confidence.is_none());
        }
    }

    #[test]
    fn generated_sentences_keep_closing_quotes_and_repeated_punctuation() {
        let text = "光を辿る。『ここにいる！』\n歌おう！？\r\nまた会える";
        let lines = generated_lyric_lines(text);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["光を辿る。", "『ここにいる！』", "歌おう！？", "また会える"]
        );
        assert!(lines.iter().all(|line| line.confidence.is_none()));
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            text.chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
        );
    }

    #[test]
    fn generated_lines_keep_decimal_points_and_unpunctuated_lines() {
        let lines = generated_lyric_lines("Sing at 3.5 now. \"Come home!\"\nSing again\n\nWith me");
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [
                "Sing at 3.5 now.",
                "\"Come home!\"",
                "Sing again",
                "With me"
            ]
        );
        assert_eq!(generated_lyric_lines("sing now").len(), 1);
        assert!(generated_lyric_lines(" \r\n ").is_empty());
        assert_eq!(
            generated_lyric_lines("光る.歌う.またね.")
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["光る.", "歌う.", "またね."]
        );
    }

    #[test]
    fn mixed_scripts_and_unspaced_ascii_sentences_keep_their_boundaries() {
        assert_lines(
            "Hello.World.光る.Sing again.再见。終わり．Next!",
            &[
                "Hello.",
                "World.",
                "光る.",
                "Sing again.",
                "再见。",
                "終わり．",
                "Next!",
            ],
        );
        assert_lines("one.two.three.", &["one.", "two.", "three."]);
    }

    #[test]
    fn decimals_do_not_create_sentences_in_any_of_the_numeric_scripts() {
        assert_lines(
            "３.５拍で歌う.再唱３．５遍。Sing at 3.5 or .5 speed.Try −.５ now.",
            &[
                "３.５拍で歌う.",
                "再唱３．５遍。",
                "Sing at 3.5 or .5 speed.",
                "Try −.５ now.",
            ],
        );
        assert_lines("عد ٣.٥ مرات.ثم غنِّ.", &["عد ٣.٥ مرات.", "ثم غنِّ."]);
    }

    #[test]
    fn titles_initials_and_abbreviations_do_not_split_names_or_examples() {
        assert_lines(
            "Dr. Smith sings.Mr.Jones joins.J. R. R. Tolkien waits.",
            &[
                "Dr. Smith sings.",
                "Mr.Jones joins.",
                "J. R. R. Tolkien waits.",
            ],
        );
        assert_lines(
            "Try e.g. red or blue, i.e., our colours.Sing vs. whisper.",
            &[
                "Try e.g. red or blue, i.e., our colours.",
                "Sing vs. whisper.",
            ],
        );
        assert_lines(
            "At 5 p.m. we sing.In the U.S. Navy we sing.I miss the U.S. We sing again.",
            &[
                "At 5 p.m. we sing.",
                "In the U.S. Navy we sing.",
                "I miss the U.S.",
                "We sing again.",
            ],
        );
        assert_lines(
            "Red etc. stays.Red etc. We go.",
            &["Red etc. stays.", "Red etc.", "We go."],
        );
    }

    #[test]
    fn ellipses_distinguish_a_pause_from_a_clear_next_sentence() {
        assert_lines(
            "Wait... stay with me.Wait…Come home.Wait⋯ Stay here.",
            &[
                "Wait... stay with me.",
                "Wait…",
                "Come home.",
                "Wait⋯",
                "Stay here.",
            ],
        );
        assert_lines(
            "待って……まだ歌う。等一下...再唱。待って……「帰ろう！」",
            &[
                "待って……まだ歌う。",
                "等一下...再唱。",
                "待って……",
                "「帰ろう！」",
            ],
        );
        assert_lines("Wait...\r\nStay…\n……", &["Wait...", "Stay…", "……"]);
        assert_lines(
            "Really?!...Yes!!??終わり。",
            &["Really?!...", "Yes!!??", "終わり。"],
        );
    }

    #[test]
    fn adjacent_opening_quotes_belong_to_the_next_sentence() {
        assert_lines("Go.\"Stay!\"\"Come!\"", &["Go.", "\"Stay!\"", "\"Come!\""]);
        assert_lines("Go.'Stay!''Come!'", &["Go.", "'Stay!'", "'Come!'"]);
        assert_lines(
            "“光る！”『歌う？』«Encore!»",
            &["“光る！”", "『歌う？』", "«Encore!»"],
        );
        assert_lines("「『歌う！？』」次へ。", &["「『歌う！？』」", "次へ。"]);
    }

    #[test]
    fn quote_spaces_apostrophes_and_explicit_clause_continuations_stay_intact() {
        assert_lines(
            "\"Don't stop! \" 'James’ song!'Next.",
            &["\"Don't stop! \"", "'James’ song!'", "Next."],
        );
        assert_lines(
            "\"Go!\", she said.『歌う！』、そう言った。‘Don’t stop!’Next.",
            &[
                "\"Go!\", she said.",
                "『歌う！』、そう言った。",
                "‘Don’t stop!’",
                "Next.",
            ],
        );
        assert_lines(
            "(\"Sing!\")Next.[歌う。]次。",
            &["(\"Sing!\")", "Next.", "[歌う。]", "次。"],
        );
    }

    #[test]
    fn explicit_line_breaks_survive_even_inside_quotes_or_after_abbreviations() {
        assert_lines(
            " \r\n\"Sing\r\n\r\nagain.\"\nDr.\rSmith\u{0085}歌う\u{2028}再唱\u{2029}終わり \n",
            &[
                "\"Sing",
                "again.\"",
                "Dr.",
                "Smith",
                "歌う",
                "再唱",
                "終わり",
            ],
        );
        assert_lines("\r\n\t\u{2028}\u{2029}", &[]);
    }

    #[test]
    fn unicode_lyrics_are_preserved_without_normalizing_or_slicing_codepoints() {
        assert_lines(
            "Cafe\u{0301} 🎤👩‍🎤を歌う。再见，世界！\u{00a0}\"Stay—café!\"\n﨑と崎は違う",
            &[
                "Cafe\u{0301} 🎤👩‍🎤を歌う。",
                "再见，世界！",
                "\"Stay—café!\"",
                "﨑と崎は違う",
            ],
        );
    }
}
