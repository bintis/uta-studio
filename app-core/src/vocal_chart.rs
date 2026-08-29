//! UTZ 0.3 vocal-chart authoring model and analyzer import.

use serde_json::Value;

use crate::{
    analysis_artifact::{load_active_artifact, validate_artifact_revision_file},
    analysis_graph::ArtifactKind,
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
    /// Pronunciation the aligner recovered, such as the kana the MMS karaoke
    /// backend emits for Japanese. The format keeps it beside the display
    /// text rather than replacing it.
    reading: Option<String>,
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
    if let Some(chart) = load_saved_or_candidate_chart(file_hash)? {
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

/// Resolves a complete chart without falling back to the legacy
/// transcript-plus-pitch-note bundle. Active immutable revisions are read
/// directly so an Engine publication does not depend on compatibility files.
pub(crate) fn load_saved_or_candidate_chart(
    file_hash: &str,
) -> Result<Option<VocalChartV1>, UtaStudioError> {
    let cache = CacheDir::new();

    if let Some(chart) = load_active_chart(file_hash, ArtifactKind::AuthoredChart)? {
        return Ok(Some(chart));
    }
    let authored_path = cache.vocal_chart_path(file_hash);
    if authored_path.is_file() {
        return load_chart_path(&authored_path).map(Some);
    }

    if let Some(chart) = load_active_chart(file_hash, ArtifactKind::CandidateChart)? {
        return Ok(Some(chart));
    }
    let candidate_path = cache.candidate_chart_path(file_hash);
    if candidate_path.is_file() {
        return load_chart_path(&candidate_path).map(Some);
    }

    Ok(None)
}

fn load_active_chart(
    file_hash: &str,
    kind: ArtifactKind,
) -> Result<Option<VocalChartV1>, UtaStudioError> {
    let Some(revision) = load_active_artifact(file_hash, kind) else {
        return Ok(None);
    };
    if revision.invalidated {
        return Err(UtaStudioError::Other(format!(
            "active {kind:?} revision is invalidated"
        )));
    }
    validate_artifact_revision_file(&revision).map_err(UtaStudioError::Other)?;
    load_chart_path(&revision.path).map(Some)
}

pub(crate) fn validate_candidate_chart_path(path: &std::path::Path) -> Result<(), UtaStudioError> {
    load_chart_path(path).map(|_| ())
}

fn load_chart_path(path: &std::path::Path) -> Result<VocalChartV1, UtaStudioError> {
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    if value.get("contract").and_then(Value::as_str)
        == Some("uta.analysis-engine.candidate-vocal-chart")
    {
        return migrate_engine_candidate_chart(&value);
    }
    if value.get("format").and_then(Value::as_str) == Some(utz::VOCAL_CHART_FORMAT)
        && value
            .get("format_version")
            .and_then(Value::as_str)
            .is_some_and(|version| version.starts_with("0.2."))
    {
        value["format_version"] = Value::String(utz::VOCAL_CHART_VERSION.to_string());
        value["timebase"] = Value::from(utz::UTZ_TIMEBASE);
    }
    let chart: VocalChartV1 = serde_json::from_value(value)?;
    chart
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;
    Ok(chart)
}

/// Projects the Engine-owned canonical candidate into the strict UTZ 0.3
/// authoring chart. Canonical regions without a word remain analysis evidence;
/// UTZ notes must own real lyric tokens, so they are not fabricated as lyrics.
pub(crate) fn migrate_engine_candidate_chart(
    candidate: &Value,
) -> Result<VocalChartV1, UtaStudioError> {
    if candidate.get("contract").and_then(Value::as_str)
        != Some("uta.analysis-engine.candidate-vocal-chart")
        || candidate.get("version").and_then(Value::as_u64) != Some(1)
        || candidate.get("timebase").and_then(Value::as_u64) != Some(utz::UTZ_TIMEBASE)
    {
        return Err(UtaStudioError::Other(
            "Engine candidate chart contract is invalid".to_string(),
        ));
    }
    let words = candidate
        .get("words")
        .and_then(Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("Engine candidate words are missing".into()))?;
    let canonical_notes = candidate
        .get("notes")
        .and_then(Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("Engine candidate notes are missing".into()))?;

    let mut notes_by_word = std::collections::BTreeMap::<String, Vec<&Value>>::new();
    for note in canonical_notes {
        if let Some(word_id) = note
            .get("word_id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
        {
            notes_by_word
                .entry(word_id.to_string())
                .or_default()
                .push(note);
        }
    }
    let mut notes = Vec::new();
    for word in words {
        let word_id = required_string(word, "word_id", "Engine candidate word")?;
        let text = required_string(word, "text", "Engine candidate word")?;
        let range = required_range(word, "Engine candidate word")?;
        let lyric_id = format!("lyric-{word_id}");
        let mut word_notes = notes_by_word.remove(&word_id).unwrap_or_default();
        word_notes.sort_by_key(|note| {
            note.get("range")
                .and_then(|range| range.get("start"))
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX)
        });
        if word_notes.is_empty() {
            notes.push(VocalNote {
                id: format!("note-{word_id}"),
                start: range.0,
                duration: range.1 - range.0,
                pitch: None,
                vocal_mode: VocalMode::Spoken,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Rhythm,
                    weight: 1.0,
                },
                lyrics: vec![LyricToken::Text(LyricTextToken {
                    id: lyric_id,
                    text: text.clone(),
                    join_before: lyric_join_for(&notes, &text),
                    reading: None,
                    phonemes: None,
                })],
            });
            continue;
        }
        for (index, note) in word_notes.into_iter().enumerate() {
            let note_range = required_range(note, "Engine candidate note")?;
            let midi = note
                .get("midi_note")
                .and_then(Value::as_u64)
                .filter(|midi| *midi <= 127)
                .ok_or_else(|| UtaStudioError::Other("Engine candidate MIDI is invalid".into()))?
                as u8;
            let cents = note
                .get("center_offset_cents")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or(0.0)
                .round()
                .clamp(-99.0, 99.0) as i8;
            let lyrics = if index == 0 {
                vec![LyricToken::Text(LyricTextToken {
                    id: lyric_id.clone(),
                    text: text.clone(),
                    join_before: lyric_join_for(&notes, &text),
                    reading: None,
                    phonemes: None,
                })]
            } else {
                vec![LyricToken::Continuation {
                    continuation_of: lyric_id.clone(),
                }]
            };
            notes.push(VocalNote {
                id: required_string(note, "id", "Engine candidate note")?,
                start: note_range.0,
                duration: note_range.1 - note_range.0,
                pitch: Some(NotePitch { midi, cents }),
                vocal_mode: VocalMode::Pitched,
                bonus: NoteBonus::Normal,
                scoring: NoteScoring {
                    mode: ScoringMode::Pitch,
                    weight: 1.0,
                },
                lyrics,
            });
        }
    }
    notes.sort_by_key(|note| note.start);
    if notes.is_empty() {
        return Err(UtaStudioError::Other(
            "Engine candidate contains no lyric-owned notes".to_string(),
        ));
    }
    let mut chart = VocalChartV1::new(vec![VocalTrack {
        id: "lead".into(),
        role: VocalTrackRole::Lead,
        part: None,
        singer: None,
        scoring_enabled: true,
        phrases: vec![VocalPhrase {
            id: "phrase-1".into(),
            notes,
        }],
    }]);
    chart.language = candidate
        .get("transcript")
        .and_then(|value| value.get("language"))
        .and_then(Value::as_str)
        .map(str::to_string);
    chart
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;
    Ok(chart)
}

fn required_string(value: &Value, field: &str, label: &str) -> Result<String, UtaStudioError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| UtaStudioError::Other(format!("{label} has invalid {field}")))
}

fn required_range(value: &Value, label: &str) -> Result<(u64, u64), UtaStudioError> {
    let range = value
        .get("range")
        .and_then(Value::as_object)
        .ok_or_else(|| UtaStudioError::Other(format!("{label} has no range")))?;
    let start = range.get("start").and_then(Value::as_u64);
    let end = range.get("end").and_then(Value::as_u64);
    match (start, end) {
        (Some(start), Some(end)) if end > start => Ok((start, end)),
        _ => Err(UtaStudioError::Other(format!(
            "{label} has an invalid range"
        ))),
    }
}

fn lyric_join_for(existing: &[VocalNote], text: &str) -> LyricJoin {
    let previous_is_ascii_word = existing
        .iter()
        .rev()
        .flat_map(|note| note.lyrics.iter().rev())
        .find_map(|token| match token {
            LyricToken::Text(token) => Some(token.text.chars().all(|character| {
                character.is_ascii_alphanumeric() || character.is_ascii_punctuation()
            })),
            LyricToken::Continuation { .. } => None,
        })
        == Some(true);
    let current_is_ascii_word = text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character.is_ascii_punctuation());
    if previous_is_ascii_word && current_is_ascii_word {
        LyricJoin::Space
    } else {
        LyricJoin::None
    }
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

    // UTZ 0.3 notes own lyric tokens. Analyzer-only pitch regions that do not
    // overlap any lyric are evidence, not chart notes; the separate pitch
    // evidence asset preserves their continuous F0 without inventing text.
    notes.retain(|note| !note.note.lyrics.is_empty());
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
        part: None,
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

/// Converts the analyzer's frame-level f0 track into the format's fixed-hop
/// pitch evidence.
///
/// Evidence is an editor aid and a visualization source, never scoring data:
/// the chart's authored note targets stay authoritative, and nothing here is
/// ever written back into a note.
pub fn migrate_pitch_evidence(track: &Value) -> Option<utz::PitchEvidenceV1> {
    let frames = track.get("frames")?.as_array()?;
    let first = frames.first()?;
    let start_seconds = number(first, "time")?;
    let hop_seconds = number(track, "hop_seconds")
        .filter(|hop| *hop > 0.0)
        .or_else(|| {
            // Fall back to the spacing of the first two frames when the
            // analyzer did not record its hop.
            let second = number(frames.get(1)?, "time")?;
            Some(second - start_seconds).filter(|hop| *hop > 0.0)
        })?;
    let hop = seconds_to_units(hop_seconds).max(1);
    let start = seconds_to_units(start_seconds);

    // Place each frame on the fixed grid rather than assuming the analyzer
    // emitted an unbroken run, so a gap reads as unvoiced instead of shifting
    // everything after it.
    let mut frequency_hz: Vec<Option<f64>> = Vec::with_capacity(frames.len());
    let mut confidence: Vec<f64> = Vec::with_capacity(frames.len());
    for frame in frames {
        let Some(time) = number(frame, "time") else {
            continue;
        };
        let slot = (((seconds_to_units(time).saturating_sub(start)) as f64) / hop as f64).round();
        if !slot.is_finite() || slot < 0.0 {
            continue;
        }
        let slot = slot as usize;
        if slot >= frequency_hz.len() {
            frequency_hz.resize(slot + 1, None);
            confidence.resize(slot + 1, 0.0);
        }
        frequency_hz[slot] = number(frame, "hz").filter(|hz| *hz > 0.0);
        confidence[slot] = number(frame, "confidence").unwrap_or(0.0).clamp(0.0, 1.0);
    }
    if frequency_hz.is_empty() {
        return None;
    }

    let evidence = utz::PitchEvidenceV1 {
        format: utz::PITCH_EVIDENCE_FORMAT.to_string(),
        format_version: utz::PITCH_EVIDENCE_VERSION.to_string(),
        timebase: DEFAULT_TIMEBASE,
        start,
        hop,
        frequency_hz,
        confidence,
        model: track
            .get("model")
            .and_then(Value::as_object)
            .cloned()
            .filter(|model| !model.is_empty()),
    };
    evidence.validate().ok().map(|()| evidence)
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
                    reading: word
                        .get("reading")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|reading| !reading.is_empty())
                        .map(str::to_owned),
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
                reading: None,
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
        reading: word.reading.clone(),
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
    use super::{migrate_analyzer_chart, migrate_engine_candidate_chart, migrate_pitch_evidence};
    use utz::{DEFAULT_TIMEBASE, LyricToken, ScoringMode};

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
    fn engine_candidate_projects_only_lyric_owned_regions_to_strict_utz() {
        let candidate = serde_json::json!({
            "contract": "uta.analysis-engine.candidate-vocal-chart",
            "version": 1,
            "timebase": 1_000_000,
            "transcript": {"language": "en"},
            "words": [
                {"word_id": "word-1", "text": "hello", "range": {"start": 1_000_000, "end": 2_000_000}},
                {"word_id": "word-2", "text": "world", "range": {"start": 3_000_000, "end": 4_000_000}}
            ],
            "notes": [
                {"id": "evidence-only", "range": {"start": 0, "end": 500_000}, "midi_note": 50, "center_offset_cents": 0.0, "word_id": null},
                {"id": "hello-1", "range": {"start": 1_000_000, "end": 1_500_000}, "midi_note": 60, "center_offset_cents": 12.0, "word_id": "word-1"},
                {"id": "hello-2", "range": {"start": 1_500_000, "end": 2_000_000}, "midi_note": 62, "center_offset_cents": -8.0, "word_id": "word-1"}
            ]
        });
        let chart = migrate_engine_candidate_chart(&candidate).unwrap();
        chart.validate().unwrap();
        let notes = &chart.tracks[0].phrases[0].notes;
        assert_eq!(notes.len(), 3);
        assert!(notes.iter().all(|note| note.id != "evidence-only"));
        assert!(matches!(
            notes[1].lyrics[0],
            LyricToken::Continuation { .. }
        ));
        assert!(notes[2].pitch.is_none());
        let LyricToken::Text(world) = &notes[2].lyrics[0] else {
            panic!("the unpitched word must own text")
        };
        assert_eq!(world.join_before, utz::LyricJoin::Space);
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

    #[test]
    fn pitch_evidence_carries_the_analyzer_grid_and_marks_unvoiced_frames() {
        let track = serde_json::json!({
            "hop_seconds": 0.01,
            "model": {"id": "crepe", "version": "1"},
            "frames": [
                {"time": 0.0, "hz": 220.0, "confidence": 0.9},
                {"time": 0.01, "hz": null, "confidence": 0.1},
                {"time": 0.02, "hz": 440.0, "confidence": 0.8},
            ],
        });
        let evidence = migrate_pitch_evidence(&track).expect("evidence");
        evidence.validate().expect("valid evidence");
        assert_eq!(evidence.start, 0);
        assert_eq!(evidence.hop, DEFAULT_TIMEBASE / 100);
        assert_eq!(evidence.frequency_hz, [Some(220.0), None, Some(440.0)]);
        assert_eq!(evidence.confidence, [0.9, 0.1, 0.8]);
        assert_eq!(
            evidence
                .model
                .as_ref()
                .and_then(|model| model.get("id"))
                .and_then(serde_json::Value::as_str),
            Some("crepe")
        );
    }

    #[test]
    fn a_gap_in_the_analyzer_frames_reads_as_unvoiced_rather_than_shifting_time() {
        let track = serde_json::json!({
            "hop_seconds": 0.01,
            "frames": [
                {"time": 1.0, "hz": 220.0, "confidence": 0.9},
                {"time": 1.03, "hz": 330.0, "confidence": 0.7},
            ],
        });
        let evidence = migrate_pitch_evidence(&track).expect("evidence");
        evidence.validate().expect("valid evidence");
        // The evidence starts where the frames do, and the missing two hops
        // stay in place as unvoiced.
        assert_eq!(evidence.start, DEFAULT_TIMEBASE);
        assert_eq!(
            evidence.frequency_hz,
            [Some(220.0), None, None, Some(330.0)]
        );
    }

    #[test]
    fn a_track_with_no_frames_produces_no_evidence_asset() {
        assert!(migrate_pitch_evidence(&serde_json::json!({"frames": []})).is_none());
        assert!(migrate_pitch_evidence(&serde_json::json!({})).is_none());
    }
}
