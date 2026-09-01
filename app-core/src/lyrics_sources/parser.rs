use std::{io::Read, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::ZlibDecoder;
use regex::Regex;
use serde_json::Value;

use super::tripledes::{DECRYPT, decrypt_block, key_setup};
use super::{
    LyricsProviderError, ProviderLyricDocument, ProviderLyricLine, parse_standard_lrc,
    plain_document,
};

const QRC_KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";
const KRC_KEY: &[u8; 16] = b"@Gaw^2tGQ61-\xce\xd2ni";

pub(super) fn decrypt_qrc_cloud(encrypted_hex: &str) -> Result<String, LyricsProviderError> {
    let mut encrypted = hex::decode(encrypted_hex.trim())
        .map_err(|error| LyricsProviderError::Decode(format!("invalid QRC hex: {error}")))?;
    if encrypted.is_empty() || encrypted.len() % 8 != 0 {
        return Err(LyricsProviderError::Decode(
            "QRC ciphertext length is not a non-zero multiple of 8".into(),
        ));
    }
    let schedule = key_setup(QRC_KEY, DECRYPT);
    for chunk in encrypted.as_chunks_mut::<8>().0 {
        let block = *chunk;
        chunk.copy_from_slice(&decrypt_block(block, &schedule));
    }
    inflate_utf8(&encrypted, "QRC")
}

pub(super) fn decrypt_krc(encoded: &str) -> Result<String, LyricsProviderError> {
    let encrypted = BASE64
        .decode(encoded)
        .map_err(|error| LyricsProviderError::Decode(format!("invalid KRC base64: {error}")))?;
    if encrypted.len() < 4 {
        return Err(LyricsProviderError::Decode(
            "KRC payload is too short".into(),
        ));
    }
    let decrypted = encrypted[4..]
        .iter()
        .enumerate()
        .map(|(index, value)| value ^ KRC_KEY[index % KRC_KEY.len()])
        .collect::<Vec<_>>();
    inflate_utf8(&decrypted, "KRC")
}

fn inflate_utf8(data: &[u8], label: &str) -> Result<String, LyricsProviderError> {
    let mut decoder = ZlibDecoder::new(data);
    let mut output = String::new();
    decoder.read_to_string(&mut output).map_err(|error| {
        LyricsProviderError::Decode(format!("{label} zlib decode failed: {error}"))
    })?;
    Ok(output)
}

pub(super) fn parse_qrc(text: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    if !text.contains("LyricContent=") {
        if has_comma_timed_lines(text) {
            return parse_qrc_content(text);
        }
        if text.contains('[') && text.contains(']') {
            return parse_standard_lrc(text);
        }
        return Ok(plain_document(text));
    }
    let content_re = Regex::new(r#"(?s)<Lyric_1 LyricType=\"1\" LyricContent=\"(.*?)\"/>"#)
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let content = content_re
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|value| xml_unescape(value.as_str()))
        .ok_or_else(|| LyricsProviderError::Parse("QRC LyricContent was not found".into()))?;
    parse_qrc_content(&content)
}

fn parse_qrc_content(content: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    let line_re = Regex::new(r"^\[(\d+),(\d+)\](.*)$")
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let suffix_word_re = Regex::new(r"(?:\[\d+,\d+\])?(.*?)\((\d+),(\d+)(?:,\d+)?\)")
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let mut lines = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        let Some(captures) = line_re.captures(line) else {
            continue;
        };
        let start_ms = parse_u64(&captures[1])?;
        let duration_ms = parse_u64(&captures[2])?;
        let body = captures.get(3).map_or("", |value| value.as_str());
        let mut text = if body.trim_start().starts_with('(') {
            parse_marker_text(body, '(', ')')
        } else {
            String::new()
        };
        if text.is_empty() {
            for word in suffix_word_re.captures_iter(body) {
                let part = word.get(1).map_or("", |value| value.as_str());
                if part != "\r" {
                    text.push_str(part);
                }
            }
        }
        if text.is_empty() {
            text = strip_qrc_word_timestamps(body);
        }
        push_line(&mut lines, start_ms, duration_ms, text);
    }
    finish_document(lines, "QRC")
}

pub(super) fn parse_yrc(text: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    let line_re = Regex::new(r"^\[(\d+),(\d+)\](.*)$")
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Some(captures) = line_re.captures(raw.trim()) else {
            continue;
        };
        let start_ms = parse_u64(&captures[1])?;
        let duration_ms = parse_u64(&captures[2])?;
        let body = captures.get(3).map_or("", |value| value.as_str());
        let text = parse_marker_text(body, '(', ')');
        push_line(
            &mut lines,
            start_ms,
            duration_ms,
            if text.is_empty() {
                body.trim().to_string()
            } else {
                text
            },
        );
    }
    finish_document(lines, "YRC")
}

pub(super) fn parse_krc(text: &str) -> Result<ProviderLyricDocument, LyricsProviderError> {
    let line_re = Regex::new(r"^\[(\d+),(\d+)\](.*)$")
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let language_re = Regex::new(r"^\[language:([^\]]+)\]$")
        .map_err(|error| LyricsProviderError::Parse(error.to_string()))?;
    let mut language_payload = None;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(captures) = language_re.captures(line) {
            language_payload = captures.get(1).map(|value| value.as_str().to_owned());
            continue;
        }
        let Some(captures) = line_re.captures(line) else {
            continue;
        };
        let start_ms = parse_u64(&captures[1])?;
        let duration_ms = parse_u64(&captures[2])?;
        let body = captures.get(3).map_or("", |value| value.as_str());
        let text = parse_marker_text(body, '<', '>');
        push_line(
            &mut lines,
            start_ms,
            duration_ms,
            if text.is_empty() {
                body.trim().to_string()
            } else {
                text
            },
        );
    }
    if lines.is_empty() {
        return Err(LyricsProviderError::Parse(
            "KRC contained no lyric lines".into(),
        ));
    }
    apply_krc_languages(&mut lines, language_payload.as_deref())?;
    Ok(ProviderLyricDocument { lines })
}

fn push_line(lines: &mut Vec<ProviderLyricLine>, start_ms: u64, duration_ms: u64, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let start = Duration::from_millis(start_ms);
    lines.push(ProviderLyricLine {
        start,
        end: start + Duration::from_millis(duration_ms.max(1)),
        text,
        translation: None,
        romanization: None,
    });
}

fn finish_document(
    lines: Vec<ProviderLyricLine>,
    label: &str,
) -> Result<ProviderLyricDocument, LyricsProviderError> {
    if lines.is_empty() {
        Err(LyricsProviderError::Parse(format!(
            "{label} contained no lyric lines"
        )))
    } else {
        Ok(ProviderLyricDocument { lines })
    }
}

pub(super) fn has_comma_timed_lines(text: &str) -> bool {
    text.lines().any(|raw| {
        let line = raw.trim_start();
        let Some(close) = line.find(']') else {
            return false;
        };
        let Some(inner) = line
            .strip_prefix('[')
            .and_then(|value| value.get(..close.saturating_sub(1)))
        else {
            return false;
        };
        let Some((start, duration)) = inner.split_once(',') else {
            return false;
        };
        !start.is_empty()
            && !duration.is_empty()
            && start.chars().all(|ch| ch.is_ascii_digit())
            && duration.chars().all(|ch| ch.is_ascii_digit())
    })
}

fn apply_krc_languages(
    lines: &mut [ProviderLyricLine],
    encoded: Option<&str>,
) -> Result<(), LyricsProviderError> {
    let Some(encoded) = encoded.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let decoded = BASE64.decode(encoded.trim()).map_err(|error| {
        LyricsProviderError::Decode(format!("invalid KRC language payload: {error}"))
    })?;
    let value: Value = serde_json::from_slice(&decoded).map_err(|error| {
        LyricsProviderError::Parse(format!("invalid KRC language JSON: {error}"))
    })?;
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return Ok(());
    };
    for language in content {
        let kind = language.get("type").and_then(Value::as_i64);
        let Some(rows) = language.get("lyricContent").and_then(Value::as_array) else {
            continue;
        };
        match kind {
            Some(0) => {
                let mut row_index = 0_usize;
                for line in lines.iter_mut() {
                    let Some(row) = rows.get(row_index).and_then(Value::as_array) else {
                        break;
                    };
                    let romanized = row
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !romanized.trim().is_empty() {
                        line.romanization = Some(romanized);
                    }
                    row_index += 1;
                }
            }
            Some(1) => {
                for (line, row) in lines.iter_mut().zip(rows) {
                    let translation = row
                        .as_array()
                        .and_then(|parts| parts.first())
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !translation.trim().is_empty() {
                        line.translation = Some(translation.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_marker_text(body: &str, open: char, close: char) -> String {
    let mut output = String::new();
    let mut cursor = 0_usize;
    while cursor < body.len() {
        let Some(open_rel) = body[cursor..].find(open) else {
            break;
        };
        let marker_start = cursor + open_rel;
        let marker_content_start = marker_start + open.len_utf8();
        let Some(close_rel) = body[marker_content_start..].find(close) else {
            break;
        };
        let marker_end = marker_content_start + close_rel;
        let mut parts = body[marker_content_start..marker_end].split(',');
        if parts.next().and_then(|value| value.parse::<u64>().ok()).is_none()
            || parts.next().and_then(|value| value.parse::<u64>().ok()).is_none()
        {
            cursor = marker_end + close.len_utf8();
            continue;
        }
        let text_start = marker_end + close.len_utf8();
        let text_end = body[text_start..]
            .find(open)
            .map_or(body.len(), |next| text_start + next);
        output.push_str(&body[text_start..text_end]);
        cursor = text_end;
    }
    output.trim().to_string()
}

fn strip_qrc_word_timestamps(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut depth = 0_u8;
    for ch in value.chars() {
        match ch {
            '(' => depth = 1,
            ')' if depth == 1 => depth = 0,
            _ if depth == 0 => output.push(ch),
            _ => {}
        }
    }
    output.trim().to_owned()
}

fn xml_unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let Some(relative_start) = value[cursor..].find('&') else {
            output.push_str(&value[cursor..]);
            break;
        };
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        let Some(relative_end) = value[start + 1..].find(';') else {
            output.push_str(&value[start..]);
            break;
        };
        let end = start + 1 + relative_end;
        let entity = &value[start + 1..end];
        if let Some(decoded) = decode_xml_entity(entity) {
            output.push(decoded);
        } else {
            output.push_str(&value[start..=end]);
        }
        cursor = end + 1;
    }
    output
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "apos" => Some('\''),
        "gt" => Some('>'),
        "lt" => Some('<'),
        "nbsp" => Some('\u{a0}'),
        "quot" => Some('"'),
        _ => entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|value| u32::from_str_radix(value, 16).ok())
            .or_else(|| entity.strip_prefix('#').and_then(|value| value.parse().ok()))
            .and_then(char::from_u32),
    }
}

fn parse_u64(value: &str) -> Result<u64, LyricsProviderError> {
    value.parse().map_err(|error| {
        LyricsProviderError::Parse(format!("invalid lyric timestamp {value:?}: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qrc_prefix_timing_does_not_leak_markup() {
        let document = parse_qrc(
            "[143040,29060](143040,210,0)我(143250,210,0)不(143460,240,0)愿(143700,220,0)再",
        )
        .unwrap();
        assert_eq!(document.lines[0].text, "我不愿再");
        assert!(!document.lines[0].text.contains('('));
    }

    #[test]
    fn krc_language_payload_adds_auxiliary_text() {
        let language = serde_json::json!({
            "content": [
                {"type": 0, "lyricContent": [["ni", "hao"]]},
                {"type": 1, "lyricContent": [["hello"]]}
            ]
        });
        let encoded = BASE64.encode(serde_json::to_vec(&language).unwrap());
        let text = format!("[language:{encoded}]\n[1000,1000]<0,400,0>你<400,600,0>好");
        let document = parse_krc(&text).unwrap();
        assert_eq!(document.lines[0].translation.as_deref(), Some("hello"));
        assert_eq!(document.lines[0].romanization.as_deref(), Some("ni hao"));
    }
}
