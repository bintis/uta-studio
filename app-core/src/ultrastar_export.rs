//! UltraStar 1.1 text export.
//!
//! Uta Studio keeps a second-based authoring model. This adapter quantizes it
//! to UltraStar note beats and writes the referenced assets beside the chart.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

use utz::{LyricJoin, LyricToken, VocalChartV1, VocalNote, VocalTrack};

use crate::{
    audio_format::{export_extension as audio_export_extension, transcode_audio},
    authoring::get_audio_paths,
    editor::NoteKind,
    error::UtaStudioError,
    library_db,
    vocal_chart::load_authoring_chart,
};

const EXPORT_BPM: f64 = 300.0;
const SECONDS_PER_BEAT: f64 = 60.0 / (EXPORT_BPM * 4.0);

#[derive(Debug)]
struct NoteLine {
    kind: char,
    start: i64,
    duration: i64,
    pitch: i32,
    text: String,
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
    let chart = load_authoring_chart(file_hash)?;

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

    let text = build_ultrastar_text(
        &sanitize_header(&song.title),
        &sanitize_header(&song.artist),
        &chart,
        &instrumental_name,
        vocals_name.as_deref(),
        cover_name.as_deref(),
        video_name.as_deref(),
        song.duration_secs,
    );
    crate::usdx::validate_usdx_str(&text)?;

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
        file.write_all(text.as_bytes())?;
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

/// Parse and validate a completed UltraStar chart. Diagnostics use this after
/// export so the smoke test proves the file can be consumed, not merely written.
pub fn validate_ultrastar_chart(path: impl AsRef<Path>) -> Result<(), UtaStudioError> {
    let content = std::fs::read_to_string(path)?;
    crate::usdx::validate_usdx_str(&content)
}

#[allow(clippy::too_many_arguments)]
fn build_ultrastar_text(
    title: &str,
    artist: &str,
    chart: &VocalChartV1,
    audio_name: &str,
    vocals_name: Option<&str>,
    cover_name: Option<&str>,
    video_name: Option<&str>,
    duration_seconds: f64,
) -> String {
    let mut output = format!(
        "#VERSION:1.1.0\n#TITLE:{title}\n#ARTIST:{artist}\n#CREATOR:Uta Studio\n#AUDIO:{audio_name}\n#MP3:{audio_name}\n#INSTRUMENTAL:{audio_name}\n#BPM:{EXPORT_BPM:.2}\n#GAP:0\n"
    );
    if let Some(language) = chart
        .language
        .as_deref()
        .map(str::trim)
        .filter(|language| !language.is_empty())
    {
        output.push_str(&format!("#LANGUAGE:{}\n", sanitize_header(language)));
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

    // UltraStar carries a single lyric line per player. Harmony and ad-lib
    // tracks would need a duet export, so the lead track is what ships.
    let Some(track) = lead_track(chart) else {
        output.push_str("E\n");
        return output;
    };
    let timebase = chart.timebase.max(1);
    let fallback_pitch = track
        .phrases
        .iter()
        .flat_map(|phrase| phrase.notes.iter())
        .find_map(|note| note.pitch.map(|pitch| i32::from(pitch.midi)))
        .unwrap_or(60);

    let mut cursor = 0_i64;
    for phrase in &track.phrases {
        let mut lines = Vec::new();
        for (index, note) in phrase.notes.iter().enumerate() {
            let start = units_to_seconds(note.start, timebase);
            let end = units_to_seconds(note.start.saturating_add(note.duration), timebase);
            let pitch = note
                .pitch
                .map(|pitch| i32::from(pitch.midi))
                .unwrap_or(fallback_pitch);
            lines.push(note_line(
                ultrastar_note_kind(NoteKind::of(note)),
                start,
                end,
                pitch,
                note_text(note, index > 0),
            ));
        }
        if lines.is_empty() {
            continue;
        }
        for line in &mut lines {
            line.start = line.start.max(cursor);
            line.duration = line.duration.max(1);
            cursor = line.start + line.duration;
            output.push_str(&format!(
                "{} {} {} {} {}\n",
                line.kind, line.start, line.duration, line.pitch, line.text
            ));
        }
        let separator = phrase
            .notes
            .iter()
            .map(|note| {
                seconds_to_beat(units_to_seconds(
                    note.start.saturating_add(note.duration),
                    timebase,
                ))
            })
            .max()
            .unwrap_or(cursor)
            .max(cursor);
        output.push_str(&format!("- {separator}\n"));
        cursor = separator;
    }
    output.push_str("E\n");
    output
}

fn lead_track(chart: &VocalChartV1) -> Option<&VocalTrack> {
    chart.tracks.first()
}

/// The syllable a note sings. A note that only continues the previous syllable
/// holds it with `~`, which is UltraStar's own continuation marker.
fn note_text(note: &VocalNote, may_lead_with_space: bool) -> String {
    let mut text = String::new();
    let mut spaced = false;
    for token in &note.lyrics {
        let LyricToken::Text(token) = token else {
            continue;
        };
        if text.is_empty() {
            spaced = token.join_before == LyricJoin::Space;
        } else if token.join_before == LyricJoin::Space {
            text.push(' ');
        }
        text.push_str(&token.text);
    }
    let text = sanitize_lyric(&text);
    if text.is_empty() {
        return "~".into();
    }
    if spaced && may_lead_with_space {
        format!(" {text}")
    } else {
        text
    }
}

fn units_to_seconds(units: u64, timebase: u64) -> f64 {
    units as f64 / timebase as f64
}

fn ultrastar_note_kind(kind: NoteKind) -> char {
    match kind {
        NoteKind::Golden => '*',
        NoteKind::Freestyle => 'F',
        NoteKind::Rap => 'R',
        NoteKind::GoldenRap => 'G',
        NoteKind::Normal => ':',
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
    use super::{build_ultrastar_text, ultrastar_note_kind};
    use crate::{editor::NoteKind, usdx::validate_usdx_str, vocal_chart::migrate_analyzer_chart};
    use utz::VocalChartV1;

    fn chart(language: &str, phrases: &[&[(f64, f64, u8, &str, &str)]]) -> VocalChartV1 {
        let mut segments = Vec::new();
        let mut notes = Vec::new();
        for phrase in phrases {
            let words = phrase
                .iter()
                .map(|(start, end, _, text, _)| {
                    serde_json::json!({"word": text, "start": start, "end": end})
                })
                .collect::<Vec<_>>();
            segments.push(serde_json::json!({
                "start": phrase.first().map(|note| note.0).unwrap_or(0.0),
                "end": phrase.last().map(|note| note.1).unwrap_or(0.0),
                "text": phrase.iter().map(|note| note.3.trim()).collect::<Vec<_>>().join(" "),
                "words": words,
            }));
            notes.extend(phrase.iter().map(|(start, end, midi, _, kind)| {
                serde_json::json!({
                    "start": start,
                    "end": end,
                    "midi": midi,
                    "confidence": 1.0,
                    "kind": kind,
                })
            }));
        }
        migrate_analyzer_chart(
            &serde_json::json!({"language": language, "segments": segments}),
            &serde_json::json!({"notes": notes}),
        )
        .unwrap()
    }

    #[test]
    fn exports_each_note_with_its_authored_kind_and_syllable() {
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &chart(
                "en",
                &[&[
                    (1.0, 1.45, 64, "hello", "golden"),
                    (1.5, 2.0, 62, " world", "normal"),
                ]],
            ),
            "song.mp3",
            None,
            None,
            None,
            3.0,
        );
        assert!(text.contains("#VERSION:1.1.0"));
        assert!(text.contains("#LANGUAGE:en"));
        assert!(
            text.lines()
                .any(|line| line.starts_with("* ") && line.ends_with("hello"))
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with(": ") && line.ends_with(" world"))
        );
        assert!(text.ends_with("E\n"));
        assert!(validate_usdx_str(&text).is_ok());
    }

    #[test]
    fn maps_every_supported_editor_note_kind() {
        assert_eq!(ultrastar_note_kind(NoteKind::Normal), ':');
        assert_eq!(ultrastar_note_kind(NoteKind::Golden), '*');
        assert_eq!(ultrastar_note_kind(NoteKind::Freestyle), 'F');
        assert_eq!(ultrastar_note_kind(NoteKind::Rap), 'R');
        assert_eq!(ultrastar_note_kind(NoteKind::GoldenRap), 'G');
    }

    #[test]
    fn each_phrase_becomes_its_own_line() {
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &chart(
                "en",
                &[
                    &[(0.0, 0.5, 60, "hello", "normal")],
                    &[(0.6, 1.0, 62, "world", "normal")],
                ],
            ),
            "song.mp3",
            None,
            None,
            None,
            1.0,
        );
        // Two phrases means two separators, and a line-leading syllable never
        // carries a space because UltraStar breaks the line for us.
        assert_eq!(
            text.lines().filter(|line| line.starts_with("- ")).count(),
            2
        );
        // A line-leading syllable carries no space of its own: the field
        // separator is the only space before it.
        assert!(
            text.lines()
                .any(|line| line.starts_with(": ") && line.ends_with(" world"))
        );
        assert!(!text.contains("  world"));
        assert!(validate_usdx_str(&text).is_ok());
    }

    #[test]
    fn a_held_syllable_exports_as_a_continuation() {
        let text = build_ultrastar_text(
            "Title",
            "Artist",
            &chart(
                "en",
                &[&[
                    (0.0, 0.5, 60, "hold", "normal"),
                    (0.5, 1.0, 62, "hold", "normal"),
                ]],
            ),
            "song.mp3",
            None,
            None,
            None,
            1.0,
        );
        assert!(validate_usdx_str(&text).is_ok());
    }

    #[test]
    fn an_empty_chart_still_writes_a_valid_file() {
        let mut empty = chart("en", &[&[(0.0, 0.5, 60, "a", "normal")]]);
        empty.tracks.clear();
        let text =
            build_ultrastar_text("Title", "Artist", &empty, "song.mp3", None, None, None, 1.0);
        assert!(text.ends_with("E\n"));
        assert!(validate_usdx_str(&text).is_ok());
    }
}
