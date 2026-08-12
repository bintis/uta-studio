use std::path::Path;

use crate::{
    error::UtaStudioError,
    vendor::{ffmpeg_path, silent_command},
};

pub(crate) fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub(crate) fn browser_can_decode(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "mp3" | "flac" | "wav" | "wave" | "ogg" | "oga" | "opus" | "m4a" | "aac"
    )
}

pub(crate) fn is_lossless(path: &Path) -> bool {
    let extension = extension(path);
    if matches!(
        extension.as_str(),
        "flac" | "wav" | "wave" | "aif" | "aiff" | "alac" | "ape" | "wv" | "tta"
    ) {
        return true;
    }
    if !matches!(extension.as_str(), "m4a" | "mp4" | "mov" | "mkv" | "wma") {
        return false;
    }
    let ffmpeg = ffmpeg_path();
    if !ffmpeg.is_file() {
        return false;
    }
    let Ok(output) = silent_command(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .output()
    else {
        return false;
    };
    let details = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    [
        "audio: alac",
        "audio: flac",
        "audio: pcm_",
        "audio: wavpack",
        "audio: ape",
        "audio: tta",
    ]
    .iter()
    .any(|codec| details.contains(codec))
}

pub(crate) fn export_extension(path: &Path) -> &'static str {
    if is_lossless(path) { "flac" } else { "mp3" }
}

pub(crate) fn media_type(path: &Path) -> &'static str {
    match extension(path).as_str() {
        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" | "wave" => "audio/wav",
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

pub(crate) fn transcode_audio(source: &Path, target: &Path) -> Result<(), UtaStudioError> {
    let target_extension = extension(target);
    if extension(source) == target_extension {
        std::fs::copy(source, target)?;
        return Ok(());
    }
    let mut command = silent_command(ffmpeg_path());
    command.args(["-y", "-i"]).arg(source).arg("-vn");
    if target_extension == "flac" {
        command.args(["-c:a", "flac", "-compression_level", "8"]);
    } else if target_extension == "mp3" {
        command.args(["-c:a", "libmp3lame", "-q:a", "2"]);
    } else {
        return Err(UtaStudioError::Other(format!(
            "unsupported audio export target: {target_extension}"
        )));
    }
    let status = command.args(["-v", "error"]).arg(target).status()?;
    if status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(target);
        Err(UtaStudioError::Other(format!(
            "ffmpeg could not create {} audio ({status})",
            target_extension.to_ascii_uppercase()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_sources_target_flac_and_lossy_sources_target_mp3() {
        assert_eq!(export_extension(Path::new("track.wav")), "flac");
        assert_eq!(export_extension(Path::new("track.flac")), "flac");
        assert_eq!(export_extension(Path::new("track.mp3")), "mp3");
        assert_eq!(export_extension(Path::new("track.opus")), "mp3");
    }
}
