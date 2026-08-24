// Unit-test builds intentionally compile retired analyzer compatibility helpers
// that are unavailable from the product API but still exercise migration data.
#![cfg_attr(test, allow(dead_code))]

mod analysis_artifact;
mod analysis_engine_adapter;
mod analysis_experience;
mod analysis_graph;
mod analysis_plan;
mod analysis_profile;
mod analyzer;
mod api;
mod applog;
mod artifact_workbench;
mod audio_format;
mod audio_model;
mod audio_processing;
mod authoring;
mod backend_cli;
mod cache;
mod chart;
mod config;
mod editor;
mod error;
mod export_destination;
mod library_db;
mod library_menu;
mod library_model;
mod lrc;
mod lyrics;
mod native_runtime;
mod scanner;
mod singing;
mod song;
mod source;
mod ultrastar_export;
mod usdx;
mod utz_export;
mod vendor;
mod vocal_chart;
mod workflow;

pub use analysis_artifact::{
    ArtifactRevision, ArtifactRevisionComparison, ArtifactStore, ArtifactSummary, artifact_present,
    cached_artifact_presence, cached_artifact_presence_for_song, compare_artifact_revisions,
    compute_config_hash, compute_native_config_hash, delete_artifact_revision, hash_file_contents,
    import_legacy_artifacts, invalidate_artifact_revision, load_active_artifact,
    load_analysis_artifacts, load_artifact_revisions, migrate_artifact_revisions_to_store,
    record_artifact_revision, set_active_artifact_revision,
};
pub use analysis_engine_adapter::{
    AnalysisRequestIntent, EngineRunDraft, EngineRunPreview, QueuedEngineRun,
    ResolvedAnalysisSource, StudioLyricToken, StudioLyricsContext, StudioLyricsContextProjection,
    StudioLyricsMode, compile_analyze_request_v1, preview_analyze_request_v1,
    preview_and_queue_engine_run, preview_engine_run, project_lyrics_context, queue_exact_preview,
    resolve_true_source,
};
pub use analysis_experience::{
    ANALYSIS_EXPERIENCE_SCHEMA_VERSION, AnalysisAudioPreferences, AnalysisDefaultTarget,
    AnalysisExperienceOverride, AnalysisExperienceSettings, AnalysisLyricsPreferences,
    AnalysisQualityProfile, AnalysisSettingSource, AnalysisSingingPreferences, AutomaticStrategy,
    EffectiveAnalysisExperience, EffectiveAnalysisSetting, resolve_analysis_experience,
};
pub use analysis_graph::{
    AnalysisEdge, AnalysisGraphSpec, AnalysisNodeId, AnalysisNodeSpec, ArtifactKind, CachePolicy,
    DisablePolicy, GraphValidationError, active_stem_nodes_from_settings,
    analysis_node_for_audio_step, baseline_graph_spec, default_active_stem_nodes,
    lyrics_route_node_ids, optional_stem_node_ids, stem_group_node_ids,
};
pub use analysis_plan::{
    AnalysisPlan, AnalysisRequest, LyricsRoute, NodeState, PlanError, PlanWarning, PlannedNode,
    build_plan, get_analysis_graph, preview_analysis_plan,
};
pub use analysis_profile::{
    AnalysisProfileSnapshot, ProfileField, ProfileFieldResolution, ProfileSource,
    get_song_analysis_profile, reset_song_analysis_profile, resolve_profile_field,
    set_song_analysis_profile,
};
pub use analyzer::{
    AnalysisBatchQueueResult, AnalysisProgressSnapshot, AnalysisQueue, AnalysisRunComparison,
    AnalysisRunHistory, AnalysisStageRoute, AnalysisTask, EngineRunHistoryProjection, NodeAttempt,
    NodeAttemptComparison, PendingAnalysisIntent, QueuedStatus, SongAuthoringState,
    analysis_log_lines, analysis_log_path_for, cancel_analysis_run, clear_analysis_history,
    compare_analysis_runs, compare_node_attempt_with_previous_run, delete_cache,
    downstream_node_ids, enqueue_all, enqueue_one, load_analysis_history,
    load_analysis_node_attempts, load_analysis_tasks, preview_analysis_plan_for_selection,
    preview_full_analysis_plan, realign, reanalyze_force_transcribe, reanalyze_full,
    reanalyze_pitch, reanalyze_transcript, resolve_song_authoring_state, shutdown_server,
    stop_analysis_run,
};
pub use api::{API_CAPABILITIES, ApiCapability, api_capabilities};
pub use applog::{LogLine, get_log_path, get_recent_logs, log_lines_in_window, record_log_text};
pub use artifact_workbench::{
    ArtifactBinding, ArtifactBindingState, ArtifactCapability, ArtifactDirection,
    ArtifactDraftCommit, ArtifactDraftContent, ArtifactDraftKind, ArtifactEditDraft,
    ArtifactHealth, ArtifactHealthStatus, ArtifactInspection, ArtifactLineage, ArtifactLineageNode,
    ArtifactMediaType, ArtifactPreview, ArtifactRef, ArtifactSaveMode, ArtifactSaveOptions,
    ArtifactTypedDiff, ChartRevisionMergeMode, DownstreamImpact, ImpactTrigger, NodeIoInspection,
    analysis_request_from_impact, apply_artifact_revision_to_chart, artifact_capabilities,
    artifact_editor_text, artifact_health, artifact_lineage, authored_chart_is_pinned,
    begin_artifact_edit, capture_analysis_run_artifacts, commit_artifact_edit,
    compare_artifacts_typed, inspect_analysis_node_io, inspect_artifact, merge_chart_revisions,
    preview_artifact, preview_artifact_downstream_impact, preview_artifact_edit_impact,
    preview_frozen_downstream_impact, preview_node_downstream_impact,
    queued_request_matches_preview, resolve_artifact_for_run, resolve_graph_edge_binding,
    set_artifact_pinned,
};
pub use audio_model::{
    AudioModelCatalogSummary, AudioModelFileStatus, AudioModelLicense, AudioModelStatus,
    AudioParameterMap, AudioParameterSpec, AudioParameterValue, DEFAULT_BGM_MODEL_ID,
    DEFAULT_VOCAL_MODEL_ID, REQUIRED_AUDIO_MODEL_IDS, audio_model_dir, audio_processing_root,
};
pub use audio_processing::{
    AudioInputReference, AudioOutputBinding, AudioProcessingPlanSnapshot, AudioProcessingSettings,
    AudioProcessingStep, AudioRuntimeRequest, ResolvedAudioParameter, cleanup_model_enabled,
    get_audio_model_status, install_audio_model, list_audio_models, preview_effective_audio_params,
    reinstall_audio_model, remove_audio_model, validate_audio_processing_profile,
};
pub use authoring::{
    AudioPaths, ShiftDone, ShiftResult, get_audio_paths, load_pitch_guide, load_transcript,
    shift_key, shift_key_done_payload, shift_tempo, shift_tempo_done_payload,
};
pub use backend_cli::{
    AnalysisCliClient, AnalysisPlanWireV1, AnalysisResultManifestWireV1, AnalyzeRequestWireV1,
    BackendCliError, RuntimeCliClient, RuntimeResourceDetailsWireV1, RuntimeResourceStatusWireV1,
};
pub use cache::{
    CacheDir, CachePaths, CacheStats, cache_roots, clear_models, default_uta_studio_dir,
    normalized_target_path, same_path, uta_studio_dir,
};
pub use chart::{
    CandidateChartStatus, CandidateChartSummary, ChartAudio, ChartDocument, ChartReadiness,
    ChartUpdatePolicy, ChartWaveform, candidate_chart_status, chart_problem_count, chart_readiness,
    decode_chart_waveform, load_chart, replace_authored_chart_with_fresh_analysis,
    save_vocal_chart, save_vocal_chart_from_revision,
};
pub use config::{AppConfig, LibrarySource};
pub use editor::{
    ChartLyric, ChartNote, ChartProblem, ClipboardNote, CorrectionType, EDITOR_ACTIONS,
    EditorActionAccess, EditorActionDef, EditorActionGroup, EditorAudioArtifact, EditorDocument,
    EditorSourceContext, EditorSuggestion, EditorSuggestionKind, EvidenceKind, EvidencePoint,
    EvidenceTrack, HumanCorrection, KeyChord, LyricAddress, MIN_NOTE_SECONDS, NoteKind,
    ProblemKind, ProblemReport, ReviewReason, ReviewRegion, ReviewSeverity, Severity,
    SingingEvidenceBundle, Syllable, TrackRole, TrackSummary, apply_editor_suggestion,
    editor_action, editor_action_for_chord, editor_actions, kana_morae, syllables,
    technique_evidence_track,
};
pub use export_destination::{
    ExportNodeInspection, ExportPackageKind, inspect_export_node, last_export_destination,
    record_last_export, validate_export_node, validate_export_package,
};
pub use library_db::{init_library, library_db_path, load_song_by_hash};
pub use library_menu::{LibraryMenuItem, LibraryMenuItems, load_library_menu_items};
pub use library_model::{LibraryMenuFilters, LoadSongsParams, SongsMeta, SongsStore};
pub use lyrics::{
    LrclibCandidate, LyricsFile, apply_timed_lyrics, load_lyrics_file, provide_lrc,
    save_lyrics_and_realign, search_lrclib_for_hash,
};
pub use native_runtime::{
    BackendCapability, NATIVE_WORKER_PROTOCOL_VERSION, NativeBackend, NativeModelRuntime,
    NativeRuntimeLock, NativeTask, NativeTaskOutput, NativeTaskResult,
    OPENVINO_WORKER_RECIPE_SHA256, RUNTIME_LOCK_JSON, RUNTIME_LOCK_SHA256, ResolvedNativeRuntime,
    RuntimeRouteError, ValidationState, WorkerCommand, WorkerFrame, component_executable,
    native_analyzer_path, native_runtime_lock, native_runtime_registry, resolve_native_runtime,
    run_native_task, runtime_recipe_digest,
};
pub use scanner::{clear_library_index, start_scan};
pub use singing::{
    CANONICAL_TIMELINE_STEP_MS, CalibrationMethod, CanonicalLyrics, CanonicalNote,
    CanonicalNoteEvidence, CanonicalSingingTrack, CanonicalWordBoundary, EvidenceFrame,
    EvidenceProvenance, EvidenceSeries, ExpertTask, F0Point, FusedEstimate, HarmonyMetadata,
    PitchAlternative, PitchBendPoint, ScalarEvidence, ScoreCalibrator, SegmentCandidate,
    SingingReviewReason, SingingReviewRegion, TechniqueScores, TimeRange, TranscriptHypothesis,
    TranscriptTokenEvidence, WeightedEstimate, WordBoundaryEvidence, build_canonical_singing_track,
    build_review_regions, correlation_aware_score, decode_candidate_graph, fuse_scalar,
    fuse_transcripts, fuse_word_boundaries,
};
pub use song::{
    MusicAnalysis, MusicAnalysisDescriptors, MusicKeyAnalysis, MusicRhythmAnalysis, Song,
    SongOrigin, TranscriptSource, load_music_analysis, update_song_settings,
};
pub use source::{
    FolderSource, LibraryFolderEntry, MediaSource, active_source, list_library_folder,
};
pub use ultrastar_export::{export_ultrastar, validate_ultrastar_chart, validate_ultrastar_text};
pub use utz::VocalChartV1;
pub use utz_export::{
    ExportProgress, ExportableSong, export_utz, export_utz_with_progress, list_exportable_songs,
};
pub use vendor::{
    AnalysisRuntimeStatus, AnalysisStrategyResourceStatus, ComputeBackend, ModelDownloadTarget,
    ModelInstallStatus, SetupFolders, SetupProgress, SetupStep, SetupTask, SetupTaskState,
    analysis_runtime_status, analysis_strategy_resource_statuses, ffmpeg_path,
    invalidate_analysis_runtime_status_cache, is_ready, model_install_statuses,
    resolve_data_path_input, run_vendor_setup, step_download_model,
};
pub use vocal_chart::migrate_analyzer_chart;
pub use workflow::{
    AnalyzerBinding, AudioArtifactDescriptor, AudioRole, CapabilityClass, CapabilityId,
    CompiledArtifactBinding, CompiledNodeBinding, ConditionalExecution, ExecutionPolicy,
    NodeCapability, NodePosition, QualityMode, ResolvedRuntimeKind, StoredWorkflow,
    WorkflowCompileError, WorkflowDefinition, WorkflowEdge, WorkflowExecutionSnapshot, WorkflowId,
    WorkflowLayout, WorkflowNodeId, WorkflowNodeInstance, WorkflowPortRef, WorkflowPortSpec,
    WorkflowPortType, WorkflowValidationCode, WorkflowValidationIssue, WorkflowValidationReport,
    bind_workflow_analyzer, compile_workflow, default_workflow, duplicate_audio_transformation,
    list_workflow_capabilities, load_song_workflow, preview_workflow_compile,
    reorder_audio_transformation, save_song_workflow, set_workflow_execution_policy,
    set_workflow_priority, validate_workflow, workflow_definition_digest,
    workflow_from_audio_settings,
};

pub fn startup() -> Result<(), String> {
    // Load and repair configuration before opening SQLite: the configured
    // data root decides which library database belongs to this process.
    let config = AppConfig::load();
    init_library().map_err(|e| e.to_string())?;

    // Exact Engine requests resume from their durable snapshots independently
    // of the legacy aggregate readiness signal. Request-specific readiness was
    // already confirmed by uta-analyze before queueing; startup must not route
    // them through a different global heuristic or reconstruct their intent.
    let resumable = AnalysisQueue::load()
        .entries
        .into_iter()
        .filter_map(|(file_hash, status)| {
            matches!(
                status,
                analyzer::QueuedStatus::Queued | analyzer::QueuedStatus::Analyzing(_)
            )
            .then_some(file_hash)
        })
        .collect::<Vec<_>>();
    for file_hash in resumable {
        if library_db::analysis_queue_engine_intent(&file_hash)
            .ok()
            .flatten()
            .is_some()
        {
            analyzer::resume_engine_intent(&file_hash);
        } else {
            let _ = library_db::analysis_queue_upsert_row(
                &file_hash,
                "failed",
                None,
                Some("Legacy queue entry has no exact Engine request; rebuild Plan Preview."),
            );
        }
    }
    if is_ready() && config.auto_analyze() {
        analyzer::enqueue_all(&LibraryMenuFilters::default());
    }

    Ok(())
}
