//! Request-owned, cross-process exact PCM reuse. No model/runtime dependency.
mod process;

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceIdentity {
    location: Location,
    bytes: u64,
    modified: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Location {
    #[cfg(unix)]
    File { device: u64, inode: u64 },
    #[cfg(not(unix))]
    Path(PathBuf),
}

impl SourceIdentity {
    pub fn read(path: &Path) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        let metadata = path.metadata().map_err(|error| error.to_string())?;
        #[cfg(unix)]
        let location = {
            use std::os::unix::fs::MetadataExt;
            Location::File {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        };
        #[cfg(not(unix))]
        let location = Location::Path(path);
        Ok(Self {
            location,
            bytes: metadata.len(),
            modified: metadata
                .modified()
                .map_err(|error| error.to_string())?
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamSelection {
    First,
    Automatic,
}

/// Entire source, FFmpeg's unchanged default resampler/channel mixer, float PCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Representation {
    pub rate: u32,
    pub channels: u16,
    pub stream: StreamSelection,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Key {
    source: SourceIdentity,
    decoder: SourceIdentity,
    representation: Representation,
}

#[derive(Clone)]
pub struct Cache {
    directory: PathBuf,
}
impl Cache {
    /// Directory lifetime belongs to the analysis, after its readers/workers exit.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn restore_wave(
        &self,
        ffmpeg: &Path,
        source: &Path,
        representation: Representation,
        target: &Path,
    ) -> Result<bool, String> {
        let claim = self.claim(&key(ffmpeg, source, representation)?, &|| false)?;
        let wave = claim.directory.join("ready.wav");
        if !wave.is_file() {
            return Ok(false);
        }
        copy_new(&wave, target)?;
        Ok(true)
    }

    pub fn publish_wave(
        &self,
        ffmpeg: &Path,
        source: &Path,
        representation: Representation,
        wave: &Path,
    ) -> Result<(), String> {
        let claim = self.claim(&key(ffmpeg, source, representation)?, &|| false)?;
        let target = claim.directory.join("ready.wav");
        if !target.exists() {
            let temporary = claim.directory.join("building.wav");
            let _ = std::fs::remove_file(&temporary);
            copy_new(wave, &temporary)?;
            std::fs::rename(&temporary, &target).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn claim(&self, key: &Key, cancelled: &dyn Fn() -> bool) -> Result<Claim, String> {
        let catalog = locked(&self.directory.join("catalog.lock"), cancelled)?;
        let mut found = None;
        for entry in std::fs::read_dir(&self.directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if !path.is_dir() {
                continue;
            }
            if let Ok(bytes) = std::fs::read(path.join("key.json"))
                && serde_json::from_slice::<Key>(&bytes).ok().as_ref() == Some(key)
            {
                found = Some(path);
                break;
            }
        }
        let directory = match found {
            Some(path) => path,
            None => {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_nanos();
                let path = self
                    .directory
                    .join(format!("pcm-{}-{nonce}", std::process::id()));
                std::fs::create_dir(&path).map_err(|error| error.to_string())?;
                let file =
                    File::create(path.join("key.json")).map_err(|error| error.to_string())?;
                serde_json::to_writer(file, key).map_err(|error| error.to_string())?;
                path
            }
        };
        drop(catalog);
        let lock = locked(&directory.join("producer.lock"), cancelled)?;
        Ok(Claim {
            directory,
            _lock: lock,
        })
    }
}

fn key(ffmpeg: &Path, source: &Path, representation: Representation) -> Result<Key, String> {
    Ok(Key {
        source: SourceIdentity::read(source)?,
        decoder: SourceIdentity::read(ffmpeg)?,
        representation,
    })
}

fn copy_new(source: &Path, target: &Path) -> Result<(), String> {
    if std::fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    let mut input = File::open(source).map_err(|error| error.to_string())?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)
        .map_err(|error| error.to_string())?;
    if let Err(error) = std::io::copy(&mut input, &mut output) {
        drop(output);
        let _ = std::fs::remove_file(target);
        return Err(error.to_string());
    }
    Ok(())
}

fn locked(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    loop {
        if cancelled() {
            return Err("audio reuse cancelled".into());
        }
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

struct Claim {
    directory: PathBuf,
    _lock: File,
}
impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.directory.join("building.pcm"));
        let _ = std::fs::remove_file(self.directory.join("building.wav"));
    }
}

/// The first consumer tees the real decoder output; later consumers read the
/// published bytes. Different representations never resample each other's PCM.
/// Cache I/O failures only disable reuse. A real producer failure is published
/// to waiting consumers rather than silently retrying the failed decode.
pub fn stream(
    ffmpeg: &Path,
    source: &Path,
    representation: Representation,
    own_process_group: bool,
    cache: Option<&Cache>,
    cancelled: &dyn Fn() -> bool,
    consume: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    let claim = cache.and_then(|cache| {
        let key = Key {
            source: SourceIdentity::read(source).ok()?,
            decoder: SourceIdentity::read(ffmpeg).ok()?,
            representation,
        };
        match cache.claim(&key, cancelled) {
            Ok(claim) => Some(claim),
            Err(error) => {
                eprintln!("[Uta! Studio] optional PCM reuse unavailable: {error}");
                None
            }
        }
    });
    if cancelled() {
        return Err("audio decode cancelled".into());
    }
    if let Some(claim) = &claim {
        if let Ok(message) = std::fs::read_to_string(claim.directory.join("failed.txt")) {
            return Err(message);
        }
        if let Ok(mut file) = File::open(claim.directory.join("ready.pcm")) {
            read_chunks(&mut file, cancelled, consume)?;
            return Ok(true);
        }
        if claim.directory.join("producing").exists() {
            return Err("previous PCM producer did not publish a completion".into());
        }
        let _ = std::fs::write(
            claim.directory.join("producing"),
            b"decoder execution intent",
        );
    }
    let mut output = claim
        .as_ref()
        .and_then(|claim| File::create(claim.directory.join("building.pcm")).ok());
    let result = process::decode(
        ffmpeg,
        source,
        representation,
        own_process_group,
        cancelled,
        &mut |bytes| {
            if let Some(file) = &mut output
                && file.write_all(bytes).is_err()
            {
                output = None;
            }
            consume(bytes)
        },
    );
    if let Some(claim) = &claim {
        match &result {
            Ok(()) => {
                if let Some(mut file) = output.take() {
                    let flushed = file.flush().is_ok();
                    drop(file);
                    if flushed {
                        let _ = std::fs::rename(
                            claim.directory.join("building.pcm"),
                            claim.directory.join("ready.pcm"),
                        );
                    }
                }
            }
            Err(message) => {
                let _ = std::fs::write(claim.directory.join("failed.txt"), message);
            }
        }
        let _ = std::fs::remove_file(claim.directory.join("producing"));
    }
    result.map(|()| false)
}

fn read_chunks(
    input: &mut dyn Read,
    cancelled: &dyn Fn() -> bool,
    consume: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut bytes = vec![0; 64 * 1024];
    loop {
        if cancelled() {
            return Err("audio decode cancelled".into());
        }
        let count = input.read(&mut bytes).map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(());
        }
        consume(&bytes[..count])?;
    }
}

#[cfg(test)]
mod tests;
