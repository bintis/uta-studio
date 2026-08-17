use serde::Serialize;

/// Framework-independent description of an app-owned command.
///
/// The Bevy desktop shell consumes this catalogue as its local, in-process
/// command contract so feature classification cannot drift from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCapability {
    pub area: &'static str,
    pub command: &'static str,
    pub access: &'static str,
    pub automated_check: bool,
    pub description: &'static str,
}

macro_rules! capability {
    ($area:literal, $command:literal, $access:literal, $checked:literal, $description:literal) => {
        ApiCapability {
            area: $area,
            command: $command,
            access: $access,
            automated_check: $checked,
            description: $description,
        }
    };
}

/// Discoverable catalogue for every app-owned command. Destructive endpoints
/// are intentionally visible here but must never be invoked by diagnostics.
pub const API_CAPABILITIES: &[ApiCapability] = &[
    capability!(
        "app",
        "frontend_ready",
        "mutation",
        false,
        "Show the hydrated app window"
    ),
    capability!(
        "app",
        "get_log_path",
        "read",
        true,
        "Resolve the local application log"
    ),
    capability!(
        "app",
        "get_recent_logs",
        "read",
        true,
        "Read the in-memory diagnostic log"
    ),
    capability!(
        "window",
        "window_immersive",
        "read",
        false,
        "Read fullscreen state"
    ),
    capability!(
        "window",
        "minimize_window",
        "mutation",
        false,
        "Minimize the app window"
    ),
    capability!(
        "config",
        "load_config",
        "read",
        true,
        "Load persisted settings"
    ),
    capability!(
        "config",
        "save_config",
        "mutation",
        false,
        "Persist settings"
    ),
    capability!(
        "storage",
        "calculate_cache_stats",
        "read",
        true,
        "Measure cache usage"
    ),
    capability!(
        "storage",
        "clear_models_command",
        "destructive",
        false,
        "Delete downloaded models"
    ),
    capability!(
        "storage",
        "clear_all",
        "destructive",
        false,
        "Delete generated cache and models"
    ),
    capability!(
        "library",
        "trigger_scan",
        "mutation",
        false,
        "Start a library scan"
    ),
    capability!(
        "library",
        "set_library_source",
        "mutation",
        false,
        "Replace watched folders"
    ),
    capability!(
        "library",
        "add_library_folder",
        "mutation",
        false,
        "Add a watched folder"
    ),
    capability!(
        "library",
        "remove_library_folder",
        "mutation",
        false,
        "Remove a watched folder"
    ),
    capability!(
        "library",
        "list_library_folder",
        "read",
        true,
        "Browse an authorized folder"
    ),
    capability!(
        "library",
        "open_library_entry",
        "external",
        false,
        "Open media with the OS"
    ),
    capability!(
        "library",
        "reveal_library_entry",
        "external",
        false,
        "Reveal media in the OS file manager"
    ),
    capability!(
        "analysis",
        "open_artifact_entry",
        "external",
        false,
        "Open an analysis artifact revision with the OS -- path resolved and confined to the cache root"
    ),
    capability!(
        "analysis",
        "reveal_artifact_entry",
        "external",
        false,
        "Reveal an analysis artifact revision in the OS file manager -- path resolved and confined to the cache root"
    ),
    capability!(
        "storage",
        "open_export_folder",
        "external",
        false,
        "Open the configured export folder with the OS"
    ),
    capability!(
        "library",
        "clear_library_source",
        "destructive",
        false,
        "Disconnect folders and clear the index"
    ),
    capability!("library", "load_songs", "read", true, "Query songs"),
    capability!(
        "library",
        "load_song_by_hash",
        "read",
        true,
        "Load one song"
    ),
    capability!(
        "library",
        "load_songs_meta",
        "read",
        true,
        "Load library counters"
    ),
    capability!(
        "library",
        "load_analysis_queue",
        "read",
        true,
        "Load analysis queue state"
    ),
    capability!(
        "library",
        "load_analysis_tasks",
        "read",
        true,
        "Load named analysis tasks"
    ),
    capability!(
        "library",
        "update_song_settings",
        "mutation",
        false,
        "Persist composer/country/override BPM/background video edits from song settings"
    ),
    capability!(
        "analysis",
        "load_analysis_history",
        "read",
        true,
        "Load completed and failed analysis sessions"
    ),
    capability!(
        "analysis",
        "load_analysis_node_attempts",
        "read",
        true,
        "Load per-node attempt records (status, timing, device, fallback) for one analysis run"
    ),
    capability!(
        "analysis",
        "compare_analysis_runs",
        "read",
        true,
        "Diff per-node attempt records between two analysis runs of the same song"
    ),
    capability!(
        "analysis",
        "compare_node_attempt_with_previous_run",
        "read",
        true,
        "Diff one node's attempt against the nearest earlier analysis run of the same song"
    ),
    capability!(
        "analysis",
        "clear_analysis_history",
        "destructive",
        false,
        "Delete saved analysis session history without touching songs or generated assets"
    ),
    capability!(
        "library",
        "load_library_menu_items",
        "read",
        true,
        "Load sidebar facets"
    ),
    capability!(
        "analysis",
        "enqueue_one",
        "mutation",
        false,
        "Queue one song for analysis"
    ),
    capability!(
        "analysis",
        "enqueue_all",
        "mutation",
        false,
        "Queue a filtered set for analysis"
    ),
    capability!(
        "analysis",
        "delete_song_cache",
        "destructive",
        false,
        "Delete generated data for one song"
    ),
    capability!(
        "analysis",
        "reanalyze_transcript",
        "mutation",
        false,
        "Regenerate the transcript"
    ),
    capability!(
        "analysis",
        "reanalyze_full",
        "mutation",
        false,
        "Regenerate all analysis assets"
    ),
    capability!(
        "analysis",
        "reanalyze_pitch",
        "mutation",
        false,
        "Regenerate pitch assets"
    ),
    capability!(
        "analysis",
        "realign",
        "mutation",
        false,
        "Realign known lyrics with the selected local backend"
    ),
    capability!(
        "analysis",
        "reanalyze_force_transcribe",
        "mutation",
        false,
        "Force transcription"
    ),
    capability!(
        "analysis",
        "run_analysis_plan",
        "mutation",
        false,
        "Run an explicit target/disabled-node set through the generic per-node executor"
    ),
    capability!(
        "analysis",
        "run_analysis_node",
        "mutation",
        false,
        "Run a single node and its real upstream closure only"
    ),
    capability!(
        "analysis",
        "run_analysis_node_downstream",
        "mutation",
        false,
        "Run a node and every node that transitively consumes its output"
    ),
    capability!(
        "analysis",
        "disable_analysis_node_for_run",
        "mutation",
        false,
        "Run the default full analysis with one node turned off for this run"
    ),
    capability!(
        "analysis",
        "freeze_analysis_node_outputs_for_run",
        "mutation",
        false,
        "Force-reuse a node's current on-disk output for this run, ignoring config-driven cache invalidation"
    ),
    capability!(
        "analysis",
        "bypass_analysis_node_with_original_mix_for_run",
        "mutation",
        false,
        "Route stems.separate around for this run, using the source media directly in place of a separated vocal stem"
    ),
    capability!(
        "analysis",
        "cancel_analysis_run",
        "mutation",
        false,
        "Remove a not-yet-started song from the analysis queue; rejects if it is already running"
    ),
    capability!(
        "analysis",
        "get_analysis_graph",
        "read",
        true,
        "Load the static analysis DAG node/edge definition"
    ),
    capability!(
        "analysis",
        "preview_analysis_plan",
        "read",
        true,
        "Preview which nodes a targeted analysis request would run, reuse, freeze, or block"
    ),
    capability!(
        "analysis",
        "load_analysis_artifacts",
        "read",
        true,
        "Load every artifact revision recorded for a song"
    ),
    capability!(
        "analysis",
        "load_artifact_revisions",
        "read",
        true,
        "Load every revision of one artifact kind for a song"
    ),
    capability!(
        "analysis",
        "import_legacy_artifacts",
        "mutation",
        false,
        "Record existing cached files on disk as artifact revisions, without modifying them"
    ),
    capability!(
        "analysis",
        "set_active_artifact_revision",
        "mutation",
        false,
        "Select which artifact revision is the active one for a song and kind"
    ),
    capability!(
        "analysis",
        "delete_artifact_revision",
        "destructive",
        false,
        "Delete one artifact revision and its backing file inside the cache root"
    ),
    capability!(
        "analysis",
        "invalidate_artifact_revision",
        "destructive",
        false,
        "Mark an artifact revision as stale/wrong and clear it from being Active; the file and row are kept, unlike delete"
    ),
    capability!(
        "analysis",
        "compare_artifact_revisions",
        "read",
        true,
        "Diff two artifact revisions of the same song and kind (content, config, algorithm version, producer, size)"
    ),
    capability!(
        "analysis",
        "get_song_analysis_profile",
        "read",
        true,
        "Load a song's saved analysis parameter override, if one exists"
    ),
    capability!(
        "analysis",
        "set_song_analysis_profile",
        "mutation",
        false,
        "Save a per-song override of analysis model/algorithm/device parameters"
    ),
    capability!(
        "analysis",
        "reset_song_analysis_profile",
        "mutation",
        false,
        "Remove a song's parameter override, falling back to global defaults"
    ),
    capability!(
        "authoring",
        "replace_authored_chart_with_fresh_analysis",
        "destructive",
        false,
        "Explicitly discard the authored chart so it rebuilds from the latest analyzer output"
    ),
    capability!(
        "analysis",
        "cached_artifact_presence_for_song",
        "read",
        true,
        "Check which analysis artifacts actually exist on disk for a song"
    ),
    capability!(
        "library",
        "resolve_song_authoring_state",
        "read",
        true,
        "Resolve which single primary action a song's detail page should surface"
    ),
    capability!(
        "analysis",
        "preview_full_analysis_plan",
        "read",
        true,
        "Preview which DAG nodes a full chart-build run would run, reuse, or block for a song"
    ),
    capability!(
        "authoring",
        "shift_key",
        "mutation",
        false,
        "Render a key variant"
    ),
    capability!(
        "authoring",
        "shift_tempo",
        "mutation",
        false,
        "Render a tempo variant"
    ),
    capability!(
        "authoring",
        "migrate_analyzer_chart",
        "read",
        true,
        "Convert a legacy transcript + pitch notes pair into a VocalChartV1 document, without touching disk"
    ),
    capability!("lyrics", "load_lyrics", "read", true, "Load local lyrics"),
    capability!(
        "lyrics",
        "search_lrclib_lyrics",
        "external",
        false,
        "Search LRCLIB"
    ),
    capability!(
        "lyrics",
        "save_lyrics",
        "mutation",
        false,
        "Save edited lyrics"
    ),
    capability!(
        "lyrics",
        "provide_lrc",
        "mutation",
        false,
        "Import an LRC file"
    ),
    capability!(
        "lyrics",
        "apply_timed_lyrics",
        "mutation",
        false,
        "Apply timed lyrics"
    ),
    capability!(
        "export",
        "export_utz",
        "mutation",
        true,
        "Export a validated UTZ package"
    ),
    capability!(
        "export",
        "export_ultrastar",
        "mutation",
        true,
        "Export an UltraStar text bundle"
    ),
    capability!(
        "export",
        "export_all_utz",
        "mutation",
        true,
        "Export every authoring-ready chart as an atomic UTZ package"
    ),
    capability!(
        "export",
        "export_all_ultrastar",
        "mutation",
        true,
        "Export every authoring-ready chart as an atomic UltraStar bundle"
    ),
    capability!(
        "editor",
        "chart_readiness",
        "read",
        true,
        "Check editor prerequisites"
    ),
    capability!(
        "editor",
        "load_chart",
        "read",
        true,
        "Load editable chart data"
    ),
    capability!(
        "editor",
        "load_chart_audio",
        "read",
        true,
        "Stream local chart audio bytes"
    ),
    capability!(
        "editor audio",
        "editor_audio_load",
        "mutation",
        true,
        "Load chart audio into the native player"
    ),
    capability!(
        "editor audio",
        "editor_audio_play",
        "mutation",
        false,
        "Start native chart audition playback"
    ),
    capability!(
        "editor audio",
        "editor_audio_pause",
        "mutation",
        false,
        "Pause native chart audition playback"
    ),
    capability!(
        "editor audio",
        "editor_audio_seek",
        "mutation",
        false,
        "Seek native chart audition playback"
    ),
    capability!(
        "editor audio",
        "editor_audio_status",
        "read",
        true,
        "Read native chart audition position and state"
    ),
    capability!(
        "editor audio",
        "editor_audio_stop",
        "mutation",
        false,
        "Release the native chart audition pipeline"
    ),
    capability!(
        "library audio",
        "library_audio_load",
        "mutation",
        true,
        "Load an indexed source song into the native library player"
    ),
    capability!(
        "library audio",
        "library_audio_play",
        "mutation",
        false,
        "Start native library playback"
    ),
    capability!(
        "library audio",
        "library_audio_pause",
        "mutation",
        false,
        "Pause native library playback"
    ),
    capability!(
        "library audio",
        "library_audio_seek",
        "mutation",
        false,
        "Seek native library playback"
    ),
    capability!(
        "library audio",
        "library_audio_volume",
        "mutation",
        false,
        "Set native library playback volume"
    ),
    capability!(
        "library audio",
        "library_audio_queue",
        "mutation",
        false,
        "Navigate and inspect the native library playback queue"
    ),
    capability!(
        "library audio",
        "library_audio_playback_options",
        "mutation",
        false,
        "Set native library shuffle and repeat behavior"
    ),
    capability!(
        "library audio",
        "library_audio_status",
        "read",
        true,
        "Read native library playback position and state"
    ),
    capability!(
        "library audio",
        "library_audio_stop",
        "mutation",
        false,
        "Release the native library playback pipeline"
    ),
    capability!(
        "editor",
        "save_vocal_chart",
        "mutation",
        false,
        "Persist the edited vocal chart"
    ),
    capability!(
        "editor",
        "editor_actions",
        "read",
        true,
        "List the editor command registry with its key chords"
    ),
    capability!(
        "authoring",
        "load_transcript",
        "read",
        true,
        "Load the active transcript"
    ),
    capability!(
        "models",
        "analysis_runtime_status",
        "read",
        true,
        "Inspect tools, models, and backend"
    ),
    capability!(
        "models",
        "trigger_setup",
        "external",
        false,
        "Install the shared runtime or one explicitly confirmed model family"
    ),
    capability!(
        "diagnostics",
        "api_capabilities",
        "read",
        true,
        "List feature API contracts"
    ),
    capability!(
        "diagnostics",
        "run_feature_diagnostics",
        "temporary",
        true,
        "Run non-destructive feature checks"
    ),
];

pub fn api_capabilities() -> &'static [ApiCapability] {
    API_CAPABILITIES
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn catalogue_has_unique_commands_and_known_access_classes() {
        let commands = API_CAPABILITIES
            .iter()
            .map(|capability| capability.command)
            .collect::<BTreeSet<_>>();
        assert_eq!(commands.len(), API_CAPABILITIES.len());
        assert!(API_CAPABILITIES.iter().all(|capability| matches!(
            capability.access,
            "read" | "mutation" | "destructive" | "external" | "temporary"
        )));
    }
}
