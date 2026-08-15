//! UTZ 0.2 vocal-chart authoring model and analyzer import.

use serde_json::Value;

use crate::{
    authoring::{load_pitch_guide, load_transcript},
    cache::CacheDir,
};
use utz::{
    DEFAULT_TIMEBASE, LyricJoin, LyricTextToken, LyricToken, NoteBonus, NotePitch, NoteScoring,
    ScoringMode, VocalChartV1, VocalMode, VocalNote, VocalPhrase, VocalTrack, VocalTrackRole,
};

use crate::error::UtaStudioError;

#[derive(Clone)]
struct AnalyzerWord {
    segment: usize,
    id: String,
    text: String,
    start: f64,
    end: f64,
}

#[derive(Clone)]
struct MigratedNote {
    phrase: Option<usize>,
    note: VocalNote,
}

/// Loads the authoritative vocal chart for a song.
///
/// A saved chart is authority: it carries the author's phrase structure, lyric
/// tokens, and per-note scoring intent, none of which survive a re-migration.
/// Only a song that has never been edited falls back to migrating analyzer
/// output.
pub(crate) fn load_authoring_chart(file_hash: &str) -> Result<VocalChartV1, UtaStudioError> {
    let cache = CacheDir::new();
    let path = cache.vocal_chart_path(file_hash);
    if path.is_file() {
        let chart: VocalChartV1 = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        chart
            .validate()
            .map_err(|error| UtaStudioError::Other(error.to_string()))?;
        return Ok(chart);
    }

    let transcript = load_transcript(file_hash)?;
    let guide = load_pitch_guide(file_hash)?
        .ok_or_else(|| UtaStudioError::Other("pitch track and guide notes are not ready".into()))?;
    let notes = guide
        .get("notes")
        .ok_or_else(|| UtaStudioError::Other("pitch guide has no notes".into()))?;
    migrate_analyzer_chart(&transcript, notes)
}

pub fn migrate_analyzer_chart(
    transcript: &Value,
    pitch_notes: &Value,
) -> Result<VocalChartV1, UtaStudioError> {
    let language = transcript
        .get("language")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let segments = transcript
        .get("segments")
        .and_then(Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("transcript.segments must be an array".into()))?;
    let words = analyzer_words(segments);
    let mut notes = analyzer_notes(pitch_notes)?;

    for word in words {
        let overlapping = notes
            .iter()
            .enumerate()
            .filter(|(_, note)| {
                let start = units_to_seconds(note.note.start);
                let end = units_to_seconds(note.note.start + note.note.duration);
                start < word.end && end > word.start
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if overlapping.is_empty() {
            notes.push(MigratedNote {
                phrase: Some(word.segment),
                note: VocalNote {
                    id: format!("note-lyric-{}", word.id),
                    start: seconds_to_units(word.start),
                    duration: duration_to_units(word.start, word.end),
                    pitch: None,
                    vocal_mode: VocalMode::Spoken,
                    bonus: NoteBonus::Normal,
                    scoring: NoteScoring {
                        mode: ScoringMode::Rhythm,
                        weight: 1.0,
                    },
                    lyrics: vec![LyricToken::Text(text_token(&word))],
                },
            });
            continue;
        }

        let first = overlapping[0];
        notes[first].phrase.get_or_insert(word.segment);
        notes[first]
            .note
            .lyrics
            .push(LyricToken::Text(text_token(&word)));
        for index in overlapping.into_iter().skip(1) {
            notes[index].phrase.get_or_insert(word.segment);
            notes[index].note.lyrics.push(LyricToken::Continuation {
                continuation_of: word.id.clone(),
            });
        }
    }

    notes.sort_by_key(|note| note.note.start);
    assign_orphan_phrases(&mut notes, segments);
    ensure_non_overlapping(&notes)?;
    if notes.is_empty() {
        return Err(UtaStudioError::Other(
            "the transcript and pitch guide contain no editable notes".into(),
        ));
    }

    let mut phrases = Vec::<VocalPhrase>::new();
    let mut current_segment = None;
    let mut run = 0usize;
    for migrated in notes {
        if current_segment != migrated.phrase || phrases.is_empty() {
            current_segment = migrated.phrase;
            run += 1;
            let label = current_segment
                .map(|segment| format!("{}", segment + 1))
                .unwrap_or_else(|| "guide".into());
            phrases.push(VocalPhrase {
                id: format!("phrase-{label}-{run}"),
                notes: Vec::new(),
            });
        }
        phrases
            .last_mut()
            .expect("a phrase was created")
            .notes
            .push(migrated.note);
    }

    let mut chart = VocalChartV1::new(vec![VocalTrack {
        id: "lead".into(),
        role: VocalTrackRole::Lead,
        singer: None,
        scoring_enabled: true,
        phrases,
    }]);
    chart.language = language;
    chart
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;
    Ok(chart)
}

fn analyzer_words(segments: &[Value]) -> Vec<AnalyzerWord> {
    let mut result = Vec::new();
    for (segment_index, segment) in segments.iter().enumerate() {
        let segment_start = number(segment, "start").unwrap_or(0.0);
        let segment_end = number(segment, "end").unwrap_or(segment_start + 0.04);
        let words = segment.get("words").and_then(Value::as_array);
        if let Some(words) = words.filter(|words| !words.is_empty()) {
            result.extend(words.iter().enumerate().filter_map(|(word_index, word)| {
                let text = word.get("word")?.as_str()?.to_string();
                let start = number(word, "start").unwrap_or(segment_start);
                let end = number(word, "end")
                    .unwrap_or(segment_end)
                    .max(start + 0.001);
                Some(AnalyzerWord {
                    segment: segment_index,
                    id: word
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            format!("lyric-{}-{}", segment_index + 1, word_index + 1)
                        }),
                    text,
                    start,
                    end,
                })
            }));
        } else if let Some(text) = segment.get("text").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            result.push(AnalyzerWord {
                segment: segment_index,
                id: format!("lyric-{}-1", segment_index + 1),
                text: text.to_string(),
                start: segment_start,
                end: segment_end.max(segment_start + 0.001),
            });
        }
    }
    result
}

fn analyzer_notes(value: &Value) -> Result<Vec<MigratedNote>, UtaStudioError> {
    let values = value
        .get("notes")
        .and_then(Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("pitch_notes.notes must be an array".into()))?;
    Ok(values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let start = number(value, "start")?;
            let end = number(value, "end")?;
            let midi = number(value, "midi")?.round().clamp(0.0, 127.0) as u8;
            let kind = value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("normal");
            let (vocal_mode, bonus, scoring_mode) = match kind {
                "golden" => (VocalMode::Pitched, NoteBonus::Golden, ScoringMode::Pitch),
                "rap" => (VocalMode::Rap, NoteBonus::Normal, ScoringMode::Rhythm),
                "golden_rap" => (VocalMode::Rap, NoteBonus::Golden, ScoringMode::Rhythm),
                "freestyle" => (VocalMode::Freestyle, NoteBonus::Normal, ScoringMode::None),
                _ => (VocalMode::Pitched, NoteBonus::Normal, ScoringMode::Pitch),
            };
            Some(MigratedNote {
                phrase: None,
                note: VocalNote {
                    id: value
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("note-{}", index + 1)),
                    start: seconds_to_units(start),
                    duration: duration_to_units(start, end),
                    pitch: Some(NotePitch { midi, cents: 0 }),
                    vocal_mode,
                    bonus,
                    scoring: NoteScoring {
                        mode: scoring_mode,
                        weight: 1.0,
                    },
                    lyrics: Vec::new(),
                },
            })
        })
        .collect())
}

fn assign_orphan_phrases(notes: &mut [MigratedNote], segments: &[Value]) {
    for migrated in notes.iter_mut().filter(|note| note.phrase.is_none()) {
        let midpoint = units_to_seconds(migrated.note.start + migrated.note.duration / 2);
        migrated.phrase = segments
            .iter()
            .enumerate()
            .filter_map(|(index, segment)| {
                let start = number(segment, "start")?;
                let end = number(segment, "end")?;
                let distance = if midpoint < start {
                    start - midpoint
                } else if midpoint > end {
                    midpoint - end
                } else {
                    0.0
                };
                Some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index);
    }
}

fn ensure_non_overlapping(notes: &[MigratedNote]) -> Result<(), UtaStudioError> {
    for pair in notes.windows(2) {
        let left_end = pair[0].note.start.saturating_add(pair[0].note.duration);
        if pair[1].note.start < left_end {
            return Err(UtaStudioError::Other(format!(
                "notes {} and {} overlap; move one note or place harmony on another track",
                pair[0].note.id, pair[1].note.id
            )));
        }
    }
    Ok(())
}

fn text_token(word: &AnalyzerWord) -> LyricTextToken {
    let leading_space = word.text.chars().next().is_some_and(char::is_whitespace);
    LyricTextToken {
        id: word.id.clone(),
        text: word.text.trim().to_string(),
        join_before: if leading_space {
            LyricJoin::Space
        } else {
            LyricJoin::None
        },
        reading: None,
        phonemes: None,
    }
}

fn number(value: &Value, field: &str) -> Option<f64> {
    value
        .get(field)?
        .as_f64()
        .filter(|number| number.is_finite())
}

fn seconds_to_units(seconds: f64) -> u64 {
    (seconds.max(0.0) * DEFAULT_TIMEBASE as f64).round() as u64
}

fn duration_to_units(start: f64, end: f64) -> u64 {
    seconds_to_units((end - start).max(1.0 / DEFAULT_TIMEBASE as f64)).max(1)
}

fn units_to_seconds(value: u64) -> f64 {
    units_to_seconds_with_timebase(value, DEFAULT_TIMEBASE)
}

fn units_to_seconds_with_timebase(value: u64, timebase: u64) -> f64 {
    value as f64 / timebase.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::migrate_analyzer_chart;
    use utz::{LyricToken, ScoringMode};

    #[test]
    fn migrates_note_owned_lyrics_and_unpitched_words() {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "hello world",
                "start": 1.0,
                "end": 3.0,
                "words": [
                    {"word": "hello", "start": 1.0, "end": 1.5},
                    {"word": " world", "start": 2.0, "end": 2.5}
                ]
            }]
        });
        let notes = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 1.0}]
        });
        let chart = migrate_analyzer_chart(&transcript, &notes).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        assert!(matches!(notes[0].lyrics[0], LyricToken::Text(_)));
        assert!(notes[1].pitch.is_none());
        assert_eq!(notes[1].scoring.mode, ScoringMode::Rhythm);
        chart.validate().unwrap();
    }

    #[test]
    fn rejects_overlapping_analyzer_notes() {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "a b",
                "start": 0.0,
                "end": 2.0,
                "words": [
                    {"word": "a", "start": 0.0, "end": 1.0},
                    {"word": "b", "start": 1.0, "end": 2.0}
                ]
            }]
        });
        let notes = serde_json::json!({
            "notes": [
                {"start": 0.0, "end": 1.2, "midi": 60, "confidence": 1.0},
                {"start": 1.0, "end": 2.0, "midi": 62, "confidence": 1.0}
            ]
        });
        let error = migrate_analyzer_chart(&transcript, &notes)
            .expect_err("overlapping analyzer notes must not import silently");
        assert!(
            error.to_string().contains("overlap"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn word_spanning_two_notes_becomes_a_resolvable_continuation() {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "long",
                "start": 0.0,
                "end": 2.0,
                "words": [{"word": "long", "start": 0.0, "end": 2.0}]
            }]
        });
        let notes = serde_json::json!({
            "notes": [
                {"start": 0.0, "end": 1.0, "midi": 60, "confidence": 1.0},
                {"start": 1.0, "end": 2.0, "midi": 64, "confidence": 1.0}
            ]
        });
        let chart = migrate_analyzer_chart(&transcript, &notes).unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 2);
        let LyricToken::Text(head) = &notes[0].lyrics[0] else {
            panic!("the first note must own the text token");
        };
        assert_eq!(head.text, "long");
        let LyricToken::Continuation { continuation_of } = &notes[1].lyrics[0] else {
            panic!("the held note must carry a continuation token");
        };
        assert_eq!(continuation_of, &head.id);
        // The reference must resolve inside the same track.
        chart.validate().unwrap();
    }
}
