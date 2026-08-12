use std::sync::Mutex;

use serde::Serialize;

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
    #[cfg(target_os = "linux")]
    inner: Mutex<linux::PlayerInner>,
    initialization_error: Option<String>,
}

impl EditorAudioPlayer {
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let initialization_error = gstreamer::init()
                .err()
                .map(|error| format!("Could not initialize native audio: {error}"));
            Self {
                inner: Mutex::new(linux::PlayerInner::default()),
                initialization_error,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {
                initialization_error: Some(
                    "Native editor audio is currently available in the Linux package".to_string(),
                ),
            }
        }
    }

    fn ready(&self) -> Result<(), String> {
        match &self.initialization_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use gst::prelude::*;
    use gstreamer as gst;

    use super::EditorAudioStatus;

    #[derive(Default)]
    pub struct PlayerInner {
        player: Option<gst::Element>,
        path: Option<PathBuf>,
        ended: bool,
        error: Option<String>,
    }

    impl PlayerInner {
        pub fn load(&mut self, path: &Path) -> Result<EditorAudioStatus, String> {
            self.stop();
            if !path.is_file() {
                return Err(format!("Chart audio does not exist: {}", path.display()));
            }

            let uri = gst::glib::filename_to_uri(path, None)
                .map_err(|error| format!("Could not convert audio path to URI: {error}"))?;
            let player = gst::ElementFactory::make("playbin")
                .property("uri", uri.as_str())
                .build()
                .map_err(|error| format!("Could not create native audio player: {error}"))?;

            // PulseAudio's PipeWire compatibility sink is markedly more stable
            // than WebKitGTK's media process on some Linux desktops. A generous
            // local buffer keeps UI/compositor activity from starving playback.
            if let Ok(sink) = gst::ElementFactory::make("pulsesink")
                .property("buffer-time", 250_000i64)
                .property("latency-time", 25_000i64)
                .build()
            {
                player.set_property("audio-sink", sink);
            }

            player
                .set_state(gst::State::Paused)
                .map_err(|error| format!("Could not prepare chart audio: {error:?}"))?;
            self.player = Some(player);
            self.path = Some(path.to_path_buf());
            self.ended = false;
            self.error = None;
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
            let pending_state = player.pending_state();
            let effective_state = if pending_state == gst::State::VoidPending {
                current_state
            } else {
                pending_state
            };
            let duration_secs = player
                .query_duration::<gst::ClockTime>()
                .map(|value| value.seconds_f64())
                .unwrap_or(0.0);
            let position_secs = player
                .query_position::<gst::ClockTime>()
                .map(|value| value.seconds_f64())
                .unwrap_or(0.0);
            EditorAudioStatus {
                loaded: self.path.is_some(),
                playing: effective_state == gst::State::Playing
                    && !self.ended
                    && self.error.is_none(),
                position_secs,
                duration_secs,
                ended: self.ended,
                error: self.error.clone(),
            }
        }

        pub fn stop(&mut self) -> EditorAudioStatus {
            if let Some(player) = self.player.take() {
                let _ = player.set_state(gst::State::Null);
            }
            self.path = None;
            self.ended = false;
            self.error = None;
            EditorAudioStatus::default()
        }
    }

    impl Drop for PlayerInner {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

fn audio_path(file_hash: &str, source: &str) -> Result<std::path::PathBuf, String> {
    let chart = app_core::load_chart(file_hash).map_err(|error| error.to_string())?;
    let path = match source {
        "vocals" => chart
            .audio
            .vocals
            .as_deref()
            .unwrap_or(&chart.audio.instrumental),
        "instrumental" => &chart.audio.instrumental,
        "original" => &chart.audio.original,
        _ => return Err(format!("Unknown chart audio source: {source}")),
    };
    Ok(path.into())
}

#[tauri::command]
pub fn editor_audio_load(
    player: tauri::State<'_, EditorAudioPlayer>,
    file_hash: String,
    source: String,
) -> Result<EditorAudioStatus, String> {
    #[cfg(target_os = "linux")]
    {
        player.ready()?;
        let path = audio_path(&file_hash, &source)?;
        player
            .inner
            .lock()
            .map_err(|_| "Native audio player lock is poisoned".to_string())?
            .load(&path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (player, file_hash, source);
        Err("Native editor audio is currently available in the Linux package".to_string())
    }
}

macro_rules! player_command {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        pub fn $name(
            player: tauri::State<'_, EditorAudioPlayer>,
        ) -> Result<EditorAudioStatus, String> {
            #[cfg(target_os = "linux")]
            {
                player.ready()?;
                player
                    .inner
                    .lock()
                    .map_err(|_| "Native audio player lock is poisoned".to_string())?
                    .$method()
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = player;
                Err("Native editor audio is currently available in the Linux package".to_string())
            }
        }
    };
}

player_command!(editor_audio_play, play);
player_command!(editor_audio_pause, pause);

pub(crate) fn probe_audio(path: &std::path::Path) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        gstreamer::init().map_err(|error| format!("Could not initialize native audio: {error}"))?;
        let mut player = linux::PlayerInner::default();
        let status = player.load(path)?;
        if !status.loaded {
            return Err("Native audio pipeline did not retain the selected source".to_string());
        }
        player.stop();
        Ok(format!(
            "Native GStreamer pipeline prepared {}",
            path.display()
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Native editor audio probe is currently available in the Linux package".to_string())
    }
}

#[tauri::command]
pub fn editor_audio_seek(
    player: tauri::State<'_, EditorAudioPlayer>,
    position_secs: f64,
) -> Result<EditorAudioStatus, String> {
    #[cfg(target_os = "linux")]
    {
        player.ready()?;
        player
            .inner
            .lock()
            .map_err(|_| "Native audio player lock is poisoned".to_string())?
            .seek(position_secs)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (player, position_secs);
        Err("Native editor audio is currently available in the Linux package".to_string())
    }
}

#[tauri::command]
pub fn editor_audio_status(
    player: tauri::State<'_, EditorAudioPlayer>,
) -> Result<EditorAudioStatus, String> {
    #[cfg(target_os = "linux")]
    {
        player.ready()?;
        Ok(player
            .inner
            .lock()
            .map_err(|_| "Native audio player lock is poisoned".to_string())?
            .status())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = player;
        Err("Native editor audio is currently available in the Linux package".to_string())
    }
}

#[tauri::command]
pub fn editor_audio_stop(
    player: tauri::State<'_, EditorAudioPlayer>,
) -> Result<EditorAudioStatus, String> {
    #[cfg(target_os = "linux")]
    {
        player.ready()?;
        Ok(player
            .inner
            .lock()
            .map_err(|_| "Native audio player lock is poisoned".to_string())?
            .stop())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = player;
        Err("Native editor audio is currently available in the Linux package".to_string())
    }
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
        assert!(!player.stop().loaded);
    }
}
