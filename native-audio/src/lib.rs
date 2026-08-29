//! Native, UI-framework-independent audition playback for Uta! Studio.
//!
//! The editor deliberately uses platform-native output instead of a UI
//! toolkit's media element: GStreamer/PipeWire on Linux and WASAPI on Windows.
//! Keeping this crate independent from Bevy makes transport and accurate-seek
//! behavior directly testable without the UI runtime.

use std::sync::Mutex;

use serde::Serialize;

mod pitch;

pub use pitch::{PitchAudition, PitchTone, render_pitch_preview};

#[derive(Clone, Debug, Default, Serialize)]
pub struct EditorAudioStatus {
    pub loaded: bool,
    pub playing: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub ended: bool,
    pub error: Option<String>,
}

pub struct EditorAudioPlayer {
    backend: Mutex<BackendState>,
}

trait AudioBackend: Send {
    fn name(&self) -> &'static str;
    fn load(&mut self, path: &std::path::Path) -> Result<EditorAudioStatus, String>;
    fn play(&mut self) -> Result<EditorAudioStatus, String>;
    fn pause(&mut self) -> Result<EditorAudioStatus, String>;
    fn seek(&mut self, position_secs: f64) -> Result<EditorAudioStatus, String>;
    fn set_volume(&mut self, volume: f64) -> Result<EditorAudioStatus, String>;
    fn status(&mut self) -> Result<EditorAudioStatus, String>;
    fn stop(&mut self) -> Result<EditorAudioStatus, String>;
}

enum BackendState {
    Ready(Box<dyn AudioBackend>),
    Unavailable(String),
}

impl EditorAudioPlayer {
    pub fn new() -> Self {
        let backend = match platform_backend() {
            Ok(backend) => BackendState::Ready(backend),
            Err(error) => BackendState::Unavailable(error),
        };
        Self {
            backend: Mutex::new(backend),
        }
    }

    fn with_backend<T>(
        &self,
        operation: impl FnOnce(&mut dyn AudioBackend) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .backend
            .lock()
            .map_err(|_| "Native audio player lock is poisoned".to_string())?;
        match &mut *state {
            BackendState::Ready(backend) => operation(backend.as_mut()),
            BackendState::Unavailable(error) => Err(error.clone()),
        }
    }

    pub fn load(&self, file_hash: &str, source: &str) -> Result<EditorAudioStatus, String> {
        let path = audio_path(file_hash, source)?;
        self.load_path(&path)
    }

    /// Load an already-authorized local source without creating a converted
    /// preview. The desktop library player only passes indexed song paths.
    pub fn load_path(&self, path: &std::path::Path) -> Result<EditorAudioStatus, String> {
        self.with_backend(|backend| backend.load(path))
    }

    pub fn play(&self) -> Result<EditorAudioStatus, String> {
        self.with_backend(|backend| backend.play())
    }

    pub fn pause(&self) -> Result<EditorAudioStatus, String> {
        self.with_backend(|backend| backend.pause())
    }

    pub fn seek(&self, position_secs: f64) -> Result<EditorAudioStatus, String> {
        if !position_secs.is_finite() {
            return Err("Audio seek position must be finite".to_string());
        }
        self.with_backend(|backend| backend.seek(position_secs))
    }

    pub fn set_volume(&self, volume: f64) -> Result<EditorAudioStatus, String> {
        if !volume.is_finite() {
            return Err("Audio volume must be finite".to_string());
        }
        self.with_backend(|backend| backend.set_volume(volume))
    }

    pub fn status(&self) -> Result<EditorAudioStatus, String> {
        self.with_backend(|backend| backend.status())
    }

    pub fn stop(&self) -> Result<EditorAudioStatus, String> {
        self.with_backend(|backend| backend.stop())
    }
}

impl Default for EditorAudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
fn platform_backend() -> Result<Box<dyn AudioBackend>, String> {
    gstreamer::init().map_err(|error| format!("Could not initialize native audio: {error}"))?;
    if let Some(error) = linux::required_plugins_error() {
        return Err(error);
    }
    Ok(Box::new(linux::PlayerInner::default()))
}

#[cfg(target_os = "windows")]
fn platform_backend() -> Result<Box<dyn AudioBackend>, String> {
    Ok(Box::new(windows::PlayerInner::default()))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_backend() -> Result<Box<dyn AudioBackend>, String> {
    Err("Native editor audio is supported on Linux and Windows".to_string())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use gst::prelude::*;
    use gstreamer as gst;

    use super::EditorAudioStatus;

    pub fn required_plugins_error() -> Option<String> {
        let mut missing = ["playbin", "decodebin", "typefind"]
            .into_iter()
            .filter(|name| gst::ElementFactory::find(name).is_none())
            .collect::<Vec<_>>();
        if ["pipewiresink", "pulsesink", "autoaudiosink"]
            .into_iter()
            .all(|name| gst::ElementFactory::find(name).is_none())
        {
            missing.push("audio output sink");
        }
        (!missing.is_empty()).then(|| {
            format!(
                "Native audio is missing required GStreamer plugins: {}. Reinstall or relaunch the packaged Uta! Studio runtime.",
                missing.join(", ")
            )
        })
    }

    pub(super) fn state_transition_confirmed(
        current: gst::State,
        pending: gst::State,
        target: gst::State,
    ) -> bool {
        current == target && pending == gst::State::VoidPending
    }

    #[derive(Default)]
    pub struct PlayerInner {
        player: Option<gst::Element>,
        path: Option<PathBuf>,
        ended: bool,
        error: Option<String>,
        /// The position we last told GStreamer to seek to, and when. A
        /// flushing seek needs to preroll before `query_position` reliably
        /// reflects it — querying right away can briefly report the
        /// pre-seek position, which reads as the playhead jumping to where
        /// you clicked and then flickering back. `status()` trusts this
        /// commanded position over a fresh (possibly not-yet-settled) query
        /// until the query catches up or the grace period lapses.
        pending_seek: Option<(f64, std::time::Instant)>,
    }

    impl PlayerInner {
        pub fn load(&mut self, path: &Path) -> Result<EditorAudioStatus, String> {
            self.stop()?;
            if !path.is_file() {
                return Err(format!("Chart audio does not exist: {}", path.display()));
            }

            let uri = gst::glib::filename_to_uri(path, None)
                .map_err(|error| format!("Could not convert audio path to URI: {error}"))?;
            let player = gst::ElementFactory::make("playbin")
                .property("uri", uri.as_str())
                .build()
                .map_err(|error| format!("Could not create native audio player: {error}"))?;

            // Uta! Studio is Wayland-only, so use PipeWire directly when it is
            // available. Pulse compatibility and GStreamer's automatic sink
            // remain safe fallbacks for packaged environments with a smaller
            // plugin set.
            let sink = gst::ElementFactory::make("pipewiresink")
                .build()
                .or_else(|_| {
                    gst::ElementFactory::make("pulsesink")
                        .property("buffer-time", 250_000i64)
                        .property("latency-time", 25_000i64)
                        .build()
                })
                .or_else(|_| gst::ElementFactory::make("autoaudiosink").build());
            if let Ok(sink) = sink {
                player.set_property("audio-sink", sink);
            }

            player
                .set_state(gst::State::Paused)
                .map_err(|error| format!("Could not prepare chart audio: {error:?}"))?;
            // `set_state` only requests the transition; PAUSED needs a preroll
            // sample before it's actually reached, which happens
            // asynchronously. A `seek`/`play` issued immediately after
            // `load()` returns (as switching the audition source to the
            // vocal stem does) can race that preroll — occasionally failing
            // outright, and even when it doesn't, landing on a pipeline that
            // isn't ready to report its position reliably yet (more of the
            // same "query too soon" symptom `seek`'s `pending_seek` grace
            // period exists for, just triggered from the other end). Block
            // briefly for the real state instead of guessing.
            let (result, _current, _pending) = player.state(gst::ClockTime::from_seconds(5));
            result.map_err(|error| format!("Chart audio did not become ready: {error:?}"))?;
            self.player = Some(player);
            self.path = Some(path.to_path_buf());
            self.ended = false;
            self.error = None;
            self.pending_seek = None;
            Ok(self.status())
        }

        pub fn play(&mut self) -> Result<EditorAudioStatus, String> {
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before playing".to_string())?;
            if self.ended {
                player
                    .seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::ClockTime::ZERO,
                    )
                    .map_err(|error| format!("Could not restart chart audio: {error}"))?;
                self.ended = false;
            }
            player
                .set_state(gst::State::Playing)
                .map_err(|error| format!("Could not play chart audio: {error:?}"))?;
            // `set_state` only requests the transition, same as `load`'s own
            // PAUSED request -- and the same race applies: switching the
            // audition source (`PlayNoteVocal`) calls `load` then `play` back
            // to back, and issuing `play` before the sink has actually
            // finished (re)starting could silently produce no audible sound
            // even though this call itself returns `Ok`. `load` already
            // blocks for its own state change; do the same here instead of
            // assuming the request lands the instant it's made.
            let (result, _current, _pending) = player.state(gst::ClockTime::from_seconds(5));
            result.map_err(|error| format!("Chart audio did not start playing: {error:?}"))?;
            Ok(self.status())
        }

        pub fn pause(&mut self) -> Result<EditorAudioStatus, String> {
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before pausing".to_string())?;
            player
                .set_state(gst::State::Paused)
                .map_err(|error| format!("Could not pause chart audio: {error:?}"))?;
            let (result, current, pending) = player.state(gst::ClockTime::from_seconds(5));
            result.map_err(|error| format!("Chart audio did not pause: {error:?}"))?;
            if !state_transition_confirmed(current, pending, gst::State::Paused) {
                return Err(format!(
                    "Chart audio pause was not confirmed (current={current:?}, pending={pending:?})"
                ));
            }
            Ok(self.status())
        }

        pub fn seek(&mut self, position_secs: f64) -> Result<EditorAudioStatus, String> {
            if !position_secs.is_finite() {
                return Err("Audio seek position must be finite".to_string());
            }
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before seeking".to_string())?;
            let duration = player
                .query_duration::<gst::ClockTime>()
                .map(|value| value.seconds_f64())
                .unwrap_or(f64::MAX);
            let position = position_secs.max(0.0).min(duration);
            player
                .seek_simple(
                    // Word and note auditioning needs sample-accurate positioning.
                    // KEY_UNIT can land noticeably before the selected lyric.
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::ClockTime::from_nseconds((position * 1_000_000_000.0) as u64),
                )
                .map_err(|error| format!("Could not seek chart audio: {error}"))?;
            self.ended = false;
            self.pending_seek = Some((position, std::time::Instant::now()));
            Ok(self.status())
        }

        pub fn set_volume(&mut self, volume: f64) -> Result<EditorAudioStatus, String> {
            if !volume.is_finite() {
                return Err("Audio volume must be finite".to_string());
            }
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before setting volume".to_string())?;
            player.set_property("volume", volume.clamp(0.0, 1.0));
            Ok(self.status())
        }

        pub fn status(&mut self) -> EditorAudioStatus {
            let Some(player) = self.player.as_ref() else {
                return EditorAudioStatus::default();
            };

            if let Some(bus) = player.bus() {
                while let Some(message) = bus.pop() {
                    match message.view() {
                        gst::MessageView::Eos(..) => self.ended = true,
                        gst::MessageView::Error(error) => {
                            self.error = Some(match error.debug() {
                                Some(debug) => format!("{} ({debug})", error.error()),
                                None => error.error().to_string(),
                            });
                        }
                        _ => {}
                    }
                }
            }

            let current_state = player.current_state();
            let duration_secs = player
                .query_duration::<gst::ClockTime>()
                .map(|value| value.seconds_f64())
                .unwrap_or(0.0);
            let mut position_secs = player
                .query_position::<gst::ClockTime>()
                .map(|value| value.seconds_f64())
                .unwrap_or(0.0);

            if let Some((target, requested_at)) = self.pending_seek {
                const SETTLE_TOLERANCE_SECS: f64 = 0.15;
                const GRACE_PERIOD: std::time::Duration = std::time::Duration::from_millis(500);
                // Compare the live query against where we'd *expect* to be
                // by now rather than the bare seek target: a seek immediately
                // followed by play (as switching the audition source to the
                // vocal stem does) means real playback keeps advancing past
                // the target within milliseconds, so checking closeness to a
                // frozen target — even "at or after" it — only holds for an
                // instant before the next poll sees "too far past" and
                // slams the reported position back down, over and over for
                // the whole grace period: the reported position visibly
                // oscillating instead of the one-time flicker this exists
                // to fix. A backward seek has the opposite problem: a stale
                // pre-seek query reads *ahead* of the target, which a bare
                // "at or after" check would wrongly accept as settled.
                // Predicting elapsed playback time handles both.
                let expected = if current_state == gst::State::Playing {
                    target + requested_at.elapsed().as_secs_f64()
                } else {
                    target
                };
                if (position_secs - expected).abs() <= SETTLE_TOLERANCE_SECS {
                    self.pending_seek = None;
                } else if requested_at.elapsed() < GRACE_PERIOD {
                    position_secs = expected;
                } else {
                    // The pipeline never reported landing near the
                    // commanded position within the grace period; trust the
                    // live query rather than freezing the UI on a guess.
                    self.pending_seek = None;
                }
            }

            EditorAudioStatus {
                loaded: self.path.is_some(),
                playing: current_state == gst::State::Playing
                    && !self.ended
                    && self.error.is_none(),
                position_secs,
                duration_secs,
                ended: self.ended,
                error: self.error.clone(),
            }
        }

        pub fn stop(&mut self) -> Result<EditorAudioStatus, String> {
            if let Some(player) = self.player.as_ref() {
                player
                    .set_state(gst::State::Null)
                    .map_err(|error| format!("Could not stop chart audio: {error:?}"))?;
                let (result, current, pending) = player.state(gst::ClockTime::from_seconds(5));
                result.map_err(|error| format!("Chart audio did not stop: {error:?}"))?;
                if !state_transition_confirmed(current, pending, gst::State::Null) {
                    return Err(format!(
                        "Chart audio stop was not confirmed (current={current:?}, pending={pending:?})"
                    ));
                }
            }
            self.player = None;
            self.path = None;
            self.ended = false;
            self.error = None;
            self.pending_seek = None;
            Ok(EditorAudioStatus::default())
        }
    }

    impl Drop for PlayerInner {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    impl super::AudioBackend for PlayerInner {
        fn name(&self) -> &'static str {
            "GStreamer"
        }

        fn load(&mut self, path: &Path) -> Result<EditorAudioStatus, String> {
            PlayerInner::load(self, path)
        }

        fn play(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::play(self)
        }

        fn pause(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::pause(self)
        }

        fn seek(&mut self, position_secs: f64) -> Result<EditorAudioStatus, String> {
            PlayerInner::seek(self, position_secs)
        }

        fn set_volume(&mut self, volume: f64) -> Result<EditorAudioStatus, String> {
            PlayerInner::set_volume(self, volume)
        }

        fn status(&mut self) -> Result<EditorAudioStatus, String> {
            Ok(PlayerInner::status(self))
        }

        fn stop(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::stop(self)
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        fs::File,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::Duration,
    };

    use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

    use super::{AudioBackend, EditorAudioStatus};

    pub struct PlayerInner {
        // Drop the player before its device sink so its source queue cannot
        // outlive the WASAPI stream it feeds.
        player: Option<Player>,
        device_sink: Option<MixerDeviceSink>,
        path: Option<PathBuf>,
        duration_secs: f64,
        ended: bool,
        volume: f32,
        stream_error: Arc<Mutex<Option<String>>>,
    }

    impl Default for PlayerInner {
        fn default() -> Self {
            Self {
                player: None,
                device_sink: None,
                path: None,
                duration_secs: 0.0,
                ended: false,
                volume: 1.0,
                stream_error: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl PlayerInner {
        pub(super) fn load_source(
            path: &Path,
        ) -> Result<(Decoder<std::io::BufReader<File>>, f64), String> {
            if !path.is_file() {
                return Err(format!("Chart audio does not exist: {}", path.display()));
            }
            let file = File::open(path).map_err(|error| {
                format!("Could not open chart audio {}: {error}", path.display())
            })?;
            let source = Decoder::try_from(file).map_err(|error| {
                format!("Could not decode chart audio {}: {error}", path.display())
            })?;
            let duration_secs = source
                .total_duration()
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0);
            Ok((source, duration_secs))
        }

        fn load(&mut self, path: &Path) -> Result<EditorAudioStatus, String> {
            self.stop();
            // Decode first so a malformed or unsupported file fails without
            // opening the user's output device.
            let (source, duration_secs) = Self::load_source(path)?;
            let stream_error = Arc::clone(&self.stream_error);
            let builder = DeviceSinkBuilder::from_default_device()
                .map_err(|error| format!("Could not open the Windows audio output: {error}"))?
                .with_error_callback(move |error| {
                    if let Ok(mut current) = stream_error.lock() {
                        *current = Some(format!("Windows audio output stopped: {error}"));
                    }
                });
            let mut device_sink = builder
                .open_sink_or_fallback()
                .map_err(|error| format!("Could not open the Windows audio output: {error}"))?;
            device_sink.log_on_drop(false);
            let player = Player::connect_new(device_sink.mixer());
            player.pause();
            player.set_volume(self.volume);
            player.append(source);

            self.player = Some(player);
            self.device_sink = Some(device_sink);
            self.path = Some(path.to_path_buf());
            self.duration_secs = duration_secs;
            self.ended = false;
            self.status()
        }

        fn play(&mut self) -> Result<EditorAudioStatus, String> {
            if self.ended {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| "Load chart audio before playing".to_string())?;
                self.load(&path)?;
            }
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before playing".to_string())?;
            player.play();
            self.status()
        }

        fn pause(&mut self) -> Result<EditorAudioStatus, String> {
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before pausing".to_string())?;
            player.pause();
            self.status()
        }

        fn seek(&mut self, position_secs: f64) -> Result<EditorAudioStatus, String> {
            if !position_secs.is_finite() {
                return Err("Audio seek position must be finite".to_string());
            }
            if self.ended {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| "Load chart audio before seeking".to_string())?;
                self.load(&path)?;
            }
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before seeking".to_string())?;
            let position = if self.duration_secs > 0.0 {
                position_secs.clamp(0.0, self.duration_secs)
            } else {
                position_secs.max(0.0)
            };
            player
                .try_seek(Duration::from_secs_f64(position))
                .map_err(|error| format!("Could not seek chart audio: {error}"))?;
            self.ended = false;
            self.status()
        }

        fn set_volume(&mut self, volume: f64) -> Result<EditorAudioStatus, String> {
            if !volume.is_finite() {
                return Err("Audio volume must be finite".to_string());
            }
            self.volume = volume.clamp(0.0, 1.0) as f32;
            let player = self
                .player
                .as_ref()
                .ok_or_else(|| "Load chart audio before setting volume".to_string())?;
            player.set_volume(self.volume);
            self.status()
        }

        fn status(&mut self) -> Result<EditorAudioStatus, String> {
            let Some(player) = self.player.as_ref() else {
                return Ok(EditorAudioStatus::default());
            };
            self.ended = player.empty();
            let error = self
                .stream_error
                .lock()
                .map_err(|_| "Windows audio error state lock is poisoned".to_string())?
                .clone();
            Ok(EditorAudioStatus {
                loaded: self.path.is_some(),
                playing: !player.is_paused() && !self.ended && error.is_none(),
                position_secs: player.get_pos().as_secs_f64(),
                duration_secs: self.duration_secs,
                ended: self.ended,
                error,
            })
        }

        fn stop(&mut self) -> EditorAudioStatus {
            if let Some(player) = self.player.take() {
                player.stop();
            }
            self.device_sink = None;
            self.path = None;
            self.duration_secs = 0.0;
            self.ended = false;
            if let Ok(mut error) = self.stream_error.lock() {
                *error = None;
            }
            EditorAudioStatus::default()
        }
    }

    impl Drop for PlayerInner {
        fn drop(&mut self) {
            self.stop();
        }
    }

    impl AudioBackend for PlayerInner {
        fn name(&self) -> &'static str {
            "WASAPI"
        }

        fn load(&mut self, path: &Path) -> Result<EditorAudioStatus, String> {
            PlayerInner::load(self, path)
        }

        fn play(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::play(self)
        }

        fn pause(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::pause(self)
        }

        fn seek(&mut self, position_secs: f64) -> Result<EditorAudioStatus, String> {
            PlayerInner::seek(self, position_secs)
        }

        fn set_volume(&mut self, volume: f64) -> Result<EditorAudioStatus, String> {
            PlayerInner::set_volume(self, volume)
        }

        fn status(&mut self) -> Result<EditorAudioStatus, String> {
            PlayerInner::status(self)
        }

        fn stop(&mut self) -> Result<EditorAudioStatus, String> {
            Ok(PlayerInner::stop(self))
        }
    }
}

fn audio_path(file_hash: &str, source: &str) -> Result<std::path::PathBuf, String> {
    enum Source {
        Vocals,
        Instrumental,
        Original,
    }

    let source = match source {
        "vocals" => Source::Vocals,
        "instrumental" => Source::Instrumental,
        "original" => Source::Original,
        _ => return Err(format!("Unknown chart audio source: {source}")),
    };
    let chart = app_core::load_chart(file_hash).map_err(|error| error.to_string())?;
    let path = match source {
        Source::Vocals => chart
            .audio
            .vocals
            .as_deref()
            .unwrap_or(&chart.audio.instrumental),
        Source::Instrumental => &chart.audio.instrumental,
        Source::Original => &chart.audio.original,
    };
    Ok(path.into())
}

pub fn probe_audio(path: &std::path::Path) -> Result<String, String> {
    let mut backend = platform_backend()?;
    let status = backend.load(path)?;
    if !status.loaded {
        return Err("Native audio pipeline did not retain the selected source".to_string());
    }
    let backend_name = backend.name();
    backend.stop()?;
    Ok(format!(
        "Native {backend_name} pipeline prepared {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloaded_audio_status_is_safe_and_explicit() {
        let status = EditorAudioStatus::default();
        assert!(!status.loaded);
        assert!(!status.playing);
        assert_eq!(status.position_secs, 0.0);
    }

    #[test]
    fn unknown_audio_source_is_rejected_before_playback() {
        let error = audio_path("missing", "preview").unwrap_err();
        assert_eq!(error, "Unknown chart audio source: preview");
    }

    #[test]
    fn non_finite_volume_is_rejected() {
        let player = EditorAudioPlayer::new();
        assert_eq!(
            player.set_volume(f64::NAN).unwrap_err(),
            "Audio volume must be finite"
        );
    }

    #[test]
    fn non_finite_seek_is_rejected_before_backend_access() {
        let player = EditorAudioPlayer::new();
        assert_eq!(
            player.seek(f64::INFINITY).unwrap_err(),
            "Audio seek position must be finite"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_decoder_prepares_pcm_without_an_output_device() {
        let path = std::env::temp_dir().join(format!(
            "uta-studio-windows-decoder-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let samples = [0_i16, 500, -500, 1_000, -1_000, 500, -500, 0];
        let data_len = (samples.len() * std::mem::size_of::<i16>()) as u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_000_u32.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();

        let result = windows::PlayerInner::load_source(&path);
        let _ = std::fs::remove_file(&path);
        let (_, duration_secs) = result.expect("embedded WAV must decode on Windows");
        assert!(duration_secs > 0.0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_state_transition_requires_the_current_target_and_no_pending_request() {
        assert!(!linux::state_transition_confirmed(
            gstreamer::State::Playing,
            gstreamer::State::Paused,
            gstreamer::State::Paused,
        ));
        assert!(linux::state_transition_confirmed(
            gstreamer::State::Paused,
            gstreamer::State::VoidPending,
            gstreamer::State::Paused,
        ));
        assert!(!linux::state_transition_confirmed(
            gstreamer::State::Playing,
            gstreamer::State::Null,
            gstreamer::State::Null,
        ));
        assert!(linux::state_transition_confirmed(
            gstreamer::State::Null,
            gstreamer::State::VoidPending,
            gstreamer::State::Null,
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires an active desktop audio session and UTA_STUDIO_AUDIO_SMOKE_PATH"]
    fn native_player_sustains_real_audio_without_stalls() {
        let path = std::env::var_os("UTA_STUDIO_AUDIO_SMOKE_PATH")
            .map(std::path::PathBuf::from)
            .expect("UTA_STUDIO_AUDIO_SMOKE_PATH must point to a real audio fixture");
        gstreamer::init().expect("GStreamer must initialize");
        let mut player = linux::PlayerInner::default();
        let loaded = player.load(&path).expect("audio must load");
        assert!(loaded.loaded);
        player.play().expect("audio must play");

        let started = std::time::Instant::now();
        let mut previous = 0.0;
        let mut longest_stall = 0u32;
        let mut current_stall = 0u32;
        while started.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let status = player.status();
            assert!(
                status.error.is_none(),
                "native playback error: {:?}",
                status.error
            );
            assert!(
                !status.ended,
                "fixture ended before sustained test completed"
            );
            if status.position_secs > previous + 0.05 {
                current_stall = 0;
                previous = status.position_secs;
            } else {
                current_stall += 1;
                longest_stall = longest_stall.max(current_stall);
            }
        }
        let status = player.status();
        eprintln!(
            "native-audio-smoke position={:.3}s longest_stall={}ms",
            status.position_secs,
            longest_stall * 250
        );
        assert!(
            status.position_secs >= 27.0,
            "playback advanced only {:.3}s",
            status.position_secs
        );
        assert!(
            longest_stall <= 3,
            "playback position stalled for more than 750 ms"
        );

        let paused = player.pause().expect("audio must pause");
        assert!(!paused.playing);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let paused_later = player.status();
        assert!(
            (paused_later.position_secs - paused.position_secs).abs() < 0.15,
            "paused playback moved by {:.3}s",
            paused_later.position_secs - paused.position_secs
        );

        player.seek(12.0).expect("audio must seek");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let sought = player.status();
        assert!(
            (sought.position_secs - 12.0).abs() < 0.75,
            "seek landed at {:.3}s",
            sought.position_secs
        );
        player.play().expect("audio must resume");
        std::thread::sleep(std::time::Duration::from_millis(750));
        assert!(player.status().position_secs > sought.position_secs + 0.4);
        assert!(!player.stop().expect("audio must stop").loaded);
    }
}
