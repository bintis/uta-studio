use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use lofty::{
    file::{AudioFile, TaggedFileExt},
    tag::Accessor,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use blake3::Hasher;
use std::{fs::File, io::Read};

use crate::{
    cache::{CacheDir, normalize_tempo},
    error::UtaStudioError,
    library_db,
    usdx::UsdxBundle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TranscriptSource {
    Lyrics,
    Generated,
    Usdx,
    /// Timing came directly from a provided LRC / Enhanced LRC file (no AI
    /// transcription or alignment).
    Lrc,
}

/// Uta Studio source media always lives on the local filesystem.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum SongOrigin {
    LocalFile,
}

pub(crate) fn default_origin() -> SongOrigin {
    SongOrigin::LocalFile
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Song {
    pub path: PathBuf,
    pub file_hash: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
    pub album_art_path: Option<PathBuf>,
    pub is_analyzed: bool,
    pub language: Option<String>,
    #[serde(default)]
    pub transcript_source: Option<TranscriptSource>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub override_key: Option<String>,
    /// The song's musical tempo in beats per minute, detected by analysis or
    /// preserved from an imported UltraStar chart's `#BPM`. Informational —
    /// unlike `tempo`, nothing in the editor's own timing depends on it.
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default = "default_tempo")]
    pub tempo: f64,
    #[serde(default)]
    pub key_offset: i32,
    pub is_video: bool,
    #[serde(default)]
    pub usdx: Option<UsdxBundle>,
    #[serde(default = "default_origin")]
    pub origin: SongOrigin,
    /// True when a provided LRC is authored over the original mix without
    /// creating separate stems.
    #[serde(default)]
    pub no_stems: bool,
    /// True only when every artifact required by package export exists.  This
    /// is refreshed from disk when a song is read; unlike `is_analyzed`, it
    /// does not mistake an older lyrics-only analysis for a finished chart.
    #[serde(default)]
    pub authoring_ready: bool,
    #[serde(default)]
    pub authoring_missing: Vec<String>,
    /// Source charts are edited at their original key and tempo. Export can
    /// still be ready while the editor asks the user to reset a live shift.
    #[serde(default)]
    pub editor_ready: bool,
    #[serde(default)]
    pub editor_blocked_reason: Option<String>,
    /// User-editable metadata that analysis never touches — set from the
    /// song settings panel, shared by the library detail page and the
    /// editor's own settings menu. `bpm` above is analysis's own estimate;
    /// this wins over it wherever the two disagree, the same way
    /// `override_key` wins over `key`.
    #[serde(default)]
    pub override_bpm: Option<f64>,
    #[serde(default)]
    pub composer: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    /// A user-picked video to show behind the song during playback,
    /// independent of any video the song's own source or a USDX import
    /// carries. Wins over both when set.
    #[serde(default)]
    pub background_video_path: Option<PathBuf>,
}

fn default_tempo() -> f64 {
    1.0
}

/// Structured musical key, as produced by `detect_key_structured` in
/// `key_detect.py`. `tonic`/`scale` are `None` (never a fabricated "C
/// major") when analysis found nothing confident to report.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MusicKeyAnalysis {
    pub tonic: Option<String>,
    pub scale: Option<String>,
    pub confidence: f64,
}

/// Structured tempo/beat analysis, as produced by `analyze_rhythm` in
/// `rhythm.py`. `bpm` is `None` when nothing could be determined; `beats`
/// (absolute seconds, strictly increasing) is empty whenever the backend
/// could only estimate a global tempo without individually detected beats.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MusicRhythmAnalysis {
    pub bpm: Option<f64>,
    pub confidence: f64,
    #[serde(default)]
    pub beats: Vec<f64>,
}

/// A few extra Essentia descriptors with no dependency-free fallback —
/// present only when Essentia was installed at analysis time (it has no
/// Windows wheel).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MusicAnalysisDescriptors {
    /// 0-1; how suitable the track's rhythm is for dancing.
    pub danceability: f64,
    pub dynamic_complexity_db: f64,
    pub loudness_db: f64,
}

/// The cached contents of `{file_hash}_music_analysis.json`, written once
/// during analysis by `analyze_music` in `pipeline.py`. Unlike `Song`, this
/// never round-trips through the library database — it's read fresh from
/// disk whenever something (the settings panel, the editor's beat grid)
/// needs it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MusicAnalysis {
    pub version: u32,
    pub key: MusicKeyAnalysis,
    pub rhythm: MusicRhythmAnalysis,
    #[serde(default)]
    pub descriptors: Option<MusicAnalysisDescriptors>,
}

/// Reads the cached key/rhythm analysis for a song, if analysis produced
/// one. `None` when the song hasn't been analyzed since this was added, or
/// the cache file is missing, unreadable, or from an older format version —
/// never an error, since every caller must keep working without it (no
/// beat grid, no extra descriptors) rather than fail.
pub fn load_music_analysis(cache: &CacheDir, file_hash: &str) -> Option<MusicAnalysis> {
    let path = cache.music_analysis_path(file_hash);
    let data = std::fs::read_to_string(path).ok()?;
    let analysis: MusicAnalysis = serde_json::from_str(&data).ok()?;
    (analysis.version == 1).then_some(analysis)
}

#[derive(Debug, Clone)]
pub struct TranscriptMetaInfo {
    pub source: TranscriptSource,
    pub language: Option<String>,
    pub key: Option<String>,
    /// The song's detected tempo in beats per minute, informational only.
    pub bpm: Option<f64>,
    pub tempo: f64,
    pub no_stems: bool,
}

impl Song {
    #[allow(clippy::too_many_arguments)]
    pub fn from_path(
        path: &Path,
        file_hash: String,
        cache: &CacheDir,
        is_analyzed: bool,
        language: Option<String>,
        transcript_source: Option<TranscriptSource>,
        key: Option<String>,
        override_key: Option<String>,
        bpm: Option<f64>,
        tempo: f64,
        key_offset: i32,
        is_video: bool,
        usdx: Option<UsdxBundle>,
        origin: SongOrigin,
    ) -> Self {
        let (mut title, mut artist, mut album, duration_secs, cover_bytes) = if is_video {
            read_video_metadata(path)
        } else {
            read_metadata(path)
        };

        if title.is_empty() {
            title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
        }
        if artist.is_empty() {
            artist = "Unknown Artist".to_string();
        }
        if album.is_empty() {
            album = "Unknown Album".to_string();
        }

        let album_art_path = cover_bytes.and_then(|bytes| {
            let cover_hash = blake3::hash(&bytes).to_hex()[..32].to_string();
            let cover_path = cache.cover_path(&cover_hash);
            if !cover_path.exists() {
                std::fs::write(&cover_path, &bytes).ok()?;
            }
            Some(cover_path)
        });

        Self {
            path: path.to_path_buf(),
            file_hash,
            title,
            artist,
            album,
            duration_secs,
            album_art_path,
            is_analyzed,
            language,
            transcript_source,
            key,
            override_key,
            bpm,
            tempo,
            key_offset,
            is_video,
            usdx,
            origin,
            no_stems: false,
            authoring_ready: false,
            authoring_missing: Vec::new(),
            editor_ready: false,
            editor_blocked_reason: None,
            override_bpm: None,
            composer: None,
            country: None,
            background_video_path: None,
        }
    }

    /// Refresh transient authoring state from the authoritative cache files.
    /// The fields are serialized for the frontend but never trusted from the
    /// database payload, because analysis may finish between library scans.
    pub(crate) fn refresh_authoring_state(&mut self, cache: &CacheDir) {
        let mut missing = Vec::new();
        let instrumental_ready = if self.no_stems {
            self.path.is_file()
        } else if let Some(bundle) = self.usdx.as_ref() {
            bundle
                .instrumental
                .as_ref()
                .unwrap_or(&bundle.audio)
                .is_file()
        } else {
            let base_ready = cache.instrumental_path(&self.file_hash).is_file();
            let effective_key = self.override_key.as_ref().or(self.key.as_ref());
            let variant_ready = effective_key.is_some_and(|key| {
                cache
                    .variant_instrumental_path(&self.file_hash, key, normalize_tempo(self.tempo))
                    .is_file()
            });
            base_ready || variant_ready
        };
        if !instrumental_ready {
            missing.push("instrumental".into());
        }
        if !cache.transcript_path(&self.file_hash).is_file() {
            missing.push("transcript".into());
        }
        if !cache.pitch_track_path(&self.file_hash).is_file() {
            missing.push("pitch_track".into());
        }
        if !cache.pitch_notes_path(&self.file_hash).is_file() {
            missing.push("pitch_notes".into());
        }

        self.authoring_ready = missing.is_empty();
        self.authoring_missing = missing;
        self.editor_blocked_reason = if self.key_offset != 0 || normalize_tempo(self.tempo) != 1.0 {
            Some("Reset key and tempo before editing the source chart".into())
        } else {
            None
        };
        self.editor_ready = self.authoring_ready && self.editor_blocked_reason.is_none();
    }
}

/// Persists an edit from the song settings panel — the one path both the
/// library detail page and the editor's own settings menu save through, so
/// neither can drift out of sync with the other. `None` for a field clears
/// it; a caller that only means to touch one field should read the current
/// `Song` first and carry the rest through unchanged.
pub fn update_song_settings(
    file_hash: &str,
    composer: Option<String>,
    country: Option<String>,
    override_bpm: Option<f64>,
    background_video_path: Option<PathBuf>,
) -> Result<(), UtaStudioError> {
    let mut song = library_db::load_song_by_hash(file_hash)
        .map_err(|e| UtaStudioError::Other(e.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    song.composer = composer;
    song.country = country;
    song.override_bpm = override_bpm;
    song.background_video_path = background_video_path;
    library_db::update_song_fields(file_hash, &song)
        .map_err(|e| UtaStudioError::Other(e.to_string()))
}

pub(crate) fn compute_file_hash(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = file.read(&mut buf)?;

        if n == 0 {
            break;
        }

        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex()[..32].to_string())
}

pub fn build_song(path: &Path, cache: &CacheDir, is_video: bool) -> Result<Song, UtaStudioError> {
    let file_hash = compute_file_hash(path)?;

    let is_analyzed = cache.transcript_exists(&file_hash);
    let (transcript_source, language, key, bpm, tempo, no_stems) = if is_analyzed {
        let meta = read_transcript_meta(cache, &file_hash);
        (
            Some(meta.source),
            meta.language,
            meta.key,
            meta.bpm,
            meta.tempo,
            meta.no_stems,
        )
    } else {
        (None, None, None, None, default_tempo(), false)
    };

    let mut song = Song::from_path(
        path,
        file_hash,
        cache,
        is_analyzed,
        language,
        transcript_source,
        key,
        None,
        bpm,
        tempo,
        0,
        is_video,
        None,
        SongOrigin::LocalFile,
    );
    song.no_stems = no_stems;
    Ok(song)
}

pub fn read_transcript_meta(cache: &CacheDir, hash: &str) -> TranscriptMetaInfo {
    #[derive(serde::Deserialize)]
    struct TranscriptMeta {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        bpm: Option<f64>,
        #[serde(default = "default_tempo")]
        tempo: f64,
        #[serde(default)]
        no_stems: bool,
    }
    let path = cache.transcript_path(hash);
    if let Ok(data) = std::fs::read_to_string(&path)
        && let Ok(parsed) = serde_json::from_str::<TranscriptMeta>(&data)
    {
        let src = match parsed.source.as_deref() {
            Some("lyrics") => TranscriptSource::Lyrics,
            Some("usdx") => TranscriptSource::Usdx,
            Some("lrc") => TranscriptSource::Lrc,
            _ => TranscriptSource::Generated,
        };
        return TranscriptMetaInfo {
            source: src,
            language: parsed.language,
            key: parsed.key,
            bpm: parsed.bpm.filter(|value| value.is_finite() && *value > 0.0),
            tempo: parsed.tempo,
            no_stems: parsed.no_stems,
        };
    }
    TranscriptMetaInfo {
        source: TranscriptSource::Generated,
        language: None,
        key: None,
        bpm: None,
        tempo: default_tempo(),
        no_stems: false,
    }
}

fn read_metadata(path: &Path) -> (String, String, String, f64, Option<Vec<u8>>) {
    let tagged = match lofty::read_from_path(path) {
        Ok(t) => t,
        Err(_) => return (String::new(), String::new(), String::new(), 0.0, None),
    };

    let properties = tagged.properties();
    let duration_secs = properties.duration().as_secs_f64();

    let tag = match tagged.primary_tag().or_else(|| tagged.first_tag()) {
        Some(t) => t,
        None => {
            return (
                String::new(),
                String::new(),
                String::new(),
                duration_secs,
                None,
            );
        }
    };

    let title = tag.title().map(|s| s.to_string()).unwrap_or_default();
    let artist = tag.artist().map(|s| s.to_string()).unwrap_or_default();
    let album = tag.album().map(|s| s.to_string()).unwrap_or_default();

    let album_art = tag.pictures().first().map(|pic| pic.data().to_vec());

    (title, artist, album, duration_secs, album_art)
}

fn read_video_metadata(path: &Path) -> (String, String, String, f64, Option<Vec<u8>>) {
    let ffmpeg = crate::vendor::ffmpeg_path();

    // Just probe the header -- no output file means ffmpeg reads metadata and exits immediately.
    let probe = crate::vendor::silent_command(&ffmpeg)
        .args(["-i", &path.to_string_lossy()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    let mut title = String::new();
    let mut artist = String::new();
    let mut album = String::new();
    let mut duration_secs = 0.0;

    if let Ok(output) = probe {
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Duration:")
                && let Some(ts) = rest.split(',').next()
            {
                duration_secs = parse_ffmpeg_duration(ts.trim());
            }
            if let Some(val) = strip_meta_tag(trimmed, "title") {
                title = val;
            }
            if let Some(val) = strip_meta_tag(trimmed, "artist") {
                artist = val;
            }
            if let Some(val) = strip_meta_tag(trimmed, "album") {
                album = val;
            }
        }
    }

    let album_art = extract_video_thumbnail(&ffmpeg, path);

    (title, artist, album, duration_secs, album_art)
}

fn extract_video_thumbnail(ffmpeg: &Path, video_path: &Path) -> Option<Vec<u8>> {
    let output = crate::vendor::silent_command(ffmpeg)
        .args([
            "-i",
            &video_path.to_string_lossy(),
            "-vframes",
            "1",
            "-f",
            "image2pipe",
            "-c:v",
            "mjpeg",
            "-vf",
            "scale=300:-1",
            "-v",
            "error",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

fn strip_meta_tag(line: &str, tag: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if lower.starts_with(tag) {
        let after = &line[tag.len()..];
        let after = after.trim_start();
        if let Some(val) = after.strip_prefix(':') {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_ffmpeg_duration(s: &str) -> f64 {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 3 {
        let h: f64 = parts[0].parse().unwrap_or(0.0);
        let m: f64 = parts[1].parse().unwrap_or(0.0);
        let s: f64 = parts[2].parse().unwrap_or(0.0);
        h * 3600.0 + m * 60.0 + s
    } else {
        0.0
    }
}
