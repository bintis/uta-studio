use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn ffmpeg_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Ok(path);
    }
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join("ffmpeg"))
                .find(|candidate| candidate.is_file())
        })
        .ok_or_else(|| "configured native ffmpeg is unavailable".to_string())
}

pub fn decode_wav(source: &Path, output_dir: &Path, task_id: &str) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err("Qwen input audio is unavailable".to_string());
    }
    let path = output_dir.join(format!("{task_id}-qwen-input.wav"));
    let status = Command::new(ffmpeg_path()?)
        .args(["-v", "error", "-nostdin", "-y", "-i"])
        .arg(source)
        .args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|error| format!("could not start ffmpeg: {error}"))?;
    if !status.success() || !path.is_file() {
        let _ = std::fs::remove_file(&path);
        return Err(format!("ffmpeg decode failed with {status}"));
    }
    Ok(path)
}

/// Return the exact duration of the canonical PCM WAV written by `decode_wav`.
/// RIFF chunks are parsed instead of assuming a fixed 44-byte header.
pub fn wav_duration_seconds(path: &Path) -> Result<f64, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read Qwen WAV: {error}"))?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Qwen decoded audio is not a RIFF/WAVE file".to_string());
    }
    let mut offset = 12_usize;
    let mut byte_rate = None;
    let mut data_bytes = None;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| "invalid Qwen WAV chunk length")?,
        ) as usize;
        let payload = offset + 8;
        let end = payload
            .checked_add(length)
            .ok_or_else(|| "Qwen WAV chunk length overflow".to_string())?;
        if end > bytes.len() {
            return Err("Qwen WAV contains a truncated chunk".to_string());
        }
        if id == b"fmt " {
            if length < 16 {
                return Err("Qwen WAV fmt chunk is truncated".to_string());
            }
            let format = u16::from_le_bytes(bytes[payload..payload + 2].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[payload + 2..payload + 4].try_into().unwrap());
            let sample_rate =
                u32::from_le_bytes(bytes[payload + 4..payload + 8].try_into().unwrap());
            let rate = u32::from_le_bytes(bytes[payload + 8..payload + 12].try_into().unwrap());
            let bits = u16::from_le_bytes(bytes[payload + 14..payload + 16].try_into().unwrap());
            if format != 1 || channels != 1 || sample_rate != 16_000 || bits != 16 || rate != 32_000
            {
                return Err("Qwen decoded WAV does not match mono 16 kHz PCM S16LE".to_string());
            }
            byte_rate = Some(rate);
        } else if id == b"data" {
            data_bytes = Some(length as u64);
        }
        offset = end + (length & 1);
    }
    let byte_rate = byte_rate.ok_or_else(|| "Qwen WAV has no valid fmt chunk".to_string())?;
    let data_bytes = data_bytes.ok_or_else(|| "Qwen WAV has no data chunk".to_string())?;
    if data_bytes == 0 {
        return Err("Qwen decoded WAV is empty".to_string());
    }
    Ok(data_bytes as f64 / f64::from(byte_rate))
}

/// Publish a bounded canonical WAV slice for long-form forced alignment.
pub fn slice_wav(
    source: &Path,
    output_dir: &Path,
    index: usize,
    start_seconds: f64,
    duration_seconds: f64,
) -> Result<PathBuf, String> {
    if !start_seconds.is_finite()
        || !duration_seconds.is_finite()
        || start_seconds < 0.0
        || duration_seconds <= 0.0
    {
        return Err("Qwen alignment window is invalid".to_string());
    }
    let destination = output_dir.join(format!("qwen-align-window-{index:03}.wav"));
    let temporary = destination.with_extension("wav.tmp");
    let status = Command::new(ffmpeg_path()?)
        .args(["-v", "error", "-nostdin", "-y", "-ss"])
        .arg(format!("{start_seconds:.6}"))
        .args(["-i"])
        .arg(source)
        .args(["-t"])
        .arg(format!("{duration_seconds:.6}"))
        .args([
            "-vn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(&temporary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|error| format!("could not start ffmpeg alignment slicer: {error}"))?;
    if !status.success() || !temporary.is_file() {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("ffmpeg alignment slicing failed with {status}"));
    }
    std::fs::rename(&temporary, &destination)
        .map_err(|error| format!("could not publish alignment window: {error}"))?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riff_parser_accepts_extra_chunks_and_rejects_wrong_contract() {
        let path = std::env::temp_dir().join(format!("uta-qwen-wav-{}.wav", std::process::id()));
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(58_u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"JUNK");
        wav.extend_from_slice(&(1_u32).to_le_bytes());
        wav.extend_from_slice(&[0, 0]);
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(16_u32).to_le_bytes());
        wav.extend_from_slice(&(1_u16).to_le_bytes());
        wav.extend_from_slice(&(1_u16).to_le_bytes());
        wav.extend_from_slice(&(16_000_u32).to_le_bytes());
        wav.extend_from_slice(&(32_000_u32).to_le_bytes());
        wav.extend_from_slice(&(2_u16).to_le_bytes());
        wav.extend_from_slice(&(16_u16).to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(32_000_u32).to_le_bytes());
        wav.resize(wav.len() + 32_000, 0);
        std::fs::write(&path, wav).unwrap();
        assert_eq!(wav_duration_seconds(&path).unwrap(), 1.0);
        std::fs::remove_file(path).unwrap();
    }
}
