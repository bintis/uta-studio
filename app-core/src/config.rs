use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cache::{CachePaths, config_path};

static CONFIG_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    #[serde(default)]
    pub separator_segment_size: Option<u32>,
    #[serde(default)]
    pub separator_overlap: Option<u32>,
    #[serde(default)]
    pub separator_batch_size: Option<u32>,
    #[serde(default)]
    pub separator_normalization_pct: Option<u32>,
    #[serde(default)]
    pub demucs_shifts: Option<u32>,
    #[serde(default)]
    pub demucs_overlap_pct: Option<u32>,
    /// Purpose-oriented audio-processing settings. Legacy `separator*` fields
    /// stay on disk for one release so older clients keep loading.
    #[serde(default)]
    pub audio_processing: Option<crate::audio_processing::AudioProcessingSettings>,
    pub asr_engine: Option<String>,
    pub align_backend: Option<String>,
    /// Pitch/frequency-analysis model. Kept explicit even while RMVPE is the
    /// only supported option so the settings and download API can evolve.
    #[serde(default)]
    pub pitch_model: Option<String>,
    pub vocal_detection_threshold_pct: Option<f64>,
    pub auto_analyze: Option<bool>,
    #[serde(default)]
    pub font_scale_percent: Option<u32>,
    /// Interface language: `en`, `zh-CN`, or `ja`. `None` follows the
    /// locale supplied by the operating environment and falls back to English.
    #[serde(default)]
    pub ui_language: Option<String>,
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
            separator_segment_size: None,
            separator_overlap: None,
            separator_batch_size: None,
            separator_normalization_pct: None,
            demucs_shifts: None,
            demucs_overlap_pct: None,
            audio_processing: None,
            asr_engine: None,
            align_backend: None,
            pitch_model: None,
            vocal_detection_threshold_pct: None,
            auto_analyze: None,
            font_scale_percent: Some(100),
            ui_language: None,
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
        if self.audio_processing.is_none() {
            self.audio_processing = Some(
                crate::audio_processing::AudioProcessingSettings::from_legacy_separator(
                    self.separator(),
                ),
            );
        }
        if !matches!(self.pitch_model.as_deref(), None | Some("rmvpe")) {
            self.pitch_model = Some("rmvpe".to_string());
        }
        if !matches!(
            self.align_backend.as_deref(),
            None | Some("whisperx" | "ctc" | "qwen" | "mms_karaoke")
        ) {
            self.align_backend = Some("whisperx".to_string());
        }
        if self
            .ui_language
            .as_deref()
            .is_some_and(|language| !matches!(language, "en" | "zh-CN" | "ja"))
        {
            self.ui_language = None;
        }
        self
    }

    fn recover_previous_install(self) -> (Self, bool) {
        if std::env::var_os("UTA_STUDIO_DATA_PATH").is_some_and(|path| !path.is_empty()) {
            return (self, false);
        }
        let default_root = Self::default_data_path();
        let candidate = dirs::document_dir().map(|documents| documents.join("uta-studio"));
        self.recover_previous_install_from(&default_root, candidate.as_deref())
    }

    fn recover_previous_install_from(
        mut self,
        default_root: &std::path::Path,
        candidate: Option<&std::path::Path>,
    ) -> (Self, bool) {
        let active_root = self.effective_data_path();
        let Some(candidate) = candidate else {
            return (self, false);
        };
        if active_root != default_root
            || candidate == default_root
            || active_root.join("vendor/.ready").is_file()
        {
            return (self, false);
        }

        let Ok(marker) = std::fs::read_to_string(candidate.join("vendor/.ready")) else {
            return (self, false);
        };
        let backend = marker
            .trim()
            .strip_prefix("runtime-v5:")
            .or_else(|| marker.trim().strip_prefix("runtime-v4:"));
        let Some(backend) = backend else {
            return (self, false);
        };
        if !matches!(backend, "cpu" | "cuda" | "intel") {
            return (self, false);
        }

        self.data_path = Some(candidate.to_path_buf());
        if self.library_source.is_none() && candidate.join("songs.db").is_file() {
            self.library_source = Some(LibrarySource::Folders {
                paths: vec![candidate.to_path_buf()],
            });
        }
        self.compute_backend
            .get_or_insert_with(|| backend.to_string());

        let models = candidate.join("models");
        if self.whisper_model.is_none() && models.join("whisper/openvino-large-v3-turbo").is_dir() {
            self.whisper_model = Some("large-v3-turbo".to_string());
        }
        if self.separator.is_none()
            && models
                .join("separation/openvino-demucs/htdemucs_v4")
                .is_dir()
        {
            self.separator = Some("openvino_demucs".to_string());
        }
        if self.asr_engine.is_none() && models.join("whisper").is_dir() {
            self.asr_engine = Some("whisper".to_string());
        }
        if self.align_backend.is_none()
            && models
                .join("huggingface/hub/models--Qwen--Qwen3-ForcedAligner-0.6B-hf")
                .is_dir()
        {
            self.align_backend = Some("qwen".to_string());
        }
        if self.pitch_model.is_none() && models.join("pitch/rmvpe/manifest.json").is_file() {
            self.pitch_model = Some("rmvpe".to_string());
        }

        (self, true)
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
                let had_invalid_align_backend = !matches!(
                    cfg.align_backend.as_deref(),
                    None | Some("whisperx" | "ctc" | "qwen" | "mms_karaoke")
                );
                let had_invalid_ui_language = cfg
                    .ui_language
                    .as_deref()
                    .is_some_and(|language| !matches!(language, "en" | "zh-CN" | "ja"));
                (
                    cfg.with_defaults(),
                    !had_data_path
                        || had_invalid_separator
                        || had_invalid_pitch_model
                        || had_invalid_align_backend
                        || had_invalid_ui_language,
                )
            }
            None => (Self::default().with_defaults(), true),
        };
        let (config, recovered_previous_install) = config.recover_previous_install();

        if (should_save || recovered_previous_install)
            && let Err(error) = config.save()
        {
            tracing::error!("Could not save Uta Studio configuration: {error}");
        }

        config
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create config directory: {error}"))?;
        }
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("could not serialize config: {error}"))?;
        write_config_atomically(&path, &json)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
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

    pub fn separator_segment_size(&self) -> u32 {
        self.separator_segment_size.unwrap_or(256).clamp(64, 1024)
    }

    pub fn separator_overlap(&self) -> u32 {
        self.separator_overlap.unwrap_or(8).clamp(2, 32)
    }

    pub fn separator_batch_size(&self) -> u32 {
        self.separator_batch_size.unwrap_or(1).clamp(1, 8)
    }

    pub fn separator_normalization_pct(&self) -> u32 {
        self.separator_normalization_pct.unwrap_or(90).clamp(1, 100)
    }

    pub fn demucs_shifts(&self) -> u32 {
        self.demucs_shifts.unwrap_or(1).clamp(1, 8)
    }

    pub fn demucs_overlap_pct(&self) -> u32 {
        self.demucs_overlap_pct.unwrap_or(25).clamp(1, 95)
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

    pub fn font_scale_percent(&self) -> u32 {
        self.font_scale_percent.unwrap_or(100).clamp(80, 140)
    }

    pub fn font_scale(&self) -> f32 {
        self.font_scale_percent() as f32 / 100.0
    }

    pub fn ui_language(&self) -> &str {
        self.ui_language.as_deref().unwrap_or("system")
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

    pub fn clear_language_override(&mut self, file_hash: &str) {
        if let Some(overrides) = self.language_overrides.as_mut() {
            overrides.remove(file_hash);
            if overrides.is_empty() {
                self.language_overrides = None;
            }
        }
    }
}

fn write_config_atomically(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");

    for _ in 0..32 {
        let sequence = CONFIG_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let result = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result;
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary config file",
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{AppConfig, LibrarySource, write_config_atomically};

    #[test]
    fn defaults_do_not_invent_an_empty_library_source() {
        assert!(
            AppConfig::default()
                .with_defaults()
                .library_source
                .is_none()
        );
    }

    #[test]
    fn interface_language_is_persisted_and_invalid_values_are_repaired() {
        let explicit = AppConfig {
            ui_language: Some("ja".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(explicit.ui_language(), "ja");

        let repaired = AppConfig {
            ui_language: Some("unsupported".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(repaired.ui_language(), "system");
        assert!(repaired.ui_language.is_none());
    }

    #[test]
    fn mms_karaoke_is_a_valid_alignment_backend() {
        let config = AppConfig {
            align_backend: Some("mms_karaoke".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(config.align_backend(), "mms_karaoke");

        let repaired = AppConfig {
            align_backend: Some("not-a-backend".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(repaired.align_backend(), "whisperx");
    }

    #[test]
    fn recovers_a_complete_previous_runtime_without_moving_user_data() {
        let test_root = std::env::temp_dir().join(format!(
            "uta-studio-config-recovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let default_root = test_root.join("default");
        let previous_root = test_root.join("previous");
        std::fs::create_dir_all(previous_root.join("vendor")).unwrap();
        std::fs::create_dir_all(previous_root.join("models/whisper/openvino-large-v3-turbo"))
            .unwrap();
        std::fs::create_dir_all(
            previous_root.join("models/separation/openvino-demucs/htdemucs_v4"),
        )
        .unwrap();
        std::fs::create_dir_all(previous_root.join("models/pitch/rmvpe")).unwrap();
        std::fs::write(previous_root.join("vendor/.ready"), "runtime-v4:intel").unwrap();
        std::fs::write(previous_root.join("models/pitch/rmvpe/manifest.json"), "{}").unwrap();
        std::fs::write(previous_root.join("songs.db"), []).unwrap();

        let config = AppConfig {
            data_path: Some(default_root.clone()),
            ..AppConfig::default()
        };
        let (recovered, changed) =
            config.recover_previous_install_from(&default_root, Some(&previous_root));

        assert!(changed);
        assert_eq!(
            recovered.data_path.as_deref(),
            Some(previous_root.as_path())
        );
        assert_eq!(recovered.compute_backend.as_deref(), Some("intel"));
        assert_eq!(recovered.whisper_model.as_deref(), Some("large-v3-turbo"));
        assert_eq!(recovered.separator.as_deref(), Some("openvino_demucs"));
        assert_eq!(recovered.pitch_model.as_deref(), Some("rmvpe"));
        assert_eq!(
            recovered.library_source,
            Some(LibrarySource::Folders {
                paths: vec![previous_root.clone()]
            })
        );
        assert!(!default_root.join("vendor").exists());
        assert!(previous_root.join("vendor/.ready").is_file());

        std::fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn atomic_config_write_replaces_the_target_without_leaving_a_temporary_file() {
        let test_root = std::env::temp_dir().join(format!(
            "uta-studio-atomic-config-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_root).unwrap();
        let target = test_root.join("config.json");
        std::fs::write(&target, b"old configuration").unwrap();

        write_config_atomically(&target, b"new configuration").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new configuration");
        assert_eq!(std::fs::read_dir(&test_root).unwrap().count(), 1);
        std::fs::remove_dir_all(test_root).unwrap();
    }
}
