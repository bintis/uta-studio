use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_AUDIO_SECONDS: usize = 4 * 60 * 60;

fn ffmpeg_path() -> Result<PathBuf, String> {
    std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .ok_or_else(|| "packaged ffmpeg is unavailable".to_string())
}

pub fn decode_mono(source: &Path, work_dir: &Path, sample_rate: usize) -> Result<Vec<f32>, String> {
    if !matches!(sample_rate, 16_000 | 22_050 | 24_000 | 44_100) {
        return Err("unsupported native model sample rate".to_string());
    }
    if !source.is_file() {
        return Err(format!("audio input is unavailable: {}", source.display()));
    }
    let raw_path = work_dir.join(format!("decode-{}.f32le", std::process::id()));
    let status = Command::new(ffmpeg_path()?)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-map_metadata",
            "-1",
            "-ac",
            "1",
            "-ar",
            &sample_rate.to_string(),
            "-f",
            "f32le",
            "-y",
        ])
        .arg(&raw_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not start ffmpeg: {error}"))?;
    if !status.status.success() {
        let _ = std::fs::remove_file(&raw_path);
        let detail = String::from_utf8_lossy(&status.stderr);
        return Err(format!("ffmpeg could not decode audio: {}", detail.trim()));
    }
    let size = std::fs::metadata(&raw_path)
        .map_err(|error| error.to_string())?
        .len() as usize;
    let max_bytes = sample_rate * MAX_AUDIO_SECONDS * std::mem::size_of::<f32>();
    if size == 0 || size > max_bytes || !size.is_multiple_of(std::mem::size_of::<f32>()) {
        let _ = std::fs::remove_file(&raw_path);
        return Err("decoded audio is empty, malformed, or exceeds four hours".to_string());
    }
    let mut bytes = Vec::with_capacity(size);
    let read_result = std::fs::File::open(&raw_path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&raw_path);
    read_result?;
    Ok(bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect())
}

pub fn decode_stereo(source: &Path, work_dir: &Path) -> Result<Vec<f32>, String> {
    if !source.is_file() {
        return Err(format!("audio input is unavailable: {}", source.display()));
    }
    let raw_path = work_dir.join(format!("decode-stereo-{}.f32le", std::process::id()));
    let output = Command::new(ffmpeg_path()?)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-vn",
            "-ac",
            "2",
            "-ar",
            "44100",
            "-f",
            "f32le",
            "-y",
        ])
        .arg(&raw_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not start ffmpeg: {error}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&raw_path);
        return Err(format!(
            "ffmpeg could not decode stereo audio: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let size = std::fs::metadata(&raw_path)
        .map_err(|error| error.to_string())?
        .len() as usize;
    let max_bytes = 44_100 * 2 * MAX_AUDIO_SECONDS * std::mem::size_of::<f32>();
    if size == 0 || size > max_bytes || !size.is_multiple_of(2 * std::mem::size_of::<f32>()) {
        let _ = std::fs::remove_file(&raw_path);
        return Err("decoded stereo audio is empty, malformed, or exceeds four hours".to_string());
    }
    let mut bytes = Vec::with_capacity(size);
    let read_result = std::fs::File::open(&raw_path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&raw_path);
    read_result?;
    let audio = bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    if audio.iter().any(|sample| !sample.is_finite()) {
        return Err("decoded stereo audio contains non-finite samples".to_string());
    }
    Ok(audio)
}

pub fn encode_stereo_flac(
    interleaved: &[f32],
    output_dir: &Path,
    filename: &str,
) -> Result<PathBuf, String> {
    if interleaved.is_empty() || !interleaved.len().is_multiple_of(2) || filename.contains('/') {
        return Err("stereo FLAC publication contract is invalid".to_string());
    }
    let destination = output_dir.join(filename);
    if destination.exists() {
        return Err("stereo FLAC output already exists".to_string());
    }
    let raw = output_dir.join(format!(".denoise-{}.f32le", std::process::id()));
    let staged = output_dir.join(format!(".{filename}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&raw).map_err(|error| error.to_string())?;
        for sample in interleaved {
            file.write_all(&sample.to_le_bytes())
                .map_err(|error| error.to_string())?;
        }
        file.sync_all().map_err(|error| error.to_string())?;
        let output = Command::new(ffmpeg_path()?)
            .args([
                "-v", "error", "-nostdin", "-f", "f32le", "-ar", "44100", "-ac", "2", "-i",
            ])
            .arg(&raw)
            .args([
                "-map_metadata",
                "-1",
                "-vn",
                "-c:a",
                "flac",
                "-f",
                "flac",
                "-y",
            ])
            .arg(&staged)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("could not start ffmpeg FLAC encoder: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ffmpeg could not encode Denoise FLAC: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        std::fs::rename(&staged, &destination)
            .map_err(|error| format!("could not atomically publish Denoise FLAC: {error}"))?;
        Ok(destination.clone())
    })();
    let _ = std::fs::remove_file(&raw);
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}
