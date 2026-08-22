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
