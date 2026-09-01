//! Runtime Manager-backed audio model lifecycle presentation.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::audio_model::{
    AUDIO_CATALOG_SCHEMA_VERSION, AUDIO_CATALOG_VERSION, AudioModelCatalogSummary, AudioModelStatus,
};
use crate::backend_cli::{
    InstallStateWireV1, NativeBackendWireV1, RuntimeCliClient, RuntimePolicyWireV1,
    RuntimeResourceDetailsWireV1, RuntimeResourceRefWireV1, ValidationStateWireV1,
};

fn studio_audio_operation(model_id: &str) -> Option<&'static str> {
    match model_id {
        "bs_roformer_leap_xe90_vocals" => Some("separate_vocals"),
        "bs_polarformer_public_instrumental" => Some("separate_instrumental"),
        "jbm555_cectc_80" => Some("transcribe_singing_notes"),
        "melband_roformer_inst_v2" => Some("separate_instrumental"),
        "melband_roformer_harmony" => Some("separate_harmony"),
        "melband_roformer_denoise_aufr33" => Some("denoise"),
        "melband_roformer_dereverb_anvuew" => Some("dereverb"),
        _ => None,
    }
}

const AUDIO_MODEL_CATALOG_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct AudioModelCatalogCache {
    value: Option<AudioModelCatalogSummary>,
    refreshed_at: Option<Instant>,
}

static AUDIO_MODEL_CATALOG_CACHE: OnceLock<Mutex<AudioModelCatalogCache>> = OnceLock::new();

fn audio_model_catalog_cache() -> &'static Mutex<AudioModelCatalogCache> {
    AUDIO_MODEL_CATALOG_CACHE.get_or_init(|| Mutex::new(AudioModelCatalogCache::default()))
}

pub(crate) fn invalidate_audio_model_catalog_cache() {
    if let Ok(mut cache) = audio_model_catalog_cache().lock() {
        cache.value = None;
        cache.refreshed_at = None;
    }
}

pub fn list_audio_models() -> Result<AudioModelCatalogSummary, String> {
    if let Ok(cache) = audio_model_catalog_cache().lock()
        && cache
            .refreshed_at
            .is_some_and(|refreshed| refreshed.elapsed() < AUDIO_MODEL_CATALOG_CACHE_TTL)
        && let Some(value) = cache.value.as_ref()
    {
        return Ok(value.clone());
    }

    let value = list_audio_models_with_client(&audio_runtime_client()?)?;
    if let Ok(mut cache) = audio_model_catalog_cache().lock() {
        cache.value = Some(value.clone());
        cache.refreshed_at = Some(Instant::now());
    }
    Ok(value)
}

fn list_audio_models_with_client(
    client: &RuntimeCliClient,
) -> Result<AudioModelCatalogSummary, String> {
    let models = client
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|status| {
            let model_id = status.resource.0.strip_prefix("model:")?.to_string();
            let operation = studio_audio_operation(&model_id)?;
            Some((status.resource, model_id, operation))
        })
        .map(|(resource, model_id, operation)| {
            let details = client.show(&resource).map_err(|error| error.to_string())?;
            audio_model_status_from_details(details, &model_id, operation)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AudioModelCatalogSummary {
        schema_version: AUDIO_CATALOG_SCHEMA_VERSION,
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
        models,
    })
}

pub fn get_audio_model_status(model_id: &str) -> Result<AudioModelStatus, String> {
    get_audio_model_status_with_client(&audio_runtime_client()?, model_id)
}

fn get_audio_model_status_with_client(
    client: &RuntimeCliClient,
    model_id: &str,
) -> Result<AudioModelStatus, String> {
    let operation = studio_audio_operation(model_id)
        .ok_or_else(|| format!("unknown audio model id: {model_id}"))?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    let details = client.show(&resource).map_err(|error| error.to_string())?;
    audio_model_status_from_details(details, model_id, operation)
}

fn audio_runtime_client() -> Result<RuntimeCliClient, String> {
    RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())
}

fn audio_model_status_from_details(
    details: RuntimeResourceDetailsWireV1,
    model_id: &str,
    operation: &str,
) -> Result<AudioModelStatus, String> {
    Ok(AudioModelStatus {
        model_id: model_id.to_string(),
        display_name: details.metadata.display_name,
        purpose: details.metadata.purpose,
        architecture: match model_id {
            "bs_polarformer_public_instrumental" => "polarformer",
            "jbm555_cectc_80" => "jbm555_cectc",
            _ => "roformer",
        }
        .to_string(),
        operation: operation.to_string(),
        runner: match model_id {
            "bs_roformer_leap_xe90_vocals" | "bs_polarformer_public_instrumental" => "native_ggml",
            "jbm555_cectc_80" => "native_openvino",
            _ => "native_roformer",
        }
        .to_string(),
        supported_backends: details
            .metadata
            .backends
            .iter()
            .filter(|backend| backend.validation != ValidationStateWireV1::Unsupported)
            .map(|backend| native_backend_label(backend.backend).to_string())
            .collect(),
        license: details
            .metadata
            .license
            .map(|license| crate::audio_model::AudioModelLicense {
                status: license.status,
                source_attribution: license.source_attribution,
                source_page: license.source_page,
            })
            .unwrap_or_else(|| crate::audio_model::AudioModelLicense {
                status: "review_required".to_string(),
                source_attribution: "Pinned model/runtime catalog".to_string(),
                source_page: None,
            }),
        estimated_bytes: details.metadata.estimated_installed_bytes,
        state: install_state_label(details.status.install_state).to_string(),
        catalog_version: AUDIO_CATALOG_VERSION.to_string(),
    })
}

fn native_backend_label(backend: NativeBackendWireV1) -> &'static str {
    match backend {
        NativeBackendWireV1::OpenVino => "openvino",
        NativeBackendWireV1::Vulkan => "vulkan",
        NativeBackendWireV1::NativeDsp => "native_dsp",
        NativeBackendWireV1::CpuReference => "cpu_reference",
    }
}

fn install_state_label(state: InstallStateWireV1) -> &'static str {
    match state {
        InstallStateWireV1::Absent => "missing",
        InstallStateWireV1::Installed | InstallStateWireV1::Legacy => "installed",
        InstallStateWireV1::Incomplete => "incomplete",
        InstallStateWireV1::Corrupt => "integrity_failed",
    }
}

pub fn install_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let client = audio_runtime_client()?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    client
        .install(&[resource])
        .map_err(|error| error.to_string())?;
    let status = get_audio_model_status_with_client(&client, model_id)?;
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

pub fn reinstall_audio_model(model_id: &str) -> Result<AudioModelStatus, String> {
    let client = audio_runtime_client()?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    client
        .reinstall(&[resource])
        .map_err(|error| error.to_string())?;
    let status = get_audio_model_status_with_client(&client, model_id)?;
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

pub fn remove_audio_model(model_id: &str) -> Result<(), String> {
    let client = audio_runtime_client()?;
    let resource = RuntimeResourceRefWireV1::model(model_id)?;
    client.show(&resource).map_err(|error| error.to_string())?;
    let result = client
        .remove(std::slice::from_ref(&resource))
        .map_err(|error| error.to_string())?;
    if result.changed.is_empty() {
        let status = client
            .status(&[resource])
            .map_err(|error| error.to_string())?;
        if status
            .first()
            .is_some_and(|status| status.install_state == InstallStateWireV1::Legacy)
        {
            return Err("legacy model data is not manager-owned; import or adopt it explicitly before removal".to_string());
        }
    }
    crate::invalidate_analysis_runtime_status_cache();
    Ok(())
}
