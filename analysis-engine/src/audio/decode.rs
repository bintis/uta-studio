use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::contract::{
    CANONICAL_TIMEBASE, DecodedAudioFactsV1, EngineError, EngineErrorCode, EngineResult,
};
use crate::execution::CancellationToken;

const FALLBACK_SAMPLE_RATE: u32 = 48_000;
const MAX_AUDIO_SECONDS: u64 = 4 * 60 * 60;
const MAX_CHANNELS: u16 = 32;
const MAX_SAMPLE_RATE: u32 = 384_000;

#[derive(Debug, Deserialize)]
struct ProbeDocument {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
}

#[derive(Debug)]
struct SourceFacts {
    container: String,
    codec: String,
    sample_rate: u32,
    channels: u16,
}

/// Facts for the Engine's canonical analysis decode. The source file remains
/// untouched; model-specific workers may derive their own sample-rate views.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub facts: DecodedAudioFactsV1,
}

pub fn decode_audio(ffmpeg: &Path, source_id: &str, source: &Path) -> EngineResult<DecodedAudio> {
    decode_audio_with_cancellation(ffmpeg, source_id, source, &CancellationToken::default())
}

pub(crate) fn decode_audio_with_cancellation(
    ffmpeg: &Path,
    source_id: &str,
    source: &Path,
    cancellation: &CancellationToken,
) -> EngineResult<DecodedAudio> {
    if !ffmpeg.is_file() {
        return Err(EngineError::new(
            EngineErrorCode::WorkerUnavailable,
            "packaged ffmpeg is unavailable for audio decode",
        )
        .with_resource("tool:ffmpeg"));
    }
    if !source.is_file() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            format!("audio source is unavailable: {}", source.display()),
        ));
    }

    let source_facts = probe_source(ffmpeg, source).unwrap_or(SourceFacts {
        container: "raw".to_string(),
        codec: "pcm_f32le".to_string(),
        sample_rate: FALLBACK_SAMPLE_RATE,
        channels: 1,
    });
    let mut command = Command::new(ffmpeg);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-vn",
            "-ac",
            &source_facts.channels.to_string(),
            "-ar",
            &source_facts.sample_rate.to_string(),
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not start packaged ffmpeg: {error}"),
        )
    })?;

    let mut stdout = child.stdout.take().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InternalError,
            "ffmpeg decode stdout was not captured",
        )
    })?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(2);
    let stdout_reader = std::thread::spawn(move || {
        loop {
            let mut buffer = vec![0_u8; 64 * 1024];
            match stdout.read(&mut buffer) {
                Ok(0) => {
                    let _ = stdout_sender.send(Ok(Vec::new()));
                    break;
                }
                Ok(count) => {
                    buffer.truncate(count);
                    if stdout_sender.send(Ok(buffer)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = stdout_sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let mut stderr = child.stderr.take().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InternalError,
            "ffmpeg decode stderr was not captured",
        )
    })?;
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        // Bound diagnostic capture while continuing to drain the pipe.
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) if bytes.len() < 64 * 1024 => {
                    let remaining = 64 * 1024 - bytes.len();
                    bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                }
                Ok(_) => {}
            }
        }
        bytes
    });

    let mut carry = Vec::with_capacity(3);
    let mut sample_count = 0_u64;
    let max_samples = u64::from(source_facts.sample_rate)
        .saturating_mul(MAX_AUDIO_SECONDS)
        .saturating_mul(u64::from(source_facts.channels));
    let mut peak = 0.0_f32;
    let mut invalid_sample = false;
    let mut read_error = None;
    let mut was_cancelled = false;
    loop {
        if cancellation.is_cancelled() {
            was_cancelled = true;
            kill_decode_process(&mut child);
            break;
        }
        let bytes = match stdout_receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(Ok(bytes)) if bytes.is_empty() => break,
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                read_error = Some(error);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        carry.extend_from_slice(&bytes);
        let complete = carry.len() / 4 * 4;
        for sample in carry[..complete].chunks_exact(4) {
            let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            if !value.is_finite() {
                invalid_sample = true;
                break;
            }
            peak = peak.max(value.abs());
            sample_count += 1;
            if sample_count > max_samples {
                break;
            }
        }
        if complete > 0 {
            carry.drain(..complete);
        }
        if invalid_sample || sample_count > max_samples {
            kill_decode_process(&mut child);
            break;
        }
    }

    drop(stdout_receiver);
    let status = child.wait().map_err(|error| {
        EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not wait for ffmpeg decode: {error}"),
        )
    })?;
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    if was_cancelled {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "audio decode was cancelled",
        ));
    }
    if let Some(error) = read_error {
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not read decoded audio: {error}"),
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr).trim().to_string();
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            if detail.is_empty() {
                format!("ffmpeg audio decode failed with {status}")
            } else {
                format!("ffmpeg audio decode failed: {detail}")
            },
        ));
    }
    if !carry.is_empty()
        || sample_count == 0
        || !sample_count.is_multiple_of(u64::from(source_facts.channels))
    {
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            "decoded audio is empty or has a malformed sample payload",
        ));
    }
    if invalid_sample {
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            "decoded audio contains non-finite samples",
        ));
    }
    if sample_count > max_samples {
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            "decoded audio exceeds the four-hour Engine v1 limit",
        ));
    }
    let frame_count = sample_count / u64::from(source_facts.channels);
    let duration = frame_count
        .checked_mul(CANONICAL_TIMEBASE as u64)
        .and_then(|value| value.checked_add(u64::from(source_facts.sample_rate / 2)))
        .map(|value| value / u64::from(source_facts.sample_rate))
        .ok_or_else(|| {
            EngineError::new(
                EngineErrorCode::TimelineInvalid,
                "decoded duration overflows the canonical timeline",
            )
        })?;

    Ok(DecodedAudio {
        facts: DecodedAudioFactsV1 {
            source_id: source_id.to_string(),
            container: source_facts.container,
            codec: source_facts.codec,
            sample_rate: source_facts.sample_rate,
            channels: source_facts.channels,
            frame_count,
            duration,
            peak,
            decode_backend: ffmpeg_identity(ffmpeg),
        },
    })
}

pub(crate) fn extract_audio_window(
    ffmpeg: &Path,
    source: &Path,
    output: &Path,
    source_offset: u64,
    duration: u64,
    cancellation: &CancellationToken,
) -> EngineResult<()> {
    if duration == 0 {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "conditional audio window has zero duration",
        ));
    }
    let parent = output.parent().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InternalError,
            "conditional audio window has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        EngineError::new(
            EngineErrorCode::InternalError,
            format!("could not create conditional audio directory: {error}"),
        )
    })?;
    let offset = format!(
        "{:.6}",
        source_offset as f64 / f64::from(CANONICAL_TIMEBASE)
    );
    let duration = format!("{:.6}", duration as f64 / f64::from(CANONICAL_TIMEBASE));
    let mut command = Command::new(ffmpeg);
    command
        .args(["-v", "error", "-nostdin", "-ss", &offset, "-i"])
        .arg(source)
        .args([
            "-t",
            &duration,
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-vn",
            "-c:a",
            "flac",
            "-y",
        ])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not start ffmpeg conditional extraction: {error}"),
        )
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        EngineError::new(
            EngineErrorCode::InternalError,
            "ffmpeg conditional extraction stderr was not captured",
        )
    })?;
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) if bytes.len() < 64 * 1024 => {
                    let remaining = 64 * 1024 - bytes.len();
                    bytes.extend_from_slice(&buffer[..count.min(remaining)]);
                }
                Ok(_) => {}
            }
        }
        bytes
    });
    let status = loop {
        if cancellation.is_cancelled() {
            kill_decode_process(&mut child);
            let _ = child.wait();
            let _ = stderr_reader.join();
            let _ = std::fs::remove_file(output);
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "conditional audio extraction was cancelled",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                kill_decode_process(&mut child);
                let _ = child.wait();
                let _ = stderr_reader.join();
                let _ = std::fs::remove_file(output);
                return Err(EngineError::new(
                    EngineErrorCode::DecodeFailed,
                    format!("could not wait for conditional audio extraction: {error}"),
                ));
            }
        }
    };
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() || !output.is_file() {
        let _ = std::fs::remove_file(output);
        let detail = String::from_utf8_lossy(&stderr).trim().to_string();
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            if detail.is_empty() {
                format!("ffmpeg conditional audio extraction failed with {status}")
            } else {
                format!("ffmpeg conditional audio extraction failed: {detail}")
            },
        ));
    }
    Ok(())
}

fn kill_decode_process(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn probe_source(ffmpeg: &Path, source: &Path) -> Option<SourceFacts> {
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    if !ffprobe.is_file() {
        return None;
    }
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type,codec_name,sample_rate,channels:format=format_name",
            "-of",
            "json",
        ])
        .arg(source)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return None;
    }
    let document: ProbeDocument = serde_json::from_slice(&output.stdout).ok()?;
    let stream = document
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))?;
    let sample_rate = stream.sample_rate?.parse::<u32>().ok()?;
    let channels = stream.channels?;
    if sample_rate == 0 || sample_rate > MAX_SAMPLE_RATE || channels == 0 || channels > MAX_CHANNELS
    {
        return None;
    }
    Some(SourceFacts {
        container: document
            .format
            .and_then(|format| format.format_name)
            .and_then(|name| name.split(',').next().map(canonical_container))
            .unwrap_or_else(|| "unknown".to_string()),
        codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
        sample_rate,
        channels,
    })
}

fn canonical_container(name: &str) -> String {
    match name {
        "matroska" | "webm" => "matroska",
        "mov" | "mp4" | "m4a" | "3gp" | "3g2" | "mj2" => "mp4",
        "ogg" => "ogg",
        "wav" => "wav",
        "flac" => "flac",
        "mp3" => "mp3",
        other => other,
    }
    .to_string()
}

fn ffmpeg_identity(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ffmpeg");
    format!("packaged:{name}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[cfg(unix)]
    fn fake_ffmpeg(root: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join("ffmpeg-fixture");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn configured_ffmpeg() -> Option<PathBuf> {
        std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
    }

    fn write_pcm16_wav(path: &Path, sample_rate: u32, channels: u16, frames: u32) {
        let data_bytes = frames * u32::from(channels) * 2;
        let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for frame in 0..frames {
            let value = (((frame % 32) as i32 - 16) * 1_000) as i16;
            for _ in 0..channels {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-analysis-decode-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    #[cfg(unix)]
    fn computes_canonical_facts_from_streamed_samples() {
        let root = temp_root();
        let source = root.join("source.wav");
        std::fs::write(&source, b"fixture").unwrap();
        let ffmpeg = fake_ffmpeg(&root, "printf '\\000\\000\\000\\077\\000\\000\\000\\277'");
        let decoded = decode_audio(&ffmpeg, "main", &source).unwrap();
        assert_eq!(decoded.facts.frame_count, 2);
        assert_eq!(decoded.facts.sample_rate, 48_000);
        assert_eq!(decoded.facts.channels, 1);
        assert_eq!(decoded.facts.duration, 42);
        assert_eq!(decoded.facts.peak, 0.5);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn rejects_empty_and_non_finite_decode_output() {
        let root = temp_root();
        let source = root.join("source.wav");
        std::fs::write(&source, b"fixture").unwrap();
        let empty = fake_ffmpeg(&root, "exit 0");
        assert_eq!(
            decode_audio(&empty, "main", &source).unwrap_err().code,
            EngineErrorCode::DecodeFailed
        );
        let invalid = fake_ffmpeg(&root, "printf '\\000\\000\\300\\177'");
        assert_eq!(
            decode_audio(&invalid, "main", &source).unwrap_err().code,
            EngineErrorCode::DecodeFailed
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_kills_and_reaps_a_stalled_decoder_group() {
        let root = temp_root();
        let source = root.join("source.wav");
        std::fs::write(&source, b"fixture").unwrap();
        let ffmpeg = fake_ffmpeg(&root, "sleep 30");
        let cancellation = CancellationToken::default();
        let thread_token = cancellation.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            thread_token.cancel();
        });
        let started = std::time::Instant::now();
        let error =
            decode_audio_with_cancellation(&ffmpeg, "main", &source, &cancellation).unwrap_err();
        canceller.join().unwrap();
        assert_eq!(error.code, EngineErrorCode::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packaged_decoder_preserves_channel_facts_and_decodes_flac_and_mp3() {
        let Some(ffmpeg) = configured_ffmpeg() else {
            return;
        };
        let root = temp_root();
        let mono = root.join("mono.wav");
        let stereo = root.join("stereo.wav");
        write_pcm16_wav(&mono, 44_100, 1, 441);
        write_pcm16_wav(&stereo, 22_050, 2, 220);

        let mono_facts = decode_audio(&ffmpeg, "mono", &mono).unwrap().facts;
        assert_eq!(mono_facts.sample_rate, 44_100);
        assert_eq!(mono_facts.channels, 1);
        assert_eq!(mono_facts.frame_count, 441);
        assert_eq!(mono_facts.container, "wav");

        let stereo_facts = decode_audio(&ffmpeg, "stereo", &stereo).unwrap().facts;
        assert_eq!(stereo_facts.sample_rate, 22_050);
        assert_eq!(stereo_facts.channels, 2);
        assert_eq!(stereo_facts.frame_count, 220);

        let flac = root.join("mono.flac");
        let status = Command::new(&ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&mono)
            .args(["-c:a", "flac", "-y"])
            .arg(&flac)
            .status()
            .unwrap();
        assert!(status.success());
        let flac_facts = decode_audio(&ffmpeg, "flac", &flac).unwrap().facts;
        assert_eq!(flac_facts.container, "flac");
        assert_eq!(flac_facts.codec, "flac");
        assert_eq!(flac_facts.frame_count, 441);

        let mp3 = root.join("mono.mp3");
        let status = Command::new(&ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&mono)
            .args(["-c:a", "libmp3lame", "-y"])
            .arg(&mp3)
            .status()
            .unwrap();
        assert!(status.success());
        let mp3_facts = decode_audio(&ffmpeg, "mp3", &mp3).unwrap().facts;
        assert_eq!(mp3_facts.container, "mp3");
        assert_eq!(mp3_facts.codec, "mp3");
        assert!(mp3_facts.frame_count > 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
