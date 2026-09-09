use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const REQUIRED_LIBRARY_GROUPS: [&[&str]; 4] = [
    &["lib/libggml.so.0"],
    &["lib/libggml-base.so.0"],
    &["lib/libggml-cpu.so", "lib/libggml-cpu.so.0"],
    &["lib/libggml-vulkan.so", "lib/libggml-vulkan.so.0"],
];
pub const RMVPE_SOURCE_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";
pub const RMVPE_GGUF_SHA256: &str =
    "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2";

struct ModelIdentity {
    id: &'static str,
    size: u64,
}

const MODEL_IDENTITIES: [ModelIdentity; 16] = [
    ModelIdentity {
        id: "bs_roformer_leap_xe90_vocals",
        size: 267_433_600,
    },
    ModelIdentity {
        id: "bs_roformer_leap_xe90_instrumental",
        size: 267_433_600,
    },
    ModelIdentity {
        id: "melband_roformer_denoise_aufr33",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "melband_roformer_dereverb_anvuew",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "melband_roformer_harmony",
        size: 457_008_736,
    },
    ModelIdentity {
        id: "bs_polarformer_public_instrumental",
        size: 204_237_408,
    },
    ModelIdentity {
        id: "rmvpe",
        size: 361_625_344,
    },
    ModelIdentity {
        id: "fcpe",
        size: 43_309_760,
    },
    ModelIdentity {
        id: "basic_pitch",
        size: 144_512,
    },
    ModelIdentity {
        id: "game_1_0_3_small",
        size: 50_734_944,
    },
    ModelIdentity {
        id: "game_1_0_3_medium",
        size: 199_584_064,
    },
    ModelIdentity {
        id: "game_1_0_3_large",
        size: 396_034_784,
    },
    ModelIdentity {
        id: "jbm555_cectc_80",
        size: 3_981_024,
    },
    ModelIdentity {
        id: "qwen3_forced_aligner_0_6b",
        size: 1_842_216_416,
    },
    ModelIdentity {
        id: "qwen3_asr_1_7b",
        size: 4_083_087_904,
    },
    ModelIdentity {
        id: "firered_asr2_aed",
        size: 4_686_918_112,
    },
];

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    libraries: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct ValidatedRuntime {
    pub library_dir: PathBuf,
    /// Content identity retained as output provenance. It is not used as an
    /// acceptance gate; runtime acceptance uses stable library roles, safe
    /// paths, required symbols, and regular-file containment.
    pub manifest_content_digest: String,
}

fn safe_relative(value: &str) -> Result<&Path, String> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("GGML runtime manifest contains an unsafe path".to_string());
    }
    Ok(path)
}

fn runtime_roots() -> Result<Vec<PathBuf>, String> {
    if let Some(configured) = std::env::var_os("UTA_STUDIO_GGML_RUNTIME_DIR") {
        return Ok(vec![PathBuf::from(configured)]);
    }
    let parent = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local/share/uta-studio/runtime"))
        .ok_or_else(|| "GGML runtime location is unavailable".to_string())?;
    let preferred = parent.join("ggml-vulkan");
    let mut discovered = std::fs::read_dir(&parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path != &preferred
                && path.join("runtime-manifest.json").is_file()
                && path.join("lib").is_dir()
        })
        .collect::<Vec<_>>();
    discovered.sort();
    let mut roots = vec![preferred];
    roots.extend(discovered);
    Ok(roots)
}

pub fn validate_runtime(model_id: &str) -> Result<ValidatedRuntime, String> {
    if !matches!(model_id, "stars" | "rosvot")
        && !MODEL_IDENTITIES
            .iter()
            .any(|identity| identity.id == model_id)
    {
        return Err(format!("model {model_id} has no Rust GGML executor"));
    }
    validate_available_runtime()
}

/// Validates the packaged runtime itself, without asking which model will be
/// run on it. Device enumeration needs the libraries and nothing else.
pub fn validate_runtime_libraries() -> Result<ValidatedRuntime, String> {
    validate_available_runtime()
}

fn validate_available_runtime() -> Result<ValidatedRuntime, String> {
    let mut failures = Vec::new();
    for root in runtime_roots()? {
        match validate_runtime_libraries_at(&root) {
            Ok(runtime) => return Ok(runtime),
            Err(error) => failures.push(format!("{}: {error}", root.display())),
        }
    }
    Err(format!(
        "GGML shared-library runtime is unavailable: {}",
        failures.join("; ")
    ))
}

#[cfg(test)]
fn validate_runtime_at(model_id: &str, root: &Path) -> Result<ValidatedRuntime, String> {
    if !matches!(model_id, "stars" | "rosvot")
        && !MODEL_IDENTITIES
            .iter()
            .any(|identity| identity.id == model_id)
    {
        return Err(format!("model {model_id} has no Rust GGML executor"));
    }
    validate_runtime_libraries_at(root)
}

fn validate_runtime_libraries_at(root: &Path) -> Result<ValidatedRuntime, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("GGML runtime directory is unavailable: {error}"))?;
    if !canonical_root.is_dir() {
        return Err("GGML runtime location is not a directory".to_string());
    }
    let bytes = std::fs::read(canonical_root.join("runtime-manifest.json"))
        .map_err(|error| format!("GGML runtime manifest is unavailable: {error}"))?;
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("GGML runtime manifest is invalid: {error}"))?;
    let declared = manifest
        .libraries
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if declared.len() != REQUIRED_LIBRARY_GROUPS.len()
        || REQUIRED_LIBRARY_GROUPS
            .iter()
            .any(|alternatives| !alternatives.iter().any(|name| declared.contains(name)))
    {
        return Err(
            "GGML runtime manifest does not declare the required library roles".to_string(),
        );
    }

    let library_dir = canonical_root.join("lib");
    if !library_dir.is_dir() {
        return Err("GGML shared-library directory is unavailable".to_string());
    }
    for entry in std::fs::read_dir(&library_dir)
        .map_err(|error| format!("GGML runtime library directory is unreadable: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("GGML runtime library entry is unreadable: {error}"))?;
        let relative = format!("lib/{}", entry.file_name().to_string_lossy());
        if !declared.contains(relative.as_str()) {
            return Err(format!(
                "GGML runtime contains an undeclared library: {relative}"
            ));
        }
    }
    for relative in &declared {
        let configured = canonical_root.join(safe_relative(relative)?);
        let canonical = configured
            .canonicalize()
            .map_err(|error| format!("GGML runtime library is unavailable: {relative}: {error}"))?;
        if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
            return Err(format!(
                "GGML runtime library escapes its package or is not a file: {relative}"
            ));
        }
    }
    Ok(ValidatedRuntime {
        library_dir,
        manifest_content_digest: format!("{:x}", Sha256::digest(&bytes)),
    })
}

pub fn validate_model(
    model_id: &str,
    configured: &Path,
    config: &Value,
) -> Result<PathBuf, String> {
    if matches!(model_id, "stars" | "rosvot") {
        let path = configured_model_path(model_id, configured, None);
        return path.is_file().then_some(path).ok_or_else(|| {
            format!(
                "{} GGUF model is unavailable",
                model_id.to_ascii_uppercase()
            )
        });
    }
    let game_variant = game_variant(model_id);
    if game_variant.is_some() {
        validate_requested_game_variant(model_id, config)?;
    }
    let expected_size = MODEL_IDENTITIES
        .iter()
        .find(|identity| identity.id == model_id)
        .map(|identity| identity.size)
        .ok_or_else(|| format!("model {model_id} has no GGML executor"))?;
    let path = configured_model_path(model_id, configured, game_variant);
    let metadata = path
        .metadata()
        .map_err(|error| format!("GGUF model is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(format!("GGUF model size mismatch for {model_id}"));
    }
    Ok(path)
}

pub fn game_variant(model_id: &str) -> Option<&'static str> {
    match model_id {
        "game_1_0_3_small" => Some("small"),
        "game_1_0_3_medium" => Some("medium"),
        "game_1_0_3_large" => Some("large"),
        _ => None,
    }
}

fn validate_requested_game_variant(model_id: &str, config: &Value) -> Result<(), String> {
    let expected =
        game_variant(model_id).ok_or_else(|| "GAME resource id is invalid".to_string())?;
    let Some(requested) = config.get("variant").and_then(Value::as_str) else {
        return Ok(());
    };
    let requested = match requested {
        "small" | "GAME-1.0.3-small-onnx" => "small",
        "medium" | "GAME-1.0.3-medium-onnx" => "medium",
        "large" | "GAME-1.0.3-large-onnx" => "large",
        _ => return Err("GAME variant must be small, medium, or large".to_string()),
    };
    if requested != expected {
        return Err(format!(
            "GAME request variant {requested} disagrees with immutable resource {model_id}"
        ));
    }
    Ok(())
}

fn configured_model_path(model_id: &str, configured: &Path, game_variant: Option<&str>) -> PathBuf {
    if configured.is_file() {
        configured.to_path_buf()
    } else if model_id == "bs_roformer_leap_xe90_vocals" {
        configured.join("bs_leap_xe_voc-F32.gguf")
    } else if model_id == "bs_roformer_leap_xe90_instrumental" {
        configured.join("bs_leap_xe_inst-F32.gguf")
    } else if model_id == "rmvpe" {
        configured.join("rmvpe-f32.gguf")
    } else if model_id == "fcpe" {
        configured.join("fcpe-f32.gguf")
    } else if model_id == "basic_pitch" {
        configured.join("basic-pitch-f32.gguf")
    } else if game_variant.is_some() {
        configured.join(format!(
            "game-{}-f32.gguf",
            game_variant.expect("GAME variant was matched")
        ))
    } else if model_id == "jbm555_cectc_80" {
        configured.join("jbm555-cectc80-f32.gguf")
    } else if model_id == "stars" {
        configured.join("stars-f32.gguf")
    } else if model_id == "rosvot" {
        configured.join("rosvot-f32.gguf")
    } else if model_id == "qwen3_forced_aligner_0_6b" {
        configured.join("Qwen3-ForcedAligner-0.6B-F16.gguf")
    } else if model_id == "qwen3_asr_1_7b" {
        configured.join("Qwen3-ASR-1.7B-F16.gguf")
    } else if model_id == "firered_asr2_aed" {
        configured.join("firered-f32.gguf")
    } else {
        configured.join("model-fp16.gguf")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-ggml-runtime-validation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("lib")).unwrap();
        for alternatives in REQUIRED_LIBRARY_GROUPS {
            std::fs::write(root.join(alternatives[0]), b"fixture").unwrap();
        }
        std::fs::write(
            root.join("runtime-manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "libraries": {
                    "lib/libggml.so.0": "provenance-only",
                    "lib/libggml-base.so.0": "provenance-only",
                    "lib/libggml-cpu.so": "provenance-only",
                    "lib/libggml-vulkan.so": "provenance-only"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn every_supported_model_has_a_full_identity() {
        assert_eq!(MODEL_IDENTITIES.len(), 16);
        for identity in MODEL_IDENTITIES {
            assert!(!identity.id.is_empty());
            match identity.id {
                "basic_pitch" => assert_eq!(identity.size, 144_512),
                "jbm555_cectc_80" => assert_eq!(identity.size, 3_981_024),
                _ => assert!(identity.size > 40_000_000),
            }
        }
    }

    #[test]
    fn runtime_requires_exact_declared_contained_libraries() {
        let root = runtime_fixture();
        let runtime = validate_runtime_at("rmvpe", &root).unwrap();
        assert_eq!(
            runtime.library_dir,
            root.canonicalize().unwrap().join("lib")
        );

        std::fs::write(root.join("lib/undeclared.so"), b"fixture").unwrap();
        assert!(
            validate_runtime_at("rmvpe", &root)
                .unwrap_err()
                .contains("undeclared library")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_rejects_extra_manifest_library_entries() {
        let root = runtime_fixture();
        let manifest_path = root.join("runtime-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["libraries"]["lib/extra.so"] = serde_json::json!("provenance-only");
        std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        assert!(
            validate_runtime_at("rmvpe", &root)
                .unwrap_err()
                .contains("required library roles")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn runtime_rejects_library_symlinks_that_escape_the_package() {
        use std::os::unix::fs::symlink;

        let root = runtime_fixture();
        let outside = root.with_extension("outside.so");
        std::fs::write(&outside, b"fixture").unwrap();
        let library = root.join("lib/libggml-vulkan.so");
        std::fs::remove_file(&library).unwrap();
        symlink(&outside, &library).unwrap();
        assert!(
            validate_runtime_at("rmvpe", &root)
                .unwrap_err()
                .contains("escapes its package")
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn public_f32_models_keep_their_distinct_filenames() {
        assert_eq!(
            configured_model_path(
                "bs_roformer_leap_xe90_vocals",
                Path::new("managed-generation"),
                None,
            ),
            Path::new("managed-generation/bs_leap_xe_voc-F32.gguf")
        );
        assert_eq!(
            configured_model_path(
                "bs_roformer_leap_xe90_instrumental",
                Path::new("managed-generation"),
                None,
            ),
            Path::new("managed-generation/bs_leap_xe_inst-F32.gguf")
        );
        assert_eq!(
            configured_model_path("rmvpe", Path::new("managed-generation"), None),
            Path::new("managed-generation/rmvpe-f32.gguf")
        );
        assert_eq!(
            configured_model_path("fcpe", Path::new("managed-generation"), None),
            Path::new("managed-generation/fcpe-f32.gguf")
        );
        assert_eq!(
            configured_model_path("basic_pitch", Path::new("managed-generation"), None),
            Path::new("managed-generation/basic-pitch-f32.gguf")
        );
    }

    #[test]
    fn game_variants_keep_distinct_immutable_resources() {
        for (model_id, variant) in [
            ("game_1_0_3_small", "small"),
            ("game_1_0_3_medium", "medium"),
            ("game_1_0_3_large", "large"),
        ] {
            assert_eq!(game_variant(model_id), Some(variant));
            assert_eq!(
                configured_model_path(model_id, Path::new("managed-generation"), Some(variant)),
                Path::new("managed-generation").join(format!("game-{variant}-f32.gguf"))
            );
            validate_requested_game_variant(model_id, &serde_json::json!({ "variant": variant }))
                .unwrap();
        }
        assert!(
            validate_requested_game_variant(
                "game_1_0_3_small",
                &serde_json::json!({ "variant": "large" }),
            )
            .is_err()
        );
    }
}
