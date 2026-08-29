//! Throwaway internal harness for item-6 final acceptance: builds a real,
//! fully-compiled `AnalyzeRequestV1` (including the Processing Studio
//! workflow extension) via app-core's actual production compile path,
//! without needing a populated library database. Test-only; not part of the
//! shipped crate surface. Safe to delete after acceptance testing.
#![cfg(test)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::analysis_engine_adapter::{
    AnalysisRequestIntent, ResolvedAnalysisSource, StudioLyricsContext, compile_analyze_request_v1,
};
use crate::analysis_experience::{
    AnalysisExperienceSettings, AnalysisOutputSelection, resolve_analysis_experience,
};
use crate::backend_cli::AudioRoleWireV1;
use crate::workflow::{
    WorkflowNodeId, compile_workflow, default_workflow, set_workflow_parameter,
    workflow_execution_extension,
};

fn sha256_hex(path: &std::path::Path) -> String {
    use std::io::Read;
    let mut file = std::fs::File::open(path).expect("open audio file");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).expect("read audio file");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing env var {name}"))
}

#[test]
#[ignore = "manual acceptance-request generator, run explicitly"]
fn generate_acceptance_request() {
    let audio_path = PathBuf::from(env_var("ACCEPTANCE_AUDIO_PATH"))
        .canonicalize()
        .expect("canonicalize audio path");
    let request_id = env_var("ACCEPTANCE_REQUEST_ID");
    let fusion_mode = env_var("ACCEPTANCE_FUSION_MODE");
    let output_path = PathBuf::from(env_var("ACCEPTANCE_OUTPUT_PATH"));

    let sha256 = sha256_hex(&audio_path);

    let source = ResolvedAnalysisSource {
        library_file_hash: sha256.clone(),
        path: audio_path,
        sha256: sha256.clone(),
        role: AudioRoleWireV1::OriginalMix,
    };

    let intent = AnalysisRequestIntent {
        request_id,
        source,
        lyrics: StudioLyricsContext::default(),
        target_override: None,
        requested_outputs: Some(AnalysisOutputSelection::default()),
        compute_backend: None,
        model_backend_overrides: BTreeMap::new(),
        default_device_class: None,
        model_device_overrides: BTreeMap::new(),
    };

    let experience = AnalysisExperienceSettings::default();
    let effective = resolve_analysis_experience(&experience, None, None);

    let mut request = compile_analyze_request_v1(intent, &effective).expect("compile request");

    let mut definition = default_workflow(&sha256);
    set_workflow_parameter(
        &mut definition,
        &WorkflowNodeId::new("evidence_fusion"),
        "fusion_mode",
        serde_json::Value::String(fusion_mode),
    )
    .expect("set fusion_mode");
    let snapshot = compile_workflow(&definition).expect("compile workflow");
    request.extensions.insert(
        crate::workflow::WORKFLOW_EXECUTION_EXTENSION_KEY.to_string(),
        workflow_execution_extension(&snapshot).expect("serialize workflow extension"),
    );

    let json = serde_json::to_string_pretty(&request).expect("serialize request");
    std::fs::write(&output_path, json).expect("write request json");
}
