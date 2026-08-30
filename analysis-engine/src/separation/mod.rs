use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::artifact::artifact_ref_for_existing;
use crate::audio::decode_audio;
use crate::contract::{ArtifactRefV1, AudioRole, EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;

const MAX_LOG_BYTES: usize = 1024 * 1024;
const PROCESS_POLL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone)]
pub struct SeparationTask<'a> {
    pub model_id: &'a str,
    pub model_path: &'a Path,
    pub executable: &'a Path,
    pub ffmpeg: &'a Path,
    pub input: &'a Path,
    pub output_root: &'a Path,
    pub output_role: AudioRole,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct SeparationOutput {
    pub role: AudioRole,
    pub artifact: ArtifactRefV1,
}

/// Publishes a caller-supplied semantic Step 1 source into this run's
/// output. Reused inputs remain read-only and are re-materialized as FLAC so
/// the result manifest is self-contained just like a freshly executed run.
pub fn materialize_semantic_stem(
    ffmpeg: &Path,
    input: &Path,
    output_root: &Path,
    role: AudioRole,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    let root = output_root.canonicalize().map_err(|error| {
        failure(format!(
            "could not authorize semantic stem output root: {error}"
        ))
    })?;
    if !input.is_file() {
        return Err(failure("semantic stem source is unavailable"));
    }
    let semantic = role.as_str();
    let source_facts = decode_audio(ffmpeg, "semantic-stem-source", input)?.facts;
    let work = root.join(format!("worker/semantic-{semantic}-materialize"));
    if work.exists() {
        return Err(failure(
            "semantic stem materialization directory already exists",
        ));
    }
    std::fs::create_dir_all(&work).map_err(|error| {
        failure(format!(
            "could not create semantic lead materialization directory: {error}"
        ))
    })?;
    let _temporary = TemporaryDirectory(work.clone());
    let staged = work.join(format!("{semantic}.flac"));
    run_command(
        command(ffmpeg, |command| {
            command
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(input)
                .args([
                    "-map",
                    "0:a:0",
                    "-map_metadata",
                    "-1",
                    "-vn",
                    "-c:a",
                    "flac",
                    "-compression_level",
                    "5",
                    "-y",
                ])
                .arg(&staged);
        }),
        Duration::from_secs(4 * 60 * 60),
        cancellation,
        "semantic stem materialization",
    )?;
    let output_facts = decode_audio(ffmpeg, "semantic-stem-artifact", &staged)?.facts;
    if output_facts.frame_count == 0
        || output_facts.duration.abs_diff(source_facts.duration) > 2_000
    {
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "semantic stem artifact did not preserve the declared source timeline",
        ));
    }
    if cancellation.is_cancelled() {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "semantic lead materialization was cancelled",
        ));
    }
    let relative = PathBuf::from(format!("stems/{semantic}.flac"));
    let destination = root.join(&relative);
    let parent = destination
        .parent()
        .ok_or_else(|| failure("semantic lead artifact target has no parent"))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| failure(format!("could not create stem directory: {error}")))?;
    if destination.exists() {
        return Err(failure("semantic stem target already exists"));
    }
    std::fs::rename(&staged, &destination).map_err(|error| {
        failure(format!(
            "could not atomically publish semantic stem: {error}"
        ))
    })?;
    Ok(SeparationOutput {
        role,
        artifact: artifact_ref_for_existing(output_root, &relative, "audio/flac")?,
    })
}

pub fn run_separation(
    task: &SeparationTask<'_>,
    cancellation: &CancellationToken,
) -> EngineResult<SeparationOutput> {
    validate_semantics(task)?;
    let root = task.output_root.canonicalize().map_err(|error| {
        failure(format!(
            "could not authorize separation output root: {error}"
        ))
    })?;
    let work = root.join(format!("worker/separation-{}", task.output_role.as_str()));
    if work.exists() {
        return Err(failure("separation task directory already exists"));
    }
    std::fs::create_dir_all(&work).map_err(|error| {
        failure(format!(
            "could not create separation task directory: {error}"
        ))
    })?;
    let temporary = TemporaryDirectory(work.clone());
    let model = model_file(task.model_path)?;
    let input_facts = decode_audio(task.ffmpeg, "separation-input", task.input)?.facts;
    let prepared = work.join("input.wav");
    run_command(
        command(task.ffmpeg, |command| {
            command
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(task.input)
                .args([
                    "-map_metadata",
                    "-1",
                    "-vn",
                    "-ar",
                    "44100",
                    "-ac",
                    "2",
                    "-c:a",
                    "pcm_f32le",
                    "-y",
                ])
                .arg(&prepared);
        }),
        task.timeout,
        cancellation,
        "audio preparation",
    )?;
    let separated = work.join("separated.wav");
    run_command(
        command(task.executable, |command| {
            command.arg(&model).arg(&prepared).arg(&separated).args([
                "--batch-size",
                "1",
                "--vulkan-device",
                "0",
            ]);
        }),
        task.timeout,
        cancellation,
        "RoFormer separation",
    )?;
    if !separated.is_file() {
        return Err(failure("RoFormer did not produce its declared WAV output"));
    }

    if input_facts.container == "raw" {
        return Err(EngineError::new(
            EngineErrorCode::OutputValidationFailed,
            "source codec identity is unavailable; refusing to guess FLAC/MP3 policy",
        ));
    }
    let lossy = matches!(
        input_facts.codec.as_str(),
        "mp3" | "aac" | "opus" | "vorbis" | "ac3" | "eac3"
    );
    let extension = if lossy { "mp3" } else { "flac" };
    let media_type = if lossy { "audio/mpeg" } else { "audio/flac" };
    let relative =
        PathBuf::from("stems").join(format!("{}.{}", task.output_role.as_str(), extension));
    let destination = root.join(&relative);
    let parent = destination.parent().expect("stem has parent");
    std::fs::create_dir_all(parent)
        .map_err(|error| failure(format!("could not create stem directory: {error}")))?;
    if destination.exists() {
        return Err(failure("separation stem target already exists"));
    }
    let staged = parent.join(format!(
        ".{}.tmp-{}.{}",
        task.output_role.as_str(),
        std::process::id(),
        extension
    ));
    let encoding = if lossy {
        vec!["-c:a", "libmp3lame", "-q:a", "2"]
    } else {
        vec!["-c:a", "flac"]
    };
    let encode_result = run_command(
        command(task.ffmpeg, |command| {
            command
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(&separated)
                .args(["-map_metadata", "-1", "-vn"])
                .args(&encoding)
                .arg("-y")
                .arg(&staged);
        }),
        task.timeout,
        cancellation,
        "stem encoding",
    );
    if let Err(error) = encode_result {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    let output_facts = decode_audio(task.ffmpeg, "separation-output", &staged)?.facts;
    if output_facts.duration.abs_diff(input_facts.duration) > 2_000 {
        let _ = std::fs::remove_file(&staged);
        return Err(EngineError::new(
            EngineErrorCode::TimelineInvalid,
            "separation output did not preserve the source timeline",
        ));
    }
    std::fs::rename(&staged, &destination).map_err(|error| {
        let _ = std::fs::remove_file(&staged);
        failure(format!(
            "could not atomically publish separated stem: {error}"
        ))
    })?;
    let artifact = artifact_ref_for_existing(&root, &relative, media_type)?;
    drop(temporary);
    Ok(SeparationOutput {
        role: task.output_role,
        artifact,
    })
}

fn validate_semantics(task: &SeparationTask<'_>) -> EngineResult<()> {
    let valid = matches!(
        (task.model_id, task.output_role),
        ("bs_roformer_leap_xe90_vocals", AudioRole::GuideVocals)
            | (
                "bs_polarformer_public_instrumental",
                AudioRole::Instrumental
            )
            | ("melband_roformer_harmony", AudioRole::LeadVocal)
            | ("melband_roformer_denoise_aufr33", AudioRole::CleanLeadVocal)
            | (
                "melband_roformer_dereverb_anvuew",
                AudioRole::CleanLeadVocal
            )
    );
    if !valid
        || !task.executable.is_file()
        || !task.ffmpeg.is_file()
        || !task.input.is_file()
        || !task.output_root.is_dir()
        || task.timeout.is_zero()
    {
        return Err(EngineError::new(
            EngineErrorCode::InvalidContract,
            "separation model semantics, executable, input, output, or timeout is invalid",
        ));
    }
    Ok(())
}

fn model_file(path: &Path) -> EngineResult<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if !path.is_dir() {
        return Err(failure("resolved RoFormer model path is unavailable"));
    }
    let mut models = std::fs::read_dir(path)
        .map_err(|error| failure(format!("could not inspect RoFormer model: {error}")))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate.is_file()
                && candidate.extension().and_then(|value| value.to_str()) == Some("gguf")
        })
        .collect::<Vec<_>>();
    models.sort();
    if models.len() != 1 {
        return Err(failure(
            "resolved RoFormer generation must contain exactly one GGUF entrypoint",
        ));
    }
    Ok(models.remove(0))
}

fn command(executable: &Path, configure: impl FnOnce(&mut Command)) -> Command {
    let mut command = Command::new(executable);
    configure(&mut command);
    command
}

fn run_command(
    mut command: Command,
    timeout: Duration,
    cancellation: &CancellationToken,
    label: &str,
) -> EngineResult<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| failure(format!("could not start native {label} process: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| failure("stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| failure("stderr unavailable"))?;
    let output = capture(stdout);
    let errors = capture(stderr);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if cancellation.is_cancelled() {
            terminate(&mut child);
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                format!("{label} was cancelled"),
            ));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| failure(format!("could not inspect {label}: {error}")))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            return Err(EngineError::new(
                EngineErrorCode::WorkerFailed,
                format!("{label} timed out"),
            ));
        }
        std::thread::sleep(PROCESS_POLL);
    };
    if !status.success() {
        let stderr = captured_text(&errors);
        return Err(EngineError::new(
            EngineErrorCode::WorkerFailed,
            format!("{label} failed with {status}: {stderr}"),
        ));
    }
    let _ = output;
    Ok(())
}

fn capture<R: Read + Send + 'static>(reader: R) -> Arc<Mutex<Vec<u8>>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&bytes);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buffer = [0_u8; 8192];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let mut bytes = captured.lock().unwrap_or_else(|error| error.into_inner());
            let remaining = MAX_LOG_BYTES.saturating_sub(bytes.len());
            bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        }
    });
    bytes
}

fn captured_text(bytes: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&bytes.lock().unwrap_or_else(|error| error.into_inner()))
        .trim()
        .to_string()
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: the child is the leader of a process group created above.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

fn failure(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::WorkerFailed, message)
}

struct TemporaryDirectory(PathBuf);

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-separation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_wav(path: &Path) {
        let frames = 4_410_u32;
        let channels = 2_u16;
        let data_bytes = frames * u32::from(channels) * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&44_100_u32.to_le_bytes());
        bytes.extend_from_slice(&(44_100_u32 * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        bytes.resize(44 + data_bytes as usize, 0);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn model_semantics_are_not_inferred_from_filename() {
        let task = SeparationTask {
            model_id: "bs_polarformer_public_instrumental",
            model_path: Path::new("missing"),
            executable: Path::new("missing"),
            ffmpeg: Path::new("missing"),
            input: Path::new("missing"),
            output_root: Path::new("missing"),
            output_role: AudioRole::LeadVocal,
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            validate_semantics(&task).unwrap_err().code,
            EngineErrorCode::InvalidContract
        );
    }

    #[test]
    #[cfg(unix)]
    fn real_decode_and_encoding_preserve_timeline_for_fake_native_model() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let Some(ffmpeg) = std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        else {
            return;
        };
        let root = temporary_root();
        let input = root.join("input.wav");
        let model = root.join("model.gguf");
        let executable = root.join("roformer");
        write_wav(&input);
        std::fs::write(&model, b"fixture model").unwrap();
        let staging = root.join("roformer.part");
        {
            let mut file = std::fs::File::create(&staging).unwrap();
            file.write_all(b"#!/bin/sh\ncp -- \"$2\" \"$3\"\n").unwrap();
            file.sync_all().unwrap();
        }
        let mut permissions = std::fs::metadata(&staging).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&staging, permissions).unwrap();
        std::fs::rename(staging, &executable).unwrap();
        let output = run_separation(
            &SeparationTask {
                model_id: "bs_polarformer_public_instrumental",
                model_path: &model,
                executable: &executable,
                ffmpeg: &ffmpeg,
                input: &input,
                output_root: &root,
                output_role: AudioRole::Instrumental,
                timeout: Duration::from_secs(30),
            },
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(output.artifact.media_type, "audio/flac");
        assert!(root.join(output.artifact.path).is_file());
        assert!(!root.join("worker/separation-instrumental").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cached_semantic_pair_is_republished_as_lossless_run_artifacts() {
        let Some(ffmpeg) = std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        else {
            return;
        };
        let root = temporary_root();
        let guide = root.join("cached-guide.wav");
        let instrumental = root.join("cached-instrumental.wav");
        write_wav(&guide);
        write_wav(&instrumental);

        let guide_output = materialize_semantic_stem(
            &ffmpeg,
            &guide,
            &root,
            AudioRole::GuideVocals,
            &CancellationToken::default(),
        )
        .unwrap();
        let instrumental_output = materialize_semantic_stem(
            &ffmpeg,
            &instrumental,
            &root,
            AudioRole::Instrumental,
            &CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(
            guide_output.artifact.path,
            Path::new("stems/guide_vocals.flac")
        );
        assert_eq!(
            instrumental_output.artifact.path,
            Path::new("stems/instrumental.flac")
        );
        assert!(root.join(guide_output.artifact.path).is_file());
        assert!(root.join(instrumental_output.artifact.path).is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
}
