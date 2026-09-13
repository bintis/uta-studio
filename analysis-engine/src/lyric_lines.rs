//! Textual lyric lines for generated transcripts. These are display groupings,
//! not measured audio boundaries or model confidence.

use crate::artifact::TranscriptToken;

pub(crate) fn generated_lyric_lines(text: &str) -> Vec<TranscriptToken> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut sentence_end = false;
    let mut characters = text.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if sentence_end && (character.is_whitespace() || !sentence_suffix(character)) {
            push_line(&mut lines, &text[start..offset]);
            start = offset;
            sentence_end = false;
        }
        if matches!(character, '\n' | '\r') {
            push_line(&mut lines, &text[start..offset]);
            start = offset + character.len_utf8();
            sentence_end = false;
        } else if matches!(character, '。' | '！' | '？' | '!' | '?')
            || (character == '.'
                && characters
                    .peek()
                    .is_none_or(|(_, next)| next.is_whitespace() || sentence_suffix(*next)))
        {
            sentence_end = true;
        }
    }
    push_line(&mut lines, &text[start..]);
    lines
}

fn sentence_suffix(character: char) -> bool {
    matches!(
        character,
        '。' | '！'
            | '？'
            | '!'
            | '?'
            | '.'
            | '…'
            | '」'
            | '』'
            | '”'
            | '’'
            | '"'
            | '\''
            | ')'
            | '）'
            | ']'
            | '】'
            | '》'
    )
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
    }
}
