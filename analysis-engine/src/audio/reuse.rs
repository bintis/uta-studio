//! Shared request ownership; no process-global library/cache reuse.
use super::DecodedAudio;
use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uta_audio_reuse::{Cache, SourceIdentity, StreamSelection};

type Identity = (SourceIdentity, SourceIdentity);
type Facts = Arc<Mutex<Option<DecodedAudio>>>;
#[derive(Default)]
struct Context {
    pcm: Option<Cache>,
    facts: Mutex<BTreeMap<Identity, Facts>>,
    single_stream: Mutex<BTreeMap<Identity, bool>>,
}
thread_local! { static CURRENT: RefCell<Option<Arc<Context>>> = const { RefCell::new(None) }; }
#[derive(Clone)]
pub(crate) struct Snapshot(Option<Arc<Context>>);
pub(crate) struct Scope {
    previous: Snapshot,
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Scope {
    pub(crate) fn enter(enabled: bool, directory: Option<PathBuf>) -> Self {
        Snapshot(enabled.then(|| {
            Arc::new(Context {
                pcm: directory.map(Cache::new),
                ..Context::default()
            })
        }))
        .enter()
    }
}
impl Snapshot {
    pub(crate) fn capture() -> Self {
        Self(CURRENT.with(|current| current.borrow().clone()))
    }
    pub(crate) fn enter(&self) -> Scope {
        Scope {
            previous: Snapshot(CURRENT.with(|current| current.replace(self.0.clone()))),
            thread: std::marker::PhantomData,
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.previous.0.take()));
    }
}
pub(super) fn pcm() -> Option<Cache> {
    CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .and_then(|context| context.pcm.clone())
    })
}
fn identity(ffmpeg: &Path, source: &Path) -> Option<Identity> {
    Some((
        SourceIdentity::read(ffmpeg).ok()?,
        SourceIdentity::read(source).ok()?,
    ))
}
pub(super) fn record_streams(ffmpeg: &Path, source: &Path, single: bool) {
    let Some(identity) = identity(ffmpeg, source) else {
        return;
    };
    CURRENT.with(|current| {
        if let Some(context) = current.borrow().as_ref() {
            context
                .single_stream
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(identity, single);
        }
    });
}
pub(super) fn selection(ffmpeg: &Path, source: &Path) -> StreamSelection {
    let single = identity(ffmpeg, source)
        .and_then(|identity| {
            CURRENT.with(|current| {
                current.borrow().as_ref().and_then(|context| {
                    context
                        .single_stream
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .get(&identity)
                        .copied()
                })
            })
        })
        .unwrap_or(false);
    if single {
        StreamSelection::Automatic
    } else {
        StreamSelection::First
    }
}
pub(super) fn facts(
    ffmpeg: &Path,
    source: &Path,
    source_id: &str,
    cancellation: &CancellationToken,
    produce: impl FnOnce() -> EngineResult<DecodedAudio>,
) -> EngineResult<DecodedAudio> {
    let context = CURRENT.with(|current| current.borrow().clone());
    let cell = context
        .as_ref()
        .zip(identity(ffmpeg, source))
        .map(|(context, identity)| {
            context
                .facts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entry(identity)
                .or_default()
                .clone()
        });
    let Some(cell) = cell else {
        return produce();
    };
    let mut value = loop {
        if cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorCode::Cancelled,
                "audio facts reuse cancelled",
            ));
        }
        match cell.try_lock() {
            Ok(value) => break value,
            Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(25))
            }
        }
    };
    if let Some(audio) = value.as_ref() {
        let mut audio = audio.clone();
        audio.facts.source_id = source_id.to_string();
        crate::debug_log::record("decoded_audio_facts_reused", &source.display().to_string());
        return Ok(audio);
    }
    let audio = produce()?;
    *value = Some(audio.clone());
    Ok(audio)
}
