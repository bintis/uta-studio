use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_AUDIO_SECONDS: usize = 4 * 60 * 60;

fn ffmpeg_path() -> Result<PathBuf, String> {
    std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join("ffmpeg"))
                    .find(|path| path.is_file())
            })
        })
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
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    Ok(samples)
}
