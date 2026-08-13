//! `.utz` export boundary for Uta Studio.
//!
//! The analyzer cache remains an implementation detail. Export resolves the
//! current key/tempo variants, transforms chart timing through the authoring
//! model, then writes a self-contained versioned package.

use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use ts_rs::TS;
use utz::{
    AssetRef, AssetSource, AudioAssets, ChartAssets, Provenance, SongMetadata, UtzManifest,
    UtzPackage, VisualAssets,
};

use crate::{
    audio_format::{
        export_extension as audio_export_extension, media_type as audio_media_type, transcode_audio,
    },
    authoring::{get_audio_paths, load_pitch_guide, load_transcript},
    cache::CacheDir,
    error::UtaStudioError,
    library_db,
    library_model::SongsStore,
};

struct ExportAudioStaging(PathBuf);

impl ExportAudioStaging {
    fn new(nonce: u128) -> Result<Self, UtaStudioError> {
        let path = std::env::temp_dir().join(format!(
            "uta-studio-export-audio-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn prepare(&self, source: &Path, label: &str) -> Result<PathBuf, UtaStudioError> {
        let extension = audio_export_extension(source);
        if source
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        {
            return Ok(source.to_path_buf());
        }
        let output = self.0.join(format!("{label}.{extension}"));
        transcode_audio(source, &output)?;
        Ok(output)
    }
}

impl Drop for ExportAudioStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportableSong {
    pub file_hash: String,
    pub title: String,
    pub artist: String,
    pub is_video: bool,
    pub source_path: PathBuf,
    pub ready: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExportProgress {
    pub phase: String,
    pub asset: Option<String>,
    pub percent: u8,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

pub fn list_exportable_songs() -> Result<Vec<ExportableSong>, UtaStudioError> {
    library_db::init_library().map_err(|error| UtaStudioError::Other(error.to_string()))?;
    let cache = CacheDir::new();
    Ok(SongsStore::load_all()
        .processed
        .into_iter()
        .map(|song| {
            let missing = missing_assets(&cache, &song.file_hash);
            ExportableSong {
                file_hash: song.file_hash,
                title: song.title,
                artist: song.artist,
                is_video: song.is_video,
                source_path: song.path,
                ready: missing.is_empty(),
                missing,
            }
        })
        .collect())
}

pub fn export_utz(file_hash: &str, output: impl AsRef<Path>) -> Result<PathBuf, UtaStudioError> {
    export_utz_with_progress(file_hash, output, |_| {})
}

pub fn export_utz_with_progress<F>(
    file_hash: &str,
    output: impl AsRef<Path>,
    mut on_progress: F,
) -> Result<PathBuf, UtaStudioError>
where
    F: FnMut(ExportProgress),
{
    on_progress(ExportProgress {
        phase: "Validating assets".into(),
        asset: None,
        percent: 0,
        completed_bytes: 0,
        total_bytes: 0,
    });
    library_db::init_library().map_err(|error| UtaStudioError::Other(error.to_string()))?;
    let song = library_db::load_song_by_hash(file_hash)
        .map_err(|error| UtaStudioError::Other(error.to_string()))?
        .ok_or_else(|| UtaStudioError::Other(format!("song not found: {file_hash}")))?;
    let output = output.as_ref();
    if output.extension().and_then(|extension| extension.to_str()) != Some("utz") {
        return Err(UtaStudioError::Other(
            "Uta Studio exports must use the .utz extension".into(),
        ));
    }
    if output.exists() {
        return Err(UtaStudioError::Other(format!(
            "refusing to overwrite {}",
            output.display()
        )));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = ExportAudioStaging::new(timestamp)?;

    let audio = get_audio_paths(file_hash);
    let instrumental_source = PathBuf::from(&audio.instrumental);
    if !instrumental_source.is_file() {
        return Err(UtaStudioError::Other(
            "instrumental stem is not ready".into(),
        ));
    }
    let guide_source = audio
        .vocals
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    let instrumental_path = staging.prepare(&instrumental_source, "instrumental")?;
    let guide_path = guide_source
        .as_deref()
        .map(|path| staging.prepare(path, "guide-vocals"))
        .transpose()?;
    let transcript = load_transcript(file_hash)?;
    let guide = load_pitch_guide(file_hash)?
        .ok_or_else(|| UtaStudioError::Other("pitch track and guide notes are not ready".into()))?;
    let pitch_track = guide
        .get("track")
        .ok_or_else(|| UtaStudioError::Other("pitch guide has no track".into()))?;
    let pitch_notes = guide
        .get("notes")
        .ok_or_else(|| UtaStudioError::Other("pitch guide has no notes".into()))?;

    let instrumental_name = format!(
        "audio/instrumental.{}",
        audio_export_extension(&instrumental_path)
    );
    let guide_name = guide_path
        .as_ref()
        .map(|path| format!("audio/guide-vocals.{}", audio_export_extension(path)));
    let mut sources = BTreeMap::from([
        (
            instrumental_name.clone(),
            AssetSource::File(instrumental_path.clone()),
        ),
        (
            "charts/transcript.json".into(),
            AssetSource::Bytes(serde_json::to_vec(&transcript)?),
        ),
        (
            "charts/pitch-track.json".into(),
            AssetSource::Bytes(serde_json::to_vec(pitch_track)?),
        ),
        (
            "charts/pitch-notes.json".into(),
            AssetSource::Bytes(serde_json::to_vec(pitch_notes)?),
        ),
    ]);
    if let (Some(name), Some(path)) = (&guide_name, &guide_path) {
        sources.insert(name.clone(), AssetSource::File(path.clone()));
    }
    let cover_path = song.album_art_path.clone().filter(|path| path.is_file());
    let source_video = song
        .usdx
        .as_ref()
        .and_then(|bundle| bundle.video.clone())
        .or_else(|| song.is_video.then_some(song.path.clone()))
        .filter(|path| path.is_file());

    let mut manifest = UtzManifest::new(
        format!("uta:{file_hash}"),
        SongMetadata {
            title: song.title,
            artist: song.artist,
            album: non_empty(song.album),
            language: song.language,
            duration_seconds: song.duration_secs,
            bpm: None,
            key: song.override_key.or(song.key),
        },
        AudioAssets {
            instrumental: AssetRef::pending(
                &instrumental_name,
                audio_media_type(&instrumental_path),
            ),
            guide_vocals: guide_name
                .as_ref()
                .zip(guide_path.as_ref())
                .map(|(name, path)| AssetRef::pending(name, audio_media_type(path))),
            original: None,
            audio_offset_seconds: 0.0,
        },
        ChartAssets {
            transcript: AssetRef::pending("charts/transcript.json", "application/json"),
            pitch_track: AssetRef::pending("charts/pitch-track.json", "application/json"),
            pitch_notes: AssetRef::pending("charts/pitch-notes.json", "application/json"),
        },
    );
    manifest.provenance = Provenance {
        generator: Some(format!("uta-studio/{}", env!("CARGO_PKG_VERSION"))),
        source: Some(file_hash.to_owned()),
        rights: None,
    };

    if let Some(cover) = cover_path {
        let cover_name = format!("artwork/cover.{}", extension_or(&cover, "jpg"));
        sources.insert(cover_name.clone(), AssetSource::File(cover.clone()));
        manifest.visuals = VisualAssets {
            cover: Some(AssetRef::pending(&cover_name, media_type(&cover))),
            video: None,
        };
    }

    if let Some(video) = source_video {
        let video_name = format!("video/background.{}", extension_or(&video, "mp4"));
        sources.insert(video_name.clone(), AssetSource::File(video.clone()));
        manifest.visuals.video = Some(AssetRef::pending(&video_name, media_type(&video)));
    }

    // Build beside the final path, then rename only after ZIP finalization and
    // fsync. A cancelled/failed export cannot leave a half-written .utz.
    let output_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export.utz");
    let temporary = output.with_file_name(format!(
        ".{output_name}.{}.{}.tmp",
        std::process::id(),
        timestamp
    ));
    let destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let mut last_logged_percent = 0u64;
    let mut last_reported_percent = 0u64;
    let result =
        UtzPackage::write_streaming(manifest, sources, destination, |asset, completed, total| {
            let percent = completed
                .saturating_mul(100)
                .checked_div(total)
                .unwrap_or(100);
            if percent >= last_logged_percent.saturating_add(10) || percent == 100 {
                tracing::info!(
                    "[export] {percent}% · {} · {completed}/{total} bytes",
                    asset
                );
                last_logged_percent = percent;
            }
            if percent > last_reported_percent || percent == 100 {
                on_progress(ExportProgress {
                    phase: "Writing package".into(),
                    asset: Some(asset.to_owned()),
                    percent: percent.min(100) as u8,
                    completed_bytes: completed,
                    total_bytes: total,
                });
                last_reported_percent = percent;
            }
        })
        .map_err(|error| UtaStudioError::Other(error.to_string()));
    let destination = match result {
        Ok(destination) => destination,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };
    if let Err(error) = destination.sync_all() {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(destination);

    // A same-filesystem hard link publishes the fully finalized inode in one
    // operation and fails if another file appeared at the target meanwhile.
    // This preserves atomicity without ever replacing an existing user file.
    if let Err(error) = std::fs::hard_link(&temporary, output) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    std::fs::remove_file(&temporary)?;
    tracing::info!("[export] Complete · {}", output.display());
    on_progress(ExportProgress {
        phase: "Complete".into(),
        asset: None,
        percent: 100,
        completed_bytes: 0,
        total_bytes: 0,
    });
    Ok(output.to_path_buf())
}

fn missing_assets(cache: &CacheDir, file_hash: &str) -> Vec<String> {
    let audio = get_audio_paths(file_hash);
    let mut missing = Vec::new();
    if !Path::new(&audio.instrumental).is_file() {
        missing.push("instrumental".into());
    }
    if !cache.transcript_path(file_hash).is_file() {
        missing.push("transcript".into());
    }
    if !cache.pitch_track_path(file_hash).is_file() {
        missing.push("pitch_track".into());
    }
    if !cache.pitch_notes_path(file_hash).is_file() {
        missing.push("pitch_notes".into());
    }
    missing
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn extension_or(path: &Path, fallback: &str) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or(fallback)
        .to_ascii_lowercase()
}

fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        "png" => "image/png",
        "webp" => "image/webp",
        "jpeg" | "jpg" => "image/jpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}
