//! Per-analysis PCM reuse across precision-isolated model workers.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

thread_local! { static DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) }; }

pub struct Scope(Option<PathBuf>);
impl Scope {
    pub fn enter(config: &serde_json::Value) -> Self {
        let directory = (config
            .get("turbo_acceleration")
            .and_then(serde_json::Value::as_bool)
            == Some(true))
        .then(|| {
            config
                .get("audio_cache_directory")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        })
        .flatten();
        Self(DIRECTORY.with(|current| current.replace(directory)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        DIRECTORY.with(|current| {
            current.replace(self.0.take());
        });
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SourceIdentity {
    path: PathBuf,
    bytes: u64,
    modified_ns: u128,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    source: SourceIdentity,
    rate: String,
    channels: String,
    pcm: String,
}

fn identity(source: &Path) -> Result<SourceIdentity, String> {
    let path = source.canonicalize().map_err(|error| error.to_string())?;
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    Ok(SourceIdentity {
        path,
        bytes: metadata.len(),
        modified_ns: metadata
            .modified()
            .map_err(|error| error.to_string())?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos(),
    })
}

fn entries(directory: &Path) -> Result<Vec<Entry>, String> {
    match std::fs::read(directory.join("decoded-index.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

fn copy_new(source: &Path, target: &Path) -> Result<(), String> {
    if std::fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    let mut input = std::fs::File::open(source).map_err(|error| error.to_string())?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| error.to_string())?;
    if let Err(error) = std::io::copy(&mut input, &mut output) {
        drop(output);
        let _ = std::fs::remove_file(target);
        return Err(error.to_string());
    }
    Ok(())
}

fn restore_from(
    directory: &Path,
    source: &Path,
    rate: &str,
    channels: &str,
    target: &Path,
) -> Result<bool, String> {
    let source = identity(source)?;
    let records = entries(directory)?;
    let Some(entry) = records
        .iter()
        .find(|entry| entry.source == source && entry.rate == rate && entry.channels == channels)
    else {
        return Ok(false);
    };
    // Only this analysis's generated PCM filenames are cache candidates.
    let pcm = Path::new(&entry.pcm);
    if pcm.components().count() != 1 || !directory.join(pcm).is_file() {
        return Ok(false);
    }
    copy_new(&directory.join(pcm), target)?;
    Ok(true)
}

pub fn restore(source: &Path, rate: &str, channels: &str, target: &Path) -> bool {
    DIRECTORY.with(|directory| {
        let directory = directory.borrow();
        let Some(directory) = directory.as_deref() else { return false; };
        match restore_from(directory, source, rate, channels, target) {
            Ok(hit) => {
                if hit { eprintln!("[super acceleration] decoded audio cache hit: {} rate={rate} channels={channels}", source.display()); }
                hit
            }
            Err(error) => { eprintln!("[super acceleration] audio cache reuse skipped: {error}"); false }
        }
    })
}

fn store_in(
    directory: &Path,
    source: &Path,
    rate: &str,
    channels: &str,
    decoded: &Path,
) -> Result<(), String> {
    let mut records = entries(directory)?;
    let source = identity(source)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let pcm = format!("decoded-{}-{nonce}.wav", std::process::id());
    copy_new(decoded, &directory.join(&pcm))?;
    records.push(Entry {
        source,
        rate: rate.to_string(),
        channels: channels.to_string(),
        pcm,
    });
    let temporary = directory.join(format!("decoded-index-{}-{nonce}.json", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(file, &records).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, directory.join("decoded-index.json"))
        .map_err(|error| error.to_string())
}

pub fn store(source: &Path, rate: &str, channels: &str, decoded: &Path) {
    DIRECTORY.with(|directory| {
        if let Some(directory) = directory.borrow().as_deref()
            && let Err(error) = store_in(directory, source, rate, channels, decoded)
        {
            eprintln!("[super acceleration] audio cache storage skipped: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "uta-studio-audio-cache-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn pcm_reuse_preserves_source_and_separates_formats_and_changed_sources() {
        let fixture = Fixture::new();
        let source = fixture.0.join("source.flac");
        let decoded = fixture.0.join("initial.wav");
        let reused = fixture.0.join("reused.wav");
        std::fs::write(&source, b"read-only source fixture").unwrap();
        std::fs::write(&decoded, b"decoded PCM fixture").unwrap();
        store_in(&fixture.0, &source, "16000", "1", &decoded).unwrap();
        std::fs::remove_file(&decoded).unwrap();
        assert!(!restore_from(&fixture.0, &source, "44100", "1", &reused).unwrap());
        assert!(!restore_from(&fixture.0, &source, "16000", "2", &reused).unwrap());
        assert!(restore_from(&fixture.0, &source, "16000", "1", &reused).unwrap());
        assert_eq!(std::fs::read(&reused).unwrap(), b"decoded PCM fixture");
        std::fs::remove_file(&reused).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"read-only source fixture");
        std::fs::write(&source, b"externally changed source").unwrap();
        assert!(!restore_from(&fixture.0, &source, "16000", "1", &reused).unwrap());
    }

    #[test]
    fn disabled_mode_never_uses_a_supplied_cache_directory() {
        let fixture = Fixture::new();
        let _scope = Scope::enter(
            &serde_json::json!({"turbo_acceleration":false,"audio_cache_directory":fixture.0}),
        );
        assert!(DIRECTORY.with(|directory| directory.borrow().is_none()));
    }
}
