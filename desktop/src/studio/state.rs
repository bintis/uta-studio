use app_core::{
    AnalysisRunHistory, AnalysisTask, AppConfig, LibraryMenuFilters, LibraryMenuItems,
    LoadSongsParams, Song, SongsMeta, SongsStore,
};
use bevy::{
    ecs::system::SystemParam,
    prelude::{Res, Resource, Vec2},
};

use super::{
    ANALYSIS_GRAPH_ZOOM_DEFAULT, AnalysisLogViewerState, AnalysisNodeContextMenu, CacheClearScope,
    DocumentationState, EditorDockSelectKind, FolderBrowser, LibraryFacet, LibraryPlayback,
    LibrarySelectKind, LibraryView, NativeEditor, NativeEditorLoadJob, NativeExportJob,
    NativeLanguageEditor, NativeLyricsEditor, NativeLyricsSearchJob, NativeLyricsWaveformJob,
    NativeSongSettings, PendingLeave, PlanPreviewDraft, SettingsSelectKind, SettingsTab,
    SetupRequest, SongContextMenu, StudioRoute, build_analysis_node_context_menu, load_songs,
};

#[derive(Resource)]
pub(crate) struct ShellState {
    pub(crate) config: AppConfig,
    pub(crate) route: StudioRoute,
    pub(crate) documentation: DocumentationState,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) notice: Option<String>,
    pub(crate) settings_scroll_offsets: [f32; 4],
}

#[derive(Resource)]
pub(crate) struct LibraryState {
    pub(crate) meta: SongsMeta,
    pub(crate) songs: SongsStore,
    pub(crate) scanning: bool,
    pub(crate) library_view: LibraryView,
    pub(crate) library_search: Option<String>,
    pub(crate) library_status: Option<String>,
    pub(crate) library_transcript_source: Option<String>,
    pub(crate) library_facet: Option<LibraryFacet>,
    pub(crate) menu_items: LibraryMenuItems,
    pub(crate) selected_song: Option<String>,
    pub(crate) folder_browser: FolderBrowser,
    pub(crate) library_scroll_offset: f32,
}

impl LibraryState {
    pub(crate) fn refresh(&mut self) {
        self.meta = SongsStore::load_meta();
        self.songs = load_songs(self.filters());
        self.menu_items = app_core::load_library_menu_items().unwrap_or_default();
    }

    pub(crate) fn load_more(&mut self) {
        let next = SongsStore::load(&LoadSongsParams {
            search: None,
            filters: self.filters(),
            skip: self.songs.processed.len(),
            take: 500,
        });
        self.songs.processed.extend(next.processed);
        self.songs.count = next.count;
        self.songs.processed_count = self.songs.processed.len();
    }

    pub(crate) fn filters(&self) -> LibraryMenuFilters {
        let mut filters = self.library_view.filters();
        filters.search = self.library_search.clone();
        filters.status = self.library_status.clone();
        filters.transcript_source = self.library_transcript_source.clone();
        match self.library_facet.as_ref() {
            Some(LibraryFacet::Artist { value, .. }) => filters.artist = Some(value.clone()),
            Some(LibraryFacet::Album { value, .. }) => filters.album = Some(value.clone()),
            Some(LibraryFacet::Playlist { value, .. }) => filters.playlist = Some(value.clone()),
            None => {}
        }
        filters
    }
}

#[derive(Resource)]
pub(crate) struct AnalysisUiState {
    pub(crate) analysis_graph_scroll_offset: f32,
    pub(crate) analysis_graph_vertical_scroll_offset: f32,
    pub(crate) analysis_graph_zoom: f32,
    pub(crate) analysis_graph_viewport_width: f32,
    pub(crate) analysis_graph_viewport_height: f32,
    pub(crate) analysis_graph_needs_fit: bool,
    pub(crate) analysis_graph_fit_active: bool,
    pub(crate) analysis_graph_follow_node: Option<String>,
    pub(crate) analysis_tasks: Vec<AnalysisTask>,
    pub(crate) analysis_history: Vec<AnalysisRunHistory>,
    pub(crate) selected_analysis_history: Option<i64>,
    pub(crate) selected_analysis_node: Option<String>,
    pub(crate) analysis_model_panel_open: bool,
    pub(crate) workflow: Option<app_core::StoredWorkflow>,
    /// Frozen local compile used by Processing Studio and the Advanced Graph.
    /// Rebuilt only when the workflow changes; rendering never launches Runtime
    /// Manager or recompiles the graph on every frame.
    pub(crate) workflow_snapshot: Option<app_core::WorkflowExecutionSnapshot>,
    pub(crate) selected_workflow_node: Option<app_core::WorkflowNodeId>,
    pub(crate) workflow_compile_error: Option<String>,
    pub(crate) processing_studio_scroll_offset: f32,
}

#[derive(Resource)]
pub(crate) struct EditorUiState {
    pub(crate) editor: Option<NativeEditor>,
}

#[derive(Resource)]
pub(crate) struct DialogState {
    pub(crate) song_context: Option<SongContextMenu>,
    pub(crate) analysis_node_context: Option<AnalysisNodeContextMenu>,
    pub(crate) pending_setup: Option<SetupRequest>,
    pub(crate) model_downloads_open: bool,
    pub(crate) diagnostic_report: Option<uta_studio_diagnostics::DiagnosticReport>,
    pub(crate) lyrics_editor: Option<NativeLyricsEditor>,
    pub(crate) pending_cache_delete: Option<String>,
    pub(crate) pending_chart_delete: Option<String>,
    pub(crate) pending_chart_replace: Option<String>,
    pub(crate) pending_song_removal: Option<String>,
    pub(crate) language_editor: Option<NativeLanguageEditor>,
    pub(crate) plan_preview_draft: Option<PlanPreviewDraft>,
    pub(crate) analysis_log_viewer: Option<AnalysisLogViewerState>,
    pub(crate) song_settings: Option<NativeSongSettings>,
    pub(crate) pending_cache_clear: Option<CacheClearScope>,
    pub(crate) pending_leave: Option<PendingLeave>,
    pub(crate) open_settings_select: Option<SettingsSelectKind>,
    pub(crate) open_library_select: Option<LibrarySelectKind>,
    pub(crate) export_all_open: bool,
    pub(crate) open_editor_select: Option<EditorDockSelectKind>,
    pub(crate) pending_analysis_history_clear: bool,
    pub(crate) search_open: bool,
    pub(crate) activity_open: bool,
    pub(crate) about_open: bool,
}

pub(crate) struct ModelSettingsSnapshot {
    pub(crate) runtime_status: app_core::AnalysisRuntimeStatus,
    pub(crate) runtime_models: Vec<app_core::RuntimeModelPresentation>,
    pub(crate) fusion_agent_adapter: Option<app_core::RuntimeResourceStatusWireV1>,
    pub(crate) fusion_agent_adapter_error: Option<String>,
    pub(crate) audio_catalog: app_core::AudioModelCatalogSummary,
    pub(crate) audio_catalog_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ModelSettingsJob {
    pub(crate) receiver:
        Option<std::sync::Mutex<std::sync::mpsc::Receiver<Result<ModelSettingsSnapshot, String>>>>,
    pub(crate) current: Option<ModelSettingsSnapshot>,
    pub(crate) error: Option<String>,
}

#[derive(Resource, Default)]
pub(crate) struct AsyncJobs {
    pub(crate) authoring_busy: bool,
    pub(crate) request_cache_stats_refresh: bool,
    pub(crate) request_model_settings_refresh: bool,
    pub(crate) model_settings_job: ModelSettingsJob,
    pub(crate) export_job: NativeExportJob,
    pub(crate) editor_load_job: NativeEditorLoadJob,
    pub(crate) lyrics_search_job: NativeLyricsSearchJob,
    pub(crate) lyrics_waveform_job: NativeLyricsWaveformJob,
}

#[derive(Resource, Default)]
pub(crate) struct PlaybackState {
    pub(crate) library_playback: LibraryPlayback,
}

pub(crate) struct StudioStateBundle {
    pub(crate) shell: ShellState,
    pub(crate) library: LibraryState,
    pub(crate) analysis: AnalysisUiState,
    pub(crate) editor: EditorUiState,
    pub(crate) dialogs: DialogState,
    pub(crate) jobs: AsyncJobs,
    pub(crate) playback: PlaybackState,
}

/// Borrowed mutable domain state for the top-level UI command router. Domain
/// handlers receive only the members they need after routing.
pub(crate) struct StudioStateMut<'a> {
    pub(crate) shell: &'a mut ShellState,
    pub(crate) library: &'a mut LibraryState,
    pub(crate) analysis: &'a mut AnalysisUiState,
    pub(crate) editor: &'a mut EditorUiState,
    pub(crate) dialogs: &'a mut DialogState,
    pub(crate) jobs: &'a mut AsyncJobs,
    pub(crate) playback: &'a mut PlaybackState,
}

#[derive(SystemParam)]
pub(crate) struct StudioStateRead<'w> {
    pub(crate) shell: Res<'w, ShellState>,
    pub(crate) library: Res<'w, LibraryState>,
    pub(crate) analysis: Res<'w, AnalysisUiState>,
    pub(crate) editor: Res<'w, EditorUiState>,
    pub(crate) dialogs: Res<'w, DialogState>,
    pub(crate) jobs: Res<'w, AsyncJobs>,
    pub(crate) playback: Res<'w, PlaybackState>,
}

impl StudioStateRead<'_> {
    pub(crate) fn view(&self) -> StudioSessionView<'_> {
        StudioSessionView::new(
            &self.shell,
            &self.library,
            &self.analysis,
            &self.editor,
            &self.dialogs,
            &self.jobs,
            &self.playback,
        )
    }
}

impl StudioStateBundle {
    pub(crate) fn load() -> Self {
        let mut config = AppConfig::load();
        if let Ok(percent) = std::env::var("UTA_STUDIO_DEBUG_FONT_SCALE_PERCENT")
            && let Ok(percent) = percent.parse::<u32>()
        {
            config.font_scale_percent = Some(percent.clamp(80, 140));
        }
        if let Ok(theme) = std::env::var("UTA_STUDIO_DEBUG_THEME") {
            config.dark_mode = match theme.trim().to_ascii_lowercase().as_str() {
                "dark" => Some(true),
                "light" => Some(false),
                _ => config.dark_mode,
            };
        }
        let folder_browser = FolderBrowser::new(&config);
        Self {
            shell: ShellState {
                config,
                route: StudioRoute::Library,
                documentation: DocumentationState::default(),
                settings_tab: SettingsTab::General,
                notice: None,
                settings_scroll_offsets: [0.0; 4],
            },
            library: LibraryState {
                meta: SongsStore::load_meta(),
                songs: load_songs(LibraryView::All.filters()),
                scanning: false,
                library_view: LibraryView::All,
                library_search: None,
                library_status: None,
                library_transcript_source: None,
                library_facet: None,
                menu_items: app_core::load_library_menu_items().unwrap_or_default(),
                selected_song: None,
                folder_browser,
                library_scroll_offset: 0.0,
            },
            analysis: AnalysisUiState {
                analysis_graph_scroll_offset: 0.0,
                analysis_graph_vertical_scroll_offset: 0.0,
                analysis_graph_zoom: ANALYSIS_GRAPH_ZOOM_DEFAULT,
                analysis_graph_viewport_width: 0.0,
                analysis_graph_viewport_height: 0.0,
                analysis_graph_needs_fit: true,
                analysis_graph_fit_active: true,
                analysis_graph_follow_node: None,
                analysis_tasks: app_core::load_analysis_tasks(),
                analysis_history: app_core::load_analysis_history(100),
                selected_analysis_history: None,
                selected_analysis_node: None,
                analysis_model_panel_open: false,
                workflow: None,
                workflow_snapshot: None,
                selected_workflow_node: None,
                workflow_compile_error: None,
                processing_studio_scroll_offset: 0.0,
            },
            editor: EditorUiState { editor: None },
            dialogs: DialogState {
                song_context: None,
                analysis_node_context: None,
                pending_setup: None,
                model_downloads_open: false,
                diagnostic_report: None,
                lyrics_editor: None,
                pending_cache_delete: None,
                pending_chart_delete: None,
                pending_chart_replace: None,
                pending_song_removal: None,
                language_editor: None,
                plan_preview_draft: None,
                analysis_log_viewer: None,
                song_settings: None,
                pending_cache_clear: None,
                pending_leave: None,
                open_settings_select: None,
                open_library_select: None,
                export_all_open: false,
                open_editor_select: None,
                pending_analysis_history_clear: false,
                search_open: false,
                activity_open: false,
                about_open: false,
            },
            jobs: AsyncJobs::default(),
            playback: PlaybackState::default(),
        }
        .with_debug_navigation()
    }

    fn with_debug_navigation(mut self) -> Self {
        if let Ok(tab) = std::env::var("UTA_STUDIO_DEBUG_OPEN_SETTINGS") {
            self.shell.route = StudioRoute::Settings;
            self.shell.settings_tab = match tab.trim().to_ascii_lowercase().as_str() {
                "storage" => SettingsTab::Storage,
                "models" | "models-runtime" | "runtime" => SettingsTab::Models,
                "analysis" => SettingsTab::Analysis,
                _ => SettingsTab::General,
            };
        }
        if let Ok(offset) = std::env::var("UTA_STUDIO_DEBUG_SETTINGS_SCROLL")
            && let Ok(offset) = offset.parse::<f32>()
        {
            self.shell.settings_scroll_offsets[self.shell.settings_tab.index()] = offset.max(0.0);
        }
        if let Ok(hash) = std::env::var("UTA_STUDIO_DEBUG_OPEN_SONG") {
            self.library.selected_song = Some(hash);
            self.shell.route = StudioRoute::SongDetail;
        }
        if std::env::var("UTA_STUDIO_DEBUG_OPEN_ACTIVITY").is_ok() {
            self.dialogs.activity_open = true;
        }
        if let Ok(id) = std::env::var("UTA_STUDIO_DEBUG_OPEN_HISTORY")
            && let Ok(id) = id.parse::<i64>()
        {
            self.analysis.selected_analysis_history = Some(id);
            self.shell.route = StudioRoute::Library;
            self.library.library_view = LibraryView::Queue;
        }
        if let Ok(hash) = std::env::var("UTA_STUDIO_DEBUG_SYNC_ARTIFACTS") {
            let _ = app_core::import_legacy_artifacts(&app_core::CacheDir::new(), &hash);
        }
        if let Ok(node) = std::env::var("UTA_STUDIO_DEBUG_SELECT_NODE") {
            self.analysis.selected_analysis_node = Some(node);
        }
        if std::env::var("UTA_STUDIO_DEBUG_OPEN_INSPECT").is_ok() {
            self.shell.route = StudioRoute::AnalysisInspect;
            self.library.library_view = LibraryView::Queue;
        }
        if let Ok(offset) = std::env::var("UTA_STUDIO_DEBUG_SCROLL_OFFSET")
            && let Ok(offset) = offset.parse::<f32>()
        {
            self.analysis.analysis_graph_scroll_offset = offset;
        }
        if let Ok(zoom) = std::env::var("UTA_STUDIO_DEBUG_GRAPH_ZOOM")
            && let Ok(zoom) = zoom.parse::<f32>()
        {
            self.analysis.analysis_graph_zoom = zoom;
            self.analysis.analysis_graph_needs_fit = false;
            self.analysis.analysis_graph_fit_active = false;
        }
        if std::env::var("UTA_STUDIO_DEBUG_OPEN_MODEL_PANEL").is_ok() {
            self.analysis.analysis_model_panel_open = true;
        }
        if let Ok(node_id) = std::env::var("UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT")
            && let Some(history) = self.analysis.selected_analysis_history.and_then(|id| {
                self.analysis
                    .analysis_history
                    .iter()
                    .find(|history| history.id == id)
            })
        {
            let capability_id = node_id.clone();
            let label = node_id.clone();
            self.dialogs.analysis_node_context = Some(build_analysis_node_context_menu(
                &node_id,
                &capability_id,
                &label,
                &history.file_hash,
                Some(history.id),
                Vec2::new(420.0, 40.0),
            ));
        }
        self
    }
}

/// Read-only aggregate used only at the render composition boundary. Runtime
/// systems mutate the domain resources directly; this view prevents render
/// helpers from regaining a monolithic mutable session.
pub(crate) struct StudioSessionView<'a> {
    pub(crate) config: &'a AppConfig,
    pub(crate) meta: &'a SongsMeta,
    pub(crate) songs: &'a SongsStore,
    pub(crate) scanning: bool,
    pub(crate) route: StudioRoute,
    pub(crate) documentation: &'a DocumentationState,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) library_view: LibraryView,
    pub(crate) library_search: &'a Option<String>,
    pub(crate) library_status: &'a Option<String>,
    pub(crate) library_transcript_source: &'a Option<String>,
    pub(crate) library_facet: &'a Option<LibraryFacet>,
    pub(crate) menu_items: &'a LibraryMenuItems,
    pub(crate) notice: &'a Option<String>,
    pub(crate) selected_song: &'a Option<String>,
    pub(crate) editor: &'a Option<NativeEditor>,
    pub(crate) folder_browser: &'a FolderBrowser,
    pub(crate) song_context: &'a Option<SongContextMenu>,
    pub(crate) analysis_node_context: &'a Option<AnalysisNodeContextMenu>,
    pub(crate) pending_setup: Option<SetupRequest>,
    pub(crate) model_downloads_open: bool,
    pub(crate) diagnostic_report: &'a Option<uta_studio_diagnostics::DiagnosticReport>,
    pub(crate) lyrics_editor: &'a Option<NativeLyricsEditor>,
    pub(crate) pending_cache_delete: &'a Option<String>,
    pub(crate) pending_chart_delete: &'a Option<String>,
    pub(crate) pending_chart_replace: &'a Option<String>,
    pub(crate) pending_song_removal: &'a Option<String>,
    pub(crate) language_editor: &'a Option<NativeLanguageEditor>,
    pub(crate) plan_preview_draft: &'a Option<PlanPreviewDraft>,
    pub(crate) analysis_log_viewer: &'a Option<AnalysisLogViewerState>,
    pub(crate) song_settings: &'a Option<NativeSongSettings>,
    pub(crate) pending_cache_clear: Option<CacheClearScope>,
    pub(crate) pending_leave: Option<PendingLeave>,
    pub(crate) open_settings_select: Option<SettingsSelectKind>,
    pub(crate) settings_scroll_offsets: [f32; 4],
    pub(crate) model_settings_job: &'a ModelSettingsJob,
    pub(crate) model_settings_refresh_pending: bool,
    pub(crate) library_scroll_offset: f32,
    pub(crate) analysis_graph_scroll_offset: f32,
    pub(crate) analysis_graph_vertical_scroll_offset: f32,
    pub(crate) analysis_graph_zoom: f32,
    pub(crate) analysis_graph_viewport_width: f32,
    pub(crate) analysis_graph_viewport_height: f32,
    pub(crate) open_library_select: Option<LibrarySelectKind>,
    pub(crate) export_all_open: bool,
    pub(crate) open_editor_select: Option<EditorDockSelectKind>,
    pub(crate) analysis_tasks: &'a [AnalysisTask],
    pub(crate) analysis_history: &'a [AnalysisRunHistory],
    pub(crate) selected_analysis_history: Option<i64>,
    pub(crate) selected_analysis_node: &'a Option<String>,
    pub(crate) analysis_model_panel_open: bool,
    pub(crate) workflow: &'a Option<app_core::StoredWorkflow>,
    pub(crate) workflow_snapshot: &'a Option<app_core::WorkflowExecutionSnapshot>,
    pub(crate) selected_workflow_node: &'a Option<app_core::WorkflowNodeId>,
    pub(crate) workflow_compile_error: &'a Option<String>,
    pub(crate) processing_studio_scroll_offset: f32,
    pub(crate) pending_analysis_history_clear: bool,
    pub(crate) search_open: bool,
    pub(crate) activity_open: bool,
    pub(crate) about_open: bool,
    pub(crate) library_playback: &'a LibraryPlayback,
    pub(crate) export_job: &'a NativeExportJob,
    pub(crate) editor_load_job: &'a NativeEditorLoadJob,
}

impl<'a> StudioSessionView<'a> {
    pub(crate) fn new(
        shell: &'a ShellState,
        library: &'a LibraryState,
        analysis: &'a AnalysisUiState,
        editor: &'a EditorUiState,
        dialogs: &'a DialogState,
        jobs: &'a AsyncJobs,
        playback: &'a PlaybackState,
    ) -> Self {
        Self {
            config: &shell.config,
            meta: &library.meta,
            songs: &library.songs,
            scanning: library.scanning,
            route: shell.route,
            documentation: &shell.documentation,
            settings_tab: shell.settings_tab,
            library_view: library.library_view,
            library_search: &library.library_search,
            library_status: &library.library_status,
            library_transcript_source: &library.library_transcript_source,
            library_facet: &library.library_facet,
            menu_items: &library.menu_items,
            notice: &shell.notice,
            selected_song: &library.selected_song,
            editor: &editor.editor,
            folder_browser: &library.folder_browser,
            song_context: &dialogs.song_context,
            analysis_node_context: &dialogs.analysis_node_context,
            pending_setup: dialogs.pending_setup,
            model_downloads_open: dialogs.model_downloads_open,
            diagnostic_report: &dialogs.diagnostic_report,
            lyrics_editor: &dialogs.lyrics_editor,
            pending_cache_delete: &dialogs.pending_cache_delete,
            pending_chart_delete: &dialogs.pending_chart_delete,
            pending_chart_replace: &dialogs.pending_chart_replace,
            pending_song_removal: &dialogs.pending_song_removal,
            language_editor: &dialogs.language_editor,
            plan_preview_draft: &dialogs.plan_preview_draft,
            analysis_log_viewer: &dialogs.analysis_log_viewer,
            song_settings: &dialogs.song_settings,
            pending_cache_clear: dialogs.pending_cache_clear,
            pending_leave: dialogs.pending_leave.clone(),
            open_settings_select: dialogs.open_settings_select,
            settings_scroll_offsets: shell.settings_scroll_offsets,
            model_settings_job: &jobs.model_settings_job,
            model_settings_refresh_pending: jobs.request_model_settings_refresh,
            library_scroll_offset: library.library_scroll_offset,
            analysis_graph_scroll_offset: analysis.analysis_graph_scroll_offset,
            analysis_graph_vertical_scroll_offset: analysis.analysis_graph_vertical_scroll_offset,
            analysis_graph_zoom: analysis.analysis_graph_zoom,
            analysis_graph_viewport_width: analysis.analysis_graph_viewport_width,
            analysis_graph_viewport_height: analysis.analysis_graph_viewport_height,
            open_library_select: dialogs.open_library_select,
            export_all_open: dialogs.export_all_open,
            open_editor_select: dialogs.open_editor_select,
            analysis_tasks: &analysis.analysis_tasks,
            analysis_history: &analysis.analysis_history,
            selected_analysis_history: analysis.selected_analysis_history,
            selected_analysis_node: &analysis.selected_analysis_node,
            analysis_model_panel_open: analysis.analysis_model_panel_open,
            workflow: &analysis.workflow,
            workflow_snapshot: &analysis.workflow_snapshot,
            selected_workflow_node: &analysis.selected_workflow_node,
            workflow_compile_error: &analysis.workflow_compile_error,
            processing_studio_scroll_offset: analysis.processing_studio_scroll_offset,
            pending_analysis_history_clear: dialogs.pending_analysis_history_clear,
            search_open: dialogs.search_open,
            activity_open: dialogs.activity_open,
            about_open: dialogs.about_open,
            library_playback: &playback.library_playback,
            export_job: &jobs.export_job,
            editor_load_job: &jobs.editor_load_job,
        }
    }

    pub(crate) fn selected_song(&self) -> Option<Song> {
        let hash = self.selected_song.as_deref()?;
        self.songs
            .processed
            .iter()
            .find(|song| song.file_hash == hash)
            .cloned()
            .or_else(|| app_core::load_song_by_hash(hash).ok().flatten())
    }

    pub(crate) fn library_title(&self) -> &str {
        self.library_facet
            .as_ref()
            .map(LibraryFacet::label)
            .unwrap_or_else(|| self.library_view.title())
    }
}
