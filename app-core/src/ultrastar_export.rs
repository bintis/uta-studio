//! UltraStar 1.1 text export.
//!
//! Uta! Studio keeps a second-based authoring model. This adapter quantizes it
//! to UltraStar note beats and writes the referenced assets beside the chart.

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use utz::{LyricJoin, LyricToken, VocalChartV1, VocalNote, VocalTrack, VocalTrackRole};

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
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct UltraStarExportStaging {
    root: PathBuf,
}

impl UltraStarExportStaging {
    fn create(output: &Path) -> Result<Self, UtaStudioError> {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        let stem = output
            .file_stem()
            .and_then(|value| value.to_str())
            .map(safe_component)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "chart".to_string());
        for _ in 0..32 {
            let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = parent.join(format!(
                ".uta-studio-ultrastar-{stem}-{}-{nanos}-{id}.tmp",
                std::process::id()
            ));
            match std::fs::create_dir(&root) {
                Ok(()) => return Ok(Self { root }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(UtaStudioError::Other(
            "could not create a unique UltraStar staging directory".to_string(),
        ))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for UltraStarExportStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug)]
struct StagedUltraStarFile {
    staged: PathBuf,
    destination: PathBuf,
}

fn publish_file_no_replace(file: &StagedUltraStarFile) -> Result<(), UtaStudioError> {
    match std::fs::hard_link(&file.staged, &file.destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(UtaStudioError::Other(format!(
                "UltraStar export refuses to overwrite existing output: {}",
                file.destination.display()
            )))
        }
        Err(error) => Err(error.into()),
    }
}

/// Publish assets first and the chart last. The chart is the logical bundle
/// commit marker: no consumer can discover a completed chart before every
/// referenced asset exists. All publications are no-replace hard links from
/// a sibling staging directory, so concurrent targets are never overwritten.
fn publish_staged_ultrastar_bundle(
    assets: &[StagedUltraStarFile],
    chart: &StagedUltraStarFile,
) -> Result<(), UtaStudioError> {
    let mut published = Vec::new();
    for file in assets.iter().chain(std::iter::once(chart)) {
        if let Err(error) = publish_file_no_replace(file) {
            for path in published.into_iter().rev() {
                let _ = std::fs::remove_file(path);
            }
            return Err(error);
        }
        published.push(file.destination.clone());
    }
    Ok(())
}

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
        .unwrap_or_else(|| "Uta! Studio Export".into());
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

    let staging = UltraStarExportStaging::create(output)?;
    let mut staged_assets = Vec::new();

    let staged_instrumental = staging.path(&instrumental_name);
    materialize_audio(&instrumental, &staged_instrumental)?;
    staged_assets.push(StagedUltraStarFile {
        staged: staged_instrumental,
        destination: instrumental_target,
    });
    if let (Some(source), Some(name), Some(destination)) =
        (vocals.as_ref(), vocals_name.as_ref(), vocals_target)
    {
        let staged = staging.path(name);
        materialize_audio(source, &staged)?;
        staged_assets.push(StagedUltraStarFile {
            staged,
            destination,
        });
    }
    for (source, name, destination) in [
        cover.zip(cover_name.as_ref()).zip(cover_target),
        video.zip(video_name.as_ref()).zip(video_target),
    ]
    .into_iter()
    .flatten()
    .map(|((source, name), destination)| (source, name, destination))
    {
        let staged = staging.path(name);
        std::fs::copy(source, &staged)?;
        staged_assets.push(StagedUltraStarFile {
            staged,
            destination,
        });
    }

    let chart_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UtaStudioError::Other("invalid UltraStar output name".to_string()))?;
    let staged_chart = staging.path(chart_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged_chart)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    validate_ultrastar_chart(&staged_chart)?;

    let published = output.to_path_buf();
    publish_staged_ultrastar_bundle(
        &staged_assets,
        &StagedUltraStarFile {
            staged: staged_chart,
            destination: published.clone(),
        },
    )?;
    drop(staging);
    let _ = crate::export_destination::record_last_export(
        file_hash,
        crate::export_destination::ExportPackageKind::UltraStar,
        &published,
    );
    Ok(published)
}

/// Parse and validate a completed UltraStar chart. Diagnostics use this after
/// export so the smoke test proves the file can be consumed, not merely written.
pub fn validate_ultrastar_chart(path: impl AsRef<Path>) -> Result<(), UtaStudioError> {
    let content = std::fs::read_to_string(path)?;
    validate_ultrastar_text(&content)
}

/// Parse and validate untrusted UltraStar text without touching the filesystem.
/// This is the narrow input boundary used by the fuzz target.
pub fn validate_ultrastar_text(content: &str) -> Result<(), UtaStudioError> {
    crate::usdx::validate_usdx_str(content)
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
        "#VERSION:1.1.0\n#TITLE:{title}\n#ARTIST:{artist}\n#CREATOR:Uta! Studio\n#AUDIO:{audio_name}\n#MP3:{audio_name}\n#INSTRUMENTAL:{audio_name}\n#BPM:{EXPORT_BPM:.2}\n#GAP:0\n"
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

    // UltraStar gives one lyric stream to each player. A chart's sung tracks
    // become those players; harmony, backing, and ad-lib tracks have nowhere
    // to go in this format and are left out of the projection.
    let players = player_tracks(chart);
    if players.is_empty() {
        output.push_str("E\n");
        return output;
    }
    let timebase = chart.timebase.max(1);
    if players.len() > 1 {
        for (index, track) in players.iter().enumerate() {
            let name = track
                .singer
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(sanitize_header)
                .unwrap_or_else(|| format!("Player {}", index + 1));
            output.push_str(&format!("#P{}:{name}\n", index + 1));
        }
    }
    for (index, track) in players.iter().enumerate() {
        if players.len() > 1 {
            output.push_str(&format!("P{}\n", index + 1));
        }
        write_track_notes(&mut output, track, timebase);
    }
    output.push_str("E\n");
    output
}

/// Writes one player's note body. Each player restarts the beat cursor,
/// because their lines run in parallel, not one after the other.
fn write_track_notes(output: &mut String, track: &VocalTrack, timebase: u64) {
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
}

/// The tracks UltraStar can carry: the ones a player is meant to sing, with
/// something in them.
///
/// UTZ's `part` field is the direct source of UltraStar's P1/P2 player
/// numbering, so a chart that assigns parts uses them, ordered by part
/// number. A chart with no part assignments (solo charts, or ones authored
/// outside Uta! Studio) falls back to every non-empty lead track in track
/// order, matching the single-player and unnumbered-duet cases.
fn player_tracks(chart: &VocalChartV1) -> Vec<&VocalTrack> {
    let has_notes =
        |track: &&VocalTrack| track.phrases.iter().any(|phrase| !phrase.notes.is_empty());
    let mut parted = chart
        .tracks
        .iter()
        .filter(|track| track.part.is_some() && has_notes(track))
        .collect::<Vec<_>>();
    if !parted.is_empty() {
        parted.sort_by_key(|track| track.part);
        return parted;
    }
    let sung = chart
        .tracks
        .iter()
        .filter(|track| track.role == VocalTrackRole::Lead && has_notes(track))
        .collect::<Vec<_>>();
    if sung.is_empty() {
        // A chart of only harmony or backing lines still deserves an export
        // rather than an empty file.
        return chart.tracks.iter().filter(has_notes).take(1).collect();
    }
    sung
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
    use super::{
        StagedUltraStarFile, UltraStarExportStaging, build_ultrastar_text,
        publish_staged_ultrastar_bundle, ultrastar_note_kind,
    };
    use crate::{editor::NoteKind, usdx::validate_usdx_str, vocal_chart::migrate_analyzer_chart};
    use utz::VocalChartV1;

    type TestNote<'a> = (f64, f64, u8, &'a str, &'a str);

    fn publication_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-ultrastar-publication-{name}-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            super::NEXT_STAGING_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    fn staged_file(
        staging: &UltraStarExportStaging,
        staged_name: &str,
        destination: std::path::PathBuf,
        bytes: &[u8],
    ) -> StagedUltraStarFile {
        let staged = staging.path(staged_name);
        std::fs::write(&staged, bytes).unwrap();
        StagedUltraStarFile {
            staged,
            destination,
        }
    }

    fn chart(language: &str, phrases: &[&[TestNote<'_>]]) -> VocalChartV1 {
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
    fn publication_race_preserves_competing_target_and_never_publishes_chart() {
        let root = publication_root("race");
        let chart_destination = root.join("song.txt");
        let asset_destination = root.join("song.mp3");
        std::fs::write(&asset_destination, b"competitor").unwrap();
        let staging = UltraStarExportStaging::create(&chart_destination).unwrap();
        let staging_root = staging.root.clone();
        let asset = staged_file(&staging, "song.mp3", asset_destination.clone(), b"ours");
        let chart = staged_file(
            &staging,
            "song.txt",
            chart_destination.clone(),
            b"#TITLE:Song\nE\n",
        );

        let error = publish_staged_ultrastar_bundle(&[asset], &chart).unwrap_err();
        assert!(error.to_string().contains("refuses to overwrite"));
        assert_eq!(std::fs::read(&asset_destination).unwrap(), b"competitor");
        assert!(!chart_destination.exists());
        drop(staging);
        assert!(!staging_root.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_failure_rolls_back_only_files_created_by_this_export() {
        let root = publication_root("rollback");
        let chart_destination = root.join("song.txt");
        let first_destination = root.join("song.mp3");
        let competing_destination = root.join("song.jpg");
        std::fs::write(&competing_destination, b"competitor").unwrap();
        let staging = UltraStarExportStaging::create(&chart_destination).unwrap();
        let staging_root = staging.root.clone();
        let first = staged_file(&staging, "song.mp3", first_destination.clone(), b"audio");
        let second = staged_file(
            &staging,
            "song.jpg",
            competing_destination.clone(),
            b"cover",
        );
        let chart = staged_file(
            &staging,
            "song.txt",
            chart_destination.clone(),
            b"#TITLE:Song\nE\n",
        );

        publish_staged_ultrastar_bundle(&[first, second], &chart).unwrap_err();
        assert!(!first_destination.exists());
        assert_eq!(
            std::fs::read(&competing_destination).unwrap(),
            b"competitor"
        );
        assert!(!chart_destination.exists());
        drop(staging);
        assert!(!staging_root.exists());
        std::fs::remove_dir_all(root).unwrap();
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

    /// Splits a chart's notes over a second lead track, assigning both
    /// tracks contiguous duet parts the way `EditorDocument` would.
    fn with_duet_track(chart: &mut VocalChartV1, singer: &str, notes: Vec<utz::VocalNote>) {
        chart.tracks[0].part = Some(1);
        chart.tracks.push(utz::VocalTrack {
            id: "duet".into(),
            role: utz::VocalTrackRole::Lead,
            part: Some(2),
            singer: Some(singer.into()),
            scoring_enabled: true,
            phrases: vec![utz::VocalPhrase {
                id: "duet-phrase".into(),
                notes,
            }],
        });
    }

    #[test]
    fn two_sung_tracks_export_as_an_ultrastar_duet() {
        let mut chart = chart("en", &[&[(0.0, 0.5, 60, "lead", "normal")]]);
        let partner = chart.tracks[0].phrases[0]
            .notes
            .iter()
            .map(|note| {
                let mut note = note.clone();
                note.id = "duet-note".into();
                note.start += chart.timebase;
                note.lyrics = vec![utz::LyricToken::Text(utz::LyricTextToken {
                    id: "duet-lyric".into(),
                    text: "partner".into(),
                    join_before: utz::LyricJoin::Space,
                    reading: None,
                    phonemes: None,
                })];
                note
            })
            .collect();
        with_duet_track(&mut chart, "Hana", partner);
        chart.validate().expect("valid duet chart");

        let text =
            build_ultrastar_text("Title", "Artist", &chart, "song.mp3", None, None, None, 4.0);
        assert!(text.contains("#P1:Player 1"));
        assert!(text.contains("#P2:Hana"));
        let players = text
            .lines()
            .filter(|line| *line == "P1" || *line == "P2")
            .collect::<Vec<_>>();
        assert_eq!(players, ["P1", "P2"]);
        assert!(text.lines().any(|line| line.ends_with("lead")));
        assert!(text.lines().any(|line| line.ends_with("partner")));
        // Each player restarts its own beat cursor, so the partner keeps the
        // beat its notes were authored at rather than being pushed after P1.
        let partner_beat = text
            .lines()
            .find(|line| line.ends_with("partner"))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|beat| beat.parse::<i64>().ok())
            .expect("partner beat");
        assert_eq!(partner_beat, super::seconds_to_beat(1.0));
        validate_usdx_str(&text).expect("duet parses back");
    }

    #[test]
    fn a_single_sung_track_keeps_the_plain_single_player_body() {
        let mut chart = chart("en", &[&[(0.0, 0.5, 60, "lead", "normal")]]);
        chart.tracks.push(utz::VocalTrack {
            id: "harmony".into(),
            role: utz::VocalTrackRole::Harmony,
            part: None,
            singer: None,
            scoring_enabled: false,
            phrases: chart.tracks[0].phrases.clone(),
        });
        // Harmony has nowhere to live in UltraStar, so it is left out and the
        // file stays a plain single-player chart.
        let text =
            build_ultrastar_text("Title", "Artist", &chart, "song.mp3", None, None, None, 4.0);
        assert!(!text.contains("#P1:"));
        assert!(!text.lines().any(|line| line == "P1"));
        assert_eq!(
            text.lines().filter(|line| line.starts_with(": ")).count(),
            1
        );
        validate_usdx_str(&text).expect("single player parses back");
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
