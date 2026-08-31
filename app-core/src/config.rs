use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cache::{CachePaths, config_path};

static CONFIG_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The local folder Uta! Studio scans for source audio and video files.
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
    /// Whether the application surface should let the native compositor show
    /// through. The desktop creates an alpha-capable Wayland surface up front
    /// and applies this preference through rendered pixel alpha.
    #[serde(default)]
    pub window_transparency: Option<bool>,
    /// Native acceleration preference. Production still uses only validated
    /// per-model routes and never treats this as permission to fall back.
    pub compute_backend: Option<String>,
    /// Global device-class preference (`cpu`, `gpu`, `integrated_gpu`),
    /// orthogonal to `compute_backend`'s runtime choice. Captured and
    /// forwarded to the Engine; Runtime Manager does not yet enumerate
    /// multiple physical devices, so this does not change device selection
    /// until that resolver support exists.
    #[serde(default)]
    pub default_device_class: Option<String>,
    /// Explicit model-specific backend choices. Missing entries use Runtime
    /// Manager's pinned route and never imply fallback.
    #[serde(default)]
    pub model_backend_overrides: BTreeMap<String, String>,
    /// Explicit model-specific device-class preference (`cpu`, `gpu`,
    /// `integrated_gpu`), orthogonal to `model_backend_overrides`'s runtime
    /// choice. Captured and forwarded to the Engine; Runtime Manager does not
    /// yet enumerate multiple physical devices, so this does not change
    /// device selection until that resolver support exists.
    #[serde(default)]
    pub model_device_overrides: BTreeMap<String, String>,
    /// Human-readable operator guidance retained in JSON because JSON has no
    /// comment syntax. Runtime routing never parses this field.
    #[serde(default = "default_model_backend_note")]
    pub model_backend_note: String,
    /// Product-level analysis intent. Backend resources and parameters are
    /// resolved only by the versioned Engine and Runtime Manager protocols.
    #[serde(default)]
    pub analysis_experience: crate::analysis_experience::AnalysisExperienceSettings,
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

fn default_model_backend_note() -> String {
    "Intel XPU recommendation: choose Vulkan / GGML for the five RoFormer models; that worker uses the tested serial/no-async path. Keep other models on their pinned backend unless validated separately. CPU is an explicit diagnostic route, never a fallback."
        .to_string()
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
            window_transparency: None,
            compute_backend: None,
            default_device_class: None,
            model_backend_overrides: BTreeMap::new(),
            model_device_overrides: BTreeMap::new(),
            model_backend_note: default_model_backend_note(),
            analysis_experience: crate::analysis_experience::AnalysisExperienceSettings::default(),
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
        // `native_dsp` is intentionally excluded here: it is a valid
        // per-model override (not every model has a native DSP path) but
        // `compile_analyze_request_v1` rejects it as a *global* backend
        // choice, so accepting it here would persist a config that then
        // fails every analysis request.
        self.compute_backend = Some(
            match self.compute_backend.as_deref() {
                Some("openvino") => "openvino",
                Some("vulkan") => "vulkan",
                Some("diagnostic_cpu") => "diagnostic_cpu",
                _ => "auto",
            }
            .to_string(),
        );
        self.model_backend_overrides.retain(|model_id, backend| {
            !model_id.trim().is_empty()
                && matches!(
                    backend.as_str(),
                    "openvino" | "vulkan" | "native_dsp" | "diagnostic_cpu"
                )
        });
        self.model_device_overrides.retain(|model_id, device| {
            !model_id.trim().is_empty()
                && matches!(device.as_str(), "cpu" | "gpu" | "integrated_gpu")
        });
        if !self
            .default_device_class
            .as_deref()
            .is_some_and(|device| matches!(device, "cpu" | "gpu" | "integrated_gpu"))
        {
            self.default_device_class = None;
        }
        if self.model_backend_note.trim().is_empty() {
            self.model_backend_note = default_model_backend_note();
        }
        self.analysis_experience.normalize();
        if self
            .ui_language
            .as_deref()
            .is_some_and(|language| !matches!(language, "en" | "zh-CN" | "ja"))
        {
            self.ui_language = None;
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
                let had_invalid_ui_language = cfg
                    .ui_language
                    .as_deref()
                    .is_some_and(|language| !matches!(language, "en" | "zh-CN" | "ja"));
                (
                    cfg.with_defaults(),
                    !had_data_path || had_invalid_ui_language,
                )
            }
            None => (Self::default().with_defaults(), true),
        };
        if should_save && let Err(error) = config.save() {
            tracing::error!("Could not save Uta! Studio configuration: {error}");
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

    pub fn analysis_quality(&self) -> crate::analysis_experience::AnalysisQualityProfile {
        self.analysis_experience.quality_profile
    }

    pub fn analysis_default_target(&self) -> crate::analysis_experience::AnalysisDefaultTarget {
        self.analysis_experience.default_target
    }

    pub fn preserve_continuous_pitch(&self) -> bool {
        self.analysis_experience.preserve_continuous_pitch
    }

    pub fn enable_quantization(&self) -> bool {
        self.analysis_experience.enable_quantization
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
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{AppConfig, write_config_atomically};

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
    fn old_backend_fields_are_dropped_when_configuration_is_rewritten() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "separator": "old",
            "beam_size": 11,
            "batch_size": 3,
            "auto_analyze": true
        }))
        .unwrap();
        let serialized = serde_json::to_value(config).unwrap();
        assert!(serialized.get("separator").is_none());
        assert!(serialized.get("beam_size").is_none());
        assert!(serialized.get("batch_size").is_none());
        assert_eq!(serialized["auto_analyze"], true);
    }

    #[test]
    fn invalid_analysis_experience_values_fall_back_to_product_defaults() {
        let json = serde_json::json!({
            "beam_size": 12,
            "analysis_experience": {
                "quality_profile": "unknown",
                "default_target": "unknown"
            }
        });
        let config: AppConfig = serde_json::from_value(json).unwrap();
        assert_eq!(
            config.analysis_quality(),
            crate::analysis_experience::AnalysisQualityProfile::Balanced
        );
        assert_eq!(
            config.analysis_default_target(),
            crate::analysis_experience::AnalysisDefaultTarget::FullCandidate
        );
    }

    #[test]
    fn model_backend_preferences_are_explicit_and_invalid_values_are_removed() {
        let repaired = AppConfig {
            model_backend_overrides: BTreeMap::from([
                (
                    "bs_roformer_leap_xe90_vocals".to_string(),
                    "vulkan".to_string(),
                ),
                ("rmvpe".to_string(), "automatic_fallback".to_string()),
            ]),
            model_backend_note: String::new(),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(
            repaired
                .model_backend_overrides
                .get("bs_roformer_leap_xe90_vocals")
                .map(String::as_str),
            Some("vulkan")
        );
        assert!(!repaired.model_backend_overrides.contains_key("rmvpe"));
        assert!(repaired.model_backend_note.contains("Intel XPU"));
        assert!(repaired.model_backend_note.contains("serial/no-async"));
    }

    #[test]
    fn model_device_preferences_are_explicit_and_invalid_values_are_removed() {
        let repaired = AppConfig {
            model_device_overrides: BTreeMap::from([
                (
                    "bs_roformer_leap_xe90_vocals".to_string(),
                    "gpu".to_string(),
                ),
                (
                    "melband_roformer_harmony".to_string(),
                    "integrated_gpu".to_string(),
                ),
                ("rmvpe".to_string(), "quantum_accelerator".to_string()),
            ]),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(
            repaired
                .model_device_overrides
                .get("bs_roformer_leap_xe90_vocals")
                .map(String::as_str),
            Some("gpu")
        );
        assert_eq!(
            repaired
                .model_device_overrides
                .get("melband_roformer_harmony")
                .map(String::as_str),
            Some("integrated_gpu")
        );
        assert!(!repaired.model_device_overrides.contains_key("rmvpe"));
    }

    #[test]
    fn default_device_class_is_explicit_and_invalid_values_are_cleared() {
        let valid = AppConfig {
            default_device_class: Some("integrated_gpu".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(
            valid.default_device_class.as_deref(),
            Some("integrated_gpu")
        );

        let invalid = AppConfig {
            default_device_class: Some("quantum_accelerator".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(invalid.default_device_class, None);
    }

    #[test]
    fn global_compute_backend_rejects_native_dsp_which_is_only_a_valid_model_override() {
        let config = AppConfig {
            compute_backend: Some("native_dsp".to_string()),
            ..AppConfig::default()
        }
        .with_defaults();
        assert_eq!(config.compute_backend.as_deref(), Some("auto"));
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
