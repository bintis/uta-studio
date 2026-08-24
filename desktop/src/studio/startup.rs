use crate::studio::*;

pub fn run() {
    let StudioStateBundle {
        shell,
        library,
        analysis,
        editor,
        dialogs,
        jobs,
        playback,
    } = StudioStateBundle::load();
    let native_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let native_library_audio = Arc::new(uta_studio_audio::EditorAudioPlayer::new());
    let theme = StudioTheme::new(shell.config.dark_mode.unwrap_or(false));
    set_ui_font_scale(shell.config.font_scale());
    let mut window = studio_window(&shell.config, theme.dark);
    let restore_window_mode = window.mode;
    if !matches!(restore_window_mode, WindowMode::Windowed) {
        window.mode = WindowMode::Windowed;
    }

    App::new()
        .insert_resource(ClearColor(theme.background))
        .insert_resource(theme)
        .insert_resource(shell)
        .insert_resource(library)
        .insert_resource(analysis)
        .insert_resource(editor)
        .insert_resource(dialogs)
        .insert_resource(jobs)
        .insert_resource(playback)
        .insert_resource(NativeAudio(native_audio))
        .insert_resource(NativePitchAudition(Arc::new(
            uta_studio_audio::PitchAudition::new(),
        )))
        .insert_resource(NativeLibraryAudio(native_library_audio))
        .insert_resource(LocalImages::default())
        .insert_resource(EditorPointerCapture::default())
        .insert_resource(EditorViewportRebuildThrottle::default())
        .insert_resource(UiInvalidated::default())
        .insert_resource(UiRebuildMetrics::default())
        .insert_resource(DebugScreenshotState::default())
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
        .insert_resource(StartupBannerState::for_launch(restore_window_mode))
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
        .add_systems(Update, update_startup_banner)
        .add_systems(
            Update,
            capture_debug_screenshot.after(update_startup_banner),
        )
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
        .add_systems(Update, handle_model_settings_request)
        .add_systems(Update, handle_window_close_requests)
        .add_systems(Update, handle_fullscreen_shortcut)
        .add_systems(Update, handle_documentation_shortcuts)
        .add_systems(Update, sync_documentation_search)
        .add_systems(Update, refresh_library_while_scanning)
        .add_systems(Update, refresh_analysis_activity)
        .add_systems(Update, handle_analysis_model_panel_scroll)
        .add_systems(
            Update,
            follow_live_analysis_node.after(refresh_analysis_activity),
        )
        .add_systems(Update, poll_native_setup)
        .add_systems(Update, poll_native_diagnostics)
        .add_systems(Update, poll_cache_stats)
        .add_systems(Update, poll_model_settings_job)
        .add_systems(Update, poll_authoring_job)
        .add_systems(Update, poll_export_job)
        .add_systems(Update, poll_editor_load_job)
        .add_systems(Update, poll_lyrics_search_job)
        .add_systems(Update, poll_lyrics_waveform_job)
        .add_systems(Update, sync_numeric_settings)
        .add_systems(Update, handle_tap_release)
        .add_systems(Update, sync_editor_word_input.after(rebuild_ui))
        .add_systems(Update, sync_editor_phrase_input)
        .add_systems(Update, sync_editor_singer_input)
        .add_systems(Update, finish_inline_lyric_edit)
        .add_systems(Update, handle_library_search_keyboard)
        .add_systems(Update, handle_plan_preview_keyboard)
        .add_systems(Update, handle_plan_preview_scroll)
        .add_systems(Update, handle_analysis_log_viewer_scroll)
        .add_systems(
            Update,
            refresh_editor_problems_cache
                .after(handle_actions)
                .after(handle_editor_pointer_capture)
                .before(rebuild_ui),
        )
        .add_systems(Update, rebuild_ui.after(handle_actions))
        .add_systems(Update, audit_ui_api_coverage.after(rebuild_ui))
        .add_systems(Update, localize_ui_text.after(rebuild_ui))
        .add_systems(Update, update_button_visuals.after(rebuild_ui))
        .add_systems(
            Update,
            finalize_ui_rebuild_metrics
                .after(rebuild_ui)
                .after(localize_ui_text)
                .after(update_button_visuals),
        )
        .add_systems(
            Update,
            update_navigation_focus_visuals
                .after(register_navigation_targets)
                .after(rebuild_ui),
        )
        .add_systems(Update, handle_editor_keyboard)
        .add_systems(
            Update,
            (handle_editor_wheel, flush_editor_viewport_rebuild)
                .chain()
                .before(rebuild_ui),
        )
        .add_systems(Update, handle_editor_pointer_capture)
        .add_systems(Update, handle_folder_scroll)
        .add_systems(Update, handle_problems_panel_scroll)
        .add_systems(Update, handle_shortcuts_panel_scroll)
        .add_systems(Update, handle_analysis_graph_scroll)
        .add_systems(Update, handle_analysis_inspect_scroll)
        .add_systems(Update, handle_library_scroll)
        .add_systems(Update, handle_song_detail_scroll)
        .add_systems(Update, handle_settings_scroll.before(rebuild_ui))
        .add_systems(Update, fit_analysis_graph_to_viewport.after(rebuild_ui))
        .add_systems(Update, sync_editor_audio)
        .add_systems(Update, sync_library_audio)
        .add_systems(Update, update_editor_geometry)
        .add_systems(Update, update_editor_playhead)
        .add_systems(Update, update_editor_binding_guides)
        .add_systems(Update, update_editor_shortcuts_panel_visibility)
        .add_systems(Update, update_library_player_ui)
        .run();
}

pub(crate) fn capture_debug_screenshot(
    mut commands: Commands,
    startup_banner: Res<StartupBannerState>,
    mut state: ResMut<DebugScreenshotState>,
) {
    let Some(path) = state.path.clone() else {
        return;
    };
    if state.requested || !startup_banner.done {
        return;
    }
    state.settled_frames = state.settled_frames.saturating_add(1);
    // The analysis activity timer performs its first scoped refresh about
    // three seconds after launch.  Capturing at 30 settled frames could land
    // in that deferred despawn/spawn frame and produce a half-empty image.
    // Wait through that first refresh so visual smoke evidence represents a
    // stable UI tree.
    // Leave enough frames for the viewport's final fullscreen width to feed
    // back through auto-Fit and rebuild once more. Capturing on that exact
    // fit frame records the previous zoom even though the interactive app
    // corrects it on the following frame.
    if state.settled_frames < 120 {
        return;
    }
    state.requested = true;
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(
            move |captured: On<bevy::render::view::screenshot::ScreenshotCaptured>,
                  mut app_exit: MessageWriter<AppExit>| {
                bevy::render::view::screenshot::save_to_disk(&path)(captured);
                app_exit.write(AppExit::Success);
            },
        );
}

pub(crate) fn studio_log_filter() -> String {
    format!("{DEFAULT_FILTER},icu_provider=error")
}

/// Application-lifecycle log capture. Per-song analysis progress, model
/// output, and tracebacks belong exclusively to each run's dedicated JSONL
/// log and must not be routed here. Writes go through
/// `tracing_subscriber::fmt`'s own event formatting into
/// `app_core::record_log_text`'s bounded ring buffer + best-effort log
/// file. Composes *alongside* Bevy's own default stdout layer via
/// `LogPlugin.custom_layer` -- stdout output is unaffected.
#[derive(Clone, Copy)]
pub(crate) struct AppLogWriter;

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

pub(crate) fn app_log_custom_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .with_writer(AppLogWriter)
            .with_ansi(false),
    ))
}

pub(crate) fn asset_root() -> String {
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
pub(crate) fn debug_window_size() -> Option<(u32, u32)> {
    let value = std::env::var("UTA_STUDIO_DEBUG_WINDOW_SIZE").ok()?;
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

pub(crate) fn studio_window(config: &AppConfig, dark: bool) -> Window {
    Window {
        title: "Uta Studio".to_string(),
        name: Some("com.uta-studio.desktop".to_string()),
        resolution: debug_window_size().unwrap_or((1280, 720)).into(),
        decorations: false,
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
        enabled_buttons: EnabledButtons {
            minimize: false,
            maximize: false,
            close: false,
        },
        ..default()
    }
}

#[derive(SystemParam)]
pub(crate) struct StartupUiResources<'w> {
    asset_server: Res<'w, AssetServer>,
    images: ResMut<'w, Assets<Image>>,
    local_images: ResMut<'w, LocalImages>,
    startup_banner: Res<'w, StartupBannerState>,
    state: StudioStateRead<'w>,
    native_setup: Res<'w, NativeSetup>,
    cache_stats: Res<'w, CacheStatsJob>,
    theme: Res<'w, StudioTheme>,
}

pub(crate) fn setup(mut commands: Commands, resources: StartupUiResources) {
    let StartupUiResources {
        asset_server,
        mut images,
        mut local_images,
        startup_banner,
        state,
        native_setup,
        cache_stats,
        theme,
    } = resources;
    commands.spawn(Camera2d);
    let brand = BrandImages {
        logo: decode_embedded_png(LOGO_BYTES, &mut images),
        banner: decode_embedded_png(BANNER_BYTES, &mut images),
        startup_banner: decode_embedded_png(STARTUP_BANNER_BYTES, &mut images),
    };
    // The very first frame, before the window (and any editor route that
    // could have a context menu open) exists — the configured default
    // resolution is a fine stand-in.
    let session = state.view();
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
    if !startup_banner.done {
        spawn_startup_banner(
            &mut commands,
            &startup_banner,
            &brand.startup_banner,
            &theme,
        );
    }
    commands.insert_resource(brand);
}

pub(crate) fn spawn_startup_banner(
    commands: &mut Commands,
    state: &StartupBannerState,
    startup_banner: &Handle<Image>,
    theme: &StudioTheme,
) {
    commands
        .spawn((
            StartupBannerRoot,
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
            BackgroundColor(theme.background),
            ZIndex(800),
        ))
        .with_children(|parent| {
            parent.spawn((
                StartupBannerImage,
                Node {
                    width: px(STARTUP_BANNER_WIDTH),
                    height: px(STARTUP_BANNER_HEIGHT),
                    ..default()
                },
                ImageNode::new(startup_banner.clone())
                    .with_color(Color::WHITE.with_alpha(state.alpha())),
            ));
        });
}

pub(crate) fn update_startup_banner(
    mut state: ResMut<StartupBannerState>,
    time: Res<Time>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut banner: Query<&mut ImageNode, With<StartupBannerImage>>,
    splash: Query<Entity, With<StartupBannerRoot>>,
    banner_image: Query<Entity, With<StartupBannerImage>>,
    mut commands: Commands,
) {
    if state.done {
        return;
    }

    state.timer.tick(time.delta());
    if let Ok(mut window) = windows.single_mut()
        && matches!(window.mode, WindowMode::Windowed)
    {
        window.enabled_buttons = EnabledButtons {
            minimize: false,
            maximize: false,
            close: false,
        };
        window.decorations = !state.timer.is_finished();
    }
    if let Ok(mut image) = banner.single_mut() {
        image.color = Color::WHITE.with_alpha(state.alpha());
    }

    if state.timer.is_finished() {
        state.done = true;
        if let Ok(mut window) = windows.single_mut() {
            window.mode = state.restore_window_mode;
            if matches!(window.mode, WindowMode::Windowed) {
                window.decorations = true;
                window.enabled_buttons = EnabledButtons {
                    minimize: true,
                    maximize: true,
                    close: true,
                };
            }
        }
        for image_entity in banner_image.iter() {
            commands.entity(image_entity).despawn();
        }
        for entity in splash.iter() {
            commands.entity(entity).despawn();
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct UiRegionQueries<'w, 's> {
    roots: Query<'w, 's, Entity, With<StudioUiRoot>>,
    bodies: Query<'w, 's, Entity, With<StudioBodyRoot>>,
    workspace_regions: Query<'w, 's, Entity, With<WorkspaceRegionRoot>>,
    editor_regions: Query<'w, 's, Entity, With<EditorRegionRoot>>,
    overlay_regions: Query<'w, 's, Entity, With<OverlayRegionRoot>>,
    children: Query<'w, 's, &'static Children>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

// Bevy systems expose each independently tracked resource/query as a parameter.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    brand: Res<BrandImages>,
    mut local_images: ResMut<LocalImages>,
    state: StudioStateRead,
    native_setup: Res<NativeSetup>,
    cache_stats: Res<CacheStatsJob>,
    theme: Res<StudioTheme>,
    mut invalidated: ResMut<UiInvalidated>,
    mut metrics: ResMut<UiRebuildMetrics>,
    ui: UiRegionQueries,
) {
    let Some(regions) = invalidated.take() else {
        return;
    };
    let started = std::time::Instant::now();
    // Counting every descendant twice made even an overlay-only context menu
    // pay O(the entire DAG) instrumentation cost. Keep those diagnostics
    // available when their debug target is enabled, but free in normal use.
    let collect_metrics = bevy::log::tracing::enabled!(
        target: "uta_studio::ui_rebuild",
        bevy::log::Level::DEBUG
    );
    let old_entities = if collect_metrics {
        count_ui_entities(&ui.roots, &ui.children)
    } else {
        0
    };
    let window_size = ui
        .windows
        .single()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let session = state.view();
    let workspace_dirty = regions.contains(UiDirtyRegion::Library)
        || regions.contains(UiDirtyRegion::Analysis)
        || regions.contains(UiDirtyRegion::Settings)
        || regions.contains(UiDirtyRegion::Documentation);
    let editor_dirty = regions.contains(UiDirtyRegion::Editor);
    // Editor context menus live in the lightweight overlay region. Any editor
    // rebuild may clear one as part of an action, so keep that small region in
    // sync without putting the menus back inside the expensive editor tree.
    let overlay_dirty = regions.contains(UiDirtyRegion::Dialog)
        || (editor_dirty && session.route == StudioRoute::Editor);
    let mut full_rebuild = regions.requires_full_rebuild() || ui.roots.single().is_err();
    if !full_rebuild && workspace_dirty && session.route != StudioRoute::Editor {
        full_rebuild = ui.bodies.single().is_err() || ui.workspace_regions.single().is_err();
    }
    if !full_rebuild && editor_dirty && session.route == StudioRoute::Editor {
        full_rebuild = ui.editor_regions.single().is_err();
    }
    if !full_rebuild && overlay_dirty {
        full_rebuild = ui.overlay_regions.single().is_err();
    }

    let mut rebuilt = false;
    if full_rebuild {
        for entity in &ui.roots {
            commands.entity(entity).despawn();
        }
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
        rebuilt = true;
    } else {
        if workspace_dirty && session.route != StudioRoute::Editor {
            for entity in &ui.workspace_regions {
                commands.entity(entity).despawn();
            }
            if let Ok(body) = ui.bodies.single() {
                commands.entity(body).with_children(|body| {
                    spawn_workspace_region(
                        body,
                        asset_server.load(FONT_PATH),
                        asset_server.load(ICON_ATLAS_PATH),
                        &asset_server,
                        &mut images,
                        &mut local_images,
                        &session,
                        &native_setup,
                        &cache_stats,
                        &theme,
                    );
                });
                rebuilt = true;
            }
        }
        if editor_dirty && session.route == StudioRoute::Editor {
            for entity in &ui.editor_regions {
                commands.entity(entity).despawn();
            }
            if let Ok(root) = ui.roots.single() {
                commands.entity(root).with_children(|root| {
                    spawn_editor_region(
                        root,
                        asset_server.load(FONT_PATH),
                        asset_server.load(ICON_ATLAS_PATH),
                        &session,
                        &theme,
                    );
                });
                rebuilt = true;
            }
        }
        if overlay_dirty {
            for entity in &ui.overlay_regions {
                commands.entity(entity).despawn();
            }
            if let Ok(root) = ui.roots.single() {
                commands.entity(root).with_children(|root| {
                    spawn_overlay_region(
                        root,
                        asset_server.load(FONT_PATH),
                        asset_server.load(ICON_ATLAS_PATH),
                        &brand,
                        &session,
                        &theme,
                        window_size,
                    );
                });
                rebuilt = true;
            }
        }
    }
    if rebuilt && collect_metrics {
        metrics.begin(started, started.elapsed(), old_entities, regions);
    }
}

fn count_ui_entities(
    roots: &Query<Entity, With<StudioUiRoot>>,
    children: &Query<&Children>,
) -> usize {
    let mut count = 0;
    let mut pending = roots.iter().collect::<Vec<_>>();
    while let Some(entity) = pending.pop() {
        count += 1;
        if let Ok(descendants) = children.get(entity) {
            pending.extend(descendants.iter());
        }
    }
    count
}

pub(crate) fn finalize_ui_rebuild_metrics(
    mut metrics: ResMut<UiRebuildMetrics>,
    roots: Query<Entity, With<StudioUiRoot>>,
    children: Query<&Children>,
) {
    let Some(pending) = metrics.finish() else {
        return;
    };
    let new_entities = count_ui_entities(&roots, &children);
    bevy::log::debug!(
        target: "uta_studio::ui_rebuild",
        rebuild = pending.sequence,
        regions = ?pending.regions,
        old_entities = pending.old_entities,
        new_entities,
        render_ms = pending.render_elapsed.as_secs_f64() * 1_000.0,
        materialized_ms = pending.started.elapsed().as_secs_f64() * 1_000.0,
        "rebuilt Studio UI"
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_ui(
    commands: &mut Commands,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    brand: &BrandImages,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
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
                spawn_editor_region(root, font.clone(), icons.clone(), session, theme);
            } else {
                root.spawn((
                    StudioBodyRoot,
                    Node {
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                ))
                .with_children(|body| {
                    spawn_sidebar(
                        body,
                        font.clone(),
                        icons.clone(),
                        brand.banner.clone(),
                        session,
                        theme,
                    );
                    spawn_workspace_region(
                        body,
                        font.clone(),
                        icons.clone(),
                        asset_server,
                        images,
                        local_images,
                        session,
                        native_setup,
                        cache_stats,
                        theme,
                    );
                });
            }
            spawn_overlay_region(root, font, icons, brand, session, theme, window_size);
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_workspace_region(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
    session: &StudioSessionView<'_>,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            WorkspaceRegionRoot,
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .with_children(|region| {
            spawn_workspace(
                region,
                font,
                asset_server,
                images,
                local_images,
                session,
                native_setup,
                cache_stats,
                icons,
                theme,
            );
        });
}

fn spawn_editor_region(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            EditorRegionRoot,
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .with_children(|region| {
            spawn_editor(region, font, icons, session, theme);
        });
}

fn spawn_overlay_region(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    brand: &BrandImages,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
    window_size: Vec2,
) {
    parent
        .spawn((
            OverlayRegionRoot,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                ..default()
            },
        ))
        .with_children(|overlay| {
            if let Some(context) = session.analysis_node_context.as_ref() {
                spawn_analysis_node_context_menu(overlay, font.clone(), theme, context);
            }
            if session.route == StudioRoute::Editor
                && let Some(editor) = session.editor.as_ref()
            {
                if editor.file_menu_open {
                    spawn_editor_file_menu(overlay, font.clone(), theme, editor);
                }
                if editor.layout_menu_open {
                    spawn_editor_layout_menu(overlay, font.clone(), theme, editor);
                }
                if let Some(context) = editor.note_context.as_ref() {
                    spawn_note_context_menu(
                        overlay,
                        font.clone(),
                        theme,
                        editor,
                        context,
                        window_size,
                    );
                }
                if let Some(context) = editor.lyric_context.as_ref() {
                    spawn_lyric_context_menu(
                        overlay,
                        font.clone(),
                        theme,
                        editor,
                        context,
                        window_size,
                    );
                }
                if let Some(context) = editor.waveform_context.as_ref() {
                    spawn_waveform_context_menu(
                        overlay,
                        font.clone(),
                        theme,
                        editor,
                        context,
                        window_size,
                    );
                }
            }
            if session.activity_open {
                spawn_activity_center(overlay, font.clone(), icons, session, theme);
            }
            if let Some(revision) = session.pending_artifact_delete.as_ref() {
                spawn_artifact_delete_confirmation(overlay, font.clone(), theme, revision);
            }
            if let Some(revision) = session.pending_artifact_invalidate.as_ref() {
                spawn_artifact_invalidate_confirmation(overlay, font.clone(), theme, revision);
            }
            if let Some(revision) = session.pending_artifact_active.as_ref() {
                spawn_artifact_active_confirmation(overlay, font.clone(), theme, revision);
            }
            if let Some(file_hash) = session.pending_intermediate_capture.as_deref() {
                spawn_intermediate_capture_confirmation(
                    overlay,
                    font.clone(),
                    session.config,
                    theme,
                    file_hash,
                );
            }
            if let Some(file_hash) = session.pending_chart_replace.as_deref() {
                spawn_chart_replace_confirmation(overlay, font.clone(), theme, file_hash);
            }
            if let Some(diff) = session.artifact_diff.as_ref() {
                spawn_artifact_diff_panel(overlay, font.clone(), theme, diff);
            }
            if let Some(lineage) = session.artifact_lineage.as_ref() {
                spawn_artifact_lineage_panel(overlay, font.clone(), theme, lineage);
            }
            if let Some(impact) = session.artifact_impact.as_ref() {
                spawn_artifact_impact_panel(overlay, font.clone(), theme, impact);
            }
            if session.about_open {
                spawn_about_dialog(
                    overlay,
                    font.clone(),
                    brand.logo.clone(),
                    session.config,
                    theme,
                );
            }
            if let Some(panel) = session.song_settings.as_ref() {
                spawn_song_settings_panel(overlay, font.clone(), theme, panel);
            }
            if let Some(destination) = session.pending_leave {
                spawn_leave_confirmation(overlay, font, theme, session, destination);
            }
        });
}

pub(crate) fn spawn_leave_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    destination: PendingLeave,
) {
    let dirty = session.editor.as_ref().is_some_and(|editor| editor.dirty);
    let (title, action) = match destination {
        PendingLeave::Exit => ("Close Uta Studio?", "Close"),
        PendingLeave::Back | PendingLeave::Home | PendingLeave::Documentation => {
            ("Leave the editor?", "Leave")
        }
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
                            UiAction::from(AppCommand::CancelLeave),
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
                            UiAction::from(AppCommand::ConfirmLeave),
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
