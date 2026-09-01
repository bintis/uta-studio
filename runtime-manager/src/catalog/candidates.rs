use super::*;

pub(super) const LEAP_XE90_VOCALS_ID: &str = "bs_roformer_leap_xe90_vocals";
pub(super) const PUBLIC_POLARFORMER_INSTRUMENTAL_ID: &str = "bs_polarformer_public_instrumental";
pub(super) const JBM555_CECTC_80_ID: &str = "jbm555_cectc_80";

/// Task 23 models are normal executable catalog resources. Provenance is
/// descriptive; acquisition and execution do not depend on certificate,
/// license-acceptance, or artifact-hash gates.
pub(super) fn task_23_models() -> RuntimeManagerResult<Vec<ModelCatalogEntry>> {
    use NativeBackend::{CpuReference, OpenVino, Vulkan};
    use ValidationState::{Experimental, ProductionPinned};

    let openvino_routes = || {
        vec![
            BackendCapability {
                backend: OpenVino,
                validation: ProductionPinned,
                evidence_id: Some("task23-native-openvino-execution".to_string()),
            },
            BackendCapability {
                backend: CpuReference,
                validation: Experimental,
                evidence_id: Some("task23-openvino-cpu-diagnostic".to_string()),
            },
        ]
    };

    Ok(vec![
        ModelCatalogEntry {
            id: ModelId::new(LEAP_XE90_VOCALS_ID)?,
            display_name: "BS-RoFormer Leap XE90 Vocals".to_string(),
            purpose: "44.1 kHz stereo GuideVocals extraction".to_string(),
            capabilities: vec!["audio.extract_vocals".to_string()],
            source: SourceIdentity {
                repository: Some("scragnog/HOT-Step-CPP-SuperSep".to_string()),
                revision: Some("440487b8300dcd61453cc52ec244a38150b03456".to_string()),
                filename: Some("bs_leap_xe_voc-F32.gguf".to_string()),
                sha256: None,
                source_format: Some("gguf-f32".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://huggingface.co/pcunwa/BS-Roformer-Leap".to_string(),
                    revision: Some("4e47d6662ae82eaa8b4ac4329fe66099a843b48e".to_string()),
                    license_id: "source-attribution".to_string(),
                }),
                artifacts: Vec::new(),
                converted_artifact: None,
            },
            license: LicenseInfo {
                status: "informational".to_string(),
                source_attribution: "pcunwa BS-RoFormer Leap model; public GGUF conversion by scragnog".to_string(),
                source_page: Some(
                    "https://huggingface.co/scragnog/HOT-Step-CPP-SuperSep/blob/440487b8300dcd61453cc52ec244a38150b03456/bs_leap_xe_voc-F32.gguf".to_string(),
                ),
            },
            acquisition: vec![acquisition(
                AcquisitionMethod::ManagedDownload,
                "download the public native F32 GGUF model",
            )],
            dependencies: vec![ResourceRef::runtime("ggml_vulkan_v1")?],
            backends: vec![BackendCapability {
                backend: Vulkan,
                validation: ProductionPinned,
                evidence_id: Some("task23-leap-native-gguf".to_string()),
            }],
            pinned_backend: Some(Vulkan),
            estimated_download_bytes: Some(267_433_600),
            estimated_installed_bytes: Some(267_433_600),
            recipe_digest: catalog_recipe_digest(LEAP_XE90_VOCALS_ID),
            runtime_recipe_digest: Some(
                "4c2784c0e58358f852ed9ee95cd7a5b99e4e6c226f72a4790e7beeb42f7d631a"
                    .to_string(),
            ),
        },
        ModelCatalogEntry {
            id: ModelId::new(PUBLIC_POLARFORMER_INSTRUMENTAL_ID)?,
            display_name: "BS-PolarFormer Public Instrumental".to_string(),
            purpose: "44.1 kHz stereo Instrumental extraction".to_string(),
            // The checkpoint's single trained stem is vocals
            // (config.yaml's `training.target_instrument: vocals`);
            // "instrumental" is a derived mix-minus-vocals residual. Both
            // are real, selectable product roles for this one model.
            capabilities: vec![
                "audio.extract_instrumental".to_string(),
                "audio.extract_vocals".to_string(),
            ],
            source: SourceIdentity {
                repository: Some("bgkb/bs_polarformer".to_string()),
                revision: Some("9158719ee2173edd480a735764627526506fe4af".to_string()),
                filename: Some("bs_polarformer_fp16.onnx".to_string()),
                sha256: None,
                source_format: Some("onnx-fp16-weights-fp32-compute".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://huggingface.co/bgkb/bs_polarformer".to_string(),
                    revision: Some("9158719ee2173edd480a735764627526506fe4af".to_string()),
                    license_id: "MIT".to_string(),
                }),
                artifacts: Vec::new(),
                converted_artifact: None,
            },
            license: LicenseInfo {
                status: "mit".to_string(),
                source_attribution: "bgkb public BS-PolarFormer ONNX model".to_string(),
                source_page: Some("https://huggingface.co/bgkb/bs_polarformer".to_string()),
            },
            acquisition: vec![
                acquisition(
                    AcquisitionMethod::ManagedDownload,
                    "download the public FP16-weight ONNX model",
                ),
                acquisition(
                    AcquisitionMethod::LocalImport,
                    "explicit import of the locally-converted PoPE GGUF (no upstream GGUF exists for this checkpoint)",
                ),
            ],
            dependencies: vec![
                ResourceRef::runtime("openvino_2026_3")?,
                ResourceRef::runtime("ggml_vulkan_v1")?,
            ],
            backends: {
                let mut routes = openvino_routes();
                routes.push(BackendCapability {
                    backend: Vulkan,
                    validation: ProductionPinned,
                    // The PoPE (not RoPE) positional embedding required a
                    // dedicated GGML graph, not a relabel of the existing
                    // bs_roformer graph. Its output matches the reference
                    // bs_polarformer FP32 ONNX Runtime evidence closely
                    // (correlation 0.9999996, max abs diff 3.8e-5 on a
                    // real mask tensor). It has since been run on real
                    // Vulkan hardware under this repository's explicit
                    // authorization policy -- a bounded smoke test and a
                    // full-song A/B against melband_roformer_inst_v2 --
                    // with no crashes, and the repository owner confirmed
                    // by ear that the output quality is good. Promoted to
                    // ProductionPinned and made the default route on that
                    // basis.
                    evidence_id: Some("task23-polarformer-ggml-vulkan-fullsong-ab-2026-09-01".to_string()),
                });
                routes
            },
            pinned_backend: Some(Vulkan),
            estimated_download_bytes: Some(108_325_429),
            estimated_installed_bytes: Some(108_325_429),
            recipe_digest: catalog_recipe_digest(PUBLIC_POLARFORMER_INSTRUMENTAL_ID),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        },
        ModelCatalogEntry {
            id: ModelId::new(JBM555_CECTC_80_ID)?,
            display_name: "JBM555 CE-CTC 80 Japanese Note Expert".to_string(),
            purpose: "Japanese singing onset, offset, octave, and pitch-class evidence".to_string(),
            capabilities: vec!["notes.jbm555".to_string()],
            source: SourceIdentity {
                repository: Some("https://github.com/york135/CECTC_baseline_APSIPA25".to_string()),
                revision: Some("d1352eda1ea69d94cf7b1b06bf0b003d874b389a".to_string()),
                filename: Some("jbm555-cectc80.onnx".to_string()),
                sha256: None,
                source_format: Some("onnx".to_string()),
                algorithm: Some(AlgorithmIdentity {
                    repository: "https://github.com/york135/CECTC_baseline_APSIPA25".to_string(),
                    revision: Some("d1352eda1ea69d94cf7b1b06bf0b003d874b389a".to_string()),
                    license_id: "source-attribution".to_string(),
                }),
                artifacts: Vec::new(),
                converted_artifact: None,
            },
            license: LicenseInfo {
                status: "informational".to_string(),
                source_attribution: "york135 CE+CTC baseline trained for the JBM555 dataset".to_string(),
                source_page: Some(
                    "https://github.com/york135/CECTC_baseline_APSIPA25".to_string(),
                ),
            },
            acquisition: vec![acquisition(
                AcquisitionMethod::LocalImport,
                "import an ONNX export of the published CE-CTC 80 checkpoint",
            )],
            dependencies: vec![ResourceRef::runtime("openvino_2026_3")?],
            backends: openvino_routes(),
            pinned_backend: Some(OpenVino),
            estimated_download_bytes: Some(3_990_463),
            estimated_installed_bytes: Some(3_990_463),
            recipe_digest: catalog_recipe_digest(JBM555_CECTC_80_ID),
            runtime_recipe_digest: Some(OPENVINO_WORKER_RECIPE_SHA256.to_string()),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_23_models_are_executable_catalog_resources() {
        let catalog = ResourceCatalog::default_catalog().unwrap();
        for id in [
            LEAP_XE90_VOCALS_ID,
            PUBLIC_POLARFORMER_INSTRUMENTAL_ID,
            JBM555_CECTC_80_ID,
        ] {
            let model = catalog.model(id).expect("Task 23 model is visible");
            assert!(!model.capabilities.is_empty());
            assert!(!model.backends.is_empty());
            assert!(!model.dependencies.is_empty());
            assert!(model.pinned_backend.is_some());
            assert!(model.acquisition.iter().all(|item| {
                matches!(
                    item.method,
                    AcquisitionMethod::ManagedDownload | AcquisitionMethod::LocalImport
                )
            }));
        }
        assert!(catalog.model("bs_roformer_vocals_ep317").is_none());
    }
}
