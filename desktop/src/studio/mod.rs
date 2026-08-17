use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use app_core::{
    AppConfig, LibraryFolderEntry, LibraryMenuFilters, LibrarySource, LoadSongsParams, Song,
    SongsMeta, SongsStore,
};
use bevy::{
    asset::RenderAssetUsages,
    color::Mix,
    ecs::system::SystemParam,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    input_focus::{
        AutoFocus, FocusCause, InputFocus, InputFocusVisible,
        tab_navigation::{NavAction, TabGroup, TabIndex, TabNavigation, TabNavigationPlugin},
    },
    log::{DEFAULT_FILTER, LogPlugin},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    window::{MonitorSelection, PrimaryWindow, WindowMode, WindowTheme},
};

use crate::theme::StudioTheme;
mod analysis;
mod analysis_layout;
mod analysis_model;
mod editor;
mod folders;
mod i18n;
mod library;
mod settings;
mod song_detail;
mod song_settings;
mod widgets;

use self::analysis::*;

pub(crate) use analysis_layout::*;
pub(crate) use analysis_model::*;
pub(crate) use editor::*;
pub(crate) use folders::*;
pub(crate) use i18n::*;
pub(crate) use library::*;
pub(crate) use settings::*;
pub(crate) use song_detail::*;
pub(crate) use song_settings::*;
pub(crate) use widgets::*;

const FONT_PATH: &str = "desktop/assets/fonts/NotoSansCJKsc-Regular.otf";

const LOGO_PATH: &str = "icon.png";

/// Baked into the binary (see `setup`'s `BrandImages`) rather than loaded
/// via `AssetServer` like `LOGO_PATH` -- neither needs to be user-replaceable
/// at runtime, and embedding means one less file the packaged build has to
/// carry and locate correctly.
const LOGO_BYTES: &[u8] = include_bytes!("../../../icon.png");

const BANNER_BYTES: &[u8] = include_bytes!("../../../Banner.png");

/// Decoded once in `setup` from `LOGO_BYTES`/`BANNER_BYTES` and reused by
/// every `rebuild_ui` pass after that, the same "decode once, hand out
/// cheap `Handle` clones" shape `LocalImages` already uses for cover art.
#[derive(Resource, Clone)]
struct BrandImages {
    logo: Handle<Image>,
    banner: Handle<Image>,
}

fn decode_embedded_png(bytes: &[u8], images: &mut Assets<Image>) -> Handle<Image> {
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .expect("brand PNGs embedded at compile time are always well-formed");
    images.add(image)
}

const SIDEBAR_WIDTH: f32 = 265.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StudioRoute {
    #[default]
    Library,
    Folders,
    SongDetail,
    Settings,
    Editor,
}

#[derive(Resource)]
pub(crate) struct StudioSession {
    config: AppConfig,
    meta: SongsMeta,
    songs: SongsStore,
    scanning: bool,
    route: StudioRoute,
    settings_tab: SettingsTab,
    library_view: LibraryView,
    library_search: Option<String>,
    library_status: Option<String>,
    library_transcript_source: Option<String>,
    library_facet: Option<LibraryFacet>,
    menu_items: app_core::LibraryMenuItems,
    notice: Option<String>,
    selected_song: Option<String>,
    editor: Option<NativeEditor>,
    folder_browser: FolderBrowser,
    song_context: Option<SongContextMenu>,
    analysis_node_context: Option<AnalysisNodeContextMenu>,
    pending_setup: Option<SetupRequest>,
    diagnostic_report: Option<uta_studio_diagnostics::DiagnosticReport>,
    lyrics_editor: Option<NativeLyricsEditor>,
    pending_cache_delete: Option<String>,
    pending_artifact_delete: Option<app_core::ArtifactRevision>,
    /// Phase 6 `invalidate_artifact_revision` / Phase 7 §7.6 "Invalidate":
    /// destructive-classified, so it goes through the same
    /// request/cancel/confirm flow as `pending_artifact_delete` even though
    /// (unlike Delete) it never removes the file.
    pending_artifact_invalidate: Option<app_core::ArtifactRevision>,
    /// Phase 5 §5.4 "Replace 必须经过确认": file_hash of the song whose
    /// Authored Chart the user has asked (but not yet confirmed) to discard
    /// in favor of the current candidate analysis output.
    pending_chart_replace: Option<String>,
    authoring_busy: bool,
    language_editor: Option<NativeLanguageEditor>,
    /// Phase 8 "Configure for this run…" -- a draft one-run override for a
    /// single node's profile-controlled field, committed via
    /// `app_core::configure_analysis_node_for_run`.
    node_config_dialog: Option<NativeNodeConfigDialog>,
    /// Phase 7/8 Plan Preview panel: a staged, not-yet-committed
    /// disabled-node combination, previewed live via
    /// `app_core::preview_analysis_plan_for_selection` and committed via
    /// `run_analysis_plan` only when the user explicitly runs it.
    plan_preview_draft: Option<PlanPreviewDraft>,
    /// §7.5 "View logs" -- which node's context menu it was opened from.
    app_log_viewer: Option<AppLogViewerState>,
    song_settings: Option<NativeSongSettings>,
    pending_cache_clear: Option<CacheClearScope>,
    pending_leave: Option<PendingLeave>,
    open_settings_select: Option<SettingsSelectKind>,
    open_analysis_advanced: Option<AnalysisAdvancedSection>,
    settings_scroll_offsets: [f32; 4],
    library_scroll_offset: f32,
    analysis_graph_scroll_offset: f32,
    /// DAG canvas zoom (§7.8/§9.3 "DAG 支持 Pan、Zoom、Fit"). 1.0 is
    /// unscaled; clamped to `ANALYSIS_GRAPH_ZOOM_RANGE`. Applied by scaling
    /// the already-computed layout rects before spawning node/edge boxes
    /// (`spawn_analysis_session_overview`), not via a visual-only UI
    /// transform, so the scroll viewport's real content size -- and
    /// therefore panning and click hit-testing -- stays correct at any
    /// zoom level instead of drifting out of sync with what's drawn.
    analysis_graph_zoom: f32,
    open_library_select: Option<LibrarySelectKind>,
    export_all_open: bool,
    open_editor_select: Option<EditorDockSelectKind>,
    /// Whether the track strip is pinned open. A multi-track chart always
    /// shows it, because the active track decides what an edit touches.
    editor_tracks_open: bool,
    analysis_tasks: Vec<app_core::AnalysisTask>,
    analysis_history: Vec<app_core::AnalysisRunHistory>,
    selected_analysis_history: Option<i64>,
    selected_analysis_stage: Option<String>,
    /// §7.3 "Music Analysis 支持展开": which compound nodes currently show
    /// their children as separate boxes instead of one collapsed box with a
    /// "N sub-checks not shown" note. Toggled from the Node Context Menu
    /// (`UiAction::ToggleAnalysisCompoundNode`) -- `analysis_model.rs`'s
    /// `build_graph_view_model` has taken this set as a parameter since
    /// Phase 7 landed, it just always got an empty one until this field
    /// existed to feed it something real.
    expanded_compound_nodes: std::collections::BTreeSet<app_core::AnalysisNodeId>,
    /// Mutually exclusive with the full DAG canvas -- toggled by the "VIEW"
    /// row's MINI/Full button (`UiAction::ToggleAnalysisMiniView`). While on,
    /// the graph is built as if `expanded_compound_nodes` were empty, so
    /// only the top-level, model-backed nodes render regardless of what's
    /// individually expanded in the full view.
    analysis_mini_view: bool,
    pending_analysis_history_clear: bool,
    request_cache_stats_refresh: bool,
    search_open: bool,
    activity_open: bool,
    about_open: bool,
    library_playback: LibraryPlayback,
    export_job: NativeExportJob,
    editor_load_job: NativeEditorLoadJob,
    lyrics_search_job: NativeLyricsSearchJob,
}

impl StudioSession {
    fn load() -> Self {
        let config = AppConfig::load();
        let folder_browser = FolderBrowser::new(&config);
        Self {
            config,
            meta: SongsStore::load_meta(),
            songs: load_songs(LibraryView::All.filters()),
            scanning: false,
            route: StudioRoute::Library,
            settings_tab: SettingsTab::General,
            library_view: LibraryView::All,
            library_search: None,
            library_status: None,
            library_transcript_source: None,
            library_facet: None,
            menu_items: app_core::load_library_menu_items().unwrap_or_default(),
            notice: None,
            selected_song: None,
            editor: None,
            folder_browser,
            song_context: None,
            analysis_node_context: None,
            pending_setup: None,
            diagnostic_report: None,
            lyrics_editor: None,
            pending_cache_delete: None,
            pending_artifact_delete: None,
            pending_artifact_invalidate: None,
            pending_chart_replace: None,
            authoring_busy: false,
            language_editor: None,
            node_config_dialog: None,
            plan_preview_draft: None,
            app_log_viewer: None,
            song_settings: None,
            pending_cache_clear: None,
            pending_leave: None,
            open_settings_select: None,
            open_analysis_advanced: None,
            settings_scroll_offsets: [0.0; 4],
            library_scroll_offset: 0.0,
            analysis_graph_scroll_offset: 0.0,
            analysis_graph_zoom: 1.0,
            open_library_select: None,
            export_all_open: false,
            open_editor_select: None,
            editor_tracks_open: false,
            analysis_tasks: app_core::load_analysis_tasks(),
            analysis_history: app_core::load_analysis_history(100),
            selected_analysis_history: None,
            selected_analysis_stage: None,
            expanded_compound_nodes: std::collections::BTreeSet::new(),
            analysis_mini_view: false,
            pending_analysis_history_clear: false,
            request_cache_stats_refresh: false,
            search_open: false,
            activity_open: false,
            about_open: false,
            library_playback: LibraryPlayback::default(),
            export_job: NativeExportJob::default(),
            editor_load_job: NativeEditorLoadJob::default(),
            lyrics_search_job: NativeLyricsSearchJob::default(),
        }
        .with_debug_navigation()
    }

    /// Dev-only startup navigation for visually verifying UI changes with a
    /// real screenshot tool instead of guessing from source. Inert unless
    /// one of these env vars is explicitly set, so it can never affect a
    /// real user's session. `UTA_STUDIO_DEBUG_OPEN_SONG=<file_hash>` opens
    /// Song Detail for that song; `UTA_STUDIO_DEBUG_OPEN_ACTIVITY=1` opens
    /// the Activity Center (the DAG canvas panel) on top of whatever route
    /// that resolves to.
    fn with_debug_navigation(mut self) -> Self {
        if let Ok(hash) = std::env::var("UTA_STUDIO_DEBUG_OPEN_SONG") {
            self.selected_song = Some(hash);
            self.route = StudioRoute::SongDetail;
        }
        if std::env::var("UTA_STUDIO_DEBUG_OPEN_ACTIVITY").is_ok() {
            self.activity_open = true;
        }
        if let Ok(id) = std::env::var("UTA_STUDIO_DEBUG_OPEN_HISTORY")
            && let Ok(id) = id.parse::<i64>()
        {
            // The DAG canvas (`spawn_analysis_session_overview`) only
            // renders on the Analysis Queue library view
            // (`library.rs`'s `LibraryView::Queue` branch) -- it is not
            // part of the Activity slide-over panel, which only ever
            // lists queued jobs.
            self.selected_analysis_history = Some(id);
            self.route = StudioRoute::Library;
            self.library_view = LibraryView::Queue;
        }
        // §7.6 "Artifact Context Menu": the artifact-revision rows this
        // renders (and whatever button gating depends on `ArtifactKind`,
        // e.g. "Play audio artifact") are otherwise empty for any song
        // that has never had `import_legacy_artifacts`/"Sync from disk" run
        // for it -- this drives that same real, already-shipped action on
        // startup so the resulting UI can be screenshotted without a real
        // click.
        if let Ok(hash) = std::env::var("UTA_STUDIO_DEBUG_SYNC_ARTIFACTS") {
            let _ = app_core::import_legacy_artifacts(&app_core::CacheDir::new(), &hash);
        }
        // No Wayland input-synthesis tool is available in this sandbox to
        // drive a real node click or canvas drag (confirmed by actually
        // trying `ydotool`/`ydotoold`: it starts but cannot create its
        // virtual uinput device here, a sandbox/namespace restriction, not
        // a permissions gap -- the ACL on /dev/uinput is fine). These two
        // env vars set the same session state a click/drag would produce,
        // so the inspector panel for a non-default node and a panned
        // canvas can still be screenshotted and checked for real.
        if let Ok(stage) = std::env::var("UTA_STUDIO_DEBUG_SELECT_STAGE") {
            self.selected_analysis_stage = Some(stage);
        }
        if let Ok(offset) = std::env::var("UTA_STUDIO_DEBUG_SCROLL_OFFSET")
            && let Ok(offset) = offset.parse::<f32>()
        {
            self.analysis_graph_scroll_offset = offset;
        }
        if let Ok(zoom) = std::env::var("UTA_STUDIO_DEBUG_GRAPH_ZOOM")
            && let Ok(zoom) = zoom.parse::<f32>()
        {
            self.analysis_graph_zoom = zoom;
        }
        // §7.3 "Music Analysis 支持展开" -- same substitute-for-a-real-click
        // purpose as the other debug vars above: forces a compound node's
        // children to render as separate boxes so that can be screenshotted
        // without a real click on its context-menu toggle.
        if let Ok(node_id) = std::env::var("UTA_STUDIO_DEBUG_EXPAND_COMPOUND") {
            self.expanded_compound_nodes
                .insert(app_core::AnalysisNodeId::new(node_id));
        }
        // §7.5's Node Context Menu opens on a real secondary-click
        // (`open_analysis_node_from_click`), but no Wayland input-synthesis
        // tool in this sandbox can produce one -- see the ydotool note
        // above. This forces it open at a fixed on-screen position so it
        // can still be screenshotted and checked for real, same purpose as
        // the other `UTA_STUDIO_DEBUG_*` vars.
        if let Ok(node_id) = std::env::var("UTA_STUDIO_DEBUG_OPEN_NODE_CONTEXT")
            && let Some(history) = self
                .selected_analysis_history
                .and_then(|id| self.analysis_history.iter().find(|h| h.id == id))
        {
            let stage_id =
                bucket_stage_id(analysis_node_stage_index(&node_id).unwrap_or(0)).to_string();
            self.analysis_node_context = Some(AnalysisNodeContextMenu {
                node_id: node_id.clone(),
                stage_id,
                label: node_id.clone(),
                retry_action: analysis_node_retry_action(&node_id, &history.file_hash),
                run_node_only_action: UiAction::RunAnalysisNodeOnly(
                    history.file_hash.clone(),
                    node_id.clone(),
                ),
                run_downstream_action: UiAction::RunAnalysisNodeDownstream(
                    history.file_hash.clone(),
                    node_id.clone(),
                ),
                disable_node_action: app_core::node_can_be_disabled_for_run(&node_id).then(|| {
                    UiAction::DisableAnalysisNodeForRun(history.file_hash.clone(), node_id.clone())
                }),
                freeze_node_action: app_core::node_can_be_frozen_for_run(
                    &history.file_hash,
                    &node_id,
                )
                .then(|| {
                    UiAction::FreezeAnalysisNodeOutputs(history.file_hash.clone(), node_id.clone())
                }),
                bypass_node_action: app_core::node_can_be_bypassed_for_run(&node_id).then(|| {
                    UiAction::BypassAnalysisNodeWithOriginalMix(
                        history.file_hash.clone(),
                        node_id.clone(),
                    )
                }),
                compare_node_action: Some(UiAction::CompareNodeAttemptWithPrevious(
                    history.file_hash.clone(),
                    node_id.clone(),
                    history.id,
                )),
                save_as_song_profile_action: app_core::node_can_be_configured_for_run(&node_id)
                    .then(|| {
                        UiAction::SaveNodeConfigAsSongProfile(
                            history.file_hash.clone(),
                            node_id.clone(),
                        )
                    }),
                open_configure_dialog_action: app_core::node_can_be_configured_for_run(&node_id)
                    .then(|| {
                        UiAction::OpenNodeConfigDialog(history.file_hash.clone(), node_id.clone())
                    }),
                force_transcribe_action: node_can_force_transcribe(&node_id)
                    .then(|| UiAction::ForceTranscribe(history.file_hash.clone())),
                refetch_align_action: node_can_refetch_and_align(&node_id)
                    .then(|| UiAction::ReanalyzeTranscript(history.file_hash.clone())),
                view_logs_action: Some(UiAction::OpenAppLogViewer(
                    history.file_hash.clone(),
                    node_id.clone(),
                )),
                compound_toggle: analysis_node_compound_toggle_action(
                    &node_id,
                    self.expanded_compound_nodes
                        .contains(&app_core::AnalysisNodeId::new(node_id.clone())),
                ),
                position: Vec2::new(420.0, 340.0),
            });
        }
        self
    }

    fn refresh_library(&mut self) {
        self.meta = SongsStore::load_meta();
        self.songs = load_songs(self.library_filters());
        self.menu_items = app_core::load_library_menu_items().unwrap_or_default();
    }

    fn load_more_songs(&mut self) {
        let next = SongsStore::load(&LoadSongsParams {
            search: None,
            filters: self.library_filters(),
            skip: self.songs.processed.len(),
            take: 500,
        });
        self.songs.processed.extend(next.processed);
        self.songs.count = next.count;
        self.songs.processed_count = self.songs.processed.len();
    }

    fn selected_song(&self) -> Option<Song> {
        let hash = self.selected_song.as_deref()?;
        app_core::load_song_by_hash(hash).ok().flatten()
    }

    fn library_filters(&self) -> LibraryMenuFilters {
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

    fn library_title(&self) -> &str {
        self.library_facet
            .as_ref()
            .map(LibraryFacet::label)
            .unwrap_or_else(|| self.library_view.title())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingLeave {
    Exit,
    Back,
    Home,
}

#[derive(Resource)]
pub(crate) struct NativeAudio(#[allow(dead_code)] Arc<uta_studio_audio::EditorAudioPlayer>);

/// The synthesized pitch stream. It is a second player so auditioning a note
/// target never alters, mixes into, or re-encodes the song audio.
#[derive(Resource)]
pub(crate) struct NativePitchAudition(pub(crate) Arc<uta_studio_audio::PitchAudition>);

#[derive(Resource, Default)]
pub(crate) struct UiInvalidated(bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDirection {
    Previous,
    Next,
}

#[derive(Resource, Default)]
pub(crate) struct NavigationInputState {
    held_direction: Option<NavigationDirection>,
    repeat_at: Option<Instant>,
    activated: Option<Entity>,
}

#[derive(Component)]
pub(crate) struct StudioUiRoot;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) enum UiAction {
    Back,
    Home,
    SetLibraryView(LibraryView),
    SetLibraryFacet(LibraryFacet),
    LoadMoreSongs,
    ApplyLibrarySearch,
    ClearLibrarySearch,
    ToggleLibraryLayout,
    ToggleExportAllMenu,
    ExportAllUtz,
    ExportAllUltraStar,
    ToggleGlobalSearch,
    OpenLibrarySelect(LibrarySelectKind),
    SelectLibraryValue(LibrarySelectKind, String),
    AnalyzeAll,
    Folders,
    Settings,
    ToggleActivity,
    CloseActivity,
    SelectAnalysisHistory(Option<i64>),
    SelectAnalysisStage(String),
    /// Percent-point delta (e.g. +15/-15), clamped in the handler --
    /// `UiAction` derives `Eq` so this carries an integer, not the session's
    /// underlying `f32` zoom.
    AdjustAnalysisGraphZoom(i32),
    /// Mutually exclusive with the full DAG canvas -- see
    /// `StudioSession::analysis_mini_view`.
    ToggleAnalysisMiniView,
    /// Payload is the *unscaled* canvas width in px, computed fresh each
    /// render pass, so the handler can size zoom to the real current
    /// viewport without duplicating the layout algorithm.
    FitAnalysisGraph(i32),
    /// (scroll offset px, inspector stage id) for a §7.8 "Focus
    /// Current/Failed/Stale" button, computed at render time in
    /// `spawn_analysis_session_overview` from the real layout and plan.
    FocusAnalysisGraphNode(i32, String),
    DismissAnalysisNodeContext,
    RequestClearAnalysisHistory,
    CancelClearAnalysisHistory,
    ConfirmClearAnalysisHistory,
    OpenAbout,
    CloseAbout,
    SettingsTab(SettingsTab),
    ToggleFullscreen,
    OpenLog,
    RunDiagnostics,
    RefreshRuntimeStatus,
    OpenSettingsSelect(SettingsSelectKind),
    SelectSettingsValue(SettingsSelectKind, String),
    ToggleAnalysisAdvanced(AnalysisAdvancedSection),
    RequestSetup(Option<app_core::ModelDownloadTarget>),
    CancelSetup,
    ConfirmSetup,
    RescanLibrary,
    ToggleTheme,
    ChooseFolder,
    ChooseExportFolder,
    ClearExportFolder,
    SelectFolderRoot(PathBuf),
    FolderUp,
    OpenFolderEntry(PathBuf),
    RevealFolderEntry(PathBuf),
    DismissFolderContext,
    RequestRemoveFolder(PathBuf),
    CancelRemoveFolder,
    ConfirmRemoveFolder,
    AdjustBeamSize(i8),
    AdjustBatchSize(i8),
    AdjustSeparatorSegmentSize(i32),
    AdjustSeparatorOverlap(i32),
    AdjustSeparatorBatchSize(i32),
    AdjustSeparatorNormalization(i32),
    AdjustDemucsShifts(i32),
    AdjustDemucsOverlap(i32),
    AdjustUiFontScale(i8),
    ToggleAutoAnalyze,
    AdjustVocalThreshold(i8),
    RestoreAnalysisDefaults,
    RequestClearCache(CacheClearScope),
    CancelClearCache,
    ConfirmClearCache,
    OpenSong(String),
    AnalyzeSong(String),
    OpenEditor(String),
    ExportUtz(String),
    ExportUltraStar(String),
    OpenSource(PathBuf),
    RevealSource(PathBuf),
    DismissSongContext,
    OpenLyricsEditor(String),
    CloseLyricsEditor,
    ToggleLyricsInputMode,
    ToggleLyricsSeparateStems,
    SearchLrclibLyrics,
    PreviousLrclibCandidate,
    NextLrclibCandidate,
    UseLrclibPlain,
    UseLrclibTimed,
    SaveLyricsEditor,
    OpenLanguageEditor(String),
    CloseLanguageEditor,
    ToggleLanguageReprocess,
    ToggleLanguagePicker,
    SelectAnalysisLanguage(String),
    SaveLanguageEditor,
    OpenSongSettings(String),
    CloseSongSettings,
    ChooseBackgroundVideo,
    ClearBackgroundVideo,
    SaveSongSettings,
    RealignSong(String),
    ReanalyzeTranscript(String),
    ForceTranscribe(String),
    ReanalyzePitch(String),
    ReanalyzeFull(String),
    /// §7.5 "Run this node only": file_hash, node_id. Phase 4's generic
    /// `app_core::run_analysis_node` executor, not another special-cased
    /// command.
    RunAnalysisNodeOnly(String, String),
    /// §7.5 "Run this node and downstream": file_hash, node_id.
    /// `app_core::run_analysis_node_downstream`.
    RunAnalysisNodeDownstream(String, String),
    /// §7.5 "Disable for this run": file_hash, node_id.
    /// `app_core::disable_analysis_node_for_run`.
    DisableAnalysisNodeForRun(String, String),
    /// §7.5 "Freeze current outputs": file_hash, node_id. Phase 4 §4.5's
    /// Freeze consumer, `app_core::freeze_analysis_node_outputs_for_run`.
    FreezeAnalysisNodeOutputs(String, String),
    /// §7.5 "Choose bypass": file_hash, node_id. Phase 4 §4.5's Bypass
    /// consumer, `app_core::bypass_analysis_node_with_original_mix_for_run`.
    /// Not actually a chooser -- Original Mix is the only real bypass
    /// choice any node in the graph has today (only `stems.separate`
    /// qualifies at all), so there's nothing to pick between yet.
    BypassAnalysisNodeWithOriginalMix(String, String),
    /// §7.5 "Compare with previous attempt": file_hash, node_id,
    /// current_run_id. `app_core::compare_node_attempt_with_previous_run`.
    CompareNodeAttemptWithPrevious(String, String, i64),
    /// §7.5 "Save as song profile": file_hash, node_id. Fires immediately
    /// (no dialog) -- persists whatever value is currently in effect for
    /// this node's field. `app_core::save_node_config_as_song_profile`.
    SaveNodeConfigAsSongProfile(String, String),
    /// §7.5 "Configure for this run…": file_hash, node_id. Opens
    /// `NativeNodeConfigDialog` since a new value has to be picked first.
    OpenNodeConfigDialog(String, String),
    CloseNodeConfigDialog,
    ToggleNodeConfigPicker,
    SelectNodeConfigValue(String),
    /// Commits the dialog's draft value via
    /// `app_core::configure_analysis_node_for_run` and queues the run.
    RunNodeConfigDialog,
    /// Phase 7/8 Plan Preview panel: file_hash. Seeds an empty staged
    /// disabled-node set.
    OpenPlanPreview(String),
    ClosePlanPreview,
    /// node_id: flips it in/out of the draft's staged `disabled_nodes`.
    TogglePlanPreviewDisabledNode(String),
    /// Commits the draft via `app_core::run_analysis_plan` (empty targets,
    /// the default-full-run convention) and closes the panel.
    RunPlanPreviewDraft,
    /// §7.5's "View logs": file_hash, node_id.
    OpenAppLogViewer(String, String),
    CloseAppLogViewer,
    /// Opens `app_core::get_log_path()` with the OS default program. No
    /// path payload -- the target is always the one real app log file, an
    /// internally-computed path rather than anything user/library-derived,
    /// so this doesn't need `validate_cache_path`/`validate_source_path`'s
    /// boundary check the way an artifact/library path does.
    OpenAppLogFile,
    /// §7.3 "Music Analysis 支持展开": toggles whether a compound node's
    /// children render as separate boxes. node_id (always a real compound
    /// node's id, e.g. `music.analysis`).
    ToggleAnalysisCompoundNode(String),
    RequestDeleteSongCache(String),
    /// Phase 6 `app_core::cancel_analysis_run`: file_hash. Only offered for
    /// a job that's still `Queued` (not yet started) -- see that
    /// function's doc comment for why a running job can't be cancelled
    /// mid-run yet.
    CancelAnalysisRun(String),
    CancelDeleteSongCache,
    /// Phase 5 §5.4 "Compare / Merge / Replace": file_hash. Opens the
    /// confirmation modal; the actual discard only happens on
    /// `ConfirmReplaceAuthoredChart`.
    RequestReplaceAuthoredChart(String),
    CancelReplaceAuthoredChart,
    /// `app_core::replace_authored_chart_with_fresh_analysis` -- discards
    /// the Authored Chart so the next load rebuilds it from the latest
    /// analyzer output. Never called except from here, after confirmation.
    ConfirmReplaceAuthoredChart,
    ConfirmDeleteSongCache,
    /// Imports every existing cached file for a song into the Phase 2
    /// artifact revision table (read-only toward the files themselves).
    /// Explicit and user-triggered rather than run on every render, since
    /// it hashes file contents.
    SyncArtifactRevisions(String),
    SetActiveArtifactRevision(app_core::ArtifactRevision),
    OpenArtifactRevision(PathBuf),
    /// §7.6 "Preview": bounded in-app text preview, for JSON/text artifacts
    /// only (see `artifact_kind_is_playable` for the audio ones, which use
    /// "Play" instead).
    PreviewArtifactRevision(PathBuf),
    RevealArtifactRevision(PathBuf),
    RequestDeleteArtifactRevision(app_core::ArtifactRevision),
    CancelDeleteArtifactRevision,
    ConfirmDeleteArtifactRevision,
    /// Phase 6 `invalidate_artifact_revision` / Phase 7 §7.6 "Invalidate".
    RequestInvalidateArtifactRevision(app_core::ArtifactRevision),
    CancelInvalidateArtifactRevision,
    ConfirmInvalidateArtifactRevision,
    /// §7.6 "Inspect provenance": read-only, no confirmation needed --
    /// every field it shows is already on `ArtifactRevision`, this just
    /// surfaces it.
    InspectArtifactProvenance(app_core::ArtifactRevision),
    /// §7.6 "Compare revisions": revision, active_revision_id. Compares a
    /// non-active revision against whichever revision is currently Active
    /// for its song+kind -- the real, common comparison ("how does this
    /// differ from what's in use now"), not a free-form two-picker (no
    /// multi-select UI exists for artifact revisions yet).
    CompareArtifactRevisions(app_core::ArtifactRevision, String),
    CancelLeave,
    ConfirmLeave,
    ShiftSongKey(String, i8),
    ShiftSongTempo(String, i8),
    PlayLibrarySong(String),
    /// §7.6 "Play audio artifact": path to the artifact revision file.
    PlayArtifactRevision(PathBuf),
    ToggleLibraryPlayback,
    SeekLibraryRelative(i8),
    PreviousLibrarySong,
    NextLibrarySong,
    ToggleLibraryShuffle,
    CycleLibraryRepeat,
    AdjustLibraryVolume(i8),
    ToggleLibraryMute,
    ToggleLibraryQueue,
    /// A registered editor command. Every toolbar button, inspector button,
    /// and key chord routes through the one registry entry it names.
    Editor(EditorAction),
    /// Jump the playhead and viewport to a chart problem: the track it is on,
    /// and where in the timeline, in milliseconds.
    FocusChartProblem(usize, u64),
    OpenEditorSelect(EditorDockSelectKind),
    SelectEditorValue(EditorDockSelectKind, String),
    SelectEditorWord(usize, usize, u64),
    SelectEditorTrack(usize),
    MoveSelectionToTrack(usize),
    SetNoteKind(app_core::NoteKind),
    DismissLyricContext,
    DismissNoteContext,
    SelectWaveformSource(WaveformSource),
    SelectWaveformStyle(WaveformStyle),
    DismissWaveformContext,
    SetProblemsFilter(ProblemsFilter),
    ApplyAllLyricsEdit,
    ExtendLyricOverNote(WordSelection, usize),
    DismissProblemsPanel,
    DismissShortcutsPanel,
}

pub fn run() {
    let session = StudioSession::load();
    let native_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let native_library_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let theme = StudioTheme::new(session.config.dark_mode.unwrap_or(false));
    set_ui_font_scale(session.config.font_scale());
    let window = studio_window(&session.config, theme.dark);

    App::new()
        .insert_resource(ClearColor(theme.background))
        .insert_resource(theme)
        .insert_resource(session)
        .insert_resource(NativeAudio(native_audio))
        .insert_resource(NativePitchAudition(Arc::new(
            uta_studio_audio::PitchAudition::new(),
        )))
        .insert_resource(NativeLibraryAudio(native_library_audio))
        .insert_resource(LocalImages::default())
        .insert_resource(EditorPointerCapture::default())
        .insert_resource(UiInvalidated::default())
        .insert_resource(NavigationInputState::default())
        .insert_resource(LibraryRefreshTimer(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .insert_resource(AnalysisRefreshTimer(Timer::from_seconds(
            0.75,
            TimerMode::Repeating,
        )))
        .insert_resource(EditorAudioSyncTimer(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        )))
        .insert_resource(LibraryAudioSyncTimer(Timer::from_seconds(
            0.1,
            TimerMode::Repeating,
        )))
        .insert_resource(NativeSetup::default())
        .insert_resource(NativeDiagnostics::default())
        .insert_resource(NativeAuthoringJob::default())
        .insert_resource(CacheStatsJob::default())
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    // Parley 0.9 asks ICU for non-complex word segmentation even
                    // for no-wrap labels. ICU 2.2 logs that expected fallback once
                    // per CJK text node; keep real ICU errors while avoiding that
                    // misleading warning storm in the native shell.
                    filter: studio_log_filter(),
                    custom_layer: app_log_custom_layer,
                    ..default()
                })
                .set(AssetPlugin {
                    // During the transition, use the canonical repository logo
                    // and the same bundled CJK font as the current desktop UI.
                    // Keeping the source paths explicit also makes the later
                    // package asset-copy step auditable.
                    file_path: asset_root(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(window),
                    close_when_requested: false,
                    ..default()
                }),
        )
        .add_plugins(TabNavigationPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                register_navigation_targets,
                handle_accessible_navigation,
                handle_actions,
            )
                .chain(),
        )
        .add_systems(Update, handle_cache_stats_request)
        .add_systems(Update, handle_window_close_requests)
        .add_systems(Update, handle_fullscreen_shortcut)
        .add_systems(Update, refresh_library_while_scanning)
        .add_systems(Update, refresh_analysis_activity)
        .add_systems(Update, poll_native_setup)
        .add_systems(Update, poll_native_diagnostics)
        .add_systems(Update, poll_cache_stats)
        .add_systems(Update, poll_authoring_job)
        .add_systems(Update, poll_export_job)
        .add_systems(Update, poll_editor_load_job)
        .add_systems(Update, poll_lyrics_search_job)
        .add_systems(Update, sync_numeric_settings)
        .add_systems(Update, handle_tap_release)
        .add_systems(Update, sync_editor_word_input.after(rebuild_ui))
        .add_systems(Update, sync_editor_phrase_input)
        .add_systems(Update, sync_editor_singer_input)
        .add_systems(Update, finish_inline_lyric_edit)
        .add_systems(Update, handle_library_search_keyboard)
        .add_systems(Update, handle_plan_preview_keyboard)
        .add_systems(Update, handle_app_log_viewer_scroll)
        .add_systems(
            Update,
            refresh_editor_problems_cache
                .after(handle_actions)
                .after(handle_editor_pointer_capture)
                .before(rebuild_ui),
        )
        .add_systems(Update, rebuild_ui.after(handle_actions))
        .add_systems(Update, localize_ui_text.after(rebuild_ui))
        .add_systems(Update, update_button_visuals.after(rebuild_ui))
        .add_systems(
            Update,
            update_navigation_focus_visuals
                .after(register_navigation_targets)
                .after(rebuild_ui),
        )
        .add_systems(Update, handle_editor_keyboard)
        .add_systems(Update, handle_editor_wheel)
        .add_systems(Update, handle_editor_pointer_capture)
        .add_systems(Update, handle_folder_scroll)
        .add_systems(Update, handle_problems_panel_scroll)
        .add_systems(Update, handle_shortcuts_panel_scroll)
        .add_systems(Update, handle_analysis_graph_scroll)
        .add_systems(Update, handle_library_scroll)
        .add_systems(Update, handle_song_detail_scroll)
        .add_systems(Update, handle_settings_scroll)
        .add_systems(Update, sync_editor_audio)
        .add_systems(Update, sync_library_audio)
        .add_systems(Update, update_editor_geometry)
        .add_systems(Update, update_editor_playhead)
        .add_systems(Update, update_editor_binding_guides)
        .add_systems(Update, update_editor_shortcuts_panel_visibility)
        .add_systems(Update, update_library_player_ui)
        .run();
}

fn studio_log_filter() -> String {
    format!("{DEFAULT_FILTER},icu_provider=error")
}

/// Real app-log capture (Node Context Menu "View logs" -- previously the
/// last declined Phase 7 §7.5 item, since nothing captured log output
/// anywhere before this). Writes go through `tracing_subscriber::fmt`'s own
/// event formatting (reused, not reimplemented) into
/// `app_core::record_log_text`'s bounded ring buffer + best-effort log
/// file. Composes *alongside* Bevy's own default stdout layer via
/// `LogPlugin.custom_layer` -- stdout output is unaffected.
#[derive(Clone, Copy)]
struct AppLogWriter;

impl std::io::Write for AppLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(text) = std::str::from_utf8(buf) {
            app_core::record_log_text(text);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for AppLogWriter {
    type Writer = AppLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}

fn app_log_custom_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .with_writer(AppLogWriter)
            .with_ansi(false),
    ))
}

fn asset_root() -> String {
    if let Some(path) = std::env::var_os("UTA_STUDIO_ASSET_PATH") {
        return path.to_string_lossy().into_owned();
    }

    if let Ok(executable) = std::env::current_exe()
        && let Some(prefix) = executable.parent().and_then(std::path::Path::parent)
    {
        let packaged = prefix.join("share/uta-studio");
        if packaged.join(LOGO_PATH).is_file() && packaged.join(FONT_PATH).is_file() {
            return packaged.to_string_lossy().into_owned();
        }
    }

    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("desktop crate must remain inside the Uta Studio workspace")
        .to_string_lossy()
        .into_owned()
}

/// Dev-only: `WIDTHxHEIGHT`, e.g. `560x900`, for a narrow-window screenshot
/// pass (§9.3 "窄窗口无严重重叠"). Forces windowed mode at that exact size,
/// taking priority over the other debug env vars' fullscreen branch, since
/// there is no way to interactively resize a Wayland-native window in this
/// sandbox without input synthesis -- see the ydotool note in
/// docs/analysis-dag-redesign.md.
fn debug_window_size() -> Option<(u32, u32)> {
    let value = std::env::var("UTA_STUDIO_DEBUG_WINDOW_SIZE").ok()?;
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn studio_window(config: &AppConfig, dark: bool) -> Window {
    Window {
        title: "Uta Studio".to_string(),
        name: Some("com.uta-studio.desktop".to_string()),
        resolution: debug_window_size().unwrap_or((1280, 720)).into(),
        decorations: true,
        transparent: false,
        resizable: true,
        mode: if debug_window_size().is_some() {
            WindowMode::Windowed
        } else if std::env::var("UTA_STUDIO_DEBUG_OPEN_SONG").is_ok()
            || std::env::var("UTA_STUDIO_DEBUG_OPEN_ACTIVITY").is_ok()
            || std::env::var("UTA_STUDIO_DEBUG_OPEN_HISTORY").is_ok()
        {
            // Dev-only: land on the monitor the user set aside for visual
            // verification screenshots (DP-2, marked Xwayland-primary),
            // not wherever COSMIC's tiler happens to place a new window.
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        } else if config.fullscreen.unwrap_or(false) {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        },
        window_theme: Some(if dark {
            WindowTheme::Dark
        } else {
            WindowTheme::Light
        }),
        ..default()
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut local_images: ResMut<LocalImages>,
    session: Res<StudioSession>,
    native_setup: Res<NativeSetup>,
    cache_stats: Res<CacheStatsJob>,
    theme: Res<StudioTheme>,
) {
    commands.spawn(Camera2d);
    let brand = BrandImages {
        logo: decode_embedded_png(LOGO_BYTES, &mut images),
        banner: decode_embedded_png(BANNER_BYTES, &mut images),
    };
    // The very first frame, before the window (and any editor route that
    // could have a context menu open) exists — the configured default
    // resolution is a fine stand-in.
    render_ui(
        &mut commands,
        &asset_server,
        &mut images,
        &brand,
        &mut local_images,
        &session,
        &native_setup,
        &cache_stats,
        &theme,
        Vec2::new(1280.0, 720.0),
    );
    commands.insert_resource(brand);
}

// Bevy systems expose each independently tracked resource/query as a parameter.
#[allow(clippy::too_many_arguments)]
fn rebuild_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    brand: Res<BrandImages>,
    mut local_images: ResMut<LocalImages>,
    session: Res<StudioSession>,
    native_setup: Res<NativeSetup>,
    cache_stats: Res<CacheStatsJob>,
    theme: Res<StudioTheme>,
    mut invalidated: ResMut<UiInvalidated>,
    roots: Query<Entity, With<StudioUiRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if !invalidated.0 {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    let window_size = windows
        .single()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 800.0));
    render_ui(
        &mut commands,
        &asset_server,
        &mut images,
        &brand,
        &mut local_images,
        &session,
        &native_setup,
        &cache_stats,
        &theme,
        window_size,
    );
    invalidated.0 = false;
}

#[allow(clippy::too_many_arguments)]
fn render_ui(
    commands: &mut Commands,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    brand: &BrandImages,
    local_images: &mut LocalImages,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
    window_size: Vec2,
) {
    let font = asset_server.load(FONT_PATH);
    let icons = asset_server.load(ICON_ATLAS_PATH);
    commands
        .spawn((
            StudioUiRoot,
            TabGroup::new(0),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|root| {
            if session.route == StudioRoute::Editor {
                spawn_editor(
                    root,
                    font.clone(),
                    icons.clone(),
                    session,
                    theme,
                    window_size,
                );
            } else {
                root.spawn(Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    spawn_sidebar(
                        body,
                        font.clone(),
                        icons.clone(),
                        brand.banner.clone(),
                        session,
                        theme,
                    );
                    spawn_workspace(
                        body,
                        font.clone(),
                        asset_server,
                        images,
                        local_images,
                        session,
                        native_setup,
                        cache_stats,
                        icons.clone(),
                        theme,
                    );
                });
            }
            if session.activity_open {
                spawn_activity_center(root, font.clone(), icons.clone(), session, theme);
            }
            if let Some(revision) = session.pending_artifact_delete.as_ref() {
                spawn_artifact_delete_confirmation(root, font.clone(), theme, revision);
            }
            if let Some(revision) = session.pending_artifact_invalidate.as_ref() {
                spawn_artifact_invalidate_confirmation(root, font.clone(), theme, revision);
            }
            if session.about_open {
                spawn_about_dialog(root, font.clone(), brand.logo.clone(), theme);
            }
            if let Some(panel) = session.song_settings.as_ref() {
                spawn_song_settings_panel(root, font.clone(), theme, panel);
            }
            if let Some(destination) = session.pending_leave {
                spawn_leave_confirmation(root, font, theme, session, destination);
            }
        });
}

fn spawn_leave_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    destination: PendingLeave,
) {
    let dirty = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let (title, action) = match destination {
        PendingLeave::Exit => ("Close Uta Studio?", "Close"),
        PendingLeave::Back | PendingLeave::Home => ("Leave the editor?", "Leave"),
    };
    let description = if dirty {
        "This chart has unsaved edits. Leaving now discards those edits. Source media is never changed."
    } else {
        "A scan, setup, diagnostic, or rendering task is still active. Closing now interrupts that work. Source media is never changed."
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.8)),
        ZIndex(120),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(description),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelLeave,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Stay"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmLeave,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new(action),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

fn spawn_sidebar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    banner: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: px(SIDEBAR_WIDTH),
                height: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(10), px(16)),
                ..default()
            },
            BackgroundColor(theme.sidebar),
        ))
        .with_children(|sidebar| {
            sidebar
                .spawn(Node {
                    width: percent(100),
                    height: px(82),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn((
                            Button,
                            UiAction::OpenAbout,
                            Node {
                                height: px(68),
                                align_items: AlignItems::Center,
                                // Lines the wordmark's left edge up with
                                // "BROWSE"/"MY LIBRARY" below it
                                // (`spawn_section_label`'s own `left: px(8)`
                                // margin), not the sidebar's raw padding
                                // edge.
                                margin: UiRect::left(px(8)),
                                padding: UiRect::right(px(8)),
                                column_gap: px(10),
                                ..default()
                            },
                        ))
                        .with_children(|brand| {
                            spawn_text(
                                brand,
                                font.clone(),
                                "Uta! Studio",
                                18.0,
                                theme.sidebar_foreground,
                            );
                        });
                    header.spawn(Node {
                        min_width: px(4),
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon_button(
                        header,
                        icons.clone(),
                        theme,
                        UiIcon::Settings,
                        UiAction::Settings,
                        false,
                        false,
                        30.0,
                    );
                });

            spawn_section_label(sidebar, font.clone(), theme, "BROWSE");
            let analysis_count = session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                    )
                })
                .count();
            for (view, icon, label, count) in [
                (
                    LibraryView::All,
                    UiIcon::Home,
                    "All music",
                    session.meta.songs_count,
                ),
                (
                    LibraryView::Queue,
                    UiIcon::Queue,
                    "Analysis",
                    analysis_count,
                ),
                (
                    LibraryView::Completed,
                    UiIcon::CircleCheck,
                    "Completed Charts",
                    session.meta.analyzed_count,
                ),
                (
                    LibraryView::Videos,
                    UiIcon::Video,
                    "Video",
                    session.meta.videos_count,
                ),
            ] {
                spawn_sidebar_filter_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    Some(icon),
                    label,
                    count,
                    UiAction::SetLibraryView(view),
                    session.route == StudioRoute::Library && session.library_view == view,
                );
            }
            spawn_section_label(sidebar, font.clone(), theme, "MY LIBRARY");
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Artists,
                "Artists",
                UiAction::SetLibraryView(LibraryView::Artists),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Artists
                    && session.library_facet.is_none(),
            );
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Albums,
                "Albums",
                UiAction::SetLibraryView(LibraryView::Albums),
                session.route == StudioRoute::Library
                    && session.library_view == LibraryView::Albums
                    && session.library_facet.is_none(),
            );
            if !session.menu_items.playlists.is_empty() {
                spawn_sidebar_item(
                    sidebar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    UiIcon::List,
                    "Playlists",
                    0,
                );
                for item in session.menu_items.playlists.iter().take(2) {
                    let facet = LibraryFacet::Playlist {
                        value: item.value.clone(),
                        label: item.label.clone(),
                    };
                    spawn_sidebar_filter_item(
                        sidebar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        None,
                        format!("  {}", item.label),
                        item.count.try_into().unwrap_or(usize::MAX),
                        UiAction::SetLibraryFacet(facet.clone()),
                        session.route == StudioRoute::Library
                            && session.library_facet.as_ref() == Some(&facet),
                    );
                }
            }
            spawn_sidebar_nav_item(
                sidebar,
                font.clone(),
                icons.clone(),
                theme,
                UiIcon::Folder,
                "Folders",
                UiAction::Folders,
                session.route == StudioRoute::Folders,
            );
            sidebar.spawn(Node {
                min_height: px(14),
                flex_grow: 1.0,
                ..default()
            });
            sidebar.spawn((
                Button,
                UiAction::OpenAbout,
                Node {
                    width: percent(100),
                    // Banner.png is a fixed 4:3 image (1448x1086) -- sized
                    // from the sidebar's own inner width (minus its
                    // `padding: UiRect::axes(px(10), ..)`) so it fills the
                    // sidebar edge-to-edge without stretching or
                    // letterboxing.
                    height: px((SIDEBAR_WIDTH - 20.0) * 3.0 / 4.0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                ImageNode::new(banner),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_sidebar_filter_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: Option<UiIcon>,
    label: impl Into<String>,
    count: usize,
    action: UiAction,
    active: bool,
) {
    let label = label.into();
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(32),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            if let Some(icon) = icon {
                spawn_icon(
                    row,
                    icons,
                    icon,
                    15.0,
                    if active {
                        theme.primary
                    } else {
                        theme.sidebar_foreground.with_alpha(0.62)
                    },
                );
                row.spawn(Node {
                    width: px(9),
                    ..default()
                });
            }
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|label_node| {
                label_node.spawn((
                    Text::new(label),
                    ui_text_font(font.clone(), 12.0),
                    TextColor(if active {
                        theme.sidebar_foreground.with_alpha(0.68)
                    } else {
                        theme.sidebar_foreground
                    }),
                    TextLayout::no_wrap(),
                ));
            });
            if count > 0 {
                row.spawn(Node {
                    width: px(28),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|count_node| {
                    spawn_text(
                        count_node,
                        font,
                        count.to_string(),
                        10.0,
                        theme.sidebar_foreground.with_alpha(0.38),
                    );
                });
            }
        });
}

fn spawn_sidebar_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    count: usize,
) {
    parent
        .spawn(Node {
            width: percent(100),
            height: px(32),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            ..default()
        })
        .with_children(|row| {
            spawn_icon(
                row,
                icons,
                icon,
                15.0,
                theme.sidebar_foreground.with_alpha(0.58),
            );
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(row, font.clone(), label, 12.0, theme.sidebar_foreground);
            row.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if count > 0 {
                spawn_text(
                    row,
                    font,
                    count.to_string(),
                    10.0,
                    theme.sidebar_foreground.with_alpha(0.38),
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_sidebar_nav_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    icon: UiIcon,
    label: &'static str,
    action: UiAction,
    active: bool,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                height: px(32),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            spawn_icon(
                row,
                icons,
                icon,
                15.0,
                if active {
                    theme.primary
                } else {
                    theme.sidebar_foreground.with_alpha(0.62)
                },
            );
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(
                row,
                font,
                label,
                12.0,
                if active {
                    theme.sidebar_foreground.with_alpha(0.68)
                } else {
                    theme.sidebar_foreground
                },
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_workspace(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    icons: Handle<Image>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|workspace| {
            spawn_top_bar(workspace, font.clone(), icons.clone(), session, theme);
            match session.route {
                StudioRoute::Library if session.config.library_paths().is_empty() => {
                    spawn_empty_library(workspace, font.clone(), session.scanning, theme);
                }
                StudioRoute::Library
                    if session.library_facet.is_none()
                        && matches!(
                            session.library_view,
                            LibraryView::Artists | LibraryView::Albums
                        ) =>
                {
                    spawn_library_collection(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::Library => spawn_library(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Folders => {
                    spawn_folders(workspace, font.clone(), icons.clone(), session, theme)
                }
                StudioRoute::SongDetail => spawn_song_detail(
                    workspace,
                    font.clone(),
                    asset_server,
                    images,
                    local_images,
                    session,
                    theme,
                ),
                StudioRoute::Settings => spawn_settings(
                    workspace,
                    font.clone(),
                    icons.clone(),
                    session,
                    native_setup,
                    cache_stats,
                    theme,
                ),
                StudioRoute::Editor => {}
            }
            spawn_library_player(
                workspace,
                font,
                icons,
                asset_server,
                images,
                local_images,
                session,
                theme,
            );
        });
}

fn spawn_top_bar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(56),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(12),
                    right: px(12),
                    ..default()
                },
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.72)),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|bar| {
            spawn_icon_button(
                bar,
                icons.clone(),
                theme,
                UiIcon::ArrowLeft,
                UiAction::Back,
                false,
                false,
                34.0,
            );
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(title) = match session.route {
                StudioRoute::Folders => Some("Folders"),
                StudioRoute::SongDetail => Some("Song"),
                StudioRoute::Editor => Some("Editor"),
                StudioRoute::Library | StudioRoute::Settings => None,
            } {
                spawn_text(bar, font.clone(), title, 11.0, theme.muted_foreground);
            }
            bar.spawn(Node {
                position_type: PositionType::Relative,
                width: px(34),
                height: px(34),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|search| {
                spawn_icon_button(
                    search,
                    icons.clone(),
                    theme,
                    UiIcon::Search,
                    UiAction::ToggleGlobalSearch,
                    session.search_open || session.library_search.is_some(),
                    false,
                    34.0,
                );
                if session.search_open {
                    search
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(0),
                                top: px(40),
                                width: px(410),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(12)),
                                row_gap: px(9),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(7)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.98)),
                            BorderColor::all(theme.border.with_alpha(0.86)),
                            BoxShadow::new(
                                Color::srgba(0.0, 0.0, 0.0, 0.24),
                                px(0),
                                px(14),
                                px(30),
                                px(-10),
                            ),
                            ZIndex(75),
                        ))
                        .with_children(|popover| {
                            popover.spawn((
                                LibrarySearchInput,
                                EditableText {
                                    visible_width: Some(38.0),
                                    max_characters: Some(120),
                                    ..EditableText::new(
                                        session.library_search.as_deref().unwrap_or(""),
                                    )
                                },
                                Node {
                                    width: percent(100),
                                    height: px(42),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(12)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                ui_text_font(font.clone(), 12.0),
                                TextColor(theme.foreground),
                                TextLayout::no_wrap(),
                                TextCursorStyle {
                                    color: theme.primary,
                                    selected_text_color: Some(theme.primary_foreground),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.72)),
                                BorderColor::all(theme.border.with_alpha(0.68)),
                                TabIndex(0),
                                AutoFocus,
                            ));
                            popover
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|footer| {
                                    spawn_wrapped_text(
                                        footer,
                                        font.clone(),
                                        "Search tracks, artists, albums, and playlists · Enter",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                    footer.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    if session.library_search.is_some() {
                                        spawn_text_button(
                                            footer,
                                            font.clone(),
                                            theme,
                                            "Clear",
                                            9.0,
                                            UiAction::ClearLibrarySearch,
                                        );
                                    }
                                    spawn_text_button(
                                        footer,
                                        font.clone(),
                                        theme,
                                        "Search",
                                        9.0,
                                        UiAction::ApplyLibrarySearch,
                                    );
                                });
                        });
                }
            });
            let has_active_analysis = session.analysis_tasks.iter().any(|task| {
                matches!(
                    task.status,
                    app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
                )
            });
            spawn_activity_button(
                bar,
                icons.clone(),
                theme,
                session.activity_open,
                has_active_analysis,
            );
        });
}

fn spawn_about_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    logo: Handle<Image>,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseAbout,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(110),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: px(520),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(12),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            UiTransform::from_xy(percent(-50), percent(-50)),
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            ZIndex(111),
        ))
        .with_children(|dialog| {
            dialog
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    ..default()
                })
                .with_children(|identity| {
                    identity.spawn((
                        Node {
                            width: px(72),
                            height: px(72),
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::all(px(16)),
                            ..default()
                        },
                        ImageNode::new(logo),
                    ));
                    identity
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(copy, font.clone(), "Uta! Studio", 24.0, theme.foreground);
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "AI generation · precise chart editing · interoperable export",
                                10.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_text_button(
                        identity,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::CloseAbout,
                    );
                });
            spawn_text(
                dialog,
                font.clone(),
                format!("Version {}", env!("CARGO_PKG_VERSION")),
                11.0,
                theme.foreground,
            );
            spawn_text(
                dialog,
                font.clone(),
                "License · GPL-3.0-or-later",
                10.0,
                theme.muted_foreground,
            );
            dialog.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.7)),
            ));
            spawn_text(dialog, font.clone(), "ATTRIBUTIONS", 9.0, theme.primary);
            for attribution in [
                "Lyrics data · LRCLIB",
                "Stem separation · UVR / Demucs",
                "Speech recognition · WhisperX / OpenAI Whisper / NVIDIA Parakeet",
                "Forced alignment · WhisperX / torchaudio / Qwen3-ForcedAligner / FA-Kara (MIT)",
                "Optional Japanese model · NextFire MMS Karaoke (AGPL-3.0)",
                "CJK romanization · fugashi / pypinyin / hangul-romanize / ToJyutping",
            ] {
                spawn_wrapped_text(
                    dialog,
                    font.clone(),
                    attribution,
                    9.0,
                    theme.muted_foreground,
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn handle_window_close_requests(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut commands: Commands,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    mut session: ResMut<StudioSession>,
    setup: Res<NativeSetup>,
    diagnostics: Res<NativeDiagnostics>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let Some(request) = requests.read().next() else {
        return;
    };
    let has_unsaved_edits = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let background_work = session.scanning
        || session.authoring_busy
        || setup.receiver.is_some()
        || diagnostics.receiver.is_some()
        || session.export_job.receiver.is_some()
        || session.editor_load_job.receiver.is_some()
        || session.lyrics_search_job.receiver.is_some()
        || session.analysis_tasks.iter().any(|task| {
            matches!(
                task.status,
                app_core::QueuedStatus::Queued | app_core::QueuedStatus::Analyzing(_)
            )
        });
    if has_unsaved_edits || background_work {
        session.pending_leave = Some(PendingLeave::Exit);
        invalidated.0 = true;
    } else {
        let _ = audio.0.stop();
        let _ = library_audio.0.stop();
        commands.entity(request.window).despawn();
    }
}

const NAVIGATION_INITIAL_REPEAT: Duration = Duration::from_millis(400);

const NAVIGATION_REPEAT_RATE: Duration = Duration::from_millis(80);

const NAVIGATION_STICK_DEADZONE: f32 = 0.5;

fn register_navigation_targets(
    mut commands: Commands,
    targets: Query<(Entity, &UiAction), (Added<UiAction>, With<Button>)>,
) {
    for (entity, action) in &targets {
        if !action_is_navigation_target(action) {
            continue;
        }
        commands
            .entity(entity)
            .try_insert((TabIndex(0), Outline::new(px(1), px(1), Color::NONE)));
    }
}

pub(crate) fn action_is_navigation_target(action: &UiAction) -> bool {
    !matches!(
        action,
        UiAction::CloseActivity
            | UiAction::DismissFolderContext
            | UiAction::DismissSongContext
            | UiAction::DismissLyricContext
            | UiAction::DismissNoteContext
            | UiAction::DismissWaveformContext
            | UiAction::DismissProblemsPanel
            | UiAction::DismissShortcutsPanel
            | UiAction::DismissAnalysisNodeContext
            | UiAction::ClosePlanPreview
            | UiAction::CloseAppLogViewer
    )
}

fn navigation_repeat(
    state: &mut NavigationInputState,
    direction: Option<NavigationDirection>,
    now: Instant,
) -> Option<NavigationDirection> {
    let Some(direction) = direction else {
        state.held_direction = None;
        state.repeat_at = None;
        return None;
    };
    if state.held_direction != Some(direction) {
        state.held_direction = Some(direction);
        state.repeat_at = Some(now + NAVIGATION_INITIAL_REPEAT);
        return Some(direction);
    }
    if state.repeat_at.is_some_and(|repeat_at| now >= repeat_at) {
        state.repeat_at = Some(now + NAVIGATION_REPEAT_RATE);
        return Some(direction);
    }
    None
}

fn navigation_back_action(session: &StudioSession) -> Option<UiAction> {
    if session.pending_leave.is_some() {
        return Some(UiAction::CancelLeave);
    }
    if session.pending_setup.is_some() {
        return Some(UiAction::CancelSetup);
    }
    if session.pending_cache_clear.is_some() {
        return Some(UiAction::CancelClearCache);
    }
    if session.pending_cache_delete.is_some() {
        return Some(UiAction::CancelDeleteSongCache);
    }
    if session.pending_artifact_delete.is_some() {
        return Some(UiAction::CancelDeleteArtifactRevision);
    }
    if session.pending_artifact_invalidate.is_some() {
        return Some(UiAction::CancelInvalidateArtifactRevision);
    }
    if session.pending_chart_replace.is_some() {
        return Some(UiAction::CancelReplaceAuthoredChart);
    }
    if session.pending_analysis_history_clear {
        return Some(UiAction::CancelClearAnalysisHistory);
    }
    if session.lyrics_editor.is_some() {
        return Some(UiAction::CloseLyricsEditor);
    }
    if session.language_editor.is_some() {
        return Some(UiAction::CloseLanguageEditor);
    }
    if session.song_settings.is_some() {
        return Some(UiAction::CloseSongSettings);
    }
    if session.about_open {
        return Some(UiAction::CloseAbout);
    }
    if session.activity_open {
        return Some(UiAction::CloseActivity);
    }
    if session.search_open {
        return Some(UiAction::ToggleGlobalSearch);
    }
    if session.song_context.is_some() {
        return Some(UiAction::DismissSongContext);
    }
    if session.analysis_node_context.is_some() {
        return Some(UiAction::DismissAnalysisNodeContext);
    }
    if session.folder_browser.context_menu.is_some() {
        return Some(UiAction::DismissFolderContext);
    }
    if let Some(kind) = session.open_settings_select {
        return Some(UiAction::OpenSettingsSelect(kind));
    }
    if let Some(kind) = session.open_library_select {
        return Some(UiAction::OpenLibrarySelect(kind));
    }
    if session.export_all_open {
        return Some(UiAction::ToggleExportAllMenu);
    }
    if let Some(kind) = session.open_editor_select {
        return Some(UiAction::OpenEditorSelect(kind));
    }
    if session.library_playback.queue_open {
        return Some(UiAction::ToggleLibraryQueue);
    }
    if session
        .editor
        .as_ref()
        .is_some_and(|editor| editor.inspector_open)
    {
        return Some(UiAction::Editor(EditorAction::ToggleInspector));
    }
    (session.route != StudioRoute::Library).then_some(UiAction::Back)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_accessible_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    navigation: TabNavigation,
    mut state: ResMut<NavigationInputState>,
    mut focus: ResMut<InputFocus>,
    mut focus_visible: ResMut<InputFocusVisible>,
    session: Res<StudioSession>,
    editable: Query<(), With<EditableText>>,
    mut targets: Query<(Entity, &UiAction, &mut Interaction), With<Button>>,
) {
    if let Some(entity) = state.activated.take()
        && let Ok((_, _, mut interaction)) = targets.get_mut(entity)
        && *interaction == Interaction::Pressed
    {
        *interaction = Interaction::None;
    }

    let focused = focus.get();
    let editing = focused.is_some_and(|entity| editable.contains(entity));
    let focused_action = focused.is_some_and(|entity| targets.contains(entity));

    let gamepad_back = gamepads.iter().any(|gamepad| {
        gamepad.just_pressed(GamepadButton::East) || gamepad.just_pressed(GamepadButton::Start)
    });
    if gamepad_back
        && let Some(back_action) = navigation_back_action(&session)
        && let Some((entity, _, mut interaction)) = targets
            .iter_mut()
            .find(|(_, action, _)| **action == back_action)
    {
        *interaction = Interaction::Pressed;
        state.activated = Some(entity);
        return;
    }

    let keyboard_direction = if !editing && (session.route != StudioRoute::Editor || focused_action)
    {
        if keys.pressed(KeyCode::ArrowUp) || keys.pressed(KeyCode::ArrowLeft) {
            Some(NavigationDirection::Previous)
        } else if keys.pressed(KeyCode::ArrowDown) || keys.pressed(KeyCode::ArrowRight) {
            Some(NavigationDirection::Next)
        } else {
            None
        }
    } else {
        None
    };
    let gamepad_direction = gamepads.iter().find_map(|gamepad| {
        let dpad = gamepad.dpad();
        let stick = gamepad.left_stick();
        let direction = if dpad.length_squared() > 0.0 {
            dpad
        } else {
            stick
        };
        if direction.length_squared() < NAVIGATION_STICK_DEADZONE.powi(2) {
            None
        } else if direction.y.abs() >= direction.x.abs() {
            Some(if direction.y > 0.0 {
                NavigationDirection::Previous
            } else {
                NavigationDirection::Next
            })
        } else {
            Some(if direction.x < 0.0 {
                NavigationDirection::Previous
            } else {
                NavigationDirection::Next
            })
        }
    });
    if let Some(direction) = navigation_repeat(
        &mut state,
        keyboard_direction.or(gamepad_direction),
        Instant::now(),
    ) {
        let action = match direction {
            NavigationDirection::Previous => NavAction::Previous,
            NavigationDirection::Next => NavAction::Next,
        };
        let next =
            navigation.navigate(&focus, action).or_else(|error| {
                match error {
            bevy::input_focus::tab_navigation::TabNavigationError::NoTabGroupForCurrentFocus {
                new_focus,
                ..
            } => Ok(new_focus),
            other => Err(other),
        }
            });
        if let Ok(next) = next {
            focus.set(next, FocusCause::Navigated);
            focus_visible.0 = true;
        }
    }

    let gamepad_confirm = gamepads
        .iter()
        .any(|gamepad| gamepad.just_pressed(GamepadButton::South));
    let keyboard_confirm = keys.just_pressed(KeyCode::Enter)
        || (session.route != StudioRoute::Editor && keys.just_pressed(KeyCode::Space));
    if (gamepad_confirm || keyboard_confirm)
        && let Some(entity) = focus.get()
        && let Ok((_, _, mut interaction)) = targets.get_mut(entity)
    {
        *interaction = Interaction::Pressed;
        state.activated = Some(entity);
    }
}

/// The song-detail and whole-song lyrics textareas `handle_actions` reads on
/// save/apply. Grouped into one `SystemParam` because `handle_actions` was
/// already at Bevy's per-system parameter limit — bundling two related
/// queries here costs one slot instead of two.
#[derive(SystemParam)]
struct EditorTextInputs<'w, 's> {
    lyrics: Query<'w, 's, &'static EditableText, With<LyricsEditorInput>>,
    all_lyrics: Query<
        'w,
        's,
        &'static EditableText,
        (With<EditorAllLyricsInput>, Without<LyricsEditorInput>),
    >,
    song_settings_composer: Query<'w, 's, &'static EditableText, With<SongSettingsComposerInput>>,
    song_settings_country: Query<'w, 's, &'static EditableText, With<SongSettingsCountryInput>>,
    song_settings_bpm: Query<'w, 's, &'static EditableText, With<SongSettingsBpmInput>>,
}

// Bevy's `IntoSystem` impls top out at 16 function params -- this groups the
// primary window query with the (newly added) DAG canvas viewport query so
// `handle_actions` doesn't cross that ceiling.
#[derive(SystemParam)]
struct PrimaryWindowAndAnalysisViewport<'w, 's> {
    windows: Query<'w, 's, (Entity, &'static mut Window), With<PrimaryWindow>>,
    analysis_graph_viewport: Query<'w, 's, &'static ComputedNode, With<AnalysisGraphViewport>>,
}

// Keeping these as separate Bevy system parameters preserves change detection.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_actions(
    mut commands: Commands,
    interactions: Query<(&Interaction, &UiAction), Changed<Interaction>>,
    keys: Res<ButtonInput<KeyCode>>,
    text_inputs: EditorTextInputs,
    search_inputs: Query<
        &EditableText,
        (
            With<LibrarySearchInput>,
            Without<LyricsEditorInput>,
            Without<LanguageEditorInput>,
        ),
    >,
    mut windows: PrimaryWindowAndAnalysisViewport,
    audio: Res<NativeAudio>,
    library_audio: Res<NativeLibraryAudio>,
    pitch_audition: Res<NativePitchAudition>,
    mut session: ResMut<StudioSession>,
    mut setup: ResMut<NativeSetup>,
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut authoring: ResMut<NativeAuthoringJob>,
    mut theme: ResMut<StudioTheme>,
    mut clear_color: ResMut<ClearColor>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Ok((window_entity, mut window)) = windows.windows.single_mut() else {
            continue;
        };
        match action {
            UiAction::ToggleGlobalSearch => {
                session.search_open = !session.search_open;
                session.activity_open = false;
                session.about_open = false;
                invalidated.0 = true;
            }
            UiAction::ToggleActivity => {
                session.activity_open = !session.activity_open;
                session.search_open = false;
                session.about_open = false;
                session.analysis_tasks = app_core::load_analysis_tasks();
                invalidated.0 = true;
            }
            UiAction::CloseActivity => {
                session.activity_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectAnalysisHistory(id) => {
                session.selected_analysis_history = *id;
                session.selected_analysis_stage = None;
                session.activity_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectAnalysisStage(stage) => {
                // The only caller is the node context menu's "View in
                // inspector" button -- without closing the menu here (the
                // way a direct primary-click on a node already does, in
                // `open_analysis_node_from_click`), the inspector panel
                // *did* update, just invisibly, underneath the still-open
                // menu, which looked like the button did nothing.
                session.selected_analysis_stage = Some(stage.clone());
                session.analysis_node_context = None;
                invalidated.0 = true;
            }
            UiAction::AdjustAnalysisGraphZoom(delta_percent) => {
                session.analysis_graph_zoom = clamp_analysis_graph_zoom(
                    session.analysis_graph_zoom + (*delta_percent as f32) / 100.0,
                );
                invalidated.0 = true;
            }
            UiAction::ToggleAnalysisMiniView => {
                session.analysis_mini_view = !session.analysis_mini_view;
                invalidated.0 = true;
            }
            UiAction::FitAnalysisGraph(canvas_width_px) => {
                let viewport_width = windows
                    .analysis_graph_viewport
                    .iter()
                    .next()
                    .map(|computed| computed.size().x * computed.inverse_scale_factor())
                    .unwrap_or(*canvas_width_px as f32);
                let fit = if *canvas_width_px > 0 {
                    viewport_width / *canvas_width_px as f32
                } else {
                    1.0
                };
                session.analysis_graph_zoom = clamp_analysis_graph_zoom(fit);
                session.analysis_graph_scroll_offset = 0.0;
                invalidated.0 = true;
            }
            UiAction::FocusAnalysisGraphNode(scroll, stage_id) => {
                session.analysis_graph_scroll_offset = (*scroll).max(0) as f32;
                session.selected_analysis_stage = Some(stage_id.clone());
                invalidated.0 = true;
            }
            UiAction::DismissAnalysisNodeContext => {
                session.analysis_node_context = None;
                invalidated.0 = true;
            }
            UiAction::RequestClearAnalysisHistory => {
                session.pending_analysis_history_clear = true;
                invalidated.0 = true;
            }
            UiAction::CancelClearAnalysisHistory => {
                session.pending_analysis_history_clear = false;
                invalidated.0 = true;
            }
            UiAction::ConfirmClearAnalysisHistory => {
                session.pending_analysis_history_clear = false;
                match app_core::clear_analysis_history() {
                    Ok(()) => {
                        session.analysis_history.clear();
                        session.selected_analysis_history = None;
                        session.selected_analysis_stage = None;
                        session.notice = Some("Analysis history deleted.".into());
                    }
                    Err(error) => {
                        session.notice =
                            Some(format!("Could not delete analysis history: {error}"));
                    }
                }
                invalidated.0 = true;
            }
            UiAction::OpenAbout => {
                session.about_open = true;
                session.activity_open = false;
                session.search_open = false;
                invalidated.0 = true;
            }
            UiAction::CloseAbout => {
                session.about_open = false;
                invalidated.0 = true;
            }
            UiAction::Back => {
                session.open_settings_select = None;
                session.open_editor_select = None;
                if session.route == StudioRoute::Editor {
                    if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                        session.pending_leave = Some(PendingLeave::Back);
                    } else {
                        let _ = audio.0.stop();
                        session.editor = None;
                        session.route = StudioRoute::SongDetail;
                        session.notice = None;
                    }
                    invalidated.0 = true;
                } else if session.route != StudioRoute::Library {
                    session.route = StudioRoute::Library;
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::Home => {
                session.open_settings_select = None;
                session.open_editor_select = None;
                if session.route == StudioRoute::Editor {
                    if session.editor.as_ref().is_some_and(|editor| editor.dirty) {
                        session.pending_leave = Some(PendingLeave::Home);
                        invalidated.0 = true;
                        continue;
                    }
                    let _ = audio.0.stop();
                    session.editor = None;
                }
                if session.route != StudioRoute::Library {
                    session.route = StudioRoute::Library;
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::CancelLeave => {
                session.pending_leave = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmLeave => {
                let destination = session.pending_leave.take();
                let _ = audio.0.stop();
                match destination {
                    Some(PendingLeave::Exit) => {
                        let _ = library_audio.0.stop();
                        commands.entity(window_entity).despawn();
                    }
                    Some(PendingLeave::Back) => {
                        session.editor = None;
                        session.route = StudioRoute::SongDetail;
                        session.notice = None;
                        invalidated.0 = true;
                    }
                    Some(PendingLeave::Home) => {
                        session.editor = None;
                        session.route = StudioRoute::Library;
                        session.notice = None;
                        invalidated.0 = true;
                    }
                    None => {}
                }
            }
            UiAction::SetLibraryView(view) => {
                let view_changed = session.library_view != *view;
                session.library_view = *view;
                session.library_status = None;
                session.library_search = None;
                session.library_facet = None;
                session.route = StudioRoute::Library;
                session.song_context = None;
                session.activity_open = false;
                session.about_open = false;
                session.search_open = false;
                session.notice = None;
                if view_changed {
                    session.library_scroll_offset = 0.0;
                    session.analysis_graph_scroll_offset = 0.0;
                }
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::SetLibraryFacet(facet) => {
                session.library_view = LibraryView::All;
                session.library_search = None;
                session.library_facet = Some(facet.clone());
                session.route = StudioRoute::Library;
                session.song_context = None;
                session.notice = None;
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::LoadMoreSongs => {
                session.load_more_songs();
                invalidated.0 = true;
            }
            UiAction::ApplyLibrarySearch => {
                let value = search_inputs
                    .single()
                    .map(|input| input.value().to_string())
                    .unwrap_or_default();
                let value = value.trim();
                session.library_search = (!value.is_empty()).then(|| value.to_string());
                session.route = StudioRoute::Library;
                session.library_view = LibraryView::All;
                session.library_facet = None;
                session.search_open = false;
                session.refresh_library();
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::ClearLibrarySearch => {
                session.library_search = None;
                session.route = StudioRoute::Library;
                session.refresh_library();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryLayout => {
                session.config.song_list_view = Some(
                    if session.config.song_list_view.as_deref() == Some("grid") {
                        "table"
                    } else {
                        "grid"
                    }
                    .to_string(),
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::ToggleExportAllMenu => {
                session.export_all_open = !session.export_all_open;
                session.open_library_select = None;
                invalidated.0 = true;
            }
            UiAction::ExportAllUtz | UiAction::ExportAllUltraStar => {
                session.export_all_open = false;
                let extension = if matches!(action, UiAction::ExportAllUtz) {
                    "utz"
                } else {
                    "txt"
                };
                if let Some(export_directory) = session.config.export_path.clone() {
                    session.notice = Some(start_export_all_job(
                        extension,
                        export_directory,
                        &mut session.export_job,
                    ));
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Storage;
                    session.request_cache_stats_refresh = true;
                    session.notice =
                        Some("Choose a default export folder before using Export all.".to_string());
                }
                invalidated.0 = true;
            }
            UiAction::OpenLibrarySelect(kind) => {
                session.open_library_select = if session.open_library_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                session.export_all_open = false;
                invalidated.0 = true;
            }
            UiAction::SelectLibraryValue(kind, value) => {
                let value = (value != "all").then(|| value.clone());
                match kind {
                    LibrarySelectKind::Status => session.library_status = value,
                    LibrarySelectKind::TranscriptSource => {
                        session.library_transcript_source = value
                    }
                }
                session.open_library_select = None;
                session.refresh_library();
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::AnalyzeAll => {
                if app_core::analysis_runtime_status().ready {
                    app_core::enqueue_all(&session.library_filters());
                    session.analysis_tasks = app_core::load_analysis_tasks();
                    session.library_view = LibraryView::Queue;
                    session.library_facet = None;
                    session.route = StudioRoute::Library;
                    session.refresh_library();
                    session.notice = Some("Matching unanalyzed songs were queued.".to_string());
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Models;
                    session.notice = Some(
                        "Analysis is disabled until setup is completed in Settings > Models & runtime."
                            .to_string(),
                    );
                }
                invalidated.0 = true;
            }
            UiAction::Folders => {
                session.route = StudioRoute::Folders;
                session.notice = None;
                session.folder_browser.context_menu = None;
                if session.folder_browser.root.is_none()
                    && let Some(path) = session.config.library_paths().into_iter().next()
                {
                    session.folder_browser.select_root(path);
                }
                invalidated.0 = true;
            }
            UiAction::Settings => {
                session.route = StudioRoute::Settings;
                session.notice = None;
                session.open_settings_select = None;
                if session.settings_tab == SettingsTab::Storage {
                    session.request_cache_stats_refresh = true;
                }
                invalidated.0 = true;
            }
            UiAction::SettingsTab(tab) => {
                session.route = StudioRoute::Settings;
                session.settings_tab = *tab;
                session.notice = None;
                session.open_settings_select = None;
                session.request_cache_stats_refresh =
                    matches!(session.settings_tab, SettingsTab::Storage);
                invalidated.0 = true;
            }
            UiAction::ToggleFullscreen => {
                if let Some(error) = toggle_fullscreen(&mut window, &mut session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::OpenLog => {
                let path = app_core::default_uta_studio_dir().join("uta-studio.log");
                session.notice = Some(if path.is_file() {
                    match open::that_detached(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(error) => format!("Could not open {}: {error}", path.display()),
                    }
                } else {
                    format!("No application log exists yet at {}", path.display())
                });
                invalidated.0 = true;
            }
            UiAction::RunDiagnostics => {
                if diagnostics.receiver.is_some() {
                    session.notice = Some("Feature diagnostics are already running.".to_string());
                } else {
                    let (sender, receiver) = mpsc::channel();
                    std::thread::spawn(move || {
                        let report = uta_studio_diagnostics::run_feature_diagnostics(
                            uta_studio_diagnostics::DiagnosticRequest {
                                file_hash: None,
                                include_export_smoke: true,
                            },
                        );
                        let _ = sender.send(report);
                    });
                    diagnostics.receiver = Some(Mutex::new(receiver));
                    session.diagnostic_report = None;
                    session.notice =
                        Some("Running safe diagnostics and temporary export checks…".to_string());
                }
                invalidated.0 = true;
            }
            UiAction::RefreshRuntimeStatus => {
                session.notice = Some("Runtime status refreshed from local files.".to_string());
                invalidated.0 = true;
            }
            UiAction::OpenSettingsSelect(kind) => {
                session.open_settings_select = if session.open_settings_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
                invalidated.0 = true;
            }
            UiAction::SelectSettingsValue(kind, value) => {
                match kind {
                    SettingsSelectKind::UiLanguage => {
                        session.config.ui_language = (value != "system").then(|| value.clone());
                    }
                    SettingsSelectKind::ComputeBackend => {
                        session.config.compute_backend = Some(value.clone());
                    }
                    SettingsSelectKind::Separator => {
                        session.config.separator = Some(value.clone());
                    }
                    SettingsSelectKind::SeparatorPreset => {
                        apply_separator_preset(&mut session.config, value);
                    }
                    SettingsSelectKind::AsrEngine => {
                        session.config.asr_engine = Some(value.clone());
                    }
                    SettingsSelectKind::WhisperModel => {
                        session.config.whisper_model = Some(value.clone());
                    }
                    SettingsSelectKind::AlignBackend => {
                        session.config.align_backend = Some(value.clone());
                    }
                    SettingsSelectKind::PitchModel => {
                        session.config.pitch_model = Some(value.clone());
                    }
                }
                if session.config.compute_backend.as_deref() != Some("intel")
                    && session.config.separator() == "openvino_demucs"
                {
                    session.config.separator = Some("karaoke".to_string());
                }
                session.open_settings_select = None;
                session.notice = save_config_error(&session.config).or_else(|| {
                    Some(match kind {
                        SettingsSelectKind::UiLanguage => {
                            "Interface language updated.".to_string()
                        }
                        SettingsSelectKind::ComputeBackend => format!(
                            "Acceleration set to {}. Reconfigure the runtime to apply it.",
                            settings_select_label(*kind, value)
                        ),
                        SettingsSelectKind::SeparatorPreset => format!(
                            "{} separation profile applied. Existing stems change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                        _ => format!(
                            "{} selected. Existing charts change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                    })
                });
                invalidated.0 = true;
            }
            UiAction::ToggleAnalysisAdvanced(section) => {
                session.open_analysis_advanced = if session.open_analysis_advanced == Some(*section)
                {
                    None
                } else {
                    Some(*section)
                };
                session.open_settings_select = None;
                invalidated.0 = true;
            }
            UiAction::RequestSetup(target) => {
                if setup.receiver.is_some() {
                    session.notice = Some("A runtime setup job is already running.".to_string());
                } else {
                    session.pending_setup = Some(SetupRequest { target: *target });
                    session.notice = None;
                }
                invalidated.0 = true;
            }
            UiAction::CancelSetup => {
                session.pending_setup = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmSetup => {
                if let Some(request) = session.pending_setup.take() {
                    start_native_setup(&session.config, request, &mut setup);
                    session.notice = Some("Preparing analysis runtime…".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::RescanLibrary => {
                if session.config.library_paths().is_empty() {
                    session.notice = Some("Add a watched folder before scanning.".to_string());
                } else if session.scanning {
                    session.notice = Some("A library scan is already running.".to_string());
                } else {
                    session.scanning = true;
                    session.notice = Some("Library scan started.".to_string());
                    app_core::start_scan();
                }
                invalidated.0 = true;
            }
            UiAction::ToggleTheme => {
                session.config.dark_mode = Some(!theme.dark);
                session.notice = save_config_error(&session.config);
                *theme = StudioTheme::new(!theme.dark);
                clear_color.0 = theme.background;
                window.window_theme = Some(if theme.dark {
                    WindowTheme::Dark
                } else {
                    WindowTheme::Light
                });
                invalidated.0 = true;
            }
            UiAction::ChooseFolder => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let mut paths = session.config.library_paths();
                    if !paths.contains(&path) {
                        paths.push(path.clone());
                        session.config.library_source = Some(LibrarySource::Folders { paths });
                        if let Some(error) = save_config_error(&session.config) {
                            session.notice = Some(error);
                        } else {
                            session.scanning = true;
                            session.notice =
                                Some("Folder added; library scan started.".to_string());
                            app_core::start_scan();
                            session.refresh_library();
                            if session.route == StudioRoute::Folders {
                                session.folder_browser.select_root(path);
                            }
                        }
                    } else {
                        session.notice = Some("That folder is already watched.".to_string());
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::ChooseExportFolder => {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    session.config.export_path = Some(path);
                    session.notice = save_config_error(&session.config)
                        .or_else(|| Some("Default export folder updated.".to_string()));
                    invalidated.0 = true;
                }
            }
            UiAction::ClearExportFolder => {
                session.config.export_path = None;
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some("Export dialogs will use the system default.".to_string()));
                invalidated.0 = true;
            }
            UiAction::SelectFolderRoot(path) => {
                session.folder_browser.select_root(path.clone());
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::FolderUp => {
                if let Some(parent) = session.folder_browser.parent() {
                    session.folder_browser.current = Some(parent);
                    session.folder_browser.context_menu = None;
                    session.folder_browser.refresh();
                    session.notice = None;
                    invalidated.0 = true;
                }
            }
            UiAction::OpenFolderEntry(path) => {
                session.folder_browser.context_menu = None;
                if path.is_dir() {
                    session.folder_browser.current = Some(path.clone());
                    session.folder_browser.refresh();
                    session.notice = None;
                } else {
                    session.notice = Some(open_library_entry(path, &session.config));
                }
                invalidated.0 = true;
            }
            UiAction::RevealFolderEntry(path) => {
                session.folder_browser.context_menu = None;
                session.notice = Some(reveal_library_entry(path, &session.config));
                invalidated.0 = true;
            }
            UiAction::DismissFolderContext => {
                session.folder_browser.context_menu = None;
                invalidated.0 = true;
            }
            UiAction::RequestRemoveFolder(path) => {
                session.folder_browser.context_menu = None;
                session.folder_browser.pending_remove = Some(path.clone());
                invalidated.0 = true;
            }
            UiAction::CancelRemoveFolder => {
                session.folder_browser.pending_remove = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmRemoveFolder => {
                if let Some(path) = session.folder_browser.pending_remove.take() {
                    let mut paths = session.config.library_paths();
                    paths.retain(|entry| entry != &path);
                    session.config.library_source = if paths.is_empty() {
                        None
                    } else {
                        Some(LibrarySource::Folders { paths })
                    };
                    if let Some(error) = save_config_error(&session.config) {
                        session.notice = Some(error);
                    } else {
                        if session.config.library_source.is_some() {
                            session.scanning = true;
                            app_core::start_scan();
                        } else {
                            app_core::clear_library_index();
                            session.scanning = false;
                        }
                        session.notice = Some(format!(
                            "Stopped watching {}. No source media was moved or deleted.",
                            path.display()
                        ));
                    }
                    session.folder_browser = FolderBrowser::new(&session.config);
                    session.refresh_library();
                    invalidated.0 = true;
                }
            }
            UiAction::AdjustBeamSize(delta) => {
                session.config.beam_size = Some(
                    (i64::from(session.config.beam_size()) + i64::from(*delta)).clamp(1, 16) as u32,
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustBatchSize(delta) => {
                session.config.batch_size = Some(
                    (i64::from(session.config.batch_size()) + i64::from(*delta)).clamp(1, 16)
                        as u32,
                );
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorSegmentSize(delta) => {
                session.config.separator_segment_size = Some(
                    (i64::from(session.config.separator_segment_size()) + i64::from(*delta))
                        .clamp(64, 1024) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorOverlap(delta) => {
                session.config.separator_overlap = Some(
                    (i64::from(session.config.separator_overlap()) + i64::from(*delta)).clamp(2, 32)
                        as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorBatchSize(delta) => {
                session.config.separator_batch_size = Some(
                    (i64::from(session.config.separator_batch_size()) + i64::from(*delta))
                        .clamp(1, 8) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustSeparatorNormalization(delta) => {
                session.config.separator_normalization_pct = Some(
                    (i64::from(session.config.separator_normalization_pct()) + i64::from(*delta))
                        .clamp(1, 100) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustDemucsShifts(delta) => {
                session.config.demucs_shifts = Some(
                    (i64::from(session.config.demucs_shifts()) + i64::from(*delta)).clamp(1, 8)
                        as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustDemucsOverlap(delta) => {
                session.config.demucs_overlap_pct = Some(
                    (i64::from(session.config.demucs_overlap_pct()) + i64::from(*delta))
                        .clamp(1, 95) as u32,
                );
                session.notice = save_config_error(&session.config);
                invalidated.0 = true;
            }
            UiAction::AdjustUiFontScale(delta) => {
                let current = ui_font_size_percent_to_points(session.config.font_scale_percent());
                let next = (i64::from(current)
                    + i64::from(*delta) * i64::from(UI_FONT_SIZE_STEP_PX))
                .clamp(
                    i64::from(UI_FONT_SIZE_MIN_PX),
                    i64::from(UI_FONT_SIZE_MAX_PX),
                );
                let next_percent = ui_font_points_to_scale_percent(next as u32);
                session.config.font_scale_percent = Some(next_percent);
                set_ui_font_scale(next_percent as f32 / 100.0);
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some(format!("Font size: {}px", next)));
                invalidated.0 = true;
            }
            UiAction::ToggleAutoAnalyze => {
                session.config.auto_analyze = Some(!session.config.auto_analyze());
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::AdjustVocalThreshold(delta) => {
                let current = (session.config.vocal_detection_threshold_pct() * 100.0).round();
                let value = (current + f64::from(*delta)).clamp(0.0, 60.0) / 100.0;
                session.config.vocal_detection_threshold_pct = Some(value);
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                }
                invalidated.0 = true;
            }
            UiAction::RestoreAnalysisDefaults => {
                session.config.separator = Some("karaoke".to_string());
                session.config.separator_segment_size = None;
                session.config.separator_overlap = None;
                session.config.separator_batch_size = None;
                session.config.separator_normalization_pct = None;
                session.config.demucs_shifts = None;
                session.config.demucs_overlap_pct = None;
                session.config.asr_engine = Some("whisper".to_string());
                session.config.align_backend = Some("whisperx".to_string());
                session.config.pitch_model = Some("rmvpe".to_string());
                session.config.vocal_detection_threshold_pct = Some(0.15);
                session.config.whisper_model = Some("large-v3".to_string());
                session.config.beam_size = Some(8);
                session.config.batch_size = Some(8);
                session.config.compute_backend = Some("cpu".to_string());
                session.config.auto_analyze = Some(false);
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some("Analysis defaults restored.".to_string()));
                invalidated.0 = true;
            }
            UiAction::RequestClearCache(scope) => {
                session.pending_cache_clear = Some(*scope);
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CancelClearCache => {
                session.pending_cache_clear = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmClearCache => {
                if let Some(scope) = session.pending_cache_clear.take() {
                    match scope {
                        CacheClearScope::Generated => {
                            app_core::CacheDir::new().clear_all();
                            session.refresh_library();
                            session.request_cache_stats_refresh = true;
                            session.notice = Some(
                                "Generated cache cleared. Source media was not changed."
                                    .to_string(),
                            );
                        }
                        CacheClearScope::Models => {
                            app_core::clear_models();
                            session.request_cache_stats_refresh = true;
                            session.notice = Some(
                                "Downloaded models cleared. Runtime setup now reports the missing artifacts."
                                    .to_string(),
                            );
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::OpenSong(file_hash) => {
                session.song_context = None;
                session.selected_song = Some(file_hash.clone());
                session.route = StudioRoute::SongDetail;
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::AnalyzeSong(file_hash) => {
                session.song_context = None;
                if app_core::analysis_runtime_status().ready {
                    app_core::enqueue_one(file_hash);
                    session.analysis_tasks = app_core::load_analysis_tasks();
                    session.library_view = LibraryView::Queue;
                    session.library_facet = None;
                    session.route = StudioRoute::Library;
                    session.refresh_library();
                    session.notice = Some("Song queued for analysis.".to_string());
                } else {
                    session.route = StudioRoute::Settings;
                    session.settings_tab = SettingsTab::Models;
                    session.notice = Some(
                        "Analysis is disabled until the runtime and selected models are installed."
                            .to_string(),
                    );
                }
                invalidated.0 = true;
            }
            UiAction::OpenEditor(file_hash) => {
                session.song_context = None;
                session.selected_song = Some(file_hash.clone());
                session.editor = None;
                session.route = StudioRoute::Editor;
                if session.library_playback.status.playing
                    && let Ok(status) = library_audio.0.pause()
                {
                    session.library_playback.visible_position = status.position_secs;
                    session.library_playback.status = status;
                    session.library_playback.last_audio_sync = Instant::now();
                }
                session.notice = Some(start_editor_load_job(
                    file_hash,
                    Arc::clone(&audio.0),
                    &mut session.editor_load_job,
                ));
                invalidated.0 = true;
            }
            UiAction::ExportUtz(file_hash) => {
                session.song_context = None;
                let export_directory = session.config.export_path.clone();
                session.notice = Some(start_export_job(
                    file_hash,
                    "utz",
                    export_directory,
                    &mut session.export_job,
                ));
                invalidated.0 = true;
            }
            UiAction::ExportUltraStar(file_hash) => {
                session.song_context = None;
                let export_directory = session.config.export_path.clone();
                session.notice = Some(start_export_job(
                    file_hash,
                    "txt",
                    export_directory,
                    &mut session.export_job,
                ));
                invalidated.0 = true;
            }
            UiAction::OpenSource(path) => {
                session.song_context = None;
                session.notice = Some(match validate_source_path(path, &session.config) {
                    Ok(path) => match open::that_detached(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(error) => format!("Could not open {}: {error}", path.display()),
                    },
                    Err(error) => error,
                });
                invalidated.0 = true;
            }
            UiAction::RevealSource(path) => {
                session.song_context = None;
                session.notice = Some(reveal_library_entry(path, &session.config));
                invalidated.0 = true;
            }
            UiAction::DismissSongContext => {
                session.song_context = None;
                invalidated.0 = true;
            }
            UiAction::OpenLyricsEditor(file_hash) => {
                let song = app_core::load_song_by_hash(file_hash).ok().flatten();
                let mode = if song.as_ref().is_some_and(|song| {
                    matches!(
                        song.transcript_source,
                        Some(app_core::TranscriptSource::Lrc)
                    )
                }) {
                    LyricsInputMode::TimedLrc
                } else {
                    LyricsInputMode::Plain
                };
                session.lyrics_editor = Some(NativeLyricsEditor {
                    file_hash: file_hash.clone(),
                    mode,
                    separate_stems: true,
                    initial_text: lyrics_text(file_hash, mode),
                    candidates: Vec::new(),
                    candidate_index: 0,
                    searching: false,
                });
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CloseLyricsEditor => {
                session.lyrics_editor = None;
                invalidated.0 = true;
            }
            UiAction::ToggleLyricsInputMode => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = text_inputs.lyrics.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.mode = if editor.mode == LyricsInputMode::Plain {
                        LyricsInputMode::TimedLrc
                    } else {
                        LyricsInputMode::Plain
                    };
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleLyricsSeparateStems => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = text_inputs.lyrics.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.separate_stems = !editor.separate_stems;
                    invalidated.0 = true;
                }
            }
            UiAction::SearchLrclibLyrics => {
                if session.lyrics_search_job.receiver.is_none()
                    && let Some(editor) = session.lyrics_editor.as_mut()
                {
                    if let Ok(input) = text_inputs.lyrics.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.searching = true;
                    editor.candidates.clear();
                    editor.candidate_index = 0;
                    let file_hash = editor.file_hash.clone();
                    let (sender, receiver) = mpsc::channel();
                    std::thread::spawn(move || {
                        let candidates = app_core::search_lrclib_for_hash(&file_hash);
                        let _ = sender.send(candidates);
                    });
                    session.lyrics_search_job.receiver = Some(Mutex::new(receiver));
                    session.notice = Some("Searching LRCLIB…".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::PreviousLrclibCandidate => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = text_inputs.lyrics.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.candidate_index = editor.candidate_index.saturating_sub(1);
                    invalidated.0 = true;
                }
            }
            UiAction::NextLrclibCandidate => {
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    if let Ok(input) = text_inputs.lyrics.single() {
                        editor.initial_text = input.value().to_string();
                    }
                    editor.candidate_index =
                        (editor.candidate_index + 1).min(editor.candidates.len().saturating_sub(1));
                    invalidated.0 = true;
                }
            }
            UiAction::UseLrclibPlain => {
                if let Some(editor) = session.lyrics_editor.as_mut()
                    && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                {
                    editor.initial_text = candidate.lines.join("\n");
                    editor.mode = LyricsInputMode::Plain;
                    session.notice = Some("LRCLIB plain lyrics loaded for review.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::UseLrclibTimed => {
                if let Some(editor) = session.lyrics_editor.as_mut()
                    && let Some(candidate) = editor.candidates.get(editor.candidate_index)
                    && let Some(lrc) = candidate.synced_lyrics.as_ref()
                {
                    editor.initial_text = lrc.clone();
                    editor.mode = LyricsInputMode::TimedLrc;
                    session.notice = Some("LRCLIB timed lyrics loaded for review.".to_string());
                    invalidated.0 = true;
                }
            }
            UiAction::SaveLyricsEditor => {
                let value = text_inputs
                    .lyrics
                    .single()
                    .map(|input| input.value().to_string())
                    .unwrap_or_default();
                if let Some(editor) = session.lyrics_editor.as_mut() {
                    editor.initial_text = value.clone();
                    let song = app_core::load_song_by_hash(&editor.file_hash)
                        .ok()
                        .flatten();
                    let requires_runtime = editor.mode == LyricsInputMode::Plain
                        || (editor.mode == LyricsInputMode::TimedLrc
                            && editor.separate_stems
                            && song.as_ref().is_some_and(|song| !song.is_analyzed));
                    let result = if requires_runtime && !app_core::analysis_runtime_status().ready {
                        Err("Analysis runtime is unavailable. Choose authoring on the original mix for timed LRC, or finish setup in Settings > Models & runtime.".to_string())
                    } else if editor.mode == LyricsInputMode::TimedLrc {
                        if song.as_ref().is_some_and(|song| song.is_analyzed) {
                            app_core::apply_timed_lyrics(&editor.file_hash, &value)
                        } else {
                            app_core::provide_lrc(&editor.file_hash, &value, editor.separate_stems)
                        }
                    } else {
                        app_core::save_lyrics_and_realign(
                            &editor.file_hash,
                            value.lines().map(str::to_string).collect(),
                        )
                    };
                    match result {
                        Ok(()) => {
                            session.lyrics_editor = None;
                            session.refresh_library();
                            session.notice = Some(
                                if requires_runtime {
                                    "Lyrics saved and alignment queued."
                                } else {
                                    "Timed lyrics saved."
                                }
                                .to_string(),
                            );
                        }
                        Err(error) => {
                            session.notice = Some(format!("Could not save lyrics: {error}"))
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::OpenLanguageEditor(file_hash) => {
                let config = AppConfig::load();
                let initial_language = config
                    .language_override(file_hash)
                    .map(canonical_analysis_language)
                    .unwrap_or_else(|| "auto".into());
                session.language_editor = Some(NativeLanguageEditor {
                    file_hash: file_hash.clone(),
                    initial_language,
                    force_transcribe: false,
                    picker_open: false,
                });
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::CloseLanguageEditor => {
                session.language_editor = None;
                invalidated.0 = true;
            }
            UiAction::ToggleLanguageReprocess => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.force_transcribe = !editor.force_transcribe;
                    editor.picker_open = false;
                    invalidated.0 = true;
                }
            }
            UiAction::ToggleLanguagePicker => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.picker_open = !editor.picker_open;
                    invalidated.0 = true;
                }
            }
            UiAction::SelectAnalysisLanguage(language) => {
                if let Some(editor) = session.language_editor.as_mut() {
                    editor.initial_language = canonical_analysis_language(language);
                    editor.picker_open = false;
                    invalidated.0 = true;
                }
            }
            UiAction::SaveLanguageEditor => {
                if let Some(editor) = session.language_editor.as_mut() {
                    let language = editor.initial_language.clone();
                    if !app_core::analysis_runtime_status().ready {
                        session.notice = Some(
                            "Analysis is disabled until setup is completed in Settings > Models & runtime."
                                .to_string(),
                        );
                    } else {
                        if language == "auto" {
                            let mut config = AppConfig::load();
                            config.clear_language_override(&editor.file_hash);
                            if let Err(error) = config.save() {
                                session.notice =
                                    Some(format!("Could not save the language setting: {error}"));
                                invalidated.0 = true;
                                continue;
                            }
                            if editor.force_transcribe {
                                app_core::reanalyze_force_transcribe(&editor.file_hash);
                            } else {
                                let _ = app_core::realign(&editor.file_hash, None);
                            }
                        } else if editor.force_transcribe {
                            let mut config = AppConfig::load();
                            config
                                .set_language_override(editor.file_hash.clone(), language.clone());
                            if let Err(error) = config.save() {
                                session.notice =
                                    Some(format!("Could not save the language setting: {error}"));
                                invalidated.0 = true;
                                continue;
                            }
                            app_core::reanalyze_force_transcribe(&editor.file_hash);
                        } else {
                            let _ = app_core::realign(&editor.file_hash, Some(language.clone()));
                        }
                        session.language_editor = None;
                        session.config = AppConfig::load();
                        session.notice = Some(if language == "auto" {
                            "Automatic language detection enabled; reprocessing queued.".into()
                        } else {
                            format!("Language set to {language}; reprocessing queued.")
                        });
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::SaveNodeConfigAsSongProfile(file_hash, node_id) => {
                // Same immediacy as Freeze/Disable/Bypass above -- fires
                // right away and leaves the context menu open, rather than
                // dismissing it, matching that established pattern.
                session.notice = Some(
                    match app_core::save_node_config_as_song_profile(file_hash, node_id) {
                        Ok(()) => format!(
                            "Saved {node_id}'s current configuration as this song's profile."
                        ),
                        Err(error) => format!("Could not save song profile: {error}"),
                    },
                );
                invalidated.0 = true;
            }
            UiAction::OpenNodeConfigDialog(file_hash, node_id) => {
                if let Some(field) = node_config_profile_field(node_id) {
                    let global = app_core::AnalysisProfileSnapshot::from_app_config(
                        &AppConfig::load(),
                        file_hash,
                    );
                    let song = app_core::get_song_analysis_profile(file_hash);
                    let run_override = app_core::pending_run_override_for(file_hash, node_id);
                    let value = app_core::resolve_profile_field(
                        field,
                        &global,
                        song.as_ref(),
                        run_override.as_deref(),
                    )
                    .value;
                    session.node_config_dialog = Some(NativeNodeConfigDialog {
                        file_hash: file_hash.clone(),
                        node_id: node_id.clone(),
                        field,
                        value,
                        picker_open: false,
                    });
                } else {
                    session.notice = Some(format!(
                        "{node_id} has no parameter to configure for a single run."
                    ));
                }
                session.analysis_node_context = None;
                invalidated.0 = true;
            }
            UiAction::CloseNodeConfigDialog => {
                session.node_config_dialog = None;
                invalidated.0 = true;
            }
            UiAction::ToggleNodeConfigPicker => {
                if let Some(dialog) = session.node_config_dialog.as_mut() {
                    dialog.picker_open = !dialog.picker_open;
                    invalidated.0 = true;
                }
            }
            UiAction::SelectNodeConfigValue(value) => {
                if let Some(dialog) = session.node_config_dialog.as_mut() {
                    dialog.value = value.clone();
                    dialog.picker_open = false;
                    invalidated.0 = true;
                }
            }
            UiAction::RunNodeConfigDialog => {
                if let Some(dialog) = session.node_config_dialog.take() {
                    session.notice = Some(
                        match app_core::configure_analysis_node_for_run(
                            &dialog.file_hash,
                            &dialog.node_id,
                            dialog.value.clone(),
                        ) {
                            Ok(()) => format!(
                                "Running {} with {} for this run only.",
                                dialog.node_id, dialog.value
                            ),
                            Err(error) => format!("Could not configure this run: {error}"),
                        },
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::OpenPlanPreview(file_hash) => {
                session.plan_preview_draft = Some(PlanPreviewDraft {
                    file_hash: file_hash.clone(),
                    disabled_nodes: std::collections::BTreeSet::new(),
                });
                session.notice = None;
                invalidated.0 = true;
            }
            UiAction::ClosePlanPreview => {
                session.plan_preview_draft = None;
                invalidated.0 = true;
            }
            UiAction::TogglePlanPreviewDisabledNode(node_id) => {
                if let Some(draft) = session.plan_preview_draft.as_mut() {
                    let id = app_core::AnalysisNodeId::new(node_id.clone());
                    if !draft.disabled_nodes.remove(&id) {
                        draft.disabled_nodes.insert(id);
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::RunPlanPreviewDraft => {
                if let Some(draft) = session.plan_preview_draft.take() {
                    session.notice = Some(run_analysis_action_checked(&draft.file_hash, || {
                        app_core::run_analysis_plan(
                            &draft.file_hash,
                            std::collections::BTreeSet::new(),
                            draft.disabled_nodes.clone(),
                        )
                    }));
                    invalidated.0 = true;
                }
            }
            UiAction::OpenAppLogViewer(file_hash, node_id) => {
                session.app_log_viewer = Some(AppLogViewerState {
                    file_hash: file_hash.clone(),
                    node_id: node_id.clone(),
                });
                session.analysis_node_context = None;
                invalidated.0 = true;
            }
            UiAction::CloseAppLogViewer => {
                session.app_log_viewer = None;
                invalidated.0 = true;
            }
            UiAction::OpenAppLogFile => {
                session.notice = Some(match app_core::get_log_path() {
                    Some(path) => match open::that_detached(&path) {
                        Ok(()) => format!("Opened {}", path.display()),
                        Err(error) => format!("Could not open {}: {error}", path.display()),
                    },
                    None => "The app log is not available in this environment.".to_string(),
                });
                invalidated.0 = true;
            }
            UiAction::OpenSongSettings(file_hash) => {
                session.song_settings = open_song_settings(file_hash);
                if session.song_settings.is_none() {
                    session.notice = Some("Could not load this song's settings.".to_string());
                }
                invalidated.0 = true;
            }
            UiAction::CloseSongSettings => {
                session.song_settings = None;
                invalidated.0 = true;
            }
            UiAction::ChooseBackgroundVideo => {
                if session.song_settings.is_some()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Video", &["mp4", "webm", "mkv", "mov", "avi"])
                        .pick_file()
                    && let Some(panel) = session.song_settings.as_mut()
                {
                    panel.background_video_path = Some(path);
                    invalidated.0 = true;
                }
            }
            UiAction::ClearBackgroundVideo => {
                if let Some(panel) = session.song_settings.as_mut() {
                    panel.background_video_path = None;
                    invalidated.0 = true;
                }
            }
            UiAction::SaveSongSettings => {
                if let Some(panel) = session.song_settings.take() {
                    let composer = text_inputs
                        .song_settings_composer
                        .single()
                        .map(|input| input.value().to_string().trim().to_string())
                        .unwrap_or_default();
                    let country = text_inputs
                        .song_settings_country
                        .single()
                        .map(|input| input.value().to_string().trim().to_string())
                        .unwrap_or_default();
                    let bpm_text = text_inputs
                        .song_settings_bpm
                        .single()
                        .map(|input| input.value().to_string().trim().to_string())
                        .unwrap_or_default();
                    let bpm = if bpm_text.is_empty() {
                        None
                    } else {
                        match bpm_text.parse::<f64>() {
                            Ok(value) if value.is_finite() && value > 0.0 => Some(value),
                            _ => {
                                session.notice = Some(
                                    "BPM must be a positive number, or left blank.".to_string(),
                                );
                                session.song_settings = Some(panel);
                                invalidated.0 = true;
                                continue;
                            }
                        }
                    };
                    match app_core::update_song_settings(
                        &panel.file_hash,
                        (!composer.is_empty()).then_some(composer),
                        (!country.is_empty()).then_some(country),
                        bpm,
                        panel.background_video_path.clone(),
                    ) {
                        Ok(()) => {
                            session.notice = Some("Song settings saved.".to_string());
                            session.refresh_library();
                        }
                        Err(error) => {
                            session.notice = Some(format!("Could not save song settings: {error}"));
                        }
                    }
                    invalidated.0 = true;
                }
            }
            UiAction::RealignSong(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::realign(file_hash, None)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzeTranscript(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_transcript(file_hash, None)
                }));
                invalidated.0 = true;
            }
            UiAction::ForceTranscribe(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_force_transcribe(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzePitch(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_pitch(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::ReanalyzeFull(file_hash) => {
                session.notice = Some(run_analysis_action(file_hash, || {
                    app_core::reanalyze_full(file_hash)
                }));
                invalidated.0 = true;
            }
            UiAction::RunAnalysisNodeOnly(file_hash, node_id) => {
                session.notice = Some(run_analysis_action_checked(file_hash, || {
                    app_core::run_analysis_node(file_hash, node_id)
                }));
                invalidated.0 = true;
            }
            UiAction::RunAnalysisNodeDownstream(file_hash, node_id) => {
                session.notice = Some(run_analysis_action_checked(file_hash, || {
                    app_core::run_analysis_node_downstream(file_hash, node_id)
                }));
                invalidated.0 = true;
            }
            UiAction::DisableAnalysisNodeForRun(file_hash, node_id) => {
                session.notice = Some(run_analysis_action_checked(file_hash, || {
                    app_core::disable_analysis_node_for_run(file_hash, node_id)
                }));
                invalidated.0 = true;
            }
            UiAction::FreezeAnalysisNodeOutputs(file_hash, node_id) => {
                session.notice = Some(run_analysis_action_checked(file_hash, || {
                    app_core::freeze_analysis_node_outputs_for_run(file_hash, node_id)
                }));
                invalidated.0 = true;
            }
            UiAction::BypassAnalysisNodeWithOriginalMix(file_hash, node_id) => {
                session.notice = Some(run_analysis_action_checked(file_hash, || {
                    app_core::bypass_analysis_node_with_original_mix_for_run(file_hash, node_id)
                }));
                invalidated.0 = true;
            }
            UiAction::CompareNodeAttemptWithPrevious(file_hash, node_id, current_run_id) => {
                session.notice = Some(
                    match app_core::compare_node_attempt_with_previous_run(
                        file_hash,
                        node_id,
                        *current_run_id,
                    ) {
                        Ok(comparison) => format_node_attempt_comparison(&comparison),
                        Err(error) => format!("Could not compare attempts: {error}"),
                    },
                );
                invalidated.0 = true;
            }
            UiAction::ToggleAnalysisCompoundNode(node_id) => {
                let id = app_core::AnalysisNodeId::new(node_id.clone());
                if !session.expanded_compound_nodes.remove(&id) {
                    session.expanded_compound_nodes.insert(id);
                }
                session.analysis_node_context = None;
                invalidated.0 = true;
            }
            UiAction::RequestDeleteSongCache(file_hash) => {
                session.pending_cache_delete = Some(file_hash.clone());
                invalidated.0 = true;
            }
            UiAction::CancelDeleteSongCache => {
                session.pending_cache_delete = None;
                invalidated.0 = true;
            }
            UiAction::CancelAnalysisRun(file_hash) => {
                session.notice = Some(match app_core::cancel_analysis_run(file_hash) {
                    Ok(()) => "Removed from the analysis queue.".to_string(),
                    Err(error) => format!("Could not cancel: {error}"),
                });
                invalidated.0 = true;
            }
            UiAction::ConfirmDeleteSongCache => {
                if let Some(file_hash) = session.pending_cache_delete.take() {
                    app_core::delete_cache(&file_hash);
                    session.refresh_library();
                    session.notice = Some(
                        "Generated song data deleted. Source media was not changed.".to_string(),
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::RequestReplaceAuthoredChart(file_hash) => {
                session.pending_chart_replace = Some(file_hash.clone());
                invalidated.0 = true;
            }
            UiAction::CancelReplaceAuthoredChart => {
                session.pending_chart_replace = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmReplaceAuthoredChart => {
                if let Some(file_hash) = session.pending_chart_replace.take() {
                    session.notice = Some(
                        match app_core::replace_authored_chart_with_fresh_analysis(&file_hash) {
                            Ok(()) => {
                                "Authored chart discarded. It will be rebuilt from the latest analysis the next time you open the editor.".to_string()
                            }
                            Err(error) => format!("Could not replace the chart: {error}"),
                        },
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::SyncArtifactRevisions(file_hash) => {
                let imported =
                    app_core::import_legacy_artifacts(&app_core::CacheDir::new(), file_hash);
                session.notice = Some(if imported.is_empty() {
                    "No new artifact revisions found on disk.".to_string()
                } else {
                    format!(
                        "Recorded {} artifact revision(s) from disk.",
                        imported.len()
                    )
                });
                invalidated.0 = true;
            }
            UiAction::SetActiveArtifactRevision(revision) => {
                let cache_root = app_core::CacheDir::new().path;
                session.notice = Some(
                    match app_core::set_active_artifact_revision(
                        &cache_root,
                        &revision.file_hash,
                        revision.kind,
                        &revision.id,
                    ) {
                        Ok(()) => "Active artifact revision updated.".to_string(),
                        Err(error) => format!("Could not set active artifact revision: {error}"),
                    },
                );
                invalidated.0 = true;
            }
            UiAction::OpenArtifactRevision(path) => {
                session.notice = Some(open_artifact_entry(path));
                invalidated.0 = true;
            }
            UiAction::PreviewArtifactRevision(path) => {
                session.notice = Some(preview_artifact_entry(path));
                invalidated.0 = true;
            }
            UiAction::RevealArtifactRevision(path) => {
                session.notice = Some(reveal_artifact_entry(path));
                invalidated.0 = true;
            }
            UiAction::RequestDeleteArtifactRevision(revision) => {
                session.pending_artifact_delete = Some(revision.clone());
                invalidated.0 = true;
            }
            UiAction::CancelDeleteArtifactRevision => {
                session.pending_artifact_delete = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmDeleteArtifactRevision => {
                if let Some(revision) = session.pending_artifact_delete.take() {
                    let cache_root = app_core::CacheDir::new().path;
                    session.notice = Some(
                        match app_core::delete_artifact_revision(&cache_root, &revision) {
                            Ok(()) => "Artifact revision deleted.".to_string(),
                            Err(error) => format!("Could not delete artifact revision: {error}"),
                        },
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::RequestInvalidateArtifactRevision(revision) => {
                session.pending_artifact_invalidate = Some(revision.clone());
                invalidated.0 = true;
            }
            UiAction::CancelInvalidateArtifactRevision => {
                session.pending_artifact_invalidate = None;
                invalidated.0 = true;
            }
            UiAction::ConfirmInvalidateArtifactRevision => {
                if let Some(revision) = session.pending_artifact_invalidate.take() {
                    let cache_root = app_core::CacheDir::new().path;
                    session.notice = Some(
                        match app_core::invalidate_artifact_revision(
                            &cache_root,
                            &revision.file_hash,
                            revision.kind,
                            &revision.id,
                        ) {
                            Ok(()) => {
                                "Artifact revision invalidated. It's no longer Active but the file is kept.".to_string()
                            }
                            Err(error) => format!("Could not invalidate artifact revision: {error}"),
                        },
                    );
                    invalidated.0 = true;
                }
            }
            UiAction::InspectArtifactProvenance(revision) => {
                session.notice = Some(format_artifact_provenance(revision));
                invalidated.0 = true;
            }
            UiAction::CompareArtifactRevisions(revision, active_revision_id) => {
                session.notice = Some(
                    match app_core::compare_artifact_revisions(
                        &revision.file_hash,
                        revision.kind,
                        &revision.id,
                        active_revision_id,
                    ) {
                        Ok(comparison) => format_artifact_revision_comparison(&comparison),
                        Err(error) => format!("Could not compare revisions: {error}"),
                    },
                );
                invalidated.0 = true;
            }
            UiAction::ShiftSongKey(file_hash, delta) => {
                session.notice = Some(start_key_shift(
                    file_hash,
                    *delta,
                    &mut authoring,
                    &mut session.authoring_busy,
                ));
                invalidated.0 = true;
            }
            UiAction::ShiftSongTempo(file_hash, delta) => {
                session.notice = Some(start_tempo_shift(
                    file_hash,
                    *delta,
                    &mut authoring,
                    &mut session.authoring_busy,
                ));
                invalidated.0 = true;
            }
            UiAction::PlayLibrarySong(file_hash) => {
                let queue = session.songs.processed.clone();
                prepare_library_queue(&queue, file_hash, &mut session.library_playback);
                session.notice =
                    play_library_song(&library_audio.0, file_hash, &mut session.library_playback)
                        .err();
                invalidated.0 = true;
            }
            UiAction::PlayArtifactRevision(path) => {
                session.notice =
                    play_artifact_revision(&library_audio.0, path, &mut session.library_playback)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryPlayback => {
                session.notice =
                    toggle_library_playback(&library_audio.0, &mut session.library_playback).err();
                invalidated.0 = true;
            }
            UiAction::SeekLibraryRelative(delta) => {
                session.notice = seek_library_relative(
                    &library_audio.0,
                    &mut session.library_playback,
                    f64::from(*delta),
                )
                .err();
                invalidated.0 = true;
            }
            UiAction::PreviousLibrarySong => {
                session.notice = if library_visible_position(&session.library_playback) > 5.0 {
                    restart_library_song(&library_audio.0, &mut session.library_playback).err()
                } else {
                    let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
                    advance_library_queue(&library_audio.0, &mut session.library_playback, -1, wrap)
                        .err()
                };
                invalidated.0 = true;
            }
            UiAction::NextLibrarySong => {
                let wrap = session.library_playback.repeat == LibraryRepeatMode::All;
                session.notice =
                    advance_library_queue(&library_audio.0, &mut session.library_playback, 1, wrap)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryShuffle => {
                session.library_playback.shuffle = !session.library_playback.shuffle;
                session.notice = Some(if session.library_playback.shuffle {
                    "Shuffle enabled for the playback queue.".to_string()
                } else {
                    "Shuffle disabled.".to_string()
                });
                invalidated.0 = true;
            }
            UiAction::CycleLibraryRepeat => {
                session.library_playback.repeat = session.library_playback.repeat.next();
                session.notice = Some(session.library_playback.repeat.label().to_string());
                invalidated.0 = true;
            }
            UiAction::AdjustLibraryVolume(delta) => {
                let volume = session.library_playback.volume + f64::from(*delta) / 100.0;
                session.notice =
                    set_library_volume(&library_audio.0, &mut session.library_playback, volume)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryMute => {
                let volume = if session.library_playback.volume > 0.0 {
                    session.library_playback.volume_before_mute = session.library_playback.volume;
                    0.0
                } else {
                    session.library_playback.volume_before_mute.max(0.1)
                };
                session.notice =
                    set_library_volume(&library_audio.0, &mut session.library_playback, volume)
                        .err();
                invalidated.0 = true;
            }
            UiAction::ToggleLibraryQueue => {
                session.library_playback.queue_open = !session.library_playback.queue_open;
                invalidated.0 = true;
            }
            UiAction::DismissLyricContext => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.lyric_context = None;
                }
                invalidated.0 = true;
            }
            UiAction::DismissNoteContext => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.note_context = None;
                }
                invalidated.0 = true;
            }
            UiAction::SelectWaveformSource(source) => {
                let mut notice = None;
                if let Some(editor) = session.editor.as_mut() {
                    if *source == WaveformSource::Vocals && editor.chart.audio.vocals.is_none() {
                        notice = Some("This chart has no separate vocal source.".to_string());
                    } else {
                        set_editor_waveform_source(editor, *source);
                    }
                    editor.waveform_context = None;
                }
                if notice.is_some() {
                    session.notice = notice;
                }
                invalidated.0 = true;
            }
            UiAction::SelectWaveformStyle(style) => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.waveform_style = *style;
                    editor.waveform_context = None;
                }
                invalidated.0 = true;
            }
            UiAction::DismissWaveformContext => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.waveform_context = None;
                }
                invalidated.0 = true;
            }
            UiAction::DismissProblemsPanel => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.problems_panel_open = false;
                }
                invalidated.0 = true;
            }
            UiAction::DismissShortcutsPanel => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.shortcuts_panel_open = false;
                }
                invalidated.0 = true;
            }
            UiAction::SetProblemsFilter(filter) => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.problems_filter = *filter;
                }
                invalidated.0 = true;
            }
            UiAction::ApplyAllLyricsEdit => {
                let mut notice = None;
                if let Some(editor) = session.editor.as_mut()
                    && let Ok(input) = text_inputs.all_lyrics.single()
                {
                    let text = input.value().to_string();
                    let lines = text.split('\n').collect::<Vec<_>>();
                    let phrase_count = editor.document.phrase_count();
                    if lines.len() != phrase_count {
                        notice = Some(format!(
                            "This chart has {phrase_count} phrase(s) but the text has {} line(s) — keep exactly one line per phrase (blank lines are fine), then apply again.",
                            lines.len()
                        ));
                    } else {
                        editor.checkpoint("Retype all lyrics");
                        let mut changed = false;
                        for (index, line) in lines.iter().enumerate() {
                            changed |= editor.document.set_phrase_token_text(index, line);
                        }
                        if changed {
                            editor.dirty = true;
                            notice = Some("Updated all lyrics.".to_string());
                        } else {
                            editor.undo.pop();
                        }
                        editor.all_lyrics_editor_open = false;
                    }
                }
                if notice.is_some() {
                    session.notice = notice;
                }
                invalidated.0 = true;
            }
            UiAction::ExtendLyricOverNote(word, note_index) => {
                if let Some(editor) = session.editor.as_mut() {
                    editor.checkpoint("Continue syllable");
                    if extend_editor_lyric(&mut editor.document, *word, *note_index) {
                        editor.dirty = true;
                        editor.note_context = None;
                        session.notice = Some("Extended the syllable onto this note.".to_string());
                    } else {
                        editor.undo.pop();
                        session.notice = Some(
                            "That note can't continue the syllable — it needs to sit right after it, in the same phrase, with no lyric of its own.".to_string(),
                        );
                    }
                }
                invalidated.0 = true;
            }
            UiAction::Editor(_)
            | UiAction::FocusChartProblem(..)
            | UiAction::OpenEditorSelect(_)
            | UiAction::SelectEditorValue(..)
            | UiAction::SelectEditorWord(..)
            | UiAction::SelectEditorTrack(_)
            | UiAction::MoveSelectionToTrack(_)
            | UiAction::SetNoteKind(_) => {
                handle_editor_ui_action(
                    action,
                    &keys,
                    &mut EditorActionContext {
                        audio: &audio.0,
                        tones: &pitch_audition.0,
                        session: &mut session,
                        invalidated: &mut invalidated,
                    },
                );
            }
        }
    }
}

fn update_navigation_focus_visuals(
    focus: Res<InputFocus>,
    focus_visible: Res<InputFocusVisible>,
    theme: Res<StudioTheme>,
    mut buttons: Query<(Entity, &mut Outline), With<UiAction>>,
) {
    if !focus.is_changed() && !focus_visible.is_changed() && !theme.is_changed() {
        return;
    }
    for (entity, mut outline) in &mut buttons {
        outline.color = if focus_visible.0 && focus.get() == Some(entity) {
            theme.primary.with_alpha(0.58)
        } else {
            Color::NONE
        };
    }
}

fn handle_fullscreen_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if let Some(error) = toggle_fullscreen(&mut window, &mut session.config) {
        session.notice = Some(error);
    }
    invalidated.0 = true;
}

fn toggle_fullscreen(window: &mut Window, config: &mut AppConfig) -> Option<String> {
    let fullscreen = matches!(window.mode, WindowMode::Windowed);
    window.mode = if fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    config.fullscreen = Some(fullscreen);
    save_config_error(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an editable document from (start, end, midi, syllable) tuples.
    fn document_fixture(notes: &[(f64, f64, u8, &str)]) -> app_core::EditorDocument {
        let transcript = serde_json::json!({
            "language": "en",
            "segments": [{
                "start": notes.first().map(|note| note.0).unwrap_or(0.0),
                "end": notes.last().map(|note| note.1).unwrap_or(0.0),
                "text": notes.iter().map(|note| note.3).collect::<Vec<_>>().join(" "),
                "words": notes
                    .iter()
                    .map(|(start, end, _, text)| serde_json::json!({
                        "word": text,
                        "start": start,
                        "end": end,
                    }))
                    .collect::<Vec<_>>(),
            }]
        });
        let pitch_notes = serde_json::json!({
            "notes": notes
                .iter()
                .map(|(start, end, midi, _)| serde_json::json!({
                    "start": start,
                    "end": end,
                    "midi": midi,
                    "confidence": 1.0,
                }))
                .collect::<Vec<_>>(),
        });
        app_core::EditorDocument::new(
            app_core::migrate_analyzer_chart(&transcript, &pitch_notes).unwrap(),
        )
    }

    fn chart_fixture(notes: &[(f64, f64, u8, &str)]) -> app_core::ChartDocument {
        app_core::ChartDocument {
            file_hash: "fixture".to_string(),
            vocal_chart: document_fixture(notes).to_chart(),
            pitch_track: serde_json::json!({}),
            audio: app_core::ChartAudio {
                instrumental: "instrumental.flac".to_string(),
                vocals: None,
                original: "original.flac".to_string(),
            },
            repaired_issues: Vec::new(),
        }
    }

    #[test]
    fn native_window_preserves_existing_desktop_geometry() {
        let window = studio_window(&AppConfig::default(), true);
        assert_eq!(window.title, "Uta Studio");
        assert_eq!(window.width(), 1280.0);
        assert_eq!(window.height(), 720.0);
        assert!(window.decorations);
        assert!(!window.transparent);
        assert_eq!(window.window_theme, Some(WindowTheme::Dark));
    }

    #[test]
    fn duration_format_matches_the_library_table() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(65.2), "1:05");
        assert_eq!(format_duration(f64::NAN), "0:00");
    }

    #[test]
    fn navigation_repeat_matches_the_restored_controller_cadence() {
        let started = Instant::now();
        let mut state = NavigationInputState::default();
        assert_eq!(
            navigation_repeat(&mut state, Some(NavigationDirection::Next), started),
            Some(NavigationDirection::Next)
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Next),
                started + NAVIGATION_INITIAL_REPEAT - Duration::from_millis(1),
            ),
            None
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Next),
                started + NAVIGATION_INITIAL_REPEAT,
            ),
            Some(NavigationDirection::Next)
        );
        assert_eq!(
            navigation_repeat(
                &mut state,
                Some(NavigationDirection::Previous),
                started + NAVIGATION_INITIAL_REPEAT,
            ),
            Some(NavigationDirection::Previous)
        );
        assert_eq!(navigation_repeat(&mut state, None, started), None);
        assert_eq!(state.held_direction, None);
        assert_eq!(state.repeat_at, None);
    }

    #[test]
    fn navigation_skips_invisible_dismiss_backdrops() {
        assert!(!action_is_navigation_target(&UiAction::CloseActivity));
        assert!(!action_is_navigation_target(&UiAction::DismissSongContext));
        assert!(action_is_navigation_target(&UiAction::OpenAbout));
    }

    #[test]
    fn button_feedback_preserves_authored_surfaces_and_activity_backdrop() {
        let theme = StudioTheme::new(false);
        let resting = theme.card.with_alpha(0.46);
        assert_eq!(
            button_background(
                &UiAction::ToggleLibraryLayout,
                Interaction::None,
                resting,
                &theme,
            ),
            resting
        );
        assert_ne!(
            button_background(
                &UiAction::ToggleLibraryLayout,
                Interaction::Hovered,
                resting,
                &theme,
            ),
            Color::NONE
        );
        assert_eq!(
            button_background(
                &UiAction::CloseActivity,
                Interaction::Hovered,
                theme.background.with_alpha(0.54),
                &theme,
            ),
            theme.background.with_alpha(0.54)
        );
    }

    #[test]
    fn editor_audio_failure_does_not_block_chart_authoring() {
        let status = editor_audio_status(Err("missing typefind".to_string()));
        assert!(!status.loaded);
        assert_eq!(status.error.as_deref(), Some("missing typefind"));
    }

    #[test]
    fn setup_request_preserves_the_selected_backend_and_artifact() {
        let mut config = AppConfig {
            compute_backend: Some("intel".to_string()),
            ..AppConfig::default()
        };
        let folders = setup_folders(
            &config,
            SetupRequest {
                target: Some(app_core::ModelDownloadTarget::Pitch),
            },
        );
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Intel);
        assert_eq!(
            folders.model_target,
            Some(app_core::ModelDownloadTarget::Pitch)
        );

        config.compute_backend = Some("cuda".to_string());
        let folders = setup_folders(&config, SetupRequest { target: None });
        assert_eq!(folders.compute_backend, app_core::ComputeBackend::Cuda);
        assert_eq!(folders.model_target, None);
    }

    #[test]
    fn development_asset_root_contains_the_canonical_brand_assets() {
        let root = std::path::PathBuf::from(asset_root());
        assert!(root.join(LOGO_PATH).is_file());
        assert!(root.join(FONT_PATH).is_file());
        assert!(root.join(ICON_ATLAS_PATH).is_file());
        // Baked in via `include_bytes!`, not loaded from `asset_root()` --
        // a missing file would already be a compile error, but a real PNG
        // signature is worth confirming rather than assuming.
        assert!(LOGO_BYTES.starts_with(b"\x89PNG"));
        assert!(BANNER_BYTES.starts_with(b"\x89PNG"));
        assert!(root.join("desktop/assets/icons/ui-icons.svg").is_file());
    }

    #[test]
    fn expected_icu_cjk_fallback_does_not_flood_desktop_logs() {
        assert!(studio_log_filter().contains("icu_provider=error"));
    }

    #[test]
    fn export_stem_cannot_escape_the_selected_directory() {
        assert_eq!(safe_file_stem("../A/B: C?.utz"), "_A_B_ C_.utz");
        assert_eq!(safe_file_stem("..."), "Uta Studio Export");
    }

    #[test]
    fn lyric_drag_snaps_its_closest_edge_to_a_note_boundary() {
        let words = vec![EditorWordOriginal {
            selection: WordSelection {
                segment: 0,
                word: 0,
            },
            start: 1.0,
            end: 1.4,
        }];
        let notes = vec![ChartNoteView {
            index: 0,
            start: 1.3,
            end: 1.8,
            midi: 60.0,
            pitched: true,
            placeholder: false,
            kind: app_core::NoteKind::Normal,
            lyric: None,
            continues_lyric: false,
        }];

        let snap = snap_lyric_move_to_notes(&words, 0.27, &notes, 0.05).unwrap();
        assert!((snap.delta - 0.3).abs() < f64::EPSILON);
        assert!((snap.target - 1.3).abs() < f64::EPSILON);
        assert!(snap_lyric_move_to_notes(&words, 0.2, &notes, 0.05).is_none());
    }

    #[test]
    fn lyric_note_snap_never_moves_a_group_before_zero() {
        let words = vec![EditorWordOriginal {
            selection: WordSelection {
                segment: 0,
                word: 0,
            },
            start: 0.1,
            end: 0.4,
        }];
        let notes = vec![ChartNoteView {
            index: 0,
            start: 0.0,
            end: 0.2,
            midi: 60.0,
            pitched: true,
            placeholder: false,
            kind: app_core::NoteKind::Normal,
            lyric: None,
            continues_lyric: false,
        }];

        let snap = snap_lyric_move_to_notes(&words, -0.05, &notes, 0.2).unwrap();
        assert!(snap.delta >= -0.1);
    }

    #[test]
    fn overlapping_lyrics_use_separate_lanes_and_mark_missing_guidance() {
        let mut document = document_fixture(&[(0.0, 0.7, 60, "one"), (0.8, 1.2, 62, "two")]);
        // A lyric with no pitch target is the format's way of holding an
        // unguided word, and the lane must still show it.
        let unguided = document.insert_lyric(None, 3.0).unwrap();
        document.set_lyric_text(unguided, "three");
        let lyrics = chart_lyrics(&document);
        assert_eq!(lyrics.len(), 3);
        assert!(lyrics[0].guided);
        assert!(lyrics[1].guided);
        assert!(!lyrics[2].guided);
    }

    #[test]
    fn history_names_each_edit_and_survives_undo_redo() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (1.0, 2.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.checkpoint("Move note");
        editor.document.move_note(0, 3.0, 3.5, 64.0);
        editor.checkpoint("Delete notes");
        editor.document.remove_notes(&BTreeSet::from([1]));
        assert_eq!(editor.history().0, ["Move note", "Delete notes"]);

        assert_eq!(editor.undo(), Some("Delete notes"));
        assert_eq!(editor.document.note_count(), 2);
        assert_eq!(editor.undo(), Some("Move note"));
        assert!((editor.document.notes()[0].start - 0.0).abs() < 1e-9);
        assert_eq!(editor.undo(), None);

        assert_eq!(editor.redo(), Some("Move note"));
        assert!((editor.document.notes()[0].start - 3.0).abs() < 1e-9);
        assert_eq!(editor.redo(), Some("Delete notes"));
        assert_eq!(editor.document.note_count(), 1);
        assert_eq!(editor.redo(), None);
    }

    /// Runs one tap: hold at `down`, release at `up`.
    fn tap(editor: &mut NativeEditor, down: f64, up: f64) {
        editor.visible_position = down;
        let at = editor.visible_position.max(0.0);
        match editor.tap.next_retarget() {
            Some(index) => {
                let note = chart_notes(&editor.document)[index].clone();
                let length = (note.end - note.start).max(app_core::MIN_NOTE_SECONDS);
                move_chart_note(&mut editor.document, index, at, at + length, note.midi);
                editor.tap.holding = Some((index, at));
            }
            None => {
                let index = insert_chart_note(
                    &mut editor.document,
                    at,
                    at + app_core::MIN_NOTE_SECONDS,
                    60.0,
                )
                .unwrap();
                editor.select_only_note(index);
                editor.tap.holding = Some((index, at));
            }
        }
        editor.visible_position = up;
        finish_tap(editor);
    }

    #[test]
    fn taps_retime_the_queued_notes_in_order_then_stop() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        editor.tap.retiming = vec![0, 1];

        tap(&mut editor, 5.0, 5.4);
        assert_eq!(editor.tap.remaining(), 1);
        tap(&mut editor, 6.0, 6.5);
        assert_eq!(editor.tap.remaining(), 0);

        let notes = chart_notes(&editor.document);
        assert!((notes[0].start - 5.0).abs() < 1e-9);
        assert!((notes[0].end - 5.4).abs() < 1e-9);
        assert!((notes[1].start - 6.0).abs() < 1e-9);
        // Re-timing keeps the pitch that was authored.
        assert!((notes[1].midi - 62.0).abs() < 1e-9);
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn taps_with_nothing_queued_lay_down_new_notes() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        tap(&mut editor, 2.0, 2.3);
        tap(&mut editor, 3.0, 3.2);
        let notes = chart_notes(&editor.document);
        assert_eq!(notes.len(), 3);
        assert!((notes[1].end - notes[1].start - 0.3).abs() < 1e-3);
        assert!((notes[2].start - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_tap_shorter_than_the_minimum_still_makes_a_valid_note() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.tap_mode = true;
        tap(&mut editor, 4.0, 4.0);
        let note = chart_notes(&editor.document)[1].clone();
        assert!(note.end - note.start >= app_core::MIN_NOTE_SECONDS - 1e-9);
    }

    #[test]
    fn a_ranged_audition_covers_the_selection_and_its_approaches() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[
                (4.0, 5.0, 60, "a"),
                (5.0, 6.0, 62, "b"),
                (9.0, 10.0, 64, "c"),
            ]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.viewport_start = 2.0;
        editor.viewport_duration = 6.0;
        editor.selected_notes = BTreeSet::from([0, 1]);

        let selection = audition_range(EditorAction::AuditionSelection, &editor).unwrap();
        assert!((selection.0 - 4.0).abs() < 1e-9);
        assert!((selection.1 - 6.0).abs() < 1e-9);
        // The lead-in stops where the selection starts, and the lead-out picks
        // up where it ends, so a transition is heard from both sides.
        let before = audition_range(EditorAction::AuditionBeforeSelection, &editor).unwrap();
        assert!((before.1 - 4.0).abs() < 1e-9);
        assert!(before.0 < before.1);
        let after = audition_range(EditorAction::AuditionAfterSelection, &editor).unwrap();
        assert!((after.0 - 6.0).abs() < 1e-9);
        assert!(after.1 > after.0);
        assert_eq!(
            audition_range(EditorAction::AuditionVisible, &editor),
            Some((2.0, 8.0))
        );

        editor.selected_notes.clear();
        assert!(audition_range(EditorAction::AuditionSelection, &editor).is_none());
    }

    #[test]
    fn pitch_audition_sounds_only_the_notes_in_range() {
        let editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (4.0, 5.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        let tones = pitch_tones(&editor.document, 3.5, 6.0);
        assert_eq!(tones.len(), 1);
        // Tones are positioned against the start of the audition, and clipped
        // to it, so the preview lines up with the transport.
        assert!((tones[0].start_secs - 0.5).abs() < 1e-9);
        assert!((tones[0].duration_secs - 1.0).abs() < 1e-9);
        assert!((tones[0].midi - 62.0).abs() < 1e-9);
    }

    #[test]
    fn ghost_notes_show_the_other_tracks_and_never_the_active_one() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a"), (2.0, 3.0, 62, "b")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        assert!(other_track_notes(&editor.document).is_empty());
        editor.document.add_track(app_core::TrackRole::Lead);
        editor.document.set_active_track(0);
        editor.document.move_notes_to_track(&BTreeSet::from([1]), 1);

        let notes = chart_notes(&editor.document);
        let ghosts = other_track_notes(&editor.document);
        assert_eq!(notes.len(), 1);
        assert_eq!(ghosts.len(), 1);
        assert!((ghosts[0].start - 2.0).abs() < 1e-9);
        // Switching tracks swaps which side is editable.
        editor.document.set_active_track(1);
        assert_eq!(chart_notes(&editor.document).len(), 1);
        assert!((other_track_notes(&editor.document)[0].start - 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack_and_bounds_history() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.checkpoint("Move note");
        assert_eq!(editor.undo(), Some("Move note"));
        editor.checkpoint("Add note");
        assert!(editor.history().1.is_empty());

        for _ in 0..120 {
            editor.checkpoint("Nudge notes");
        }
        assert_eq!(editor.history().0.len(), 100);
    }

    #[test]
    fn editor_viewport_maps_time_and_pitch_independently() {
        let mut editor = NativeEditor::new(
            chart_fixture(&[(0.0, 1.0, 60, "a")]),
            uta_studio_audio::EditorAudioStatus::default(),
            app_core::ChartWaveform::default(),
            WaveformSource::Instrumental,
            "instrumental",
        );
        editor.viewport_start = 10.0;
        editor.viewport_duration = 20.0;
        editor.pitch_min = 40.0;
        editor.pitch_max = 80.0;
        assert_eq!(time_percent(20.0, &editor), 50.0);
        assert_eq!(pitch_percent(60.0, &editor), 58.0);
        assert_eq!(time_percent(5.0, &editor), 0.0);
        assert_eq!(pitch_percent(100.0, &editor), 20.0);
        assert_eq!(surface_pitch_fraction(0.2), 0.0);
        assert_eq!(surface_pitch_fraction(0.96), 1.0);
        set_editor_pitch_span(&mut editor, 999.0);
        assert_eq!(editor.pitch_min, 0.0);
        assert_eq!(editor.pitch_max, 127.0);
    }

    #[test]
    fn quantization_and_safe_repair_keep_valid_note_ranges() {
        let mut document =
            document_fixture(&[(1.023, 1.071, 60, "hello"), (1.2, 1.3, 61, "world")]);
        assert_eq!(quantize_chart_notes(&mut document, None, 0.05), 2);
        let notes = chart_notes(&document);
        assert!((notes[0].start - 1.0).abs() < 1e-9);
        assert!((notes[0].end - 1.05).abs() < 1e-9);
        assert!(repair_editor_chart(&mut document));
        let notes = chart_notes(&document);
        assert!(notes[0].end <= notes[1].start);
        assert!(!document.problems().blocks_saving());
        document.to_chart().validate().unwrap();
    }

    #[test]
    fn pitch_contour_is_bounded_and_confidence_weighted() {
        let frames = (0..100)
            .map(|index| ChartPitchFrame {
                time: f64::from(index) * 0.01,
                midi: 60.0 + f64::from(index % 3),
                confidence: if index % 2 == 0 { 1.0 } else { 0.2 },
            })
            .collect::<Vec<_>>();
        let contour = abstract_pitch_contour(&frames, 20);
        assert!(contour.len() <= 20);
        assert!(contour.iter().all(|frame| frame.midi.is_finite()));
        assert!(contour.windows(2).all(|pair| pair[0].time < pair[1].time));
    }

    #[test]
    fn pitch_evidence_converts_only_voiced_finite_frames() {
        let mut chart = chart_fixture(&[(0.0, 1.0, 60, "a")]);
        chart.pitch_track = serde_json::json!({
            "frames": [
                {"time": 1.0, "hz": 440.0, "confidence": 0.9},
                {"time": 1.1, "hz": null, "confidence": 0.1}
            ]
        });
        let frames = chart_pitch_frames(&chart);
        assert_eq!(frames.len(), 1);
        assert!((frames[0].midi - 69.0).abs() < f64::EPSILON);
    }
}
