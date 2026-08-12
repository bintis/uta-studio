//! UltraStar 1.1 text export.
//!
//! Uta Studio keeps a second-based authoring model. This adapter quantizes it
//! to UltraStar note beats and writes the referenced assets beside the chart.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    audio_format::{export_extension as audio_export_extension, transcode_audio},
    authoring::{get_audio_paths, load_pitch_guide, load_transcript},
    error::UtaStudioError,
    library_db,
};

const EXPORT_BPM: f64 = 300.0;
const SECONDS_PER_BEAT: f64 = 60.0 / (EXPORT_BPM * 4.0);

#[derive(Debug, Deserialize)]
struct Transcript {
    #[serde(default)]
    language: String,
    #[serde(default)]
    segments: Vec<Segment>,
}

#[derive(Debug, Deserialize)]
struct Segment {
    #[serde(default)]
    text: String,
    start: f64,
    end: f64,
    #[serde(default)]
    words: Vec<Word>,
}

#[derive(Debug, Deserialize)]
struct Word {
    word: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct PitchNote {
    start: f64,
    end: f64,
    midi: i32,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PitchNotes {
    #[serde(default)]
    notes: Vec<PitchNote>,
}

#[derive(Debug)]
struct NoteLine {
    kind: char,
    start: i64,
    duration: i64,
    pitch: i32,
    text: String,
}

#[derive(Debug)]
struct ExportWord {
    text: String,
    start: f64,
    end: f64,
}

pub fn export_ultrastar(
    file_hash: &str,
    output: impl AsRef<Path>,
) -> Result<PathBuf, UtaStudioError> {
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    let output = output.as_ref();
    if output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| !extension.eq_ignore_ascii_case("txt"))
        .unwrap_or(true)
    {
        return Err(UtaStudioError::Other(
            "UltraStar charts must use the .txt extension".into(),
        ));
    }
    if output.exists() {
        return Err(UtaStudioError::Other(format!(
            "refusing to overwrite {}",
            output.display()
        )));
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let audio = get_audio_paths(file_hash);
    let instrumental = PathBuf::from(&audio.instrumental);
    if !instrumental.is_file() {
        return Err(UtaStudioError::Other(
            "instrumental or source audio is not ready".into(),
        ));
    }
    let transcript: Transcript = serde_json::from_value(load_transcript(file_hash)?)?;
    let guide = load_pitch_guide(file_hash)?
        .ok_or_else(|| UtaStudioError::Other("pitch guide is not ready".into()))?;
    let notes: PitchNotes = serde_json::from_value(
        guide
            .get("notes")
            .cloned()
            .ok_or_else(|| UtaStudioError::Other("pitch guide has no notes".into()))?,
    )?;

    let base = output
        .file_stem()
        .and_then(|value| value.to_str())
        .map(safe_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Uta Studio Export".into());
    let instrumental_name = format!(
        "{base} - Instrumental.{}",
        export_audio_extension(&instrumental)
    );
    let instrumental_target = parent.join(&instrumental_name);

    let vocals = audio
        .vocals
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let vocals_name = vocals
        .as_ref()
        .map(|path| format!("{base} - Vocals.{}", export_audio_extension(path)));
    let vocals_target = vocals_name.as_ref().map(|name| parent.join(name));

    let cover = song.album_art_path.as_ref().filter(|path| path.is_file());
    let cover_name = cover.map(|path| format!("{base} - Cover.{}", extension_or(path, "jpg")));
    let cover_target = cover_name.as_ref().map(|name| parent.join(name));

    let video = song
        .usdx
        .as_ref()
        .and_then(|bundle| bundle.video.as_ref())
        .or_else(|| song.is_video.then_some(&song.path))
        .filter(|path| path.is_file());
    let video_name = video.map(|path| format!("{base} - Video.{}", extension_or(path, "mp4")));
    let video_target = video_name.as_ref().map(|name| parent.join(name));

    for target in [
        Some(&instrumental_target),
        vocals_target.as_ref(),
        cover_target.as_ref(),
        video_target.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if target.exists() {
            return Err(UtaStudioError::Other(format!(
                "refusing to overwrite {}",
                target.display()
            )));
        }
    }

    let chart = build_ultrastar_text(
        &sanitize_header(&song.title),
        &sanitize_header(&song.artist),
        &transcript,
        &notes.notes,
        &instrumental_name,
        vocals_name.as_deref(),
        cover_name.as_deref(),
        video_name.as_deref(),
        song.duration_secs,
    );
    crate::usdx::validate_usdx_str(&chart)?;

    let copies = [
        cover.zip(cover_target.as_ref()),
        video.zip(video_target.as_ref()),
    ];
    let mut created = Vec::new();
    let result = (|| -> Result<(), UtaStudioError> {
        materialize_audio(&instrumental, &instrumental_target)?;
        created.push(instrumental_target.clone());
        if let (Some(source), Some(target)) = (vocals.as_ref(), vocals_target.as_ref()) {
            materialize_audio(source, target)?;
            created.push(target.clone());
        }
        for (source, target) in copies.into_iter().flatten() {
            std::fs::copy(source, target)?;
            created.push(target.to_path_buf());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        file.write_all(chart.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        for path in created {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_file(output);
        return Err(error);
    }
    Ok(output.to_path_buf())
}

#[allow(clippy::too_many_arguments)]
fn build_ultrastar_text(
    title: &str,
    artist: &str,
    transcript: &Transcript,
    notes: &[PitchNote],
    audio_name: &str,
    vocals_name: Option<&str>,
    cover_name: Option<&str>,
    video_name: Option<&str>,
    duration_seconds: f64,
) -> String {
    let mut output = format!(
        "#VERSION:1.1.0\n#TITLE:{title}\n#ARTIST:{artist}\n#CREATOR:Uta Studio\n#AUDIO:{audio_name}\n#MP3:{audio_name}\n#INSTRUMENTAL:{audio_name}\n#BPM:{EXPORT_BPM:.2}\n#GAP:0\n"
    );
    if !transcript.language.trim().is_empty() {
        output.push_str(&format!(
            "#LANGUAGE:{}\n",
            sanitize_header(&transcript.language)
        ));
    }
    if let Some(name) = vocals_name {
        output.push_str(&format!("#VOCALS:{name}\n"));
    }
    if let Some(name) = cover_name {
        output.push_str(&format!("#COVER:{name}\n"));
    }
    if let Some(name) = video_name {
        output.push_str(&format!("#VIDEO:{name}\n#VIDEOGAP:0\n"));
    }
    if duration_seconds.is_finite() && duration_seconds > 0.0 {
        output.push_str(&format!(
            "#END:{}\n",
            (duration_seconds * 1000.0).round() as i64
        ));
    }

    let mut cursor = 0_i64;
    let mut has_emitted_lyrics = false;
    for segment in &transcript.segments {
        let mut lines = segment_lines(segment, notes, &transcript.language, has_emitted_lyrics);
        for line in &mut lines {
            line.start = line.start.max(cursor);
            line.duration = line.duration.max(1);
            cursor = line.start + line.duration;
            output.push_str(&format!(
                "{} {} {} {} {}\n",
                line.kind, line.start, line.duration, line.pitch, line.text
            ));
        }
        if !lines.is_empty() {
            has_emitted_lyrics = true;
            let separator = seconds_to_beat(segment.end).max(cursor);
            output.push_str(&format!("- {separator}\n"));
            cursor = separator;
        }
    }

    if transcript.segments.is_empty() {
        for note in notes {
            let start = seconds_to_beat(note.start).max(cursor);
            let end = seconds_to_beat(note.end).max(start + 1);
            output.push_str(&format!(
                "{} {start} {} {} ~\n",
                ultrastar_note_kind(note.kind.as_deref()),
                end - start,
                note.midi - 60
            ));
            cursor = end;
        }
    }
    output.push_str("E\n");
    output
}

fn segment_lines(
    segment: &Segment,
    notes: &[PitchNote],
    language: &str,
    needs_leading_space: bool,
) -> Vec<NoteLine> {
    if segment.words.is_empty() {
        let pitch = nearest_pitch(notes, (segment.start + segment.end) / 2.0);
        let text = sanitize_lyric(&segment.text);
        return vec![note_line(
            'F',
            segment.start,
            segment.end,
            pitch,
            lyric_token(
                &text,
                needs_leading_space && token_prefers_word_spacing(&text),
            ),
        )];
    }

    let compact =
        language.starts_with("zh") || language.starts_with("ja") || language.starts_with("ko");
    let export_words = export_words(segment, compact);
    let mut result = Vec::new();
    for (word_index, word) in export_words.iter().enumerate() {
        let prefers_word_spacing = !compact || token_prefers_word_spacing(&word.text);
        let lyric = lyric_token(
            &word.text,
            prefers_word_spacing && (word_index > 0 || needs_leading_space),
        );
        let overlaps: Vec<_> = notes
            .iter()
            .filter_map(|note| {
                let start = word.start.max(note.start);
                let end = word.end.min(note.end);
                (end > start + 0.005).then_some((note, start, end))
            })
            .collect();
        if overlaps.is_empty() {
            result.push(note_line(
                'F',
                word.start,
                word.end,
                nearest_pitch(notes, (word.start + word.end) / 2.0),
                lyric,
            ));
            continue;
        }
        for (index, (note, start, end)) in overlaps.into_iter().enumerate() {
            result.push(note_line(
                ultrastar_note_kind(note.kind.as_deref()),
                start,
                end,
                note.midi,
                if index == 0 {
                    lyric.clone()
                } else {
                    "~".into()
                },
            ));
        }
    }
    result.sort_by_key(|line| line.start);
    result
}

/// Analyzer word tokens can occasionally drift across segment boundaries and
/// become values such as `youCaught`. The segment text is the cleaner display
/// transcript, so use it when it contains readable word boundaries. Preserve
/// the original word timings when counts match; otherwise distribute the clean
/// tokens over the authored segment duration instead of exporting joined text.
fn export_words(segment: &Segment, compact_language: bool) -> Vec<ExportWord> {
    let clean_text = sanitize_lyric(&segment.text);
    let clean_tokens = clean_text.split_whitespace().collect::<Vec<_>>();
    let segment_text_is_worded = !clean_tokens.is_empty()
        && (!compact_language
            || clean_tokens.iter().any(|token| {
                token
                    .chars()
                    .any(|character| character.is_ascii_alphanumeric())
            }));
    if !segment_text_is_worded {
        return segment
            .words
            .iter()
            .map(|word| ExportWord {
                text: sanitize_lyric(&word.word),
                start: word.start,
                end: word.end,
            })
            .collect();
    }

    let usable_timings = clean_tokens.len() == segment.words.len()
        && segment.words.iter().all(|word| {
            word.start.is_finite()
                && word.end.is_finite()
                && word.end > word.start + 0.005
                && word.start >= segment.start - 0.05
                && word.end <= segment.end + 0.05
        });
    if usable_timings {
        return clean_tokens
            .into_iter()
            .zip(&segment.words)
            .map(|(text, word)| ExportWord {
                text: text.to_string(),
                start: word.start.max(segment.start),
                end: word.end.min(segment.end).max(word.start + 0.005),
            })
            .collect();
    }

    let start = segment.start.max(0.0);
    let end = segment.end.max(start + 0.01);
    let total_weight = clean_tokens
        .iter()
        .map(|token| token.chars().count().max(1))
        .sum::<usize>() as f64;
    let mut cursor = start;
    clean_tokens
        .into_iter()
        .enumerate()
        .map(|(index, token)| {
            let token_start = cursor;
            let token_end = if index + 1 == clean_text.split_whitespace().count() {
                end
            } else {
                cursor + (end - start) * (token.chars().count().max(1) as f64 / total_weight)
            };
            cursor = token_end;
            ExportWord {
                text: token.to_string(),
                start: token_start,
                end: token_end,
            }
        })
        .collect()
}

fn ultrastar_note_kind(kind: Option<&str>) -> char {
    match kind {
        Some("golden") => '*',
        Some("freestyle") => 'F',
        Some("rap") => 'R',
        Some("golden_rap") => 'G',
        _ => ':',
    }
}

fn note_line(kind: char, start: f64, end: f64, midi: i32, text: String) -> NoteLine {
    let beat = seconds_to_beat(start);
    let end_beat = seconds_to_beat(end).max(beat + 1);
    NoteLine {
        kind,
        start: beat,
        duration: end_beat - beat,
        pitch: midi.clamp(0, 127) - 60,
        text: if text.trim().is_empty() {
            "~".into()
        } else {
            text
        },
    }
}

fn nearest_pitch(notes: &[PitchNote], time: f64) -> i32 {
    notes
        .iter()
        .min_by(|left, right| {
            distance_to_note(left, time).total_cmp(&distance_to_note(right, time))
        })
        .map(|note| note.midi)
        .unwrap_or(60)
}

fn distance_to_note(note: &PitchNote, time: f64) -> f64 {
    if time < note.start {
        note.start - time
    } else if time > note.end {
        time - note.end
    } else {
        0.0
    }
}

fn seconds_to_beat(seconds: f64) -> i64 {
    (seconds.max(0.0) / SECONDS_PER_BEAT).round() as i64
}

fn sanitize_header(value: &str) -> String {
    value.replace(['\r', '\n'], " ").trim().to_string()
}

fn sanitize_lyric(value: &str) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn lyric_token(value: &str, needs_space: bool) -> String {
    let value = sanitize_lyric(value);
    if needs_space && !value.starts_with(' ') {
        format!(" {value}")
    } else {
        value
    }
}

fn token_prefers_word_spacing(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_ascii_alphanumeric())
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(120)
        .collect()
}

fn extension_or(path: &Path, fallback: &str) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or(fallback)
        .to_ascii_lowercase()
}

fn export_audio_extension(path: &Path) -> String {
    audio_export_extension(path).into()
}

fn materialize_audio(source: &Path, target: &Path) -> Result<(), UtaStudioError> {
    transcode_audio(source, target)
}

#[cfg(test)]
mod tests {
    use super::{PitchNote, Segment, Transcript, Word, build_ultrastar_text, ultrastar_note_kind};
    use crate::usdx::validate_usdx_str;

    #[test]
    fn exports_timed_words_and_freestyle_fallback() {
        let transcript = Transcript {
            // This mirrors a real-world language misdetection: the transcript
            // may say Japanese while its tokens are Latin-script lyrics.
            language: "ja".into(),
            segments: vec![Segment {
                text: "hello world".into(),
                start: 1.0,
                end: 2.0,
                words: vec![
                    Word {
                        word: "hello".into(),
                        start: 1.0,
                        end: 1.45,
                    },
                    Word {
                        word: "world".into(),
                        start: 1.5,
                        end: 2.0,
                    },
                ],
            }],
        };
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &transcript,
            &[PitchNote {
                start: 1.0,
                end: 1.4,
                midi: 64,
                kind: Some("golden".into()),
            }],
            "song.mp3",
            None,
            None,
            None,
            3.0,
        );
        assert!(text.contains("#VERSION:1.1.0"));
        assert!(
            text.lines()
                .any(|line| line.starts_with("* ") && line.ends_with("hello"))
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with("F ") && line.ends_with(" world"))
        );
        assert!(text.ends_with("E\n"));
        assert!(validate_usdx_str(&text).is_ok());
    }

    #[test]
    fn maps_every_supported_editor_note_kind() {
        assert_eq!(ultrastar_note_kind(Some("normal")), ':');
        assert_eq!(ultrastar_note_kind(Some("golden")), '*');
        assert_eq!(ultrastar_note_kind(Some("freestyle")), 'F');
        assert_eq!(ultrastar_note_kind(Some("rap")), 'R');
        assert_eq!(ultrastar_note_kind(Some("golden_rap")), 'G');
        assert_eq!(ultrastar_note_kind(None), ':');
    }

    #[test]
    fn keeps_spaces_between_non_compact_transcript_segments() {
        let transcript = Transcript {
            language: "en".into(),
            segments: vec![
                Segment {
                    text: "hello".into(),
                    start: 0.0,
                    end: 0.5,
                    words: vec![Word {
                        word: "hello".into(),
                        start: 0.0,
                        end: 0.5,
                    }],
                },
                Segment {
                    text: "world".into(),
                    start: 0.6,
                    end: 1.0,
                    words: vec![Word {
                        word: "world".into(),
                        start: 0.6,
                        end: 1.0,
                    }],
                },
            ],
        };
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &transcript,
            &[],
            "song.mp3",
            None,
            None,
            None,
            1.0,
        );
        assert!(text.lines().any(|line| line.ends_with(" world")));
        assert!(validate_usdx_str(&text).is_ok());
    }

    #[test]
    fn repairs_joined_word_tokens_from_clean_segment_text() {
        let transcript = Transcript {
            language: "ja".into(),
            segments: vec![Segment {
                text: "Caught up in circles".into(),
                start: 1.0,
                end: 3.0,
                words: vec![
                    Word {
                        word: "Caught".into(),
                        start: 1.0,
                        end: 1.4,
                    },
                    Word {
                        word: "up".into(),
                        start: 1.4,
                        end: 1.8,
                    },
                    Word {
                        word: "in".into(),
                        start: 1.8,
                        end: 2.2,
                    },
                    Word {
                        word: "circlesConfusion".into(),
                        start: 2.2,
                        end: 3.0,
                    },
                ],
            }],
        };
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &transcript,
            &[],
            "song.mp3",
            None,
            None,
            None,
            3.0,
        );
        assert!(text.lines().any(|line| line.ends_with(" circles")));
        assert!(!text.contains("circlesConfusion"));
        assert!(validate_usdx_str(&text).is_ok());
    }
}
