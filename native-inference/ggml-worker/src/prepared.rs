//! A single model's resident weights, owned by its precision-isolated worker.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uta_ggml_runtime::{DeviceDescriptor, DeviceKind, GgmlRuntime};
use uta_ggml_runtime::{
    basic_pitch::BasicPitch, fcpe::Fcpe, firered::FireRed, game::Game, jbm555::Jbm555, qwen::Qwen,
    rmvpe::Rmvpe, roformer::Roformer, rosvot::Rosvot, stars::Stars,
};

pub enum Weights {
    Roformer(Roformer),
    Rmvpe(Rmvpe),
    Fcpe(Fcpe),
    BasicPitch(BasicPitch),
    Game(Game),
    Jbm(Jbm555),
    Stars(Stars),
    Rosvot(Rosvot),
    Qwen(Qwen),
    FireRed(FireRed),
}

macro_rules! loader {
    ($name:ident, $variant:ident, $model:ty) => {
        pub fn $name(
            prepared: Option<Weights>,
            runtime: Arc<GgmlRuntime>,
            device: &DeviceDescriptor,
            path: &Path,
        ) -> Result<$model, String> {
            match prepared {
                Some(Weights::$variant(model)) => {
                    eprintln!(
                        "[super acceleration] consuming resident {} weights on {}",
                        stringify!($variant),
                        device.description
                    );
                    Ok(model)
                }
                None => <$model>::load(runtime, device, path),
                Some(_) => {
                    Err("prepared weight type disagrees with the requested model".to_string())
                }
            }
        }
    };
}
loader!(roformer, Roformer, Roformer);
loader!(rmvpe, Rmvpe, Rmvpe);
loader!(fcpe, Fcpe, Fcpe);
loader!(basic_pitch, BasicPitch, BasicPitch);
loader!(game, Game, Game);
loader!(jbm, Jbm, Jbm555);
loader!(stars, Stars, Stars);
loader!(rosvot, Rosvot, Rosvot);
loader!(qwen, Qwen, Qwen);
loader!(firered, FireRed, FireRed);

fn load(
    model_id: &str,
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    path: &Path,
) -> Result<Weights, String> {
    match model_id {
        "rmvpe" => Rmvpe::load(runtime, device, path).map(Weights::Rmvpe),
        "fcpe" => Fcpe::load(runtime, device, path).map(Weights::Fcpe),
        "basic_pitch" => BasicPitch::load(runtime, device, path).map(Weights::BasicPitch),
        "jbm555_cectc_80" => Jbm555::load(runtime, device, path).map(Weights::Jbm),
        "stars" => Stars::load(runtime, device, path).map(Weights::Stars),
        "rosvot" => Rosvot::load(runtime, device, path).map(Weights::Rosvot),
        "qwen3_forced_aligner_0_6b" | "qwen3_asr_1_7b" => {
            Qwen::load(runtime, device, path).map(Weights::Qwen)
        }
        "firered_asr2_aed" => FireRed::load(runtime, device, path).map(Weights::FireRed),
        id if id.starts_with("game_") => Game::load(runtime, device, path).map(Weights::Game),
        _ => Roformer::load(runtime, device, path).map(Weights::Roformer),
    }
}

pub struct Prepared {
    model_id: String,
    model_path: PathBuf,
    route: Value,
    pub runtime: Arc<GgmlRuntime>,
    pub device: DeviceDescriptor,
    pub weights: Option<Weights>,
    pub status: String,
    pub message: String,
    pub free_bytes: Option<u64>,
}

fn route(config: &Value) -> Value {
    serde_json::json!({"backend":config["backend"],"device_class":config["device_class"],"vulkan_device":config["vulkan_device"]})
}

pub fn available(device: &DeviceDescriptor) -> Option<u64> {
    if device.kind == DeviceKind::Cpu {
        return None;
    }
    let probe = uta_gpu_probes::probe_vulkan().ok()?;
    let matches = probe
        .devices
        .iter()
        .filter(|physical| crate::engine::same_device_name(&physical.name, &device.description))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return None;
    }
    matches[0].available_device_local_bytes()
}

fn admission(free: Option<u64>, weight_bytes: u64) -> bool {
    free.is_some_and(|free| weight_bytes.saturating_add((512 * 1024 * 1024).max(free / 4)) <= free)
}

impl Prepared {
    pub fn preload(model_id: &str, config: &Value) -> Result<Self, String> {
        let validated = crate::runtime::validate_runtime(model_id)?;
        let path =
            crate::runtime::validate_model(model_id, &crate::engine::model_path(config)?, config)?;
        let runtime = crate::engine::initialize_runtime(model_id, &validated.library_dir)?;
        let device = crate::engine::execution_device(config, &runtime)?;
        let free = available(&device);
        let bytes = path.metadata().map_err(|error| error.to_string())?.len();
        let mut result = Self {
            model_id: model_id.to_string(),
            model_path: path,
            route: route(config),
            runtime,
            device,
            weights: None,
            status: "skipped".to_string(),
            message:
                "Observed memory budget is unknown or insufficient; weights will load on demand"
                    .to_string(),
            free_bytes: free,
        };
        if admission(free, bytes) {
            // Weight load only: no model graph/inference is submitted speculatively.
            match load(
                model_id,
                Arc::clone(&result.runtime),
                &result.device,
                &result.model_path,
            ) {
                Ok(weights) => {
                    result.weights = Some(weights);
                    result.status = "loaded".to_string();
                    result.message =
                        format!("Resident weights prepared on {}", result.device.description);
                }
                Err(error) => {
                    result.message = format!("Optional weight allocation skipped: {error}");
                }
            }
        }
        eprintln!(
            "[super acceleration] preload {model_id}: {} (observed free bytes: {free:?})",
            result.message
        );
        Ok(result)
    }

    pub fn matches(&self, model_id: &str, path: &Path, config: &Value) -> bool {
        self.model_id == model_id && self.model_path == path && self.route == route(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_or_low_memory_skips_only_speculation() {
        assert!(!admission(None, 1));
        assert!(!admission(Some(100), 100));
        assert!(!admission(Some(1024 * 1024 * 1024), 900 * 1024 * 1024));
        assert!(admission(Some(4 * 1024 * 1024 * 1024), 1024 * 1024 * 1024));
    }
    #[test]
    fn prepared_route_identity_covers_explicit_physical_device_without_task_details() {
        assert_ne!(
            route(&serde_json::json!({"vulkan_device":0})),
            route(&serde_json::json!({"vulkan_device":1}))
        );
        assert_eq!(
            route(&serde_json::json!({"backend":"ggml_vulkan","semantic_output":"vocals"})),
            route(&serde_json::json!({"backend":"ggml_vulkan","semantic_output":"instrumental"}))
        );
    }
}
