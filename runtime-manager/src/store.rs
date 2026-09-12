use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::LIBTORCH_XPU_RUNTIME_ID;
use crate::error::RuntimeManagerResult;
use crate::resource::{ResourceKind, ResourceRef};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorePaths {
    pub store_root: Option<PathBuf>,
    ggml_models_root: Option<PathBuf>,
    runtime_overrides: Vec<(String, PathBuf)>,
    /// Installed native runtime directories keyed by runtime id. A runtime
    /// that declares a `native_library` is ready only when that file exists
    /// under its directory.
    runtime_library_roots: Vec<(String, PathBuf)>,
    tool_overrides: Vec<(String, PathBuf)>,
    tool_fallbacks: Vec<(String, PathBuf)>,
    fusion_adapter_fallbacks: Vec<(String, PathBuf)>,
}

impl StorePaths {
    pub fn new(store_root: impl Into<PathBuf>) -> Self {
        Self::default().with_store_root(store_root)
    }

    pub fn from_env() -> Self {
        let store_root = std::env::var_os("UTA_STUDIO_RUNTIME_STORE")
            .map(PathBuf::from)
            .or_else(default_store_root);
        let ggml_models_root = select_ggml_models_root(
            std::env::var_os("UTA_STUDIO_GGML_MODELS_DIR").map(PathBuf::from),
            store_root.as_deref(),
        );
        let libtorch_runtime_root = std::env::var_os("UTA_STUDIO_LIBTORCH_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                store_root
                    .as_ref()
                    .map(|root| root.join(LIBTORCH_XPU_RUNTIME_DIRECTORY))
            });
        let mut paths = Self {
            store_root,
            ggml_models_root,
            runtime_overrides: Vec::new(),
            runtime_library_roots: Vec::new(),
            tool_overrides: Vec::new(),
            tool_fallbacks: Vec::new(),
            fusion_adapter_fallbacks: Vec::new(),
        };
        if let Some(root) = libtorch_runtime_root {
            paths = paths.with_runtime_library_root(LIBTORCH_XPU_RUNTIME_ID, root);
        }
        let executable_directory = std::env::current_exe()
            .ok()
            .and_then(|executable| executable.parent().map(Path::to_path_buf));
        let configured = std::env::var_os("UTA_STUDIO_GGML_RUNTIME_PATH").map(PathBuf::from);
        let packaged = discover_packaged_worker(executable_directory.as_deref());
        if let Some(path) = configured.or(packaged) {
            // The one packaged worker executes both native runtimes; register
            // it under its component id so every runtime that names that
            // component resolves the same executable.
            paths = paths
                .with_runtime_override("ggml_vulkan", path.clone())
                .with_runtime_override("uta-ggml-worker", path);
        }
        let ffmpeg_path = std::env::var_os("UTA_STUDIO_FFMPEG_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("PATH").and_then(|paths| {
                    std::env::split_paths(&paths)
                        .map(|directory| directory.join("ffmpeg"))
                        .find(|path| executable_file(path))
                })
            });
        if let Some(path) = ffmpeg_path {
            paths = paths.with_tool_override("ffmpeg", path);
        }
        for provider in uta_fusion_agent_adapter::Provider::ALL {
            let discovered = executable_directory
                .as_deref()
                .and_then(|directory| {
                    sibling_executable(directory, provider.adapter_executable_name())
                })
                .filter(|candidate| {
                    crate::external_tool::fusion_adapter_manifest(candidate).is_ok()
                })
                .or_else(|| discover_provider_adapter_on_path(provider));
            if let Some(path) = discovered {
                paths = paths.with_fusion_adapter_fallback(provider.id(), path);
            }
        }
        let configured_fusion_adapter = std::env::var_os("UTA_STUDIO_FUSION_AGENT_ADAPTER_PATH")
            .or_else(|| std::env::var_os("UTA_STUDIO_FUSION_AGENT_CLI_PATH"))
            .map(PathBuf::from);
        if let Some(path) = configured_fusion_adapter {
            paths = paths.with_tool_override("fusion_agent_adapter", path);
        } else {
            let discovered = executable_directory
                .as_deref()
                .and_then(|directory| sibling_executable(directory, "uta-fusion-agent-adapter"))
                .filter(|candidate| {
                    crate::external_tool::fusion_adapter_manifest(candidate).is_ok()
                })
                .or_else(discover_fusion_adapter_on_path);
            if let Some(path) = discovered {
                paths = paths.with_tool_fallback("fusion_agent_adapter", path);
            }
        }
        paths
    }

    pub fn with_store_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.store_root = Some(root.into());
        self
    }

    pub fn with_ggml_models_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.ggml_models_root = Some(root.into());
        self
    }

    pub fn ggml_model_path(&self, model_id: &str) -> Option<PathBuf> {
        let (directory, filename) = match model_id {
            "melband_roformer_harmony" => {
                ("melband_roformer_karaoke_aufr33_viperx", "model-fp16.gguf")
            }
            "bs_roformer_leap_xe90_vocals" => (model_id, "bs_leap_xe_voc-F32.gguf"),
            "bs_roformer_leap_xe90_instrumental" => (model_id, "bs_leap_xe_inst-F32.gguf"),
            "melband_roformer_denoise_aufr33"
            | "melband_roformer_dereverb_anvuew"
            | "bs_polarformer_public_instrumental" => (model_id, "model-fp16.gguf"),
            "rmvpe" => (model_id, "rmvpe-f32.gguf"),
            "fcpe" => (model_id, "fcpe-f32.gguf"),
            "basic_pitch" => (model_id, "basic-pitch-f32.gguf"),
            "game_1_0_3_small" => ("game", "game-small-f32.gguf"),
            "game_1_0_3_medium" => ("game", "game-medium-f32.gguf"),
            "game_1_0_3_large" => ("game", "game-large-f32.gguf"),
            "jbm555_cectc_80" => ("jbm555", "jbm555-cectc80-f32.gguf"),
            "stars" => (model_id, "stars-f32.gguf"),
            "rosvot" => (model_id, "rosvot-f32.gguf"),
            "firered_asr2_aed" => (model_id, "firered-f32.gguf"),
            "qwen3_asr_1_7b" => (model_id, "Qwen3-ASR-1.7B-F16.gguf"),
            "qwen3_forced_aligner_0_6b" => (model_id, "Qwen3-ForcedAligner-0.6B-F16.gguf"),
            _ => return None,
        };
        self.ggml_models_root
            .as_ref()
            .map(|root| root.join(directory).join(filename))
            .filter(|path| {
                path.is_file()
                    && (model_id != "firered_asr2_aed"
                        || path.parent().is_some_and(|directory| {
                            directory.join("cmvn.ark").is_file()
                                && directory.join("dict.txt").is_file()
                        }))
            })
    }

    pub fn with_runtime_override(
        mut self,
        runtime_id: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.runtime_overrides
            .push((runtime_id.into(), path.into()));
        self
    }

    pub fn with_runtime_library_root(
        mut self,
        runtime_id: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Self {
        self.runtime_library_roots
            .push((runtime_id.into(), root.into()));
        self
    }

    /// Installed directory of a native runtime's shared libraries. The
    /// directory may not exist yet; readiness checks the declared library.
    pub fn runtime_library_root(&self, runtime_id: &str) -> Option<PathBuf> {
        self.runtime_library_roots
            .iter()
            .rev()
            .find(|(id, _)| id == runtime_id)
            .map(|(_, root)| root.clone())
    }

    /// The declared native library of a runtime, when it is installed.
    pub fn runtime_native_library(&self, runtime_id: &str, relative: &str) -> Option<PathBuf> {
        self.runtime_library_root(runtime_id)
            .map(|root| root.join(relative))
            .filter(|path| path.is_file())
    }

    pub fn with_tool_override(
        mut self,
        tool_id: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.tool_overrides.push((tool_id.into(), path.into()));
        self
    }

    pub fn current_pointer_path(&self, resource: &ResourceRef) -> Option<PathBuf> {
        let root = self.store_root.as_ref()?;
        let kind_dir = match resource.kind {
            ResourceKind::Model => "models",
            ResourceKind::Runtime => "runtimes",
            ResourceKind::Tool => "tools",
            ResourceKind::Bundle => return None,
        };
        Some(root.join(kind_dir).join(&resource.id).join("current.json"))
    }

    pub fn runtime_executable(&self, runtime_id: &str) -> Option<PathBuf> {
        self.runtime_overrides
            .iter()
            .rev()
            .find(|(id, _)| id == runtime_id)
            .map(|(_, path)| path.clone())
            .filter(|path| executable_file(path))
    }

    pub(crate) fn tool_override_path(&self, tool_id: &str) -> Option<PathBuf> {
        self.tool_overrides
            .iter()
            .rev()
            .find(|(id, _)| id == tool_id)
            .map(|(_, path)| path.clone())
    }

    pub fn configured_tool_path(&self, tool_id: &str) -> Option<PathBuf> {
        self.configured_tool_path_result(tool_id).ok().flatten()
    }

    pub(crate) fn configured_tool_path_result(
        &self,
        tool_id: &str,
    ) -> RuntimeManagerResult<Option<PathBuf>> {
        crate::external_tool::configured_tool_path(self, tool_id)
    }

    pub(crate) fn tool_fallback_path(&self, tool_id: &str) -> Option<PathBuf> {
        self.tool_fallbacks
            .iter()
            .rev()
            .find(|(id, _)| id == tool_id)
            .map(|(_, path)| path.clone())
    }

    fn with_tool_fallback(mut self, tool_id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.tool_fallbacks.push((tool_id.into(), path.into()));
        self
    }

    pub(crate) fn with_fusion_adapter_fallback(
        mut self,
        provider: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        self.fusion_adapter_fallbacks
            .push((provider.into(), path.into()));
        self
    }

    pub(crate) fn fusion_adapter_fallback_path(&self, provider: &str) -> Option<PathBuf> {
        self.fusion_adapter_fallbacks
            .iter()
            .rev()
            .find(|(id, _)| id == provider)
            .map(|(_, path)| path.clone())
            .filter(|path| executable_file(path))
    }

    pub fn tool_candidate_path(&self, tool_id: &str) -> Option<PathBuf> {
        self.tool_candidate_path_result(tool_id).ok().flatten()
    }

    pub(crate) fn tool_candidate_path_result(
        &self,
        tool_id: &str,
    ) -> RuntimeManagerResult<Option<PathBuf>> {
        if let Some(path) = self.tool_override_path(tool_id) {
            return Ok(Some(path));
        }
        Ok(self
            .configured_tool_path_result(tool_id)?
            .or_else(|| self.tool_fallback_path(tool_id)))
    }

    pub fn tool_executable(&self, tool_id: &str) -> Option<PathBuf> {
        self.tool_candidate_path(tool_id)
            .filter(|path| executable_file(path))
    }

    pub fn paths_summary(&self) -> PathsSummary {
        let store_root = self.store_root.clone();
        PathsSummary {
            store_root: store_root.clone(),
            model_root: store_root.as_ref().map(|root| root.join("models")),
            runtime_root: store_root.as_ref().map(|root| root.join("runtimes")),
            download_cache: store_root.as_ref().map(|root| root.join("downloads")),
            staging_root: store_root.as_ref().map(|root| root.join("staging")),
            leases_root: store_root.as_ref().map(|root| root.join("leases")),
            locks_root: store_root.as_ref().map(|root| root.join("locks")),
            ggml_models_root: self.ggml_models_root.clone(),
            ffmpeg_path: self.tool_executable("ffmpeg"),
            runtime_executables: self
                .runtime_overrides
                .iter()
                .filter(|(_, path)| executable_file(path))
                .map(|(id, path)| (id.clone(), path.clone()))
                .collect(),
            runtime_library_roots: self
                .runtime_library_roots
                .iter()
                .map(|(id, root)| (id.clone(), root.clone()))
                .collect(),
        }
    }
}

/// Directory under the managed runtime store that holds the installed native
/// LibTorch XPU runtime, beside `ggml-vulkan` and `ggml-models`.
pub const LIBTORCH_XPU_RUNTIME_DIRECTORY: &str = "libtorch-xpu";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathsSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_cache: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leases_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locks_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ggml_models_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_executables: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_library_roots: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentPointer {
    pub generation: String,
}

fn sibling_executable(directory: &Path, executable_name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{executable_name}.exe")
    } else {
        executable_name.to_string()
    };
    let path = directory.join(filename);
    executable_file(&path).then_some(path)
}

fn discover_packaged_worker(executable_directory: Option<&Path>) -> Option<PathBuf> {
    if let Some(directory) = executable_directory {
        if let Some(path) = sibling_executable(directory, "uta-ggml-worker") {
            return Some(path);
        }
        if let Some(parent) = directory.parent() {
            for candidate_directory in [
                parent.join("release"),
                parent.join("debug"),
                parent.join("bin"),
            ] {
                if let Some(path) = sibling_executable(&candidate_directory, "uta-ggml-worker") {
                    return Some(path);
                }
            }
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        let filename = if cfg!(windows) {
            "uta-ggml-worker.exe"
        } else {
            "uta-ggml-worker"
        };
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(filename);
            if executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn discover_fusion_adapter_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names = if cfg!(windows) {
        [
            "uta-fusion-agent-adapter.exe",
            "uta-fusion-agent-pi.exe",
            "uta-fusion-agent-codex.exe",
            "uta-fusion-agent-claude.exe",
        ]
    } else {
        [
            "uta-fusion-agent-adapter",
            "uta-fusion-agent-pi",
            "uta-fusion-agent-codex",
            "uta-fusion-agent-claude",
        ]
    };
    std::env::split_paths(&path)
        .flat_map(|directory| names.map(move |name| directory.join(name)))
        .find(|candidate| {
            executable_file(candidate)
                && crate::external_tool::fusion_adapter_manifest(candidate).is_ok()
        })
}

fn discover_provider_adapter_on_path(
    provider: uta_fusion_agent_adapter::Provider,
) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let filename = if cfg!(windows) {
        format!("{}.exe", provider.adapter_executable_name())
    } else {
        provider.adapter_executable_name().to_string()
    };
    std::env::split_paths(&path)
        .map(|directory| directory.join(&filename))
        .find(|candidate| {
            executable_file(candidate)
                && crate::external_tool::fusion_adapter_manifest(candidate).is_ok()
        })
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn executable_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

#[cfg(not(any(unix, windows)))]
fn executable_file(path: &Path) -> bool {
    path.is_file()
}

fn select_ggml_models_root(
    configured: Option<PathBuf>,
    store_root: Option<&Path>,
) -> Option<PathBuf> {
    configured.or_else(|| store_root.map(|root| root.join("ggml-models")))
}

fn default_store_root() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Uta! Studio").join("runtime"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local/share"))
            })
            .map(|root| root.join("uta-studio").join("runtime"))
    }
}

pub fn read_current_pointer(path: &Path) -> Option<CurrentPointer> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ggml_models_default_belongs_to_the_durable_runtime_store() {
        let store = Path::new("/durable/runtime");
        assert_eq!(
            select_ggml_models_root(None, Some(store)),
            Some(store.join("ggml-models"))
        );
        assert_eq!(
            select_ggml_models_root(Some(PathBuf::from("/explicit/models")), Some(store)),
            Some(PathBuf::from("/explicit/models"))
        );
    }

    #[test]
    fn rmvpe_uses_its_f32_filename_in_the_durable_ggml_root() {
        let root =
            std::env::temp_dir().join(format!("uta-rmvpe-ggml-path-test-{}", std::process::id()));
        let model = root.join("rmvpe/rmvpe-f32.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"fixture").unwrap();
        assert_eq!(
            StorePaths::default()
                .with_ggml_models_root(&root)
                .ggml_model_path("rmvpe")
                .as_deref(),
            Some(model.as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn leap_instrumental_uses_its_semantic_f32_filename() {
        let root = std::env::temp_dir().join(format!(
            "uta-leap-instrumental-path-test-{}",
            std::process::id()
        ));
        let model = root.join("bs_roformer_leap_xe90_instrumental/bs_leap_xe_inst-F32.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"fixture").unwrap();
        assert_eq!(
            StorePaths::default()
                .with_ggml_models_root(&root)
                .ggml_model_path("bs_roformer_leap_xe90_instrumental")
                .as_deref(),
            Some(model.as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn firered_legacy_layout_requires_model_and_both_named_sidecars() {
        let root = std::env::temp_dir().join(format!(
            "uta-firered-artifact-set-test-{}",
            std::process::id()
        ));
        let directory = root.join("firered_asr2_aed");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("firered-f32.gguf"), b"model").unwrap();
        let paths = StorePaths::default().with_ggml_models_root(&root);
        assert!(paths.ggml_model_path("firered_asr2_aed").is_none());
        std::fs::write(directory.join("cmvn.ark"), b"cmvn").unwrap();
        assert!(paths.ggml_model_path("firered_asr2_aed").is_none());
        std::fs::write(directory.join("dict.txt"), b"tokens").unwrap();
        assert_eq!(
            paths.ggml_model_path("firered_asr2_aed").as_deref(),
            Some(directory.join("firered-f32.gguf").as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_runtime_override_requires_an_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "uta-runtime-executable-test-{}",
            std::process::id()
        ));
        std::fs::write(&path, b"worker").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let paths = StorePaths::default().with_runtime_override("worker", &path);
        assert!(paths.runtime_executable("worker").is_none());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            paths.runtime_executable("worker").as_deref(),
            Some(path.as_path())
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn persisted_tool_selection_wins_over_automatic_fallback() {
        let root = std::env::temp_dir().join(format!(
            "uta-runtime-tool-fallback-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let extension = if cfg!(windows) { ".exe" } else { "" };
        let selected = root.join(format!("selected-adapter{extension}"));
        let discovered = root.join(format!("discovered-adapter{extension}"));
        std::fs::write(&selected, b"adapter").unwrap();
        std::fs::write(&discovered, b"adapter").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&selected, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::set_permissions(&discovered, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        let configured_paths = StorePaths::new(&root);
        crate::external_tool::configure_tool_path(
            &configured_paths,
            "fusion_agent_adapter",
            &selected,
        )
        .unwrap();
        let paths =
            StorePaths::new(&root).with_tool_fallback("fusion_agent_adapter", discovered.clone());
        assert_eq!(
            paths.tool_candidate_path("fusion_agent_adapter").as_deref(),
            Some(selected.as_path())
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_runtime_override_requires_an_exe_extension() {
        let root = std::env::temp_dir().join(format!(
            "uta-runtime-executable-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let no_extension = root.join("worker");
        let executable = root.join("worker.EXE");
        std::fs::write(&no_extension, b"worker").unwrap();
        std::fs::write(&executable, b"worker").unwrap();
        let paths = StorePaths::default()
            .with_runtime_override("bad", &no_extension)
            .with_runtime_override("good", &executable);
        assert!(paths.runtime_executable("bad").is_none());
        assert_eq!(
            paths.runtime_executable("good").as_deref(),
            Some(executable.as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
