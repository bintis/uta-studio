use crate::studio::*;

pub(crate) const FONT_PATH: &str = "desktop/assets/fonts/NotoSansCJKsc-Regular.otf";

pub(crate) const LOGO_PATH: &str = "icon.png";

/// Baked into the binary (see `setup`'s `BrandImages`) rather than loaded
/// via `AssetServer` like `LOGO_PATH` -- neither needs to be user-replaceable
/// at runtime, and embedding means one less file the packaged build has to
/// carry and locate correctly.
pub(crate) const LOGO_BYTES: &[u8] = include_bytes!("../../../icon.png");

pub(crate) const BANNER_BYTES: &[u8] = include_bytes!("../../../Banner.png");
pub(crate) const STARTUP_BANNER_BYTES: &[u8] = include_bytes!("../../../Banner0.png");
pub(crate) const STARTUP_BANNER_FADE_IN_SECONDS: f32 = 0.40;
pub(crate) const STARTUP_BANNER_HOLD_SECONDS: f32 = 0.30;
pub(crate) const STARTUP_BANNER_FADE_OUT_SECONDS: f32 = 0.40;
pub(crate) const STARTUP_BANNER_WIDTH: f32 = 620.0;
pub(crate) const STARTUP_BANNER_HEIGHT: f32 = STARTUP_BANNER_WIDTH * 3.0 / 4.0;

/// Decoded once in `setup` from embedded bytes and reused by
/// every `rebuild_ui` pass after that, the same "decode once, hand out
/// cheap `Handle` clones" shape `LocalImages` already uses for cover art.
#[derive(Resource, Clone)]
pub(crate) struct BrandImages {
    pub(crate) logo: Handle<Image>,
    pub(crate) banner: Handle<Image>,
    pub(crate) startup_banner: Handle<Image>,
}

#[derive(Component)]
pub(crate) struct StartupBannerRoot;

#[derive(Component)]
pub(crate) struct StartupBannerImage;

#[derive(Resource)]
pub(crate) struct StartupBannerState {
    pub(crate) timer: Timer,
    pub(crate) done: bool,
    pub(crate) restore_window_mode: WindowMode,
}

impl Default for StartupBannerState {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(
                STARTUP_BANNER_FADE_IN_SECONDS
                    + STARTUP_BANNER_HOLD_SECONDS
                    + STARTUP_BANNER_FADE_OUT_SECONDS,
                TimerMode::Once,
            ),
            done: false,
            restore_window_mode: WindowMode::Windowed,
        }
    }
}

impl StartupBannerState {
    pub(crate) fn for_launch(restore_window_mode: WindowMode) -> Self {
        let mut state = Self::default();
        state.restore_window_mode = restore_window_mode;
        state
    }
}

impl StartupBannerState {
    pub(crate) fn alpha(&self) -> f32 {
        if self.done {
            return 0.0;
        }

        let elapsed = self.timer.elapsed_secs();
        if elapsed < STARTUP_BANNER_FADE_IN_SECONDS {
            elapsed / STARTUP_BANNER_FADE_IN_SECONDS
        } else if elapsed < STARTUP_BANNER_FADE_IN_SECONDS + STARTUP_BANNER_HOLD_SECONDS {
            1.0
        } else if elapsed
            < STARTUP_BANNER_FADE_IN_SECONDS
                + STARTUP_BANNER_HOLD_SECONDS
                + STARTUP_BANNER_FADE_OUT_SECONDS
        {
            1.0 - (elapsed - STARTUP_BANNER_FADE_IN_SECONDS - STARTUP_BANNER_HOLD_SECONDS)
                / STARTUP_BANNER_FADE_OUT_SECONDS
        } else {
            0.0
        }
    }
}

pub(crate) fn decode_embedded_png(bytes: &[u8], images: &mut Assets<Image>) -> Handle<Image> {
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

pub(crate) const SIDEBAR_WIDTH: f32 = 265.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StudioRoute {
    #[default]
    Library,
    Folders,
    SongDetail,
    Settings,
    Documentation,
    Editor,
    AnalysisInspect,
}

#[derive(Resource)]
pub(crate) struct StudioSession {
    pub(crate) config: AppConfig,
    pub(crate) meta: SongsMeta,
    pub(crate) songs: SongsStore,
    pub(crate) scanning: bool,
    pub(crate) route: StudioRoute,
    pub(crate) documentation: DocumentationState,
    pub(crate) selected_artifact_inspector_tab: ArtifactInspectorTab,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) library_view: LibraryView,
    pub(crate) library_search: Option<String>,
    pub(crate) library_status: Option<String>,
    pub(crate) library_transcript_source: Option<String>,
    pub(crate) library_facet: Option<LibraryFacet>,
    pub(crate) menu_items: app_core::LibraryMenuItems,
    pub(crate) notice: Option<String>,
    pub(crate) selected_song: Option<String>,
    pub(crate) editor: Option<NativeEditor>,
    pub(crate) folder_browser: FolderBrowser,
    pub(crate) song_context: Option<SongContextMenu>,
    pub(crate) analysis_node_context: Option<AnalysisNodeContextMenu>,
    pub(crate) analysis_artifact_context: Option<AnalysisArtifactContextMenu>,
    pub(crate) analysis_export_context: Option<AnalysisExportContextMenu>,
    pub(crate) selected_graph_edge: Option<SelectedGraphEdge>,
    pub(crate) analysis_lineage_mode: bool,
    pub(crate) analysis_lineage_scope: LineageScope,
    pub(crate) pending_setup: Option<SetupRequest>,
    pub(crate) diagnostic_report: Option<uta_studio_diagnostics::DiagnosticReport>,
    pub(crate) lyrics_editor: Option<NativeLyricsEditor>,
    pub(crate) pending_cache_delete: Option<String>,
    pub(crate) pending_artifact_delete: Option<app_core::ArtifactRevision>,
    /// Phase 6 `invalidate_artifact_revision` / Phase 7 §7.6 "Invalidate":
    /// destructive-classified, so it goes through the same
    /// request/cancel/confirm flow as `pending_artifact_delete` even though
    /// (unlike Delete) it never removes the file.
    pub(crate) pending_artifact_invalidate: Option<app_core::ArtifactRevision>,
    pub(crate) pending_artifact_active: Option<app_core::ArtifactRevision>,
    pub(crate) pending_intermediate_capture: Option<String>,
    pub(crate) artifact_diff: Option<app_core::ArtifactTypedDiff>,
    pub(crate) artifact_lineage: Option<ArtifactLineagePanel>,
    pub(crate) artifact_impact: Option<app_core::DownstreamImpact>,
    /// Phase 5 §5.4 "Replace 必须经过确认": file_hash of the song whose
    /// Authored Chart the user has asked (but not yet confirmed) to discard
    /// in favor of the current candidate analysis output.
    pub(crate) pending_chart_replace: Option<String>,
    pub(crate) authoring_busy: bool,
    pub(crate) language_editor: Option<NativeLanguageEditor>,
    /// Phase 8 "Configure for this run…" -- a draft one-run override for a
    /// single node's profile-controlled field, committed via
    /// `app_core::configure_analysis_node_for_run`.
    pub(crate) node_config_dialog: Option<NativeNodeConfigDialog>,
    /// Phase 7/8 Plan Preview panel: a staged, not-yet-committed
    /// disabled-node combination, previewed live via
    /// `app_core::preview_analysis_plan_for_selection` and committed via
    /// `run_analysis_plan` only when the user explicitly runs it.
    pub(crate) plan_preview_draft: Option<PlanPreviewDraft>,
    /// §7.5 "View logs" -- which node's context menu it was opened from.
    pub(crate) app_log_viewer: Option<AppLogViewerState>,
    pub(crate) song_settings: Option<NativeSongSettings>,
    pub(crate) pending_cache_clear: Option<CacheClearScope>,
    pub(crate) pending_leave: Option<PendingLeave>,
    pub(crate) open_settings_select: Option<SettingsSelectKind>,
    pub(crate) open_analysis_advanced: Option<AnalysisAdvancedSection>,
    pub(crate) settings_scroll_offsets: [f32; 4],
    pub(crate) library_scroll_offset: f32,
    pub(crate) analysis_graph_scroll_offset: f32,
    /// DAG canvas zoom (§7.8/§9.3 "DAG 支持 Pan、Zoom、Fit"). 1.0 is
    /// unscaled; clamped to `ANALYSIS_GRAPH_ZOOM_RANGE`. Applied by scaling
    /// the already-computed layout rects before spawning node/edge boxes
    /// (`spawn_analysis_session_overview`), not via a visual-only UI
    /// transform, so the scroll viewport's real content size -- and
    /// therefore panning and click hit-testing -- stays correct at any
    /// zoom level instead of drifting out of sync with what's drawn.
    pub(crate) analysis_graph_zoom: f32,
    /// When true, `fit_analysis_graph_to_viewport` scales the DAG so the
    /// whole flow sits inside the canvas on the next layout pass.
    pub(crate) analysis_graph_needs_fit: bool,
    /// Last live `node_id` the canvas auto-centered on. Cleared when leaving
    /// the analysis page so reopening recenters the current step.
    pub(crate) analysis_graph_follow_node: Option<String>,
    pub(crate) open_library_select: Option<LibrarySelectKind>,
    pub(crate) export_all_open: bool,
    pub(crate) open_editor_select: Option<EditorDockSelectKind>,
    /// Whether the track strip is pinned open. A multi-track chart always
    /// shows it, because the active track decides what an edit touches.
    pub(crate) editor_tracks_open: bool,
    pub(crate) analysis_tasks: Vec<app_core::AnalysisTask>,
    pub(crate) analysis_history: Vec<app_core::AnalysisRunHistory>,
    pub(crate) selected_analysis_history: Option<i64>,
    pub(crate) selected_analysis_stage: Option<String>,
    /// §7.3 "Music Analysis 支持展开": which compound nodes currently show
    /// their children as separate boxes instead of one collapsed box with a
    /// "N sub-checks not shown" note. Toggled from the Node Context Menu
    /// (`UiAction::ToggleAnalysisCompoundNode`) -- `analysis_model.rs`'s
    /// `build_graph_view_model` has taken this set as a parameter since
    /// Phase 7 landed, it just always got an empty one until this field
    /// existed to feed it something real.
    pub(crate) expanded_compound_nodes: std::collections::BTreeSet<app_core::AnalysisNodeId>,
    /// Mutually exclusive with the full DAG canvas -- toggled by the "VIEW"
    /// row's MINI/Full button (`UiAction::ToggleAnalysisMiniView`). While on,
    /// the graph is built as if `expanded_compound_nodes` were empty, so
    /// only the top-level, model-backed nodes render regardless of what's
    /// individually expanded in the full view.
    pub(crate) analysis_mini_view: bool,
    pub(crate) pending_analysis_history_clear: bool,
    pub(crate) request_cache_stats_refresh: bool,
    pub(crate) search_open: bool,
    pub(crate) activity_open: bool,
    pub(crate) about_open: bool,
    pub(crate) library_playback: LibraryPlayback,
    pub(crate) export_job: NativeExportJob,
    pub(crate) editor_load_job: NativeEditorLoadJob,
    pub(crate) lyrics_search_job: NativeLyricsSearchJob,
    pub(crate) lyrics_waveform_job: NativeLyricsWaveformJob,
}

impl StudioSession {
    pub(crate) fn load() -> Self {
        let config = AppConfig::load();
        let folder_browser = FolderBrowser::new(&config);
        Self {
            config,
            meta: SongsStore::load_meta(),
            songs: load_songs(LibraryView::All.filters()),
            scanning: false,
            route: StudioRoute::Library,
            documentation: DocumentationState::default(),
            selected_artifact_inspector_tab: ArtifactInspectorTab::default(),
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
            analysis_artifact_context: None,
            analysis_export_context: None,
            selected_graph_edge: None,
            analysis_lineage_mode: false,
            analysis_lineage_scope: LineageScope::Full,
            pending_setup: None,
            diagnostic_report: None,
            lyrics_editor: None,
            pending_cache_delete: None,
            pending_artifact_delete: None,
            pending_artifact_invalidate: None,
            pending_artifact_active: None,
            pending_intermediate_capture: None,
            artifact_diff: None,
            artifact_lineage: None,
            artifact_impact: None,
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
            analysis_graph_zoom: ANALYSIS_GRAPH_ZOOM_DEFAULT,
            analysis_graph_needs_fit: true,
            analysis_graph_follow_node: None,
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
            lyrics_waveform_job: NativeLyricsWaveformJob::default(),
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
    pub(crate) fn with_debug_navigation(mut self) -> Self {
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
                capture_intermediate_action: (node_id == "lyrics.preprocess")
                    .then(|| UiAction::RequestCaptureIntermediate(history.file_hash.clone())),
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

    pub(crate) fn refresh_library(&mut self) {
        self.meta = SongsStore::load_meta();
        self.songs = load_songs(self.library_filters());
        self.menu_items = app_core::load_library_menu_items().unwrap_or_default();
    }

    pub(crate) fn load_more_songs(&mut self) {
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

    pub(crate) fn selected_song(&self) -> Option<Song> {
        let hash = self.selected_song.as_deref()?;
        app_core::load_song_by_hash(hash).ok().flatten()
    }

    pub(crate) fn library_filters(&self) -> LibraryMenuFilters {
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

    pub(crate) fn library_title(&self) -> &str {
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
    Documentation,
}

#[derive(Resource)]
pub(crate) struct NativeAudio(
    #[allow(dead_code)] pub(crate) Arc<uta_studio_audio::EditorAudioPlayer>,
);

/// The synthesized pitch stream. It is a second player so auditioning a note
/// target never alters, mixes into, or re-encodes the song audio.
#[derive(Resource)]
pub(crate) struct NativePitchAudition(pub(crate) Arc<uta_studio_audio::PitchAudition>);

#[derive(Resource, Default)]
pub(crate) struct UiInvalidated(pub(crate) bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationDirection {
    Previous,
    Next,
}

#[derive(Resource, Default)]
pub(crate) struct NavigationInputState {
    pub(crate) held_direction: Option<NavigationDirection>,
    pub(crate) repeat_at: Option<Instant>,
    pub(crate) activated: Option<Entity>,
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
    Documentation,
    OpenDocumentation(Option<String>),
    DocumentationBack,
    DocumentationForward,
    SelectArtifactInspectorTab(ArtifactInspectorTab),
    ToggleArtifactPinned(app_core::ArtifactRef),
    OpenArtifactCompatibleEditor(app_core::ArtifactRef),
    MergeCandidateChart(
        app_core::ArtifactRef,
        app_core::ArtifactRef,
        app_core::ChartRevisionMergeMode,
    ),
    MergeSelectedCandidatePhrase(app_core::ArtifactRef, app_core::ArtifactRef),
    MergeSelectedCandidateRange(app_core::ArtifactRef, app_core::ArtifactRef),
    KeepAuthoredChart,
    ShowArtifactLineage(app_core::ArtifactRef),
    ShowArtifactImpact(app_core::ArtifactRef),
    SetArtifactLineageScope(LineageScope),
    SelectArtifactLineageRevision(app_core::ArtifactRef),
    CloseArtifactLineage,
    CloseArtifactImpact,
    ConfirmArtifactImpact,
    DismissAnalysisArtifactContext,
    ToggleAnalysisLineageMode,
    DismissAnalysisExportContext,
    ValidateExportNode(String, app_core::ExportPackageKind),
    RevealLastExport(String, app_core::ExportPackageKind),
    ToggleActivity,
    CloseActivity,
    SelectAnalysisHistory(Option<i64>),
    SelectAnalysisStage(String),
    OpenAnalysisInspect(String),
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
    InstallAudioModel(String),
    RemoveAudioModel(String),
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
    SaveLyricsEditorAndRunDownstream,
    AdjustTranscriptBoundary(TranscriptBoundaryTarget, TranscriptBoundaryEdge, i32),
    PreviewTranscriptAt(String, i64),
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
    CancelSetActiveArtifactRevision,
    ConfirmSetActiveArtifactRevision,
    RequestCaptureIntermediate(String),
    CancelCaptureIntermediate,
    ConfirmCaptureIntermediateOnce,
    ConfirmCaptureIntermediatePersistent,
    ConfirmDisableIntermediateCapture,
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
    CompareArtifactRevisions(app_core::ArtifactRevision, app_core::ArtifactRef),
    CloseArtifactDiff,
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
