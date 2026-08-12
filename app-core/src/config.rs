use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cache::{CachePaths, config_path};

/// The local folder Uta Studio scans for source audio and video files.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum LibrarySource {
    Folders { paths: Vec<PathBuf> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppConfig {
    #[serde(default = "default_data_path_option")]
    pub data_path: Option<PathBuf>,
    #[serde(default)]
    pub cache_paths: Option<CachePaths>,
    #[serde(default)]
    pub library_source: Option<LibrarySource>,
    /// Preferred folder shown by every export dialog. Individual exports may
    /// still choose another destination through Save As.
    #[serde(default)]
    pub export_path: Option<PathBuf>,
    pub fullscreen: Option<bool>,
    pub dark_mode: Option<bool>,
    /// Python AI runtime selected during setup: `cpu`, `cuda`, or `intel`.
    /// Changing it requires re-running setup so the virtualenv is rebuilt.
    pub compute_backend: Option<String>,
    pub whisper_model: Option<String>,
    pub beam_size: Option<u32>,
    pub batch_size: Option<u32>,
    pub separator: Option<String>,
    pub asr_engine: Option<String>,
    pub align_backend: Option<String>,
    /// Pitch/frequency-analysis model. Kept explicit even while RMVPE is the
    /// only supported option so the settings and download API can evolve.
    #[serde(default)]
    pub pitch_model: Option<String>,
    pub vocal_detection_threshold_pct: Option<f64>,
    pub auto_analyze: Option<bool>,
    pub song_list_view: Option<String>,
    pub language_overrides: Option<HashMap<String, String>>,
}

fn default_data_path_option() -> Option<PathBuf> {
    Some(AppConfig::default_data_path())
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_path: default_data_path_option(),
            cache_paths: None,
            library_source: None,
            export_path: None,
            fullscreen: None,
            dark_mode: None,
            compute_backend: None,
            whisper_model: None,
            beam_size: None,
            batch_size: None,
            separator: None,
            asr_engine: None,
            align_backend: None,
            pitch_model: None,
            vocal_detection_threshold_pct: None,
            auto_analyze: None,
            song_list_view: None,
            language_overrides: None,
        }
    }
}

impl AppConfig {
    pub fn default_data_path() -> PathBuf {
        crate::cache::default_uta_studio_dir()
    }

    pub fn effective_data_path(&self) -> PathBuf {
        self.data_path
            .clone()
            .unwrap_or_else(Self::default_data_path)
    }

    fn with_defaults(mut self) -> Self {
        if self.data_path.is_none() {
            self.data_path = Some(Self::default_data_path());
        }
        if !matches!(
            self.separator.as_deref(),
            None | Some("karaoke" | "demucs" | "openvino_demucs")
        ) {
            self.separator = Some("karaoke".to_string());
        }
        if !matches!(self.pitch_model.as_deref(), None | Some("rmvpe")) {
            self.pitch_model = Some("rmvpe".to_string());
        }
        self
    }

    pub fn library_paths(&self) -> Vec<PathBuf> {
        match self.library_source.as_ref() {
            Some(LibrarySource::Folders { paths }) => paths.clone(),
            None => Vec::new(),
        }
    }

    pub fn load() -> Self {
        let path = config_path();

        let loaded = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str::<Self>(&contents).ok());

        let (config, should_save) = match loaded {
            Some(cfg) => {
                let had_data_path = cfg.data_path.is_some();
                let had_invalid_separator = !matches!(
                    cfg.separator.as_deref(),
                    None | Some("karaoke" | "demucs" | "openvino_demucs")
                );
                let had_invalid_pitch_model =
                    !matches!(cfg.pitch_model.as_deref(), None | Some("rmvpe"));
                (
                    cfg.with_defaults(),
                    !had_data_path || had_invalid_separator || had_invalid_pitch_model,
                )
            }
            None => (Self::default().with_defaults(), true),
        };

        if should_save {
            config.save();
        }

        config
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn whisper_model(&self) -> &str {
        self.whisper_model.as_deref().unwrap_or("large-v3")
    }

    pub fn beam_size(&self) -> u32 {
        self.beam_size.unwrap_or(8)
    }

    pub fn batch_size(&self) -> u32 {
        self.batch_size.unwrap_or(8)
    }

    pub fn separator(&self) -> &str {
        self.separator.as_deref().unwrap_or("karaoke")
    }

    pub fn asr_engine(&self) -> &str {
        self.asr_engine.as_deref().unwrap_or("whisper")
    }

    pub fn align_backend(&self) -> &str {
        self.align_backend.as_deref().unwrap_or("whisperx")
    }

    pub fn pitch_model(&self) -> &str {
        self.pitch_model.as_deref().unwrap_or("rmvpe")
    }

    pub fn vocal_detection_threshold_pct(&self) -> f64 {
        self.vocal_detection_threshold_pct
            .unwrap_or(0.15)
            .clamp(0.0, 1.0)
    }

    pub fn auto_analyze(&self) -> bool {
        self.auto_analyze.unwrap_or(false)
    }

    pub fn language_override(&self, file_hash: &str) -> Option<&str> {
        self.language_overrides
            .as_ref()
            .and_then(|m| m.get(file_hash))
            .map(|s| s.as_str())
    }

    pub fn set_language_override(&mut self, file_hash: String, lang: String) {
        self.language_overrides
            .get_or_insert_with(HashMap::new)
            .insert(file_hash, lang);
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn defaults_do_not_invent_an_empty_library_source() {
        assert!(
            AppConfig::default()
                .with_defaults()
                .library_source
                .is_none()
        );
    }
}
