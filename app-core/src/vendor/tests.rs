fn isolated_runtime_client(label: &str) -> crate::backend_cli::RuntimeCliClient {
    crate::backend_cli::RuntimeCliClient::discover()
        .expect("uta-runtime debug CLI is required for process-contract tests")
        .with_store(std::env::temp_dir().join(format!("uta-studio-runtime-{label}-{}", std::process::id())))
}

#[test]
fn runtime_status_is_read_only_and_reports_backend_protocol_fields() {
    let client = isolated_runtime_client("status");
    let status = analysis_runtime_status_with_clients(
        true,
        Some(&client),
        std::path::PathBuf::from("missing-ffmpeg"),
    );
    assert!(status.runtime_contract_current);
    assert!(status.ffmpeg_path.is_none());
    let serialized = serde_json::to_value(&status).expect("runtime status serializes");
    assert!(serialized.get("openvinoRuntimeAvailable").is_some());
}

#[test]
fn production_model_statuses_name_the_native_families() {
    let statuses = model_install_statuses_with_client(&isolated_runtime_client("models"));
    for target in [
        ModelDownloadTarget::RoFormer,
        ModelDownloadTarget::FireRed,
        ModelDownloadTarget::QwenAsr,
        ModelDownloadTarget::QwenAlign,
        ModelDownloadTarget::Pitch,
    ] {
        assert!(statuses.iter().any(|status| status.target == target));
    }
}

#[test]
fn exact_strategy_status_crosses_the_runtime_cli_without_bundle_projection() {
    let statuses = analysis_strategy_resource_statuses_with_client(&isolated_runtime_client(
        "exact-strategies",
    ))
    .unwrap();
    assert_eq!(statuses.len(), 6);
    assert!(statuses.iter().any(|status| {
        status.strategy_id == "vocal_extraction"
            && status.model_id == "bs_roformer_leap_xe90_vocals"
            && status.capability == "audio.extract_vocals"
    }));
    assert!(statuses.iter().any(|status| {
        status.strategy_id == "instrumental_extraction"
            && status.model_id == "bs_polarformer_public_instrumental"
            && status.capability == "audio.extract_instrumental"
    }));
    assert!(statuses.iter().any(|status| {
        status.strategy_id == "japanese_note_boundaries"
            && status.model_id == "jbm555_cectc_80"
            && status.capability == "notes.jbm555"
    }));
}

#[test]
fn exact_strategy_status_ignores_unrelated_roformer_bundle_members() {
    fn details(
        model_id: &str,
        capability: &str,
        usable: bool,
    ) -> crate::backend_cli::RuntimeResourceDetailsWireV1 {
        serde_json::from_value(serde_json::json!({
            "resource": format!("model:{model_id}"),
            "metadata": {
                "display_name": model_id,
                "purpose": capability,
                "capabilities": [capability],
                "dependencies": [],
                "backends": [],
                "license": null,
                "estimated_download_bytes": null,
                "estimated_installed_bytes": null,
                "recipe_digest": null,
                "runtime_recipe_digest": null
            },
            "status": {
                "resource": format!("model:{model_id}"),
                "install_state": if usable { "installed" } else { "corrupt" },
                "origin": "managed",
                "integrity_verified": usable,
                "runnable": usable,
                "validation_state": "production_pinned",
                "dependencies_ready": usable,
                "executable_ready": usable,
                "usable": usable,
                "reasons": if usable { serde_json::json!([]) } else { serde_json::json!(["corrupt"]) },
                "selected_backend": if usable { serde_json::json!("open_vino") } else { serde_json::Value::Null },
                "runtime_resource": null,
                "generation": null
            }
        }))
        .unwrap()
    }

    let mut returned = vec![
        details(
            "bs_roformer_leap_xe90_vocals",
            "audio.extract_vocals",
            true,
        ),
        details(
            "bs_polarformer_public_instrumental",
            "audio.extract_instrumental",
            false,
        ),
        details(
            "melband_roformer_harmony",
            "audio.lead_isolate",
            false,
        ),
        details("rmvpe", "pitch.track", false),
        details("game", "notes.game", false),
        details("jbm555_cectc_80", "notes.jbm555", false),
    ];
    returned.push(details(
        "melband_roformer_denoise_aufr33",
        "audio.denoise",
        false,
    ));

    let statuses = status::strategy_resource_statuses_from_details(&returned);
    let vocal = statuses
        .iter()
        .find(|status| status.strategy_id == "vocal_extraction")
        .unwrap();
    let instrumental = statuses
        .iter()
        .find(|status| status.strategy_id == "instrumental_extraction")
        .unwrap();
    assert!(vocal.available);
    assert!(!instrumental.available);
    assert_eq!(vocal.model_id, "bs_roformer_leap_xe90_vocals");
    assert_eq!(vocal.capability, "audio.extract_vocals");
}
