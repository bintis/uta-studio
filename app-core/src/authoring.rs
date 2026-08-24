use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use crate::cache::{CacheDir, normalize_tempo};
use crate::error::UtaStudioError;
use crate::library_db;
use crate::song::Song;
use crate::vendor::{ffmpeg_path, silent_command};

#[derive(Debug, Clone, Serialize)]
pub struct AudioPaths {
    pub instrumental: String,
    /// `None` when source media is authored without stem separation.
    pub vocals: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShiftResult {
    pub key: String,
    pub tempo: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ShiftDone {
    pub file_hash: String,
    pub key: Option<String>,
    pub tempo: Option<f64>,
    pub error: Option<String>,
}

pub fn load_transcript(file_hash: &str) -> Result<serde_json::Value, UtaStudioError> {
    let cache = CacheDir::new();
    let path = resolve_transcript_path(&cache, file_hash);
    let data = std::fs::read_to_string(&path)?;
    let value = serde_json::from_str(&data)?;
    Ok(value)
}

/// Loads model-derived reference pitch and visual guide notes. Absence is a
/// normal state for songs analysed before pitch guides were introduced.
pub fn load_pitch_guide(file_hash: &str) -> Result<Option<serde_json::Value>, UtaStudioError> {
    let cache = CacheDir::new();
    let track_path = cache.pitch_track_path(file_hash);
    let notes_path = cache.pitch_notes_path(file_hash);
    if !track_path.is_file() || !notes_path.is_file() {
        return Ok(None);
    }

    let track: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(track_path)?)?;
    let notes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(notes_path)?)?;
    let mut guide = serde_json::json!({ "track": track, "notes": notes });

    // The cached guide is analysed once from the original vocals. Export
    // variants produced by Rubber Band can be represented exactly enough by
    // shifting Hz/MIDI and scaling time, so changing key or tempo does not
    // require another model pass.
    if let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() {
        transform_pitch_guide(&mut guide, song.key_offset, normalize_tempo(song.tempo));
    }
    Ok(Some(guide))
}

fn transform_pitch_guide(guide: &mut serde_json::Value, key_offset: i32, tempo: f64) {
    if key_offset == 0 && tempo == 1.0 {
        return;
    }
    let pitch_ratio = 2f64.powf(f64::from(key_offset) / 12.0);
    let time_ratio = 1.0 / tempo;

    if let Some(frames) = guide
        .pointer_mut("/track/frames")
        .and_then(|v| v.as_array_mut())
    {
        for frame in frames {
            if let Some(time) = frame.get("time").and_then(|v| v.as_f64()) {
                frame["time"] = serde_json::Value::from(time * time_ratio);
            }
            if let Some(hz) = frame.get("hz").and_then(|v| v.as_f64()) {
                frame["hz"] = serde_json::Value::from(hz * pitch_ratio);
            }
        }
    }
    if let Some(notes) = guide
        .pointer_mut("/notes/notes")
        .and_then(|v| v.as_array_mut())
    {
        for note in notes {
            for field in ["start", "end"] {
                if let Some(time) = note.get(field).and_then(|v| v.as_f64()) {
                    note[field] = serde_json::Value::from(time * time_ratio);
                }
            }
            if let Some(midi) = note.get("midi").and_then(|v| v.as_i64()) {
                note["midi"] = serde_json::Value::from(midi + i64::from(key_offset));
            }
        }
    }
}

fn resolve_effective_key_tempo(song: &Song) -> Option<(String, f64)> {
    let key = song.override_key.as_ref().or(song.key.as_ref())?.clone();
    Some((key, normalize_tempo(song.tempo)))
}

fn is_base_original_selection(song: &Song, key: &str, tempo: f64) -> bool {
    song.key.as_deref() == Some(key) && normalize_tempo(tempo) == 1.0
}

fn base_pair_exists(cache: &CacheDir, file_hash: &str) -> bool {
    cache.instrumental_path(file_hash).is_file() && cache.vocals_path(file_hash).is_file()
}

fn variant_pair_exists(cache: &CacheDir, file_hash: &str, key: &str, tempo: f64) -> bool {
    cache
        .variant_instrumental_path(file_hash, key, tempo)
        .is_file()
        && cache.variant_vocals_path(file_hash, key, tempo).is_file()
}

fn resolve_transcript_path(cache: &CacheDir, file_hash: &str) -> PathBuf {
    if let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten()
        && let Some((_key, tempo)) = resolve_effective_key_tempo(&song)
    {
        if normalize_tempo(tempo) == 1.0 {
            return cache.resolve_timed_transcript_path(file_hash);
        }
        let variant = cache.variant_transcript_path(file_hash, tempo);
        if variant.is_file() {
            return variant;
        }
    }
    cache.resolve_timed_transcript_path(file_hash)
}

fn original_media_path<'a>(song_path: &'a Path, usdx_audio: Option<&'a Path>) -> &'a Path {
    // A USDX song's indexed path is its chart text, not playable media. Its
    // declared #MP3 source is the authorized primary audio for authoring.
    usdx_audio.unwrap_or(song_path)
}

/// Resolve the on-disk primary audio used by authoring and Editor A/B.
pub(crate) fn resolve_original_media(song: &Song, _cache: &CacheDir) -> String {
    original_media_path(
        &song.path,
        song.usdx.as_ref().map(|bundle| bundle.audio.as_path()),
    )
    .to_string_lossy()
    .into_owned()
}

pub fn get_audio_paths(file_hash: &str) -> AudioPaths {
    let cache = CacheDir::new();
    if let Some(song) = library_db::load_song_by_hash(file_hash).ok().flatten() {
        if song.no_stems {
            let tempo = normalize_tempo(song.tempo);
            if let Some(key) = song.override_key.as_ref().or(song.key.as_ref())
                && !is_base_original_selection(&song, key, tempo)
            {
                let variant = cache.variant_instrumental_path(file_hash, key, tempo);
                if variant.is_file() {
                    return AudioPaths {
                        instrumental: variant.to_string_lossy().into_owned(),
                        vocals: None,
                    };
                }
            }
            return AudioPaths {
                instrumental: resolve_original_media(&song, &cache),
                vocals: None,
            };
        }

        if let Some(bundle) = song.usdx.as_ref() {
            let inst = bundle.instrumental.as_ref().unwrap_or(&bundle.audio);
            return AudioPaths {
                instrumental: inst.to_string_lossy().into_owned(),
                vocals: bundle
                    .vocals
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
            };
        }

        let effective_key = song.override_key.as_ref().or(song.key.as_ref());
        let tempo = normalize_tempo(song.tempo);

        if let Some(key) = effective_key {
            let variant_instrumental = cache.variant_instrumental_path(file_hash, key, tempo);
            let variant_vocals = cache.variant_vocals_path(file_hash, key, tempo);
            if is_base_original_selection(&song, key, tempo) {
                if variant_instrumental.is_file() && variant_vocals.is_file() {
                    return AudioPaths {
                        instrumental: variant_instrumental.to_string_lossy().into_owned(),
                        vocals: Some(variant_vocals.to_string_lossy().into_owned()),
                    };
                }
                let base_instrumental = cache.instrumental_path(file_hash);
                let base_vocals = cache.vocals_path(file_hash);
                if base_instrumental.is_file() && base_vocals.is_file() {
                    return AudioPaths {
                        instrumental: base_instrumental.to_string_lossy().into_owned(),
                        vocals: Some(base_vocals.to_string_lossy().into_owned()),
                    };
                }
            }
            if variant_instrumental.is_file() && variant_vocals.is_file() {
                return AudioPaths {
                    instrumental: variant_instrumental.to_string_lossy().into_owned(),
                    vocals: Some(variant_vocals.to_string_lossy().into_owned()),
                };
            }
        }
    }

    let base_instrumental = cache.instrumental_path(file_hash);
    let base_vocals = cache.vocals_path(file_hash);
    AudioPaths {
        instrumental: base_instrumental.to_string_lossy().into_owned(),
        vocals: Some(base_vocals.to_string_lossy().into_owned()),
    }
}

fn run_rubberband_filter(
    input: &Path,
    output: &Path,
    pitch_ratio: f64,
    tempo_ratio: f64,
) -> Result<(), UtaStudioError> {
    let filter = format!("rubberband=pitch={pitch_ratio}:tempo={tempo_ratio}");
    let mut command = silent_command(ffmpeg_path());
    command
        .args(["-y", "-i"])
        .arg(input)
        .args(["-af", &filter, "-c:a"]);
    if output.extension().and_then(|value| value.to_str()) == Some("flac") {
        command.args(["flac", "-compression_level", "8"]);
    } else {
        command.args(["libmp3lame", "-q:a", "2"]);
    }
    let status = command.args(["-v", "error"]).arg(output).status()?;
    if !status.success() {
        return Err(UtaStudioError::Other(format!(
            "ffmpeg rubberband failed with status {status}"
        )));
    }
    Ok(())
}

fn run_rubberband_pair_parallel(
    source_inst: &Path,
    target_inst: &Path,
    source_voc: &Path,
    target_voc: &Path,
    pitch_ratio: f64,
    tempo_ratio: f64,
) -> Result<(), UtaStudioError> {
    let source_inst = source_inst.to_path_buf();
    let target_inst = target_inst.to_path_buf();
    let source_voc = source_voc.to_path_buf();
    let target_voc = target_voc.to_path_buf();

    let inst_worker = std::thread::spawn(move || {
        run_rubberband_filter(&source_inst, &target_inst, pitch_ratio, tempo_ratio)
            .map_err(|e| e.to_string())
    });
    let voc_worker = std::thread::spawn(move || {
        run_rubberband_filter(&source_voc, &target_voc, pitch_ratio, tempo_ratio)
            .map_err(|e| e.to_string())
    });

    let inst_result = inst_worker
        .join()
        .map_err(|_| UtaStudioError::Other("instrumental transform thread panicked".into()))?;
    let voc_result = voc_worker
        .join()
        .map_err(|_| UtaStudioError::Other("vocals transform thread panicked".into()))?;

    if let Err(err) = inst_result {
        return Err(UtaStudioError::Other(err));
    }
    if let Err(err) = voc_result {
        return Err(UtaStudioError::Other(err));
    }
    Ok(())
}

fn resolve_canonical_stems_for_key(
    cache: &CacheDir,
    file_hash: &str,
    song: &Song,
    key: &str,
) -> Result<(PathBuf, PathBuf), UtaStudioError> {
    let canonical_inst = cache.variant_instrumental_path(file_hash, key, 1.0);
    let canonical_voc = cache.variant_vocals_path(file_hash, key, 1.0);
    if canonical_inst.is_file() && canonical_voc.is_file() {
        return Ok((canonical_inst, canonical_voc));
    }

    if song.key.as_deref() == Some(key) {
        let base_instrumental = cache.instrumental_path(file_hash);
        let base_vocals = cache.vocals_path(file_hash);
        if base_instrumental.is_file() && base_vocals.is_file() {
            return Ok((base_instrumental, base_vocals));
        }
    }

    Err(UtaStudioError::Other(format!(
        "Canonical stems for key '{key}' not found. Generate/reaalyze canonical stems first."
    )))
}

/// Key/tempo shift for LRC-provided songs played without stem separation.
/// Everything is derived from the untouched original mix (single track, no
/// guide vocals), and tempo changes scale the provided transcript timings.
fn no_stems_shift(
    cache: &CacheDir,
    file_hash: &str,
    mut song: Song,
    target_key: String,
    key_offset: i32,
    target_tempo: f64,
) -> Result<ShiftResult, UtaStudioError> {
    let target_tempo = normalize_tempo(target_tempo);
    let base_key = song.key.clone().unwrap_or_else(|| target_key.clone());

    // Base selection: play the untouched original mix.
    if key_offset == 0 && target_tempo == 1.0 {
        song.override_key = None;
        song.tempo = 1.0;
        song.key_offset = 0;
        cache.delete_transcript_variants(file_hash);
        library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;
        return Ok(ShiftResult {
            key: base_key,
            tempo: 1.0,
        });
    }

    let source = resolve_original_media(&song, cache);
    let target_inst = cache.variant_instrumental_path_with_extension(
        file_hash,
        &target_key,
        target_tempo,
        crate::audio_format::export_extension(Path::new(&source)),
    );
    if !target_inst.is_file() {
        let pitch_ratio = 2f64.powf(f64::from(key_offset) / 12.0);
        run_rubberband_filter(Path::new(&source), &target_inst, pitch_ratio, target_tempo)?;
    }

    // Tempo changes stretch the timeline, so scale the LRC timings into a
    // tempo variant that export picks up for the transformed mix.
    if target_tempo != 1.0 {
        let base_transcript = std::fs::read_to_string(cache.transcript_path(file_hash))?;
        let mut transcript: Value = serde_json::from_str(&base_transcript)?;
        scale_transcript_timestamps(&mut transcript, 1.0 / target_tempo);
        transcript["tempo"] = Value::from(target_tempo);
        transcript["key"] = Value::from(target_key.clone());
        std::fs::write(
            cache.variant_transcript_path(file_hash, target_tempo),
            serde_json::to_string_pretty(&transcript)?,
        )?;
    }

    song.override_key = if base_key == target_key {
        None
    } else {
        Some(target_key.clone())
    };
    song.tempo = target_tempo;
    song.key_offset = key_offset;
    library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;

    Ok(ShiftResult {
        key: target_key,
        tempo: target_tempo,
    })
}

fn resolve_source_transcript_path(cache: &CacheDir, file_hash: &str, tempo: f64) -> PathBuf {
    if normalize_tempo(tempo) == 1.0 {
        return cache.transcript_path(file_hash);
    }
    let variant = cache.variant_transcript_path(file_hash, tempo);
    if variant.is_file() {
        return variant;
    }
    cache.transcript_path(file_hash)
}

fn round_transcript_time(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn scale_time_field(node: &mut Value, field: &str, factor: f64) {
    let Some(v) = node.get(field).and_then(|v| v.as_f64()) else {
        return;
    };
    if let Some(slot) = node.get_mut(field) {
        *slot = Value::from(round_transcript_time(v * factor));
    }
}

fn scale_transcript_timestamps(transcript: &mut Value, factor: f64) {
    let Some(segments) = transcript
        .get_mut("segments")
        .and_then(|v| v.as_array_mut())
    else {
        return;
    };
    for segment in segments {
        scale_time_field(segment, "start", factor);
        scale_time_field(segment, "end", factor);
        if let Some(words) = segment.get_mut("words").and_then(|v| v.as_array_mut()) {
            for word in words {
                scale_time_field(word, "start", factor);
                scale_time_field(word, "end", factor);
            }
        }
    }
}

pub fn shift_key(
    file_hash: &str,
    key: &str,
    pitch_ratio: f64,
    key_offset: i32,
) -> Result<ShiftResult, UtaStudioError> {
    let Some(mut song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err("Song not found".into());
    };
    if song.usdx.is_some() {
        return Err("Key shift is not supported for USDX songs".into());
    }
    let cache = CacheDir::new();
    let target_key = key.trim().to_string();
    if target_key.is_empty() {
        return Err("target key cannot be empty".into());
    }
    if song.no_stems {
        let target_tempo = normalize_tempo(song.tempo);
        return no_stems_shift(
            &cache,
            file_hash,
            song,
            target_key,
            key_offset,
            target_tempo,
        );
    }
    let target_tempo = normalize_tempo(song.tempo);
    if is_base_original_selection(&song, &target_key, target_tempo) {
        song.override_key = None;
        song.tempo = 1.0;
        song.key_offset = 0;
        library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;
        return Ok(ShiftResult {
            key: target_key,
            tempo: 1.0,
        });
    }

    let canonical_target_inst = cache.variant_instrumental_path(file_hash, &target_key, 1.0);
    let canonical_target_voc = cache.variant_vocals_path(file_hash, &target_key, 1.0);
    let target_inst = cache.variant_instrumental_path(file_hash, &target_key, target_tempo);
    let target_voc = cache.variant_vocals_path(file_hash, &target_key, target_tempo);
    if target_inst.is_file() && target_voc.is_file() {
        song.override_key = if song.key.as_deref() == Some(target_key.as_str()) {
            None
        } else {
            Some(target_key.clone())
        };
        song.tempo = target_tempo;
        song.key_offset = key_offset;
        library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;
        return Ok(ShiftResult {
            key: target_key,
            tempo: target_tempo,
        });
    }
    let canonical_target_exists = canonical_target_inst.is_file() && canonical_target_voc.is_file();
    let target_is_original_key = song.key.as_deref() == Some(target_key.as_str());
    let canonical_for_target = if target_is_original_key && !canonical_target_exists {
        resolve_canonical_stems_for_key(&cache, file_hash, &song, &target_key)?
    } else {
        (canonical_target_inst.clone(), canonical_target_voc.clone())
    };

    if !canonical_target_exists && !target_is_original_key {
        let source_key = song
            .override_key
            .clone()
            .or(song.key.clone())
            .ok_or_else(|| UtaStudioError::Other("No source key available".into()))?;
        let (source_inst, source_voc) =
            resolve_canonical_stems_for_key(&cache, file_hash, &song, &source_key)?;
        run_rubberband_pair_parallel(
            &source_inst,
            &canonical_target_inst,
            &source_voc,
            &canonical_target_voc,
            pitch_ratio,
            1.0,
        )?;
    }
    let needs_tempo_transform = target_tempo != 1.0;
    let needs_canonical_copy_from_fallback =
        target_tempo == 1.0 && target_is_original_key && !canonical_target_exists;
    if needs_tempo_transform || needs_canonical_copy_from_fallback {
        run_rubberband_pair_parallel(
            &canonical_for_target.0,
            &target_inst,
            &canonical_for_target.1,
            &target_voc,
            1.0,
            target_tempo,
        )?;
    }

    song.override_key = if song.key.as_deref() == Some(target_key.as_str()) {
        None
    } else {
        Some(target_key.clone())
    };
    song.tempo = target_tempo;
    song.key_offset = key_offset;
    library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;

    Ok(ShiftResult {
        key: target_key,
        tempo: target_tempo,
    })
}

pub fn shift_tempo(file_hash: &str, tempo: f64) -> Result<ShiftResult, UtaStudioError> {
    let Some(mut song) = library_db::load_song_by_hash(file_hash).ok().flatten() else {
        return Err("Song not found".into());
    };
    if song.usdx.is_some() {
        return Err("Tempo shift is not supported for USDX songs".into());
    }
    let cache = CacheDir::new();
    if song.no_stems {
        let key_offset = song.key_offset;
        let key = song
            .override_key
            .clone()
            .or(song.key.clone())
            .ok_or_else(|| {
                UtaStudioError::Other("Key detection still in progress; try again shortly".into())
            })?;
        return no_stems_shift(
            &cache,
            file_hash,
            song,
            key,
            key_offset,
            normalize_tempo(tempo),
        );
    }
    let key = song
        .override_key
        .clone()
        .or(song.key.clone())
        .ok_or_else(|| UtaStudioError::Other("No key available (re-analyze first)".into()))?;
    let target_tempo = normalize_tempo(tempo);
    let is_default_combo = is_base_original_selection(&song, &key, target_tempo);

    // Hard short-circuit rule:
    // If the target key/tempo assets exist, update only the database selection.
    let has_target_pair = variant_pair_exists(&cache, file_hash, &key, target_tempo)
        || (is_default_combo && base_pair_exists(&cache, file_hash));
    if has_target_pair {
        song.tempo = target_tempo;
        if is_default_combo && song.override_key.as_deref() == song.key.as_deref() {
            song.override_key = None;
        }
        library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;
        return Ok(ShiftResult {
            key,
            tempo: target_tempo,
        });
    }

    if is_default_combo {
        song.tempo = 1.0;
        if song.override_key.as_deref() == song.key.as_deref() {
            song.override_key = None;
        }
        library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;
        return Ok(ShiftResult { key, tempo: 1.0 });
    }
    let source_tempo = 1.0;
    let tempo_ratio = target_tempo / source_tempo;
    let target_inst = cache.variant_instrumental_path(file_hash, &key, target_tempo);
    let target_voc = cache.variant_vocals_path(file_hash, &key, target_tempo);
    let target_transcript_path = cache.variant_transcript_path(file_hash, target_tempo);

    let (source_inst, source_voc) =
        resolve_canonical_stems_for_key(&cache, file_hash, &song, &key)?;
    run_rubberband_pair_parallel(
        &source_inst,
        &target_inst,
        &source_voc,
        &target_voc,
        1.0,
        tempo_ratio,
    )?;

    let source_transcript_path = resolve_source_transcript_path(&cache, file_hash, source_tempo);
    let source_transcript_data = std::fs::read_to_string(&source_transcript_path)?;
    let mut source_transcript: Value = serde_json::from_str(&source_transcript_data)?;
    let scale_factor = source_tempo / target_tempo;
    scale_transcript_timestamps(&mut source_transcript, scale_factor);
    source_transcript["tempo"] = Value::from(target_tempo);
    source_transcript["key"] = Value::from(key.clone());
    std::fs::write(
        &target_transcript_path,
        serde_json::to_string_pretty(&source_transcript)?,
    )?;

    song.tempo = target_tempo;
    library_db::update_song_fields(file_hash, &song).map_err(|e| e.to_string())?;

    Ok(ShiftResult {
        key,
        tempo: target_tempo,
    })
}

pub fn shift_key_done_payload(
    file_hash: String,
    key: String,
    pitch_ratio: f64,
    key_offset: i32,
) -> ShiftDone {
    match shift_key(&file_hash, &key, pitch_ratio, key_offset) {
        Ok(done) => ShiftDone {
            file_hash,
            key: Some(done.key),
            tempo: Some(done.tempo),
            error: None,
        },
        Err(err) => ShiftDone {
            file_hash,
            key: Some(key),
            tempo: None,
            error: Some(err.to_string()),
        },
    }
}

pub fn shift_tempo_done_payload(file_hash: String, tempo: f64) -> ShiftDone {
    match shift_tempo(&file_hash, tempo) {
        Ok(done) => ShiftDone {
            file_hash,
            key: Some(done.key),
            tempo: Some(done.tempo),
            error: None,
        },
        Err(err) => ShiftDone {
            file_hash,
            key: None,
            tempo: Some(tempo),
            error: Some(err.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::original_media_path;
    use std::path::Path;

    #[test]
    fn usdx_chart_text_never_becomes_the_original_audio_source() {
        let chart = Path::new("song.txt");
        let declared_audio = Path::new("song.flac");
        assert_eq!(
            original_media_path(chart, Some(declared_audio)),
            declared_audio
        );
        assert_eq!(original_media_path(declared_audio, None), declared_audio);
    }
}
