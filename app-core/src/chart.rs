//! Editable chart boundary for Uta Studio.
//!
//! Analyzer output is an import source. The editor loads and saves the
//! authoritative UTZ 0.2 vocal chart; target formats such as UltraStar are
//! produced from it at export time.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;
use ts_rs::TS;
use utz::VocalChartV1;

use crate::{
    audio_format::{browser_can_decode, export_extension, transcode_audio},
    authoring::get_audio_paths,
    cache::{CacheDir, normalize_tempo},
    error::UtaStudioError,
    library_db,
    vocal_chart::migrate_analyzer_chart,
};

fn playable_audio(
    cache: &CacheDir,
    file_hash: &str,
    source_name: &str,
    source_path: &str,
) -> Result<String, UtaStudioError> {
    let source = Path::new(source_path);
    if !source.is_file() {
        return Err(UtaStudioError::Other(format!(
            "audio source is missing: {}",
            source.display()
        )));
    }
    if browser_can_decode(source) {
        return Ok(source.to_string_lossy().into_owned());
    }
    let extension = export_extension(source);
    let output = cache.editor_preview_path(file_hash, source_name, extension);
    let refresh = std::fs::metadata(source)
        .and_then(|source_meta| {
            let source_modified = source_meta.modified()?;
            let output_modified = std::fs::metadata(&output)?.modified()?;
            Ok(source_modified > output_modified)
        })
        .unwrap_or(true);
    if refresh {
        let filename = output
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("preview");
        let temporary = output.with_file_name(format!(".{filename}.tmp.{extension}"));
        let result = transcode_audio(source, &temporary);
        if let Err(error) = result {
            let _ = std::fs::remove_file(temporary);
            return Err(error);
        }
        if output.is_file() {
            std::fs::remove_file(&output)?;
        }
        std::fs::rename(temporary, &output)?;
    }
    Ok(output.to_string_lossy().into_owned())
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartAudio {
    pub instrumental: String,
    pub vocals: Option<String>,
    pub original: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChartWaveform {
    pub peaks: Vec<(f32, f32)>,
    pub duration_secs: f64,
}

/// Decode a bounded, display-only waveform from an already-authorized chart
/// audio path. Callers run this while native playback is stopped so decoding
/// cannot contend with the GStreamer audition clock.
pub fn decode_chart_waveform(path: &Path) -> Result<ChartWaveform, UtaStudioError> {
    const SAMPLE_RATE: usize = 4_000;
    const PEAK_BUCKETS: usize = 6_000;

    if !path.is_file() {
        return Err(UtaStudioError::Other(format!(
            "waveform source is missing: {}",
            path.display()
        )));
    }
    let output = crate::vendor::silent_command(crate::vendor::ffmpeg_path())
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "4000", "-f", "f32le", "pipe:1"])
        .output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(UtaStudioError::Other(if detail.is_empty() {
            format!("ffmpeg could not decode waveform ({})", output.status)
        } else {
            format!("ffmpeg could not decode waveform: {detail}")
        }));
    }
    let samples = output
        .stdout
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .filter(|sample| sample.is_finite())
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return Err(UtaStudioError::Other(
            "decoded waveform contains no audio samples".to_string(),
        ));
    }

    let bucket_count = samples.len().clamp(1, PEAK_BUCKETS);
    let mut peaks = Vec::with_capacity(bucket_count);
    for bucket in 0..bucket_count {
        let start = bucket * samples.len() / bucket_count;
        let end = ((bucket + 1) * samples.len() / bucket_count).max(start + 1);
        let mut minimum = 0.0f32;
        let mut maximum = 0.0f32;
        for sample in &samples[start..end.min(samples.len())] {
            minimum = minimum.min(*sample);
            maximum = maximum.max(*sample);
        }
        peaks.push((minimum.clamp(-1.0, 1.0), maximum.clamp(-1.0, 1.0)));
    }
    Ok(ChartWaveform {
        peaks,
        duration_secs: samples.len() as f64 / SAMPLE_RATE as f64,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ChartDocument {
    pub file_hash: String,
    /// The authoritative authoring document. The editor edits this directly.
    pub vocal_chart: VocalChartV1,
    /// Optional frame-level analyzer evidence, rendered behind the notes. It is
    /// never scoring data and is never written back into the chart.
    pub pitch_track: serde_json::Value,
    pub audio: ChartAudio,
    /// Safe, in-memory repairs applied while importing analyzer output.
    /// Saving the chart persists these normalized timings.
    pub repaired_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ChartReadiness {
    pub ready: bool,
    pub authoring_ready: bool,
    pub missing: Vec<String>,
    pub blocked_reason: Option<String>,
    pub can_repair_pitch: bool,
}

pub fn chart_readiness(file_hash: &str) -> Result<ChartReadiness, UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    let only_pitch_missing = !song.authoring_missing.is_empty()
        && song
            .authoring_missing
            .iter()
            .all(|asset| matches!(asset.as_str(), "pitch_track" | "pitch_notes"));
    Ok(ChartReadiness {
        ready: song.editor_ready,
        authoring_ready: song.authoring_ready,
        missing: song.authoring_missing,
        blocked_reason: song.editor_blocked_reason,
        can_repair_pitch: only_pitch_missing
            && song.transcript_source != Some(crate::song::TranscriptSource::Usdx),
    })
}

pub fn load_chart(file_hash: &str) -> Result<ChartDocument, UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;

    if song.key_offset != 0 || normalize_tempo(song.tempo) != 1.0 {
        return Err(UtaStudioError::Other(
            "Reset key and tempo before editing the source chart".into(),
        ));
    }

    let cache = CacheDir::new();
    let pitch_track = read_json(&cache.pitch_track_path(file_hash), "pitch track")?;
    let mut repaired_issues = Vec::new();

    let vocal_chart = if cache.vocal_chart_path(file_hash).is_file() {
        let chart: VocalChartV1 =
            serde_json::from_str(&std::fs::read_to_string(cache.vocal_chart_path(file_hash))?)?;
        chart
            .validate()
            .map_err(|error| UtaStudioError::Other(error.to_string()))?;
        chart
    } else {
        // A song that has never been edited still carries only analyzer output.
        let mut transcript = read_json(&cache.transcript_path(file_hash), "transcript")?;
        let mut pitch_notes = read_json(&cache.pitch_notes_path(file_hash), "pitch notes")?;
        let repaired_timings = normalize_transcript_timings(&mut transcript);
        if repaired_timings > 0 {
            repaired_issues.push(format!(
                "Repaired {repaired_timings} analyzer lyric timing{}",
                if repaired_timings == 1 { "" } else { "s" }
            ));
        }
        let repaired_notes = normalize_pitch_note_timings(&mut pitch_notes);
        if repaired_notes > 0 {
            repaired_issues.push(format!(
                "Repaired {repaired_notes} analyzer pitch note{}",
                if repaired_notes == 1 { "" } else { "s" }
            ));
        }
        validate_transcript(&transcript)?;
        validate_pitch_notes(&pitch_notes)?;
        repaired_issues.push("Prepared the analyzer chart for UTZ 0.2 note-owned lyrics".into());
        migrate_analyzer_chart(&transcript, &pitch_notes)?
    };

    let audio = get_audio_paths(file_hash);
    if !Path::new(&audio.instrumental).is_file() {
        return Err(UtaStudioError::Other(
            "instrumental or source audio is not ready".into(),
        ));
    }

    Ok(ChartDocument {
        file_hash: file_hash.to_owned(),
        vocal_chart,
        pitch_track,
        audio: ChartAudio {
            instrumental: playable_audio(&cache, file_hash, "instrumental", &audio.instrumental)?,
            vocals: audio
                .vocals
                .as_deref()
                .map(|path| playable_audio(&cache, file_hash, "vocals", path))
                .transpose()?,
            original: playable_audio(&cache, file_hash, "original", &song.path.to_string_lossy())?,
        },
        repaired_issues,
    })
}

fn normalize_transcript_timings(value: &mut serde_json::Value) -> usize {
    let Some(segments) = value
        .get_mut("segments")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let mut repaired = 0usize;
    let mut previous_segment_start = 0.0f64;

    for segment in segments {
        let original_start = segment
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(previous_segment_start);
        let segment_start = if original_start.is_finite() {
            original_start.max(0.0).max(previous_segment_start)
        } else {
            previous_segment_start
        };
        if (original_start - segment_start).abs() > f64::EPSILON {
            segment["start"] = serde_json::Value::from(segment_start);
            repaired += 1;
        }

        let original_end = segment
            .get("end")
            .and_then(serde_json::Value::as_f64)
            .filter(|end| end.is_finite())
            .unwrap_or(segment_start);
        let mut furthest_word_end = segment_start;
        if let Some(words) = segment
            .get_mut("words")
            .and_then(serde_json::Value::as_array_mut)
        {
            let original_starts = words
                .iter()
                .map(|word| word.get("start").and_then(serde_json::Value::as_f64))
                .collect::<Vec<_>>();
            let mut previous_word_start = segment_start;
            for (index, word) in words.iter_mut().enumerate() {
                let original_word_start = word
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(previous_word_start);
                let word_start = if original_word_start.is_finite() {
                    original_word_start
                        .max(segment_start)
                        .max(previous_word_start)
                } else {
                    previous_word_start
                };
                if (original_word_start - word_start).abs() > f64::EPSILON {
                    word["start"] = serde_json::Value::from(word_start);
                    repaired += 1;
                }

                let original_word_end = word
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(word_start);
                let word_end = if original_word_end.is_finite() && original_word_end > word_start {
                    original_word_end
                } else {
                    original_starts
                        .get(index + 1)
                        .copied()
                        .flatten()
                        .filter(|next| next.is_finite() && *next > word_start)
                        .or_else(|| (original_end > word_start).then_some(original_end))
                        .unwrap_or(word_start + 0.04)
                };
                if (original_word_end - word_end).abs() > f64::EPSILON {
                    word["end"] = serde_json::Value::from(word_end);
                    repaired += 1;
                }
                previous_word_start = word_start;
                furthest_word_end = furthest_word_end.max(word_end);
            }
        }

        let segment_end = original_end
            .max(furthest_word_end)
            .max(segment_start + 0.04);
        if (original_end - segment_end).abs() > f64::EPSILON {
            segment["end"] = serde_json::Value::from(segment_end);
            repaired += 1;
        }
        previous_segment_start = segment_start;
    }
    repaired
}

fn normalize_pitch_note_timings(value: &mut serde_json::Value) -> usize {
    let Some(notes) = value
        .get_mut("notes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return 0;
    };
    let was_sorted = notes.windows(2).all(|pair| {
        pair[0]
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
            <= pair[1]
                .get("start")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
    });
    notes.sort_by(|left, right| {
        left.get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
            .total_cmp(
                &right
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0),
            )
    });
    let mut repaired = usize::from(!was_sorted);
    for note in notes {
        let original_start = note
            .get("start")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let start = if original_start.is_finite() {
            original_start.max(0.0)
        } else {
            0.0
        };
        let original_end = note
            .get("end")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(start);
        let end = if original_end.is_finite() && original_end > start {
            original_end
        } else {
            start + 0.03
        };
        let original_midi = note
            .get("midi")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(60.0);
        let midi = original_midi.clamp(0.0, 127.0).round();
        let original_confidence = note
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);
        let confidence = original_confidence.clamp(0.0, 1.0);
        let changed = (original_start - start).abs() > f64::EPSILON
            || (original_end - end).abs() > f64::EPSILON
            || (original_midi - midi).abs() > f64::EPSILON
            || (original_confidence - confidence).abs() > f64::EPSILON;
        if changed {
            repaired += 1;
        }
        note["start"] = serde_json::Value::from(start);
        note["end"] = serde_json::Value::from(end);
        note["midi"] = serde_json::Value::from(midi);
        note["confidence"] = serde_json::Value::from(confidence);
    }
    repaired
}

pub fn save_vocal_chart(file_hash: &str, vocal_chart: VocalChartV1) -> Result<(), UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    if song.key_offset != 0 || normalize_tempo(song.tempo) != 1.0 {
        return Err(UtaStudioError::Other(
            "Reset key and tempo before saving the source chart".into(),
        ));
    }
    vocal_chart
        .validate()
        .map_err(|error| UtaStudioError::Other(error.to_string()))?;

    // The chart is the only thing an edit writes. Analyzer output stays as the
    // untouched record of what the models produced.
    let cache = CacheDir::new();
    let vocal_json = serde_json::to_value(&vocal_chart)?;
    atomic_write_json(&cache.vocal_chart_path(file_hash), &vocal_json)?;
    Ok(())
}

fn read_json(path: &Path, label: &str) -> Result<serde_json::Value, UtaStudioError> {
    if !path.is_file() {
        return Err(UtaStudioError::Other(format!("{label} is not ready")));
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn atomic_write_json(destination: &Path, value: &serde_json::Value) -> Result<(), UtaStudioError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("chart.json");
    let temporary: PathBuf =
        destination.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<(), UtaStudioError> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        let bytes = serde_json::to_vec_pretty(value)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn validate_transcript(value: &serde_json::Value) -> Result<(), UtaStudioError> {
    let segments = value
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("transcript.segments must be an array".into()))?;

    let mut previous_segment_start = 0.0;
    for (segment_index, segment) in segments.iter().enumerate() {
        let start = finite_number(segment, "start", "segment", segment_index)?;
        let end = finite_number(segment, "end", "segment", segment_index)?;
        validate_range(start, end, "segment", segment_index)?;
        if segment_index > 0 && start < previous_segment_start {
            return Err(UtaStudioError::Other(format!(
                "segment {segment_index} starts before the preceding segment"
            )));
        }
        previous_segment_start = start;

        let words = segment
            .get("words")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                UtaStudioError::Other(format!("segment {segment_index}.words must be an array"))
            })?;
        let mut previous_word_start = start;
        for (word_index, word) in words.iter().enumerate() {
            let word_start = finite_number(word, "start", "word", word_index)?;
            let word_end = finite_number(word, "end", "word", word_index)?;
            validate_range(word_start, word_end, "word", word_index)?;
            if word_index > 0 && word_start < previous_word_start {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} starts before the preceding word"
                )));
            }
            if word_start < start - 0.001 || word_end > end + 0.001 {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} lies outside its segment"
                )));
            }
            if word
                .get("word")
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Err(UtaStudioError::Other(format!(
                    "segment {segment_index}, word {word_index} has no text"
                )));
            }
            previous_word_start = word_start;
        }
    }
    Ok(())
}

fn validate_pitch_notes(value: &serde_json::Value) -> Result<(), UtaStudioError> {
    let notes = value
        .get("notes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| UtaStudioError::Other("pitch_notes.notes must be an array".into()))?;
    let mut previous_start = 0.0;
    for (index, note) in notes.iter().enumerate() {
        let start = finite_number(note, "start", "note", index)?;
        let end = finite_number(note, "end", "note", index)?;
        validate_range(start, end, "note", index)?;
        if index > 0 && start < previous_start {
            return Err(UtaStudioError::Other(format!(
                "note {index} starts before the preceding note"
            )));
        }
        previous_start = start;
        let midi = finite_number(note, "midi", "note", index)?;
        if !(0.0..=127.0).contains(&midi) || midi.fract().abs() > f64::EPSILON {
            return Err(UtaStudioError::Other(format!(
                "note {index} MIDI must be an integer between 0 and 127"
            )));
        }
        let confidence = finite_number(note, "confidence", "note", index)?;
        if !(0.0..=1.0).contains(&confidence) {
            return Err(UtaStudioError::Other(format!(
                "note {index} confidence must be between 0 and 1"
            )));
        }
        if let Some(kind) = note.get("kind") {
            let kind = kind.as_str().ok_or_else(|| {
                UtaStudioError::Other(format!("note {index}.kind must be a string"))
            })?;
            if !matches!(
                kind,
                "normal" | "golden" | "freestyle" | "rap" | "golden_rap"
            ) {
                return Err(UtaStudioError::Other(format!(
                    "note {index}.kind is not a supported note type"
                )));
            }
        }
    }
    Ok(())
}

fn finite_number(
    value: &serde_json::Value,
    field: &str,
    label: &str,
    index: usize,
) -> Result<f64, UtaStudioError> {
    let number = value
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| {
            UtaStudioError::Other(format!("{label} {index}.{field} must be a number"))
        })?;
    if !number.is_finite() {
        return Err(UtaStudioError::Other(format!(
            "{label} {index}.{field} must be finite"
        )));
    }
    Ok(number)
}

fn validate_range(start: f64, end: f64, label: &str, index: usize) -> Result<(), UtaStudioError> {
    if start < 0.0 || end <= start {
        return Err(UtaStudioError::Other(format!(
            "{label} {index} must have 0 <= start < end"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_pitch_note_timings, normalize_transcript_timings, validate_pitch_notes,
        validate_transcript,
    };

    #[test]
    fn validates_editor_documents() {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "text": "hello",
                "start": 1.0,
                "end": 1.5,
                "words": [{"word": "hello", "start": 1.0, "end": 1.5}]
            }]
        });
        let notes = serde_json::json!({
            "format_version": 1,
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9}]
        });
        assert!(validate_transcript(&transcript).is_ok());
        assert!(validate_pitch_notes(&notes).is_ok());
    }

    #[test]
    fn normalizes_zero_length_timings_for_the_editor() {
        let mut transcript = serde_json::json!({
            "segments": [{
                "text": "hello world",
                "start": 1.0,
                "end": 1.0,
                "words": [
                    {"word": "hello", "start": 1.0, "end": 1.0},
                    {"word": "world", "start": 1.5, "end": 1.5}
                ]
            }]
        });
        let mut notes = serde_json::json!({
            "notes": [{"start": 2.0, "end": 2.0, "midi": 60.4, "confidence": 1.2}]
        });
        assert!(normalize_transcript_timings(&mut transcript) > 0);
        assert!(normalize_pitch_note_timings(&mut notes) > 0);
        validate_transcript(&transcript).unwrap();
        validate_pitch_notes(&notes).unwrap();
    }

    #[test]
    fn rejects_invalid_midi() {
        let notes = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 128, "confidence": 0.9}]
        });
        assert!(validate_pitch_notes(&notes).is_err());
    }

    #[test]
    fn accepts_supported_note_kinds_and_rejects_unknown_ones() {
        let supported = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9, "kind": "golden"}]
        });
        let unsupported = serde_json::json!({
            "notes": [{"start": 1.0, "end": 1.5, "midi": 60, "confidence": 0.9, "kind": "spoken"}]
        });
        assert!(validate_pitch_notes(&supported).is_ok());
        assert!(validate_pitch_notes(&unsupported).is_err());
    }
}
