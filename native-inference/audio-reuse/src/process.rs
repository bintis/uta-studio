use super::{Representation, StreamSelection};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Process(Child, bool);
impl Process {
    fn kill(&mut self) {
        #[cfg(unix)]
        if self.1 {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.0.kill();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            self.kill();
        }
        let _ = self.0.wait();
    }
}

pub(super) fn decode(
    ffmpeg: &Path,
    source: &Path,
    representation: Representation,
    own_process_group: bool,
    cancelled: &dyn Fn() -> bool,
    consume: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut command = Command::new(ffmpeg);
    command.args(["-v", "error", "-nostdin", "-i"]).arg(source);
    if representation.stream == StreamSelection::First {
        command.args(["-map", "0:a:0"]);
    }
    command
        .args([
            "-map_metadata",
            "-1",
            "-vn",
            "-ac",
            &representation.channels.to_string(),
            "-ar",
            &representation.rate.to_string(),
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    if own_process_group {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Process(
        command
            .spawn()
            .map_err(|error| format!("could not start packaged ffmpeg: {error}"))?,
        own_process_group,
    );
    let mut stdout = child
        .0
        .stdout
        .take()
        .ok_or("ffmpeg stdout was not captured")?;
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or("ffmpeg stderr was not captured")?;
    let (sender, receiver) = mpsc::sync_channel(2);
    let output_reader = std::thread::spawn(move || {
        loop {
            let mut bytes = vec![0; 64 * 1024];
            match stdout.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    bytes.truncate(count);
                    if sender.send(Ok(bytes)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let error_reader = std::thread::spawn(move || {
        let mut saved = Vec::new();
        let mut bytes = [0; 8 * 1024];
        loop {
            match stderr.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let available = (64 * 1024_usize).saturating_sub(saved.len());
                    saved.extend_from_slice(&bytes[..count.min(available)]);
                }
            }
        }
        saved
    });
    let mut failure = None;
    loop {
        if cancelled() {
            failure = Some("audio decode cancelled".to_string());
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(Ok(bytes)) => {
                if let Err(error) = consume(&bytes) {
                    failure = Some(error);
                    break;
                }
            }
            Ok(Err(error)) => {
                failure = Some(error);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(receiver);
    if failure.is_some() {
        child.kill();
    }
    // A decoder may close stdout before exiting. Continue cancellation checks
    // through exit; do not block indefinitely in wait after EOF.
    let status = loop {
        if cancelled() {
            failure = Some("audio decode cancelled".into());
            child.kill();
        }
        match child.0.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                child.kill();
                break Err(error.to_string());
            }
        }
    };
    let _ = output_reader.join();
    let stderr = error_reader.join().unwrap_or_default();
    if cancelled() {
        return Err("audio decode cancelled".into());
    }
    if let Some(error) = failure {
        return Err(error);
    }
    let status = status?;
    if !status.success() {
        return Err(format!(
            "ffmpeg audio decode failed with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    Ok(())
}
