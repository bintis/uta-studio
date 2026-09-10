//! Per-analysis PCM reuse across precision-isolated model workers.
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use uta_audio_reuse::{Cache, Representation, SourceIdentity, StreamSelection};

thread_local! { static DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) }; }
thread_local! { static TASK: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub struct Scope(Option<PathBuf>, Option<String>);
impl Scope {
    pub fn enter(config: &serde_json::Value, task_id: &str) -> Self {
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
        Self(
            DIRECTORY.with(|current| current.replace(directory)),
            TASK.with(|current| current.replace(Some(task_id.to_string()))),
        )
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        DIRECTORY.with(|current| current.replace(self.0.take()));
        TASK.with(|current| current.replace(self.1.take()));
    }
}
pub fn diagnostic(message: &str) {
    eprintln!("[super acceleration] {message}");
    TASK.with(|task| {
        if let Some(task_id) = task.borrow().as_deref() {
            let _ = crate::protocol::emit(crate::protocol::WorkerFrame::Diagnostic {
                task_id,
                message,
            });
        }
    });
}

pub fn decode(
    ffmpeg: &Path,
    source: &Path,
    rate: &str,
    channels: &str,
    target: &Path,
) -> Result<bool, String> {
    let Some(cache) = DIRECTORY.with(|directory| directory.borrow().clone().map(Cache::new)) else {
        return Ok(false);
    };
    let representation = Representation {
        rate: rate.parse::<u32>().map_err(|error| error.to_string())?,
        channels: channels.parse::<u16>().map_err(|error| error.to_string())?,
        stream: StreamSelection::Automatic,
    };
    if cache
        .restore_wave(ffmpeg, source, representation, target)
        .unwrap_or(false)
    {
        diagnostic(&format!(
            "Decoded audio cache hit: {} rate={rate} channels={channels}",
            source.display()
        ));
        return Ok(true);
    }
    let before = SourceIdentity::read(source).ok();
    let result = (|| {
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target)
            .map_err(|error| error.to_string())?;
        let mut writer = hound::WavWriter::new(
            std::io::BufWriter::new(file),
            hound::WavSpec {
                channels: representation.channels,
                sample_rate: representation.rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .map_err(|error| error.to_string())?;
        let mut carry = Vec::new();
        // Stay in the supervised worker's process group: cancellation must also
        // reap this FFmpeg child, not leave a new independent process group.
        let hit = uta_audio_reuse::stream(
            ffmpeg,
            source,
            representation,
            false,
            Some(&cache),
            &|| false,
            &mut |bytes| {
                carry.extend_from_slice(bytes);
                let complete = carry.len() / 4 * 4;
                for bytes in carry[..complete].as_chunks::<4>().0 {
                    writer
                        .write_sample(f32::from_le_bytes(*bytes))
                        .map_err(|error| error.to_string())?;
                }
                carry.drain(..complete);
                Ok(())
            },
        )?;
        if !carry.is_empty() {
            return Err("decoded audio has an incomplete float sample".to_string());
        }
        writer.finalize().map_err(|error| error.to_string())?;
        if hit {
            diagnostic(&format!(
                "Decoded audio cache hit: {} rate={rate} channels={channels}",
                source.display()
            ));
        }
        if before.is_some() && before == SourceIdentity::read(source).ok() {
            if let Err(error) = cache.publish_wave(ffmpeg, source, representation, target) {
                eprintln!("[super acceleration] optional WAV reuse unavailable: {error}");
            }
        }
        Ok(true)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_mode_never_uses_a_supplied_cache_directory() {
        let _scope = Scope::enter(
            &serde_json::json!({"turbo_acceleration": false, "audio_cache_directory": "/unused"}),
            "fixture",
        );
        assert!(DIRECTORY.with(|directory| directory.borrow().is_none()));
    }
}
