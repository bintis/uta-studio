mod acceptance_gen;
mod analysis_artifact;
mod analysis_engine_adapter;
mod analysis_experience;
mod analysis_graph;
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
mod chain_cache;
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
mod lyrics_sources;
mod runtime_presentation;
mod scanner;
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
    record_artifact_revision, set_active_artifact_revision, validate_artifact_revision_file,
};
pub use analysis_engine_adapter::{
    AnalysisRequestIntent, EngineRunDraft, EngineRunPreview, QueuedEngineRun,
    ResolvedAnalysisSource, StudioLyricToken, StudioLyricsContext, StudioLyricsContextProjection,
    StudioLyricsMode, compile_analyze_request_v1, preview_analyze_request_v1,
    preview_and_queue_engine_run, preview_and_stage_engine_run, preview_engine_run,
    project_lyrics_context, queue_exact_preview, resolve_true_source, stage_exact_preview,
};
pub use analysis_experience::{
    ANALYSIS_EXPERIENCE_SCHEMA_VERSION, AnalysisAudioPreferences, AnalysisDefaultTarget,
    AnalysisExperienceOverride, AnalysisExperienceSettings, AnalysisLyricsPreferences,
    AnalysisOutputKind, AnalysisOutputSelection, AnalysisQualityProfile, AnalysisSettingSource,
    AnalysisSingingPreferences, AutomaticStrategy, EffectiveAnalysisExperience,
    EffectiveAnalysisSetting, resolve_analysis_experience,
};
pub use analysis_graph::{
    AnalysisEdge, AnalysisGraphSpec, AnalysisNodeId, AnalysisNodeSpec, ArtifactKind, CachePolicy,
    DisablePolicy, GraphValidationError,
};
pub use analysis_profile::{
    SongAnalysisProfile, get_song_analysis_profile, reset_song_analysis_profile,
    set_song_analysis_profile,
};
pub use analyzer::{
    AnalysisBatchQueueResult, AnalysisProgressSnapshot, AnalysisQueue, AnalysisRunComparison,
    AnalysisRunHistory, AnalysisStageRoute, AnalysisTask, EngineErrorHistoryProjection,
    EngineRunHistoryProjection, NodeAttempt, NodeAttemptComparison, QueuedStatus,
    SongAuthoringState, analysis_log_lines, analysis_log_path_for, cancel_analysis_run,
    clear_analysis_history, compare_analysis_runs, compare_node_attempt_with_previous_run,
    delete_cache, enqueue_all, enqueue_one, force_stop_all_analysis, load_analysis_history,
    load_analysis_node_attempts, load_analysis_tasks, move_analysis_queue_item, realign,
    reanalyze_force_transcribe, reanalyze_full, reanalyze_pitch, reanalyze_transcript,
    remove_analysis_queue_item, remove_song_from_library, resolve_song_authoring_state,
    start_queued_analysis, stop_analysis_run,
};
pub use api::{API_CAPABILITIES, ApiCapability, api_capabilities};
pub use applog::{LogLine, get_log_path, get_recent_logs, log_lines_in_window, record_log_text};
pub use artifact_workbench::{
    ArtifactBinding, ArtifactBindingState, ArtifactCapability, ArtifactDirection,
    ArtifactDraftCommit, ArtifactDraftContent, ArtifactDraftKind, ArtifactEditDraft,
    ArtifactHealth, ArtifactHealthStatus, ArtifactInspection, ArtifactLineage, ArtifactLineageNode,
    ArtifactMediaType, ArtifactPreview, ArtifactRef, ArtifactSaveMode, ArtifactSaveOptions,
    ArtifactTypedDiff, ChartRevisionMergeMode, DownstreamImpact, NodeIoInspection,
    apply_artifact_revision_to_chart, artifact_capabilities, artifact_editor_text, artifact_health,
    artifact_lineage, authored_chart_is_pinned, begin_artifact_edit,
    capture_analysis_run_artifacts, commit_artifact_edit, compare_artifacts_typed,
    inspect_analysis_node_io, inspect_artifact, merge_chart_revisions, preview_artifact,
    preview_artifact_edit_impact, resolve_artifact_for_run, resolve_graph_edge_binding,
    set_artifact_pinned,
};
pub use audio_model::{
    AudioModelCatalogSummary, AudioModelLicense, AudioModelStatus, DEFAULT_BGM_MODEL_ID,
    DEFAULT_VOCAL_MODEL_ID,
};
pub use audio_processing::{
    get_audio_model_status, install_audio_model, list_audio_models, reinstall_audio_model,
    remove_audio_model,
};
pub use authoring::{
    AudioPaths, ShiftDone, ShiftResult, get_audio_paths, load_pitch_guide, load_transcript,
    shift_key, shift_key_done_payload, shift_tempo, shift_tempo_done_payload,
};
pub use backend_cli::{
    AnalysisCliClient, AnalysisPlanWireV1, AnalysisResultManifestWireV1, AnalyzeRequestWireV1,
    BackendCliError, ContinuousF0SourceWireV1, ExpertFusionPolicyWireV1, FusionModeWireV1,
    InstallStateWireV1, NoteLengthSourceWireV1, OnsetSupportSourceWireV1, ReadinessReasonWireV1,
    ResourceOriginWireV1, RuntimeCliClient, RuntimeFusionProviderReportWireV1,
    RuntimeFusionProviderStatusWireV1, RuntimeResourceDetailsWireV1, RuntimeResourceStatusWireV1,
    WorkflowExecutionNodePlanWireV1, WorkflowExecutionPlanWireV1, WorkflowNodeExecutionStateWireV1,
};
pub use cache::{
    CacheDir, CachePaths, CacheStats, cache_roots, default_uta_studio_dir, normalized_target_path,
    same_path, uta_studio_dir,
};
pub use chart::{
    CandidateChartStatus, CandidateChartSummary, ChartAudio, ChartDocument, ChartReadiness,
    ChartUpdatePolicy, ChartWaveform, candidate_chart_status, chart_problem_count, chart_readiness,
    decode_chart_waveform, delete_authored_chart, load_chart,
    replace_authored_chart_with_fresh_analysis, save_vocal_chart, save_vocal_chart_from_revision,
};
pub use config::{AppConfig, LibrarySource};
pub use editor::{
    ChartLyric, ChartNote, ChartProblem, ClipboardNote, CorrectionType, EDITOR_ACTIONS,
    EditorActionAccess, EditorActionDef, EditorActionGroup, EditorAudioArtifact, EditorDocument,
    EditorSourceContext, EditorSuggestion, EditorSuggestionKind, EvidenceKind, EvidencePoint,
    EvidenceTrack, HumanCorrection, KeyChord, LyricAddress, MIN_NOTE_SECONDS, NoteKind,
    ProblemKind, ProblemReport, ReviewReason, ReviewRegion, ReviewSeverity, Severity,
    SingingEvidenceBundle, Syllable, TrackRole, TrackSummary, apply_editor_suggestion,
    editor_action, editor_action_for_chord, editor_actions, kana_morae,
    singing_analysis_evidence_bundle, syllables, technique_evidence_track,
};
pub use export_destination::{
    ExportNodeInspection, ExportPackageKind, inspect_export_node, last_export_destination,
    record_last_export, validate_export_node, validate_export_package,
};
pub use library_db::{init_library, library_db_path, load_song_by_hash, load_song_by_path};
pub use library_menu::{LibraryMenuItem, LibraryMenuItems, load_library_menu_items};
pub use library_model::{LibraryMenuFilters, LoadSongsParams, SongsMeta, SongsStore};
pub use lyrics::{
    CanonicalLyricsSource, CanonicalLyricsStatus, LrclibCandidate, LyricsCandidate, LyricsFile,
    LyricsProvider, LyricsProviderFailure, LyricsSearchResult, apply_timed_lyrics,
    canonical_lyrics_status, fetch_lyrics_candidate, load_lyrics_file,
    lrc_transcript_line_segments, provide_lrc, save_lyrics, save_timed_lyrics,
    search_lrclib_for_hash, search_lyrics_for_hash,
};
pub use runtime_presentation::{
    FUSION_AGENT_ADAPTER_RESOURCE_ID, RuntimeBackendCapabilityPresentation,
    RuntimeBackendPresentation, RuntimeModelPresentation, RuntimeValidationPresentation,
    clear_fusion_agent_adapter, clear_fusion_provider, configure_fusion_agent_adapter,
    configure_fusion_provider, fusion_agent_adapter_status, fusion_provider_status,
    runtime_model_presentations,
};
pub use scanner::{clear_library_index, start_scan};
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
    FusionModeV1, NodeCapability, NodePosition, OptionalWorkflowCardV1, QualityMode,
    SeparationOutputRoleV1, SeparationProviderExecutionV1, SeparationStrategyOptionV1,
    SeparationStrategyV1, StoredWorkflow, WORKFLOW_EXECUTION_EXTENSION_KEY,
    WORKFLOW_SCHEMA_VERSION, WorkflowBindingWireV1, WorkflowCompileError, WorkflowDefinition,
    WorkflowEdge, WorkflowExecutionInvocationWireV1, WorkflowExecutionSnapshot,
    WorkflowExecutionWireV1, WorkflowId, WorkflowLayout, WorkflowModelOption, WorkflowNodeId,
    WorkflowNodeInstance, WorkflowNodeWireV1, WorkflowPortRef, WorkflowPortSpec, WorkflowPortType,
    WorkflowProviderPreferencesWireV1, WorkflowTerminalOutputWireV1, WorkflowValidationCode,
    WorkflowValidationIssue, WorkflowValidationReport, add_optional_workflow_card,
    bind_workflow_analyzer, compile_workflow, default_workflow, duplicate_audio_transformation,
    fusion_mode, insert_audio_transformation_after_output, list_workflow_capabilities,
    load_song_workflow, preview_workflow_compile, remove_workflow_node,
    reorder_audio_transformation, save_song_workflow, separation_strategy_descriptor,
    separation_strategy_options, set_workflow_execution_policy, set_workflow_node_model,
    set_workflow_parameter, set_workflow_preprocessing_enabled, set_workflow_priority,
    set_workflow_separation_strategy, set_workflow_skip_if_unchanged, validate_workflow,
    workflow_definition_digest, workflow_has_optional_card, workflow_model_label,
    workflow_model_options,
};

pub fn startup() -> Result<(), String> {
    // Load and repair configuration before opening SQLite: the configured
    // data root decides which library database belongs to this process.
    let config = AppConfig::load();
    init_library().map_err(|e| e.to_string())?;

    // Exact Engine requests that the user already started resume from their
    // durable snapshots independently of the legacy aggregate readiness
    // signal. Staged requests remain held in Processing Queue until an explicit
    // Start action; startup must never turn them into automatic work.
    let resumable =
        library_db::analysis_queue_resumable_hashes().map_err(|error| error.to_string())?;
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
