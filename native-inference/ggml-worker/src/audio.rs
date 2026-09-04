use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STEREO_RESIDUAL_FILTER: &str =
    "[0:a][1:a]amerge=inputs=2[merged];[merged]pan=stereo|c0=c0-c2|c1=c1-c3[out]";

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

fn run_ffmpeg(arguments: &mut Command, label: &str) -> Result<(), String> {
    let output = arguments
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("could not start ffmpeg for {label}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg {label} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn decode_wav(
    source: &Path,
    output_dir: &Path,
    task_id: &str,
    sample_rate: &str,
    channels: &str,
) -> Result<PathBuf, String> {
    if !source.is_file() {
        return Err("GGML input audio is unavailable".to_string());
    }
    let destination = output_dir.join(format!("{task_id}-ggml-input.wav"));
    if destination.exists() {
        return Err("GGML decoded input target already exists".to_string());
    }
    let mut command = Command::new(ffmpeg_path()?);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-vn",
            "-ar",
            sample_rate,
            "-ac",
            channels,
            "-c:a",
            "pcm_f32le",
            "-f",
            "wav",
        ])
        .arg(&destination);
    if let Err(error) = run_ffmpeg(&mut command, "GGML input decode") {
        let _ = std::fs::remove_file(&destination);
        return Err(error);
    }
    if !destination.is_file() {
        return Err("ffmpeg did not publish the GGML input WAV".to_string());
    }
    Ok(destination)
}

pub fn decode_stereo_wav(
    source: &Path,
    output_dir: &Path,
    task_id: &str,
) -> Result<PathBuf, String> {
    decode_wav(source, output_dir, task_id, "44100", "2")
}

pub fn decode_mono_wav(source: &Path, output_dir: &Path, task_id: &str) -> Result<PathBuf, String> {
    decode_wav(source, output_dir, task_id, "16000", "1")
}

pub fn encode_flac(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_file() || destination.exists() {
        return Err("GGML FLAC publication paths are invalid".to_string());
    }
    let temporary = destination.with_extension("flac.tmp");
    let mut command = Command::new(ffmpeg_path()?);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args([
            "-vn", "-ar", "44100", "-ac", "2", "-c:a", "flac", "-f", "flac",
        ])
        .arg(&temporary);
    if let Err(error) = run_ffmpeg(&mut command, "GGML FLAC encode") {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, destination)
        .map_err(|error| format!("could not atomically publish GGML FLAC: {error}"))
}

pub fn encode_residual_flac(
    mixture: &Path,
    estimate: &Path,
    destination: &Path,
    label: &str,
) -> Result<(), String> {
    if !mixture.is_file() || !estimate.is_file() || destination.exists() {
        return Err(format!("{label} residual publication paths are invalid"));
    }
    let temporary = destination.with_extension("flac.tmp");
    let mut command = Command::new(ffmpeg_path()?);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(mixture)
        .args(["-i"])
        .arg(estimate)
        .args([
            "-filter_complex",
            STEREO_RESIDUAL_FILTER,
            "-map",
            "[out]",
            "-ar",
            "44100",
            "-ac",
            "2",
            "-c:a",
            "flac",
            "-f",
            "flac",
        ])
        .arg(&temporary);
    if let Err(error) = run_ffmpeg(&mut command, label) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, destination)
        .map_err(|error| format!("could not atomically publish {label}: {error}"))
}

pub fn encode_vocal_residual_flac(
    all_vocals: &Path,
    lead_vocal: &Path,
    destination: &Path,
) -> Result<(), String> {
    encode_residual_flac(
        all_vocals,
        lead_vocal,
        destination,
        "Karaoke vocal-residual encode",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocal_residual_filter_subtracts_each_lead_channel() {
        assert!(STEREO_RESIDUAL_FILTER.contains("c0=c0-c2"));
        assert!(STEREO_RESIDUAL_FILTER.contains("c1=c1-c3"));
        assert!(!STEREO_RESIDUAL_FILTER.contains("amix"));
    }
}
