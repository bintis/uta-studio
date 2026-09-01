//! Step 1 audio-chain "skip if unchanged" cache.
//!
//! Before an analysis request is compiled, decides how far into the
//! separation -> lead-isolate -> cleanup chain a prior successful,
//! still-valid artifact can be reused, and produces the fingerprints a
//! fresh run should record so a later run can match against them. See the
//! plan at the top of this feature's design: denoise and dereverb are not
//! independently cacheable in the engine's current implementation (they
//! share one combined `CleanLeadVocal` output), so they are treated as one
//! cache unit here, gated on both of their `skip_if_unchanged` boxes
//! agreeing with whichever of them the workflow actually has enabled.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analysis_artifact::{
    ArtifactRevision, ArtifactStore, compute_native_config_hash, load_active_artifact,
    record_artifact_revision, set_active_artifact_revision,
};
use crate::analysis_graph::{AnalysisNodeId, ArtifactKind};
use crate::backend_cli::AudioRoleWireV1;
use crate::workflow::{ExecutionPolicy, WorkflowDefinition, WorkflowNodeInstance};

pub const CHAIN_FINGERPRINTS_EXTENSION_KEY: &str = "uta.studio.chain_fingerprints";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChainFingerprints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumental: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChainCacheDecision {
    pub role: AudioRoleWireV1,
    pub source_path: Option<PathBuf>,
    /// Every matching Step 1 artifact is carried into the Engine request as
    /// a typed source. The deepest human-voice artifact becomes primary;
    /// earlier voice artifacts and accompaniment remain non-primary so the
    /// new result manifest can republish the complete reusable chain.
    pub cached_sources: Vec<CachedChainSource>,
    pub satisfied_capabilities: Vec<String>,
    pub fingerprints: ChainFingerprints,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedChainSource {
    pub role: AudioRoleWireV1,
    pub path: PathBuf,
    pub identity: String,
}

fn find_node<'a>(
    workflow: &'a WorkflowDefinition,
    capability: &str,
) -> Option<&'a WorkflowNodeInstance> {
    workflow
        .nodes
        .iter()
        .find(|node| node.capability_id.as_str() == capability)
}

fn enabled(node: Option<&WorkflowNodeInstance>) -> bool {
    node.is_some_and(|node| node.execution_policy != ExecutionPolicy::Disabled)
}

fn normalized_parameters(node: &WorkflowNodeInstance) -> String {
    serde_json::to_string(&(&node.model_id, &node.separation_strategy, &node.parameters))
        .unwrap_or_default()
}

/// Decides how far into the Step 1 chain a cached result can be reused for
/// `file_hash`, given the song's current workflow. Only ever returns a
/// non-`OriginalMix` role when every unit up to that point both opted in
/// (`skip_if_unchanged`) and has a still-valid cached artifact whose
/// fingerprint matches the current configuration exactly.
pub fn plan_chain_cache(file_hash: &str, workflow: &WorkflowDefinition) -> ChainCacheDecision {
    let mut decision = ChainCacheDecision::default();
    let mut chain_input_hash = file_hash.to_string();

    let Some(separation_node) = find_node(workflow, "audio.separate_vocal_bgm") else {
        return decision;
    };
    if separation_node.execution_policy == ExecutionPolicy::Disabled {
        return decision;
    }
    let separation_fingerprint = compute_native_config_hash(
        &AnalysisNodeId::new("vocal_bgm_split"),
        "audio.separate_vocal_bgm",
        &normalized_parameters(separation_node),
        &[file_hash],
        separation_node.model_id.as_deref(),
        None,
    );
    let instrumental_fingerprint = compute_native_config_hash(
        &AnalysisNodeId::new("vocal_bgm_split_instrumental"),
        "audio.separate_vocal_bgm",
        &normalized_parameters(separation_node),
        &[file_hash],
        separation_node.model_id.as_deref(),
        None,
    );
    decision.fingerprints.separation = Some(separation_fingerprint.clone());
    decision.fingerprints.instrumental = Some(instrumental_fingerprint.clone());

    if separation_node.skip_if_unchanged {
        if let Some(revision) = load_active_artifact(file_hash, ArtifactKind::VocalStem)
            && !revision.invalidated
            && revision.config_hash == separation_fingerprint
        {
            decision.role = AudioRoleWireV1::GuideVocals;
            decision.source_path = Some(revision.path.clone());
            chain_input_hash = revision.content_hash.clone();
            decision.cached_sources.push(CachedChainSource {
                role: AudioRoleWireV1::GuideVocals,
                path: revision.path,
                identity: revision.content_hash,
            });
        }
        if let Some(revision) = load_active_artifact(file_hash, ArtifactKind::InstrumentalStem)
            && !revision.invalidated
            && revision.config_hash == instrumental_fingerprint
        {
            decision.cached_sources.push(CachedChainSource {
                role: AudioRoleWireV1::Instrumental,
                path: revision.path,
                identity: revision.content_hash,
            });
            decision
                .satisfied_capabilities
                .push("audio.extract_instrumental".to_string());
        }
    }
    if decision.role == AudioRoleWireV1::OriginalMix {
        // Separation itself wasn't reusable, so nothing downstream can be
        // either: every later stage's real input depends on this one.
        return decision;
    }

    let isolate_node = find_node(workflow, "audio.lead_isolate");
    if enabled(isolate_node) {
        let isolate_node = isolate_node.expect("enabled() only true when Some");
        let isolate_fingerprint = compute_native_config_hash(
            &AnalysisNodeId::new("lead_isolate"),
            "audio.lead_isolate",
            &normalized_parameters(isolate_node),
            &[&chain_input_hash],
            isolate_node.model_id.as_deref(),
            None,
        );
        decision.fingerprints.isolate = Some(isolate_fingerprint.clone());
        if isolate_node.skip_if_unchanged
            && let Some(revision) = load_active_artifact(file_hash, ArtifactKind::AnalysisVocalStem)
            && !revision.invalidated
            && revision.config_hash == isolate_fingerprint
        {
            decision.role = AudioRoleWireV1::LeadVocal;
            decision.source_path = Some(revision.path.clone());
            chain_input_hash = revision.content_hash.clone();
            decision.cached_sources.push(CachedChainSource {
                role: AudioRoleWireV1::LeadVocal,
                path: revision.path,
                identity: revision.content_hash,
            });
        }
        if decision.role != AudioRoleWireV1::LeadVocal {
            // Isolate is enabled but not reusable this run -- it must
            // execute, so the injected source can't skip past its input.
            return decision;
        }
    }

    let denoise_node = find_node(workflow, "audio.denoise");
    let dereverb_node = find_node(workflow, "audio.dereverb");
    let denoise_enabled = enabled(denoise_node);
    let dereverb_enabled = enabled(dereverb_node);
    if !denoise_enabled && !dereverb_enabled {
        return decision;
    }
    let denoise_ready = !denoise_enabled || denoise_node.is_some_and(|node| node.skip_if_unchanged);
    let dereverb_ready =
        !dereverb_enabled || dereverb_node.is_some_and(|node| node.skip_if_unchanged);
    let cleanup_recipe = serde_json::to_string(&(
        denoise_enabled
            .then(|| denoise_node.map(normalized_parameters))
            .flatten(),
        dereverb_enabled
            .then(|| dereverb_node.map(normalized_parameters))
            .flatten(),
    ))
    .unwrap_or_default();
    let cleanup_fingerprint = compute_native_config_hash(
        &AnalysisNodeId::new("cleanup"),
        "audio.denoise+audio.dereverb",
        &cleanup_recipe,
        &[&chain_input_hash],
        None,
        None,
    );
    decision.fingerprints.cleanup = Some(cleanup_fingerprint.clone());
    if denoise_ready
        && dereverb_ready
        && let Some(revision) = load_active_artifact(file_hash, ArtifactKind::DereverbedVocalStem)
        && !revision.invalidated
        && revision.config_hash == cleanup_fingerprint
    {
        decision.role = AudioRoleWireV1::CleanLeadVocal;
        decision.source_path = Some(revision.path.clone());
        decision.cached_sources.push(CachedChainSource {
            role: AudioRoleWireV1::CleanLeadVocal,
            path: revision.path,
            identity: revision.content_hash,
        });
        if denoise_enabled {
            decision
                .satisfied_capabilities
                .push("audio.denoise".to_string());
        }
        if dereverb_enabled {
            decision
                .satisfied_capabilities
                .push("audio.dereverb".to_string());
        }
    }

    decision
}

/// Which stem roles must be requested from the engine purely so a
/// `skip_if_unchanged`-enabled node's output gets published and becomes
/// fingerprint-matchable for a *future* run -- independent of whether the
/// current run's own output selection wants that stem as a deliverable.
/// The engine only publishes a stem artifact when its role appears in
/// `requested_artifacts.stems` (see `engine.rs`'s per-role gates), so a
/// checked box with no matching request would silently never get cached.
pub fn stems_to_request_for_caching(workflow: &WorkflowDefinition) -> Vec<AudioRoleWireV1> {
    let mut roles = Vec::new();
    let separation_node = find_node(workflow, "audio.separate_vocal_bgm");
    if separation_node.is_some_and(|node| {
        node.execution_policy != ExecutionPolicy::Disabled && node.skip_if_unchanged
    }) {
        roles.push(AudioRoleWireV1::GuideVocals);
        roles.push(AudioRoleWireV1::Instrumental);
    }
    let isolate_node = find_node(workflow, "audio.lead_isolate");
    if enabled(isolate_node) && isolate_node.is_some_and(|node| node.skip_if_unchanged) {
        roles.push(AudioRoleWireV1::LeadVocal);
    }
    let denoise_node = find_node(workflow, "audio.denoise");
    let dereverb_node = find_node(workflow, "audio.dereverb");
    let denoise_wants_caching =
        enabled(denoise_node) && denoise_node.is_some_and(|node| node.skip_if_unchanged);
    let dereverb_wants_caching =
        enabled(dereverb_node) && dereverb_node.is_some_and(|node| node.skip_if_unchanged);
    if denoise_wants_caching || dereverb_wants_caching {
        roles.push(AudioRoleWireV1::CleanLeadVocal);
    }
    roles
}

/// Whether `this_node`'s own output is the *last* word in the cleanup pair
/// -- i.e. nothing routes it onward into `other_node`. Denoise and dereverb
/// can be wired in either order (the default workflow runs denoise then
/// dereverb, but a workflow can reverse that), and only the one nothing
/// else consumes produces the real, final `clean_lead_vocal` this cache
/// unit represents; the other's own output is just that stage's
/// intermediate input. `other_node: None` (the other stage doesn't exist
/// in this workflow at all) trivially makes `this_node` the last stage.
fn is_last_cleanup_stage(
    workflow: &WorkflowDefinition,
    this_node: &WorkflowNodeInstance,
    other_node: Option<&WorkflowNodeInstance>,
) -> bool {
    match other_node {
        None => true,
        Some(other) => !workflow.edges.iter().any(|edge| {
            edge.from.node == this_node.instance_id && edge.to.node == other.instance_id
        }),
    }
}

/// The active, non-invalidated revision's own content hash -- the same
/// "what actually fed the next stage" identity `plan_chain_cache` threads
/// through `chain_input_hash` above, just read back live instead of
/// computed ahead of a request. Relies on this module's own persist calls
/// already having kept `kind`'s revision current earlier in this same run
/// (separation before isolate, isolate before cleanup): each downstream
/// stage's own artifact event only ever arrives once its upstream native
/// worker has already finished, and this module persists every stage the
/// instant its own artifact event fires -- so by the time a later stage is
/// asked about, its upstream's revision is guaranteed already recorded.
fn active_content_hash(file_hash: &str, kind: ArtifactKind) -> Option<String> {
    load_active_artifact(file_hash, kind)
        .filter(|revision| !revision.invalidated)
        .map(|revision| revision.content_hash)
}

fn finalize_and_persist_stem(
    cache_root: &Path,
    file_hash: &str,
    kind: ArtifactKind,
    node_id: &str,
    fingerprint: String,
    source: &Path,
) {
    let Ok(store) = ArtifactStore::new(cache_root) else {
        return;
    };
    let Ok((path, content_hash, byte_size)) = store.capture(file_hash, kind, source) else {
        return;
    };
    let revision = ArtifactRevision {
        id: format!(
            "{file_hash}:{}:{content_hash}",
            serde_json::to_string(&kind).unwrap_or_else(|_| format!("{kind:?}"))
        ),
        file_hash: file_hash.to_string(),
        kind,
        path,
        content_hash,
        producer_node: AnalysisNodeId::new(node_id),
        input_revisions: Vec::new(),
        config_hash: fingerprint,
        algorithm_version: format!("chain-cache-v1/app-{}", env!("CARGO_PKG_VERSION")),
        created_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        byte_size,
        active: false,
        legacy: false,
        invalidated: false,
    };
    if record_artifact_revision(&revision).is_ok() {
        let _ = set_active_artifact_revision(cache_root, file_hash, kind, &revision.id);
    }
}

/// Persists a Step 1 audio-chain stem as soon as its own native worker task
/// succeeds, independent of whether the run's *later* stages ultimately
/// fail. This is what actually makes `skip_if_unchanged` useful for a song
/// whose workflow keeps failing on some downstream stage (ASR, forced
/// alignment, ...): before this, a stem only ever got recorded as a
/// reusable revision by `validate_and_publish_engine_result`, which only
/// ever runs for a *complete* Ok/OkDegraded result -- so any run that later
/// failed lost this stage's real, valid, already-paid-for output and had to
/// redo it from scratch on every retry, no matter how many times it had
/// already succeeded. Called live, from the lifecycle event the Engine
/// emits the moment a worker reports this exact output (see
/// `analysis-engine`'s `LifecycleNodeGuard::artifact_with_path`).
///
/// Covers the whole Step 1 chain: separation's two stems (`guide_vocals`,
/// `instrumental`, fingerprinted from `file_hash` alone -- see
/// `plan_chain_cache` above), lead-isolate's `lead_vocal` (fingerprinted
/// from separation's own active revision), and cleanup's combined
/// `clean_lead_vocal`/`dereverbed_vocal` (fingerprinted from whichever of
/// isolate's or separation's active revision fed it, and only for whichever
/// of denoise/dereverb is this workflow's actual terminal stage --
/// `is_last_cleanup_stage`). Each downstream stage reads its upstream
/// identity from `active_content_hash`, which this same function keeps
/// current as artifacts arrive in pipeline order within one run.
///
/// Silently does nothing for a semantic name this cache doesn't handle, a
/// disabled/opted-out node, an upstream identity that isn't recorded yet,
/// or a `source` this cache can't read/capture -- this must never turn an
/// otherwise-successful worker output into a failed run merely because
/// caching it hit a snag.
pub fn persist_cacheable_stem(cache_root: &Path, file_hash: &str, artifact: &str, source: &Path) {
    let Ok(stored_workflow) = crate::workflow::load_song_workflow(file_hash) else {
        return;
    };
    let workflow = &stored_workflow.definition;
    match artifact {
        "guide_vocals" | "instrumental" => {
            let Some(separation_node) = find_node(workflow, "audio.separate_vocal_bgm") else {
                return;
            };
            if separation_node.execution_policy == ExecutionPolicy::Disabled
                || !separation_node.skip_if_unchanged
            {
                return;
            }
            let (kind, node_id) = if artifact == "guide_vocals" {
                (ArtifactKind::VocalStem, "vocal_bgm_split")
            } else {
                (ArtifactKind::InstrumentalStem, "vocal_bgm_split_instrumental")
            };
            let fingerprint = compute_native_config_hash(
                &AnalysisNodeId::new(node_id),
                "audio.separate_vocal_bgm",
                &normalized_parameters(separation_node),
                &[file_hash],
                separation_node.model_id.as_deref(),
                None,
            );
            finalize_and_persist_stem(cache_root, file_hash, kind, node_id, fingerprint, source);
        }
        "lead_vocal" => {
            let Some(isolate_node) = find_node(workflow, "audio.lead_isolate") else {
                return;
            };
            if isolate_node.execution_policy == ExecutionPolicy::Disabled
                || !isolate_node.skip_if_unchanged
            {
                return;
            }
            let Some(chain_input_hash) = active_content_hash(file_hash, ArtifactKind::VocalStem)
            else {
                return;
            };
            let fingerprint = compute_native_config_hash(
                &AnalysisNodeId::new("lead_isolate"),
                "audio.lead_isolate",
                &normalized_parameters(isolate_node),
                &[&chain_input_hash],
                isolate_node.model_id.as_deref(),
                None,
            );
            finalize_and_persist_stem(
                cache_root,
                file_hash,
                ArtifactKind::AnalysisVocalStem,
                "lead_isolate",
                fingerprint,
                source,
            );
        }
        "clean_lead_vocal" | "dereverbed_vocal" => {
            let denoise_node = find_node(workflow, "audio.denoise");
            let dereverb_node = find_node(workflow, "audio.dereverb");
            let denoise_enabled = enabled(denoise_node);
            let dereverb_enabled = enabled(dereverb_node);
            let is_terminal_stage = match artifact {
                "clean_lead_vocal" if denoise_enabled => is_last_cleanup_stage(
                    workflow,
                    denoise_node.expect("enabled() only true when Some"),
                    dereverb_node.filter(|_| dereverb_enabled),
                ),
                "dereverbed_vocal" if dereverb_enabled => is_last_cleanup_stage(
                    workflow,
                    dereverb_node.expect("enabled() only true when Some"),
                    denoise_node.filter(|_| denoise_enabled),
                ),
                _ => false,
            };
            if !is_terminal_stage {
                return;
            }
            let denoise_ready =
                !denoise_enabled || denoise_node.is_some_and(|node| node.skip_if_unchanged);
            let dereverb_ready =
                !dereverb_enabled || dereverb_node.is_some_and(|node| node.skip_if_unchanged);
            if !denoise_ready || !dereverb_ready {
                return;
            }
            let isolate_node = find_node(workflow, "audio.lead_isolate");
            let chain_input_kind = if enabled(isolate_node) {
                ArtifactKind::AnalysisVocalStem
            } else {
                ArtifactKind::VocalStem
            };
            let Some(chain_input_hash) = active_content_hash(file_hash, chain_input_kind) else {
                return;
            };
            let cleanup_recipe = serde_json::to_string(&(
                denoise_enabled
                    .then(|| denoise_node.map(normalized_parameters))
                    .flatten(),
                dereverb_enabled
                    .then(|| dereverb_node.map(normalized_parameters))
                    .flatten(),
            ))
            .unwrap_or_default();
            let fingerprint = compute_native_config_hash(
                &AnalysisNodeId::new("cleanup"),
                "audio.denoise+audio.dereverb",
                &cleanup_recipe,
                &[&chain_input_hash],
                None,
                None,
            );
            finalize_and_persist_stem(
                cache_root,
                file_hash,
                ArtifactKind::DereverbedVocalStem,
                "cleanup",
                fingerprint,
                source,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_db::{AnalysisArtifactRow, analysis_artifacts_publish_batch};
    use crate::workflow::default_workflow;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "uta-studio-chain-cache-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn publish_active(
        file_hash: &str,
        kind: ArtifactKind,
        id: &str,
        config_hash: &str,
        content_hash: &str,
    ) {
        let kind_json = serde_json::to_string(&kind).unwrap();
        let row = AnalysisArtifactRow {
            id: id.to_string(),
            file_hash: file_hash.to_string(),
            kind: kind_json.clone(),
            path: format!("{id}.flac"),
            content_hash: content_hash.to_string(),
            producer_node: "test".to_string(),
            input_revisions: "[]".to_string(),
            config_hash: config_hash.to_string(),
            algorithm_version: "1".to_string(),
            created_at_ms: 1,
            byte_size: 1,
            active: false,
            legacy: false,
            invalidated: false,
        };
        analysis_artifacts_publish_batch(
            &[row],
            &[(file_hash.to_string(), kind_json, id.to_string())],
            &[],
        )
        .unwrap();
    }

    #[test]
    fn nothing_cached_leaves_the_source_at_original_mix() {
        let root = temp_root("empty");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-empty";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::OriginalMix);
        assert!(decision.source_path.is_none());
        assert!(decision.fingerprints.separation.is_some());
    }

    #[test]
    fn a_matching_cached_vocal_stem_is_reused_as_guide_vocals() {
        let root = temp_root("hit");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-hit";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;

        let expected = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &expected,
            "vocal-content-1",
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::GuideVocals);
        assert_eq!(
            decision.source_path,
            Some(std::path::PathBuf::from("vocal-1.flac"))
        );
    }

    #[test]
    fn a_live_artifact_event_makes_separation_reusable_on_the_very_next_plan() {
        // The whole point: a stem the worker just produced becomes a real,
        // matching cached revision *without* the run it came from ever
        // reaching a complete result manifest -- exactly the case a later,
        // unrelated stage (ASR, forced alignment, ...) failing must not be
        // allowed to erase.
        let root = temp_root("live-persist");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-live-persist";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;
        crate::workflow::save_song_workflow(
            file_hash,
            workflow.clone(),
            crate::workflow::WorkflowLayout::default(),
        )
        .unwrap();

        let source = root.join("guide-vocals.flac");
        std::fs::write(&source, b"fake vocal stem bytes").unwrap();
        persist_cacheable_stem(&root, file_hash, "guide_vocals", &source);

        let revision = load_active_artifact(file_hash, ArtifactKind::VocalStem)
            .expect("the live event must have published a matching revision");
        assert!(!revision.invalidated);
        assert_eq!(
            revision.config_hash,
            plan_chain_cache(file_hash, &workflow)
                .fingerprints
                .separation
                .unwrap(),
            "the persisted fingerprint must match what a future plan looks up"
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::GuideVocals);
        assert_eq!(decision.source_path, Some(revision.path));
    }

    #[test]
    fn live_events_persist_the_whole_chain_and_only_the_workflows_real_terminal_cleanup_stage() {
        // Default workflow order is denoise -> dereverb (see
        // `default_definition.rs`'s own `vocal_tail` comment), so dereverb's
        // own artifact is the real, final `clean_lead_vocal`; denoise's own
        // artifact is an intermediate step in this order and must not be
        // cached under the combined cleanup identity.
        let root = temp_root("live-persist-full-chain");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-live-persist-full-chain";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true; // separation
        workflow.nodes[2].execution_policy = ExecutionPolicy::Always; // lead_isolate
        workflow.nodes[2].skip_if_unchanged = true;
        workflow.nodes[3].execution_policy = ExecutionPolicy::Always; // denoise
        workflow.nodes[3].skip_if_unchanged = true;
        workflow.nodes[4].execution_policy = ExecutionPolicy::Always; // dereverb
        workflow.nodes[4].skip_if_unchanged = true;
        crate::workflow::save_song_workflow(
            file_hash,
            workflow.clone(),
            crate::workflow::WorkflowLayout::default(),
        )
        .unwrap();

        let write = |artifact: &str, name: &str| {
            let source = root.join(format!("{name}.flac"));
            std::fs::write(&source, format!("bytes for {name}")).unwrap();
            persist_cacheable_stem(&root, file_hash, artifact, &source);
        };
        write("guide_vocals", "guide-vocals");
        write("lead_vocal", "lead-vocal");
        // Wrong order for this workflow: must be silently ignored.
        write("clean_lead_vocal", "denoise-intermediate");
        write("dereverbed_vocal", "dereverb-final");

        assert!(
            load_active_artifact(file_hash, ArtifactKind::VocalStem).is_some(),
            "separation must be cached"
        );
        let isolate_revision = load_active_artifact(file_hash, ArtifactKind::AnalysisVocalStem)
            .expect("lead-isolate must be cached");
        let cleanup_revision = load_active_artifact(file_hash, ArtifactKind::DereverbedVocalStem)
            .expect("dereverb's own output must be cached as the real cleanup result");

        assert_eq!(
            isolate_revision.config_hash,
            plan_chain_cache(file_hash, &workflow)
                .fingerprints
                .isolate
                .unwrap()
        );
        // `clean_lead_vocal` (denoise, not the terminal stage in this
        // order) must not have overwritten the real cleanup revision with
        // its own intermediate bytes. `ArtifactStore::capture` names files
        // by content hash, not by source filename, so compare bytes.
        assert_eq!(
            std::fs::read_to_string(&cleanup_revision.path).unwrap(),
            "bytes for dereverb-final"
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::CleanLeadVocal);
        assert_eq!(decision.source_path, Some(cleanup_revision.path));
    }

    #[test]
    fn a_live_artifact_event_does_nothing_for_an_opted_out_node() {
        let root = temp_root("live-opt-out");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-live-opt-out";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = false;
        crate::workflow::save_song_workflow(
            file_hash,
            workflow,
            crate::workflow::WorkflowLayout::default(),
        )
        .unwrap();

        let source = root.join("guide-vocals.flac");
        std::fs::write(&source, b"fake vocal stem bytes").unwrap();
        persist_cacheable_stem(&root, file_hash, "guide_vocals", &source);

        assert!(load_active_artifact(file_hash, ArtifactKind::VocalStem).is_none());
    }

    #[test]
    fn matching_step_one_pair_carries_instrumental_bytes_into_the_next_request() {
        let root = temp_root("paired-hit");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-paired-hit";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;

        let first = plan_chain_cache(file_hash, &workflow);
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-pair",
            first.fingerprints.separation.as_deref().unwrap(),
            "vocal-pair-content",
        );
        publish_active(
            file_hash,
            ArtifactKind::InstrumentalStem,
            "instrumental-pair",
            first.fingerprints.instrumental.as_deref().unwrap(),
            "instrumental-pair-content",
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert!(decision.cached_sources.contains(&CachedChainSource {
            role: AudioRoleWireV1::GuideVocals,
            path: std::path::PathBuf::from("vocal-pair.flac"),
            identity: "vocal-pair-content".to_string(),
        }));
        assert!(decision.cached_sources.contains(&CachedChainSource {
            role: AudioRoleWireV1::Instrumental,
            path: std::path::PathBuf::from("instrumental-pair.flac"),
            identity: "instrumental-pair-content".to_string(),
        }));
        assert_eq!(
            decision.satisfied_capabilities,
            ["audio.extract_instrumental".to_string()]
        );
    }

    #[test]
    fn a_changed_model_id_invalidates_the_cache_hit() {
        let root = temp_root("stale-model");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-stale-model";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;

        let stale = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &stale,
            "vocal-content-1",
        );
        workflow.nodes[1].model_id = Some("a_different_model".to_string());

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::OriginalMix);
        assert!(decision.source_path.is_none());
    }

    #[test]
    fn the_checkbox_being_off_never_reuses_even_a_matching_cached_artifact() {
        let root = temp_root("box-off");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-box-off";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = false;

        let fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &fingerprint,
            "vocal-content-1",
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::OriginalMix);
    }

    #[test]
    fn lead_isolate_enabled_but_not_yet_cached_stops_the_chain_at_separation() {
        let root = temp_root("isolate-miss");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-isolate-miss";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;
        workflow.nodes[2].execution_policy = ExecutionPolicy::Always; // lead_isolate
        workflow.nodes[2].skip_if_unchanged = true;

        let separation_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &separation_fingerprint,
            "vocal-content-1",
        );
        // No AnalysisVocalStem published -- isolate itself has never run.

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::GuideVocals);
        assert!(decision.fingerprints.isolate.is_some());
    }

    #[test]
    fn the_full_chain_reuses_through_cleanup_when_every_stage_is_cached() {
        let root = temp_root("full-chain");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-full-chain";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true; // separation
        workflow.nodes[2].execution_policy = ExecutionPolicy::Always; // lead_isolate
        workflow.nodes[2].skip_if_unchanged = true;
        workflow.nodes[3].execution_policy = ExecutionPolicy::Always; // denoise
        workflow.nodes[3].skip_if_unchanged = true;
        workflow.nodes[4].execution_policy = ExecutionPolicy::Always; // dereverb
        workflow.nodes[4].skip_if_unchanged = true;

        let separation_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &separation_fingerprint,
            "vocal-content-1",
        );
        let isolate_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .isolate
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::AnalysisVocalStem,
            "isolate-1",
            &isolate_fingerprint,
            "isolate-content-1",
        );
        let cleanup_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .cleanup
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::DereverbedVocalStem,
            "cleanup-1",
            &cleanup_fingerprint,
            "cleanup-content-1",
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::CleanLeadVocal);
        assert_eq!(
            decision.source_path,
            Some(std::path::PathBuf::from("cleanup-1.flac"))
        );
        assert_eq!(
            decision.satisfied_capabilities,
            vec!["audio.denoise".to_string(), "audio.dereverb".to_string()]
        );
        assert_eq!(
            decision
                .cached_sources
                .iter()
                .map(|source| source.role)
                .collect::<Vec<_>>(),
            vec![
                AudioRoleWireV1::GuideVocals,
                AudioRoleWireV1::LeadVocal,
                AudioRoleWireV1::CleanLeadVocal,
            ]
        );
    }

    #[test]
    fn checking_only_denoise_never_uses_the_cleanup_cache_while_dereverb_is_also_enabled() {
        let root = temp_root("partial-cleanup");
        let _guard = crate::library_db::reconnect_for_test(&root);
        let file_hash = "song-partial-cleanup";
        let mut workflow = default_workflow(file_hash);
        workflow.nodes[1].skip_if_unchanged = true;
        workflow.nodes[3].execution_policy = ExecutionPolicy::Always; // denoise
        workflow.nodes[3].skip_if_unchanged = true;
        workflow.nodes[4].execution_policy = ExecutionPolicy::Always; // dereverb, box unchecked
        workflow.nodes[4].skip_if_unchanged = false;

        let separation_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .separation
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::VocalStem,
            "vocal-1",
            &separation_fingerprint,
            "vocal-content-1",
        );
        let cleanup_fingerprint = plan_chain_cache(file_hash, &workflow)
            .fingerprints
            .cleanup
            .unwrap();
        publish_active(
            file_hash,
            ArtifactKind::DereverbedVocalStem,
            "cleanup-1",
            &cleanup_fingerprint,
            "cleanup-content-1",
        );

        let decision = plan_chain_cache(file_hash, &workflow);
        assert_eq!(decision.role, AudioRoleWireV1::GuideVocals);
    }

    #[test]
    fn stems_to_request_for_caching_only_names_checked_and_enabled_units() {
        let file_hash = "song-stems";
        let mut workflow = default_workflow(file_hash);
        assert_eq!(
            stems_to_request_for_caching(&workflow),
            vec![AudioRoleWireV1::GuideVocals, AudioRoleWireV1::Instrumental]
        );

        workflow.nodes[3].execution_policy = ExecutionPolicy::Always;
        workflow.nodes[3].skip_if_unchanged = true;
        assert!(stems_to_request_for_caching(&workflow).contains(&AudioRoleWireV1::CleanLeadVocal));
    }
}
