use crate::studio::*;

#[derive(Clone, Copy)]
pub(crate) struct AnalysisGraphBox {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl AnalysisGraphBox {
    pub(crate) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// DAG canvas zoom bounds (§7.8/§9.3 "DAG 支持 Pan、Zoom、Fit"). 1.0 is
/// unscaled. The default sits at the previous 160% "readable node" zoom
/// now that the inspect pane no longer steals the canvas; min/max stay
/// wide enough that Fit and +/- still have room around that default.
pub(crate) const ANALYSIS_GRAPH_ZOOM_MIN: f32 = 0.5;
pub(crate) const ANALYSIS_GRAPH_ZOOM_MAX: f32 = 2.4;
pub(crate) const ANALYSIS_GRAPH_ZOOM_STEP: f32 = 0.15;
pub(crate) const ANALYSIS_GRAPH_ZOOM_DEFAULT: f32 = 1.0;
/// Inset so a fitted graph is not flush against the viewport chrome.
pub(crate) const ANALYSIS_GRAPH_FIT_PADDING: f32 = 20.0;

pub(crate) fn clamp_analysis_graph_zoom(zoom: f32) -> f32 {
    zoom.clamp(ANALYSIS_GRAPH_ZOOM_MIN, ANALYSIS_GRAPH_ZOOM_MAX)
}

/// Zoom that puts the unscaled canvas inside `viewport` with a small inset.
pub(crate) fn analysis_graph_fit_zoom(
    canvas_width: f32,
    canvas_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> f32 {
    if canvas_width <= 1.0
        || canvas_height <= 1.0
        || viewport_width <= 1.0
        || viewport_height <= 1.0
    {
        return ANALYSIS_GRAPH_ZOOM_DEFAULT;
    }
    let width = (viewport_width - ANALYSIS_GRAPH_FIT_PADDING).max(1.0);
    let height = (viewport_height - ANALYSIS_GRAPH_FIT_PADDING).max(1.0);
    clamp_analysis_graph_zoom((width / canvas_width).min(height / canvas_height))
}

/// Scales a computed layout rect into screen-space box coordinates for the
/// current zoom level. Zoom is applied here, to the actual layout numbers
/// fed into each node/edge `Node`'s `left/top/width/height`, rather than as
/// a visual-only transform on the canvas wrapper -- that keeps the
/// scrollable content size (and therefore panning range and click
/// hit-testing) consistent with what's drawn at any zoom level, instead of
/// drifting out of sync with it.
pub(crate) fn zoomed_box(rect: LayoutRect, zoom: f32) -> AnalysisGraphBox {
    AnalysisGraphBox::new(
        rect.x * zoom,
        rect.y * zoom,
        rect.width * zoom,
        rect.height * zoom,
    )
}

/// Scroll offset (left edge minus a small margin, clamped to non-negative)
/// and inspector stage id to jump to for a "Focus" button, or `None` if the
/// node isn't part of this pass's layout at all (e.g. a compound child
/// that's currently collapsed).
pub(crate) fn analysis_graph_focus_target(
    layout: Option<&GraphLayout>,
    id: &app_core::AnalysisNodeId,
    zoom: f32,
) -> Option<(i32, String)> {
    analysis_graph_center_target(layout, id, zoom, 960.0)
}

/// Scroll offset that places `id`'s box in the horizontal center of a
/// viewport of `viewport_width`, plus the inspector stage id. Used both by
/// the Focus buttons and by live follow, which recenters the running node
/// as analysis walks the DAG. Falls back to a left-aligned jump when the
/// viewport width is not yet known (first frame).
pub(crate) fn analysis_graph_center_target(
    layout: Option<&GraphLayout>,
    id: &app_core::AnalysisNodeId,
    zoom: f32,
    viewport_width: f32,
) -> Option<(i32, String)> {
    let rect = layout?.rect(id)?;
    let bucket = analysis_node_stage_index(id.as_str()).unwrap_or(0);
    let node_center = (rect.x + rect.width / 2.0) * zoom;
    let scroll = if viewport_width > 1.0 {
        (node_center - viewport_width / 2.0).max(0.0)
    } else {
        (rect.x * zoom - 60.0).max(0.0)
    };
    Some((scroll.round() as i32, bucket_stage_id(bucket).to_string()))
}

/// Horizontal scroll that keeps a live node near the middle of the canvas
/// when the full layout is not in hand yet (refresh tick before the next
/// rebuild). Rank is the same 7-bucket index the inspector already uses.
pub(crate) fn estimated_analysis_graph_center_scroll(
    node_id: &str,
    zoom: f32,
    viewport_width: f32,
) -> f32 {
    let spacing = LayoutSpacing::canvas();
    let rank = analysis_node_stage_index(node_id).unwrap_or(0) as f32;
    let center = (spacing.margin
        + rank * (spacing.node_width + spacing.column_gap)
        + spacing.node_width / 2.0)
        * zoom;
    let width = if viewport_width > 1.0 {
        viewport_width
    } else {
        960.0
    };
    (center - width / 2.0).max(0.0)
}

#[derive(Clone, Copy)]
pub(crate) enum AnalysisGraphStageState {
    Waiting,
    Running(usize),
    Complete,
}

pub(crate) fn spawn_activity_center(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::from(AppCommand::CloseActivity),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.54)),
        ZIndex(100),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(420),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20)),
                row_gap: px(12),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.9)),
            ZIndex(101),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(9),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons, UiIcon::Queue, 17.0, theme.primary);
                    spawn_text(header, font.clone(), "Activity", 18.0, theme.foreground);
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::from(AppCommand::CloseActivity),
                    );
                });
            spawn_wrapped_text(
                panel,
                font.clone(),
                "Live analysis work and the most recent native operation.",
                10.0,
                theme.muted_foreground,
            );
            panel.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.64)),
            ));
            spawn_text(
                panel,
                font.clone(),
                format!("JOBS  ·  {}", session.analysis_tasks.len()),
                9.0,
                theme.muted_foreground,
            );
            if session.analysis_tasks.is_empty() {
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            padding: UiRect::all(px(18)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.55)),
                    ))
                    .with_children(|empty| {
                        spawn_wrapped_text(
                            empty,
                            font.clone(),
                            "Nothing is running. Requested analyses and failures appear here.",
                            10.0,
                            theme.muted_foreground,
                        );
                    });
            } else {
                for task in session.analysis_tasks.iter().take(10) {
                    let (status, progress, failed) = analysis_status_copy(&task.status);
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(11)),
                                row_gap: px(4),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.36)),
                            BorderColor::all(if failed {
                                theme.destructive.with_alpha(0.62)
                            } else {
                                theme.border.with_alpha(0.58)
                            }),
                        ))
                        .with_children(|card| {
                            card.spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.title.clone(),
                                        11.0,
                                        theme.foreground,
                                    );
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.artist.clone(),
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                });
                                spawn_text(
                                    row,
                                    font.clone(),
                                    status,
                                    9.0,
                                    if failed {
                                        theme.destructive
                                    } else {
                                        theme.primary
                                    },
                                );
                                // Phase 6 `cancel_analysis_run`: only offered
                                // while still Queued -- a running job can't
                                // be safely cancelled mid-node yet, so no
                                // button is shown for it (not a disabled one
                                // that would just error).
                                if matches!(task.status, app_core::QueuedStatus::Queued) {
                                    spawn_text_button(
                                        row,
                                        font.clone(),
                                        theme,
                                        "Cancel",
                                        9.0,
                                        UiAction::from(AnalysisCommand::CancelAnalysisRun(
                                            task.file_hash.clone(),
                                        )),
                                    );
                                }
                            });
                            if let Some(live) = task.live.as_ref() {
                                spawn_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}%", live.operation, live.stage_progress),
                                    9.0,
                                    theme.primary,
                                );
                                spawn_wrapped_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}", live.implementation, live.detail),
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                            if let Some(progress) = progress {
                                card.spawn((
                                    Node {
                                        position_type: PositionType::Relative,
                                        width: percent(100),
                                        height: px(3),
                                        margin: UiRect::top(px(4)),
                                        overflow: Overflow::clip(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted),
                                ))
                                .with_children(|rail| {
                                    rail.spawn((
                                        Node {
                                            width: percent(progress.clamp(0, 100) as f32),
                                            height: percent(100),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(theme.primary),
                                    ));
                                });
                            }
                        });
                }
            }
            panel.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(notice) = session.notice.as_deref() {
                spawn_wrapped_text(panel, font.clone(), notice, 9.0, theme.muted_foreground);
            }
            spawn_action_button(
                panel,
                font,
                theme,
                "Open analysis queue",
                UiAction::from(LibraryCommand::SetLibraryView(LibraryView::Queue)),
            );
        });
}

pub(crate) fn analysis_status_copy(
    status: &app_core::QueuedStatus,
) -> (String, Option<usize>, bool) {
    match status {
        app_core::QueuedStatus::Queued => ("Queued".to_string(), None, false),
        app_core::QueuedStatus::Analyzing(progress) => {
            (format!("Analyzing · {progress}%"), Some(*progress), false)
        }
        app_core::QueuedStatus::Failed(message) => (
            if message.trim().is_empty() {
                "Failed".to_string()
            } else {
                format!("Failed · {message}")
            },
            None,
            true,
        ),
    }
}

pub(crate) fn analysis_stage_index(stage: &str) -> usize {
    match stage {
        "preparing" | "key_detection" => 0,
        "separation" => 1,
        "pitch" => 2,
        "audio_preprocessing" => 3,
        "transcription" => 4,
        "alignment" => 5,
        "finalizing" | "complete" => 6,
        _ => 0,
    }
}

/// Maps a Phase 1 `AnalysisNodeId` (analysis DAG redesign,
/// docs/analysis-dag-redesign.md) onto today's 7-bucket UI stage index.
/// This is the additive bridge described in that doc's Phase 7 status
/// note: the graph is not yet UI-driven, but live progress now carries an
/// explicit `node_id` (Phase 3) that this prefers over regexing `stage`
/// text whenever the emitting pipeline call site has migrated to
/// `progress_node`. Unmigrated call sites (still common; see the doc's
/// Phase 3 gaps) fall back to `analysis_stage_index` unchanged.
pub(crate) fn analysis_node_stage_index(node_id: &str) -> Option<usize> {
    match node_id {
        "preflight" | "music.analysis" | "music.key" | "music.rhythm" | "music.descriptors" => {
            Some(0)
        }
        "stems.separate"
        | "stems.vocals"
        | "vocals.denoise"
        | "vocals.dereverb"
        | "stems.instrumental"
        | "stems.karaoke"
        | "stems.multistem"
        | "stems.bind_analysis_outputs" => Some(1),
        "pitch.extract" => Some(2),
        "lyrics.preprocess" => Some(3),
        "lyrics.transcribe" => Some(4),
        "lyrics.align" | "lyrics.import_timed" => Some(5),
        "chart.build_candidate" => Some(6),
        _ => None,
    }
}

/// Resolves the UI stage index for a live snapshot, preferring the
/// structured `node_id` when the emitting event set one and falling back to
/// the Legacy Adapter text classification otherwise.
pub(crate) fn resolve_live_stage_index(stage: &str, node_id: Option<&str>) -> usize {
    node_id
        .and_then(analysis_node_stage_index)
        .unwrap_or_else(|| analysis_stage_index(stage))
}

/// Picks one representative Phase 1 `AnalysisNodeId` and, where a real
/// cached-file check exists, `ArtifactKind` for each of the 7 UI stage
/// buckets, so the node inspector (docs/analysis-dag-redesign.md §7's
/// Phase 7 "node inspector" item) can ground its selected-stage panel in
/// the real domain model instead of only the static per-stage copy in
/// `analysis_stage_details`. A bucket can hold several graph nodes (e.g.
/// stage 0 covers `music.analysis`/`music.key`/`music.rhythm`/
/// `music.descriptors`); this picks the one whose plan state is most
/// representative of the bucket rather than showing all of them. `None`
/// for the artifact kind means no single cached file stands in for that
/// bucket today (`cached_artifact_presence_for_song` only tracks the kinds
/// that already have one physical file per song). `lyrics.preprocess`
/// stays `None` -- `PreprocessedAudio` still has no persisted file
/// (unrelated to the §4.4 split). `lyrics.transcribe` gained a real check
/// once §4.4 split `RecognizedText`/`AsrSegments` out into their own files.
pub(crate) fn stage_primary_node_and_artifact(
    stage_index: usize,
) -> (&'static str, Option<app_core::ArtifactKind>) {
    match stage_index {
        0 => (
            "music.analysis",
            Some(app_core::ArtifactKind::MusicAnalysis),
        ),
        1 => ("stems.separate", Some(app_core::ArtifactKind::VocalStem)),
        2 => ("pitch.extract", Some(app_core::ArtifactKind::PitchTrack)),
        3 => ("lyrics.preprocess", None),
        4 => (
            "lyrics.transcribe",
            Some(app_core::ArtifactKind::RecognizedText),
        ),
        5 => (
            "lyrics.align",
            Some(app_core::ArtifactKind::TimedTranscript),
        ),
        _ => (
            "chart.build_candidate",
            Some(app_core::ArtifactKind::AuthoredChart),
        ),
    }
}

/// The one analysis-profile parameter (if any) primarily relevant to a
/// given node, for the inspector's PARAMETERS fact
/// (docs/analysis-dag-redesign.md Phase 7 §7.4). Nodes with no
/// profile-controlled parameter (preflight, music.analysis, lyrics.preprocess,
/// lyrics.align's own timing, chart.build_candidate) return `None` rather
/// than a fabricated value -- the fact row is simply omitted for those.
pub(crate) fn selected_stage_parameter(
    node_id: &str,
    profile: &app_core::AnalysisProfileSnapshot,
) -> Option<(&'static str, String)> {
    match node_id {
        "stems.separate" => Some(("SEPARATOR", profile.separator.clone())),
        "lyrics.transcribe" => Some(("ASR ENGINE", profile.asr_engine.clone())),
        "lyrics.align" => Some(("ALIGNMENT BACKEND", profile.alignment_backend.clone())),
        _ => None,
    }
}

/// The `app_core::ProfileField` a node's one profile-controlled parameter
/// maps to, if any -- same mapping as `selected_stage_parameter`'s match
/// arms (kept as a separate small function rather than merged into it,
/// since callers like the PARAMETER SOURCE resolution and the "Configure
/// for this run" dialog need the field itself, not a pre-formatted label).
pub(crate) fn node_config_profile_field(node_id: &str) -> Option<app_core::ProfileField> {
    match node_id {
        "stems.separate" => Some(app_core::ProfileField::Separator),
        "lyrics.transcribe" => Some(app_core::ProfileField::AsrEngine),
        "lyrics.align" => Some(app_core::ProfileField::AlignmentBackend),
        _ => None,
    }
}

/// Phase 8 §8.4: which of the three tiers (Global Defaults -> Song Profile
/// -> Run Override) is winning for the inspector's PARAMETER SOURCE fact,
/// backed by the same `app_core::resolve_profile_field` real execution
/// uses -- pulled out of the giant inspector-rendering function so it's
/// independently testable with fixtures, no IO. `field` is `None` for a
/// node with no profile-controlled parameter at all; the caller already
/// omits the fact row in that case (`selected_parameter.is_none()`), so the
/// "Global default" fallback here is never actually shown.
pub(crate) fn node_parameter_source_copy(
    field: Option<app_core::ProfileField>,
    global: &app_core::AnalysisProfileSnapshot,
    song: Option<&app_core::AnalysisProfileSnapshot>,
    run_override: Option<&str>,
) -> &'static str {
    let Some(field) = field else {
        return "Global default";
    };
    match app_core::resolve_profile_field(field, global, song, run_override).source {
        app_core::ProfileSource::RunOverride => "Run override (queued)",
        app_core::ProfileSource::SongProfile => "Song profile",
        app_core::ProfileSource::GlobalDefault => "Global default",
    }
}

/// Minimal, dependency-free ms-since-epoch -> `"YYYY-MM-DD HH:MM"` (UTC)
/// formatter for artifact/history timestamps in the inspector -- good
/// enough for display without pulling in a full date/time crate for one
/// field. Proleptic Gregorian civil-date conversion via Howard Hinnant's
/// well-known days-from-epoch algorithm.
pub(crate) fn format_epoch_ms(ms: i64) -> String {
    let total_seconds = ms.div_euclid(1000);
    let days = total_seconds.div_euclid(86_400);
    let secs_of_day = total_seconds.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// §7.5 "Compare with previous attempt": renders
/// `app_core::compare_node_attempt_with_previous_run`'s result as readable
/// copy for `session.notice` -- a real diff, not just a confirmation that
/// the action ran. `attempt_a` is the current run's attempt, `attempt_b`
/// the previous run's (see that function's doc comment), so changed fields
/// are shown as "previous → current".
/// §7.6 "Inspect provenance": every field here already exists on
/// `ArtifactRevision` (Phase 2's Artifact Inventory) -- this is purely a
/// display concern, no new data. Kept as a `session.notice` string (same
/// as `format_node_attempt_comparison`) rather than a dedicated modal,
/// since there's no interactive follow-up action provenance itself needs
/// (unlike the Delete/Invalidate confirmations, which gate a real
/// mutation).
pub(crate) fn format_artifact_provenance(revision: &app_core::ArtifactRevision) -> String {
    let input_summary = if revision.input_revisions.is_empty() {
        "none recorded".to_string()
    } else {
        revision.input_revisions.join(", ")
    };
    format!(
        "{:?} · produced by {} · algorithm v{} · config {} · content {} · inputs: {} · {}",
        revision.kind,
        revision.producer_node,
        revision.algorithm_version,
        revision.config_hash.chars().take(12).collect::<String>(),
        revision.content_hash.chars().take(12).collect::<String>(),
        input_summary,
        format_epoch_ms(revision.created_at_ms),
    )
}

/// §7.6 "Compare revisions": renders
/// `app_core::compare_artifact_revisions`'s result as readable copy, same
/// "session.notice, not a new diff panel" choice as
/// `format_node_attempt_comparison`.
#[allow(dead_code)]
pub(crate) fn format_artifact_revision_comparison(
    comparison: &app_core::ArtifactRevisionComparison,
) -> String {
    if comparison.same_content {
        return format!(
            "{:?} revisions are byte-identical (same content_hash) despite {}.",
            comparison.revision_a.kind,
            if comparison.changed_fields.is_empty() {
                "matching everything else too".to_string()
            } else {
                format!("differing in {}", comparison.changed_fields.join(", "))
            }
        );
    }
    if comparison.changed_fields.is_empty() {
        return format!(
            "{:?} revisions have different content but no other tracked field differs.",
            comparison.revision_a.kind
        );
    }
    format!(
        "{:?} revisions differ in: {}",
        comparison.revision_a.kind,
        comparison.changed_fields.join(", ")
    )
}

pub(crate) fn format_node_attempt_comparison(
    comparison: &app_core::NodeAttemptComparison,
) -> String {
    let (current, previous) = match (&comparison.attempt_a, &comparison.attempt_b) {
        (Some(current), Some(previous)) => (current, previous),
        (Some(_), None) => {
            return format!(
                "{} has no recorded attempt in the previous run.",
                comparison.node_id
            );
        }
        (None, Some(_)) => {
            return format!(
                "{} has no recorded attempt in the current run.",
                comparison.node_id
            );
        }
        (None, None) => {
            return format!(
                "{} has no recorded attempt in either run.",
                comparison.node_id
            );
        }
    };
    if comparison.changed_fields.is_empty() {
        return format!(
            "{} is unchanged from the previous attempt ({}).",
            comparison.node_id, current.implementation
        );
    }
    let field_value = |field: &str, attempt: &app_core::NodeAttempt| -> String {
        match field {
            "status" => attempt.status.clone(),
            "implementation" => attempt.implementation.clone(),
            "model" => attempt.model.clone(),
            "requested_device" => attempt.requested_device.clone(),
            "actual_device" => attempt.actual_device.clone(),
            "fallback_from" => attempt.fallback_from.clone().unwrap_or_default(),
            "backend_fallback_from" => attempt.backend_fallback_from.clone().unwrap_or_default(),
            _ => String::new(),
        }
    };
    let changes: Vec<String> = comparison
        .changed_fields
        .iter()
        .map(|field| {
            format!(
                "{field}: {} → {}",
                field_value(field, previous),
                field_value(field, current)
            )
        })
        .collect();
    format!(
        "{} changed since the previous attempt — {}",
        comparison.node_id,
        changes.join(", ")
    )
}

/// §7.4 "DURATION" inspector fact -- Phase 7's "Duration 检查器字段" gap
/// closed by real per-node `started_at_ms`/`finished_at_ms`
/// (`server.py::_progress_payload`), not something inferred from socket
/// receive time. `None`/incomplete data (still running, predates this
/// field, or a corrupt `finished < started`) reads as "Not yet available"
/// rather than a wrong or negative duration.
pub(crate) fn node_duration_copy(route: Option<&app_core::AnalysisStageRoute>) -> String {
    match route.and_then(|r| r.started_at_ms.zip(r.finished_at_ms)) {
        Some((started, finished)) if finished >= started => {
            format_duration((finished - started) as f64 / 1000.0)
        }
        _ => "Not yet available".to_string(),
    }
}

pub(crate) fn node_state_copy(state: app_core::NodeState) -> &'static str {
    match state {
        app_core::NodeState::Missing => "Missing",
        app_core::NodeState::Ready => "Ready to run",
        app_core::NodeState::Queued => "Queued",
        app_core::NodeState::Running => "Running",
        app_core::NodeState::Cached => "Reusing cached output",
        app_core::NodeState::Succeeded => "Succeeded",
        app_core::NodeState::SucceededWithWarnings => "Succeeded with warnings",
        app_core::NodeState::Failed => "Failed",
        app_core::NodeState::Stale => "Stale",
        app_core::NodeState::Frozen => "Frozen",
        app_core::NodeState::Disabled => "Disabled",
        app_core::NodeState::Blocked => "Blocked",
        app_core::NodeState::NotApplicable => "Not applicable to this run",
        app_core::NodeState::Cancelled => "Cancelled",
        app_core::NodeState::Bypassed => "Bypassed with an alternate input",
    }
}

/// The canonical bucket-string id `analysis_stage_details`/
/// `analysis_stage_matches` and graph-node actions already key
/// selection and copy off of. Exact inverse of `analysis_stage_index`'s
/// primary (non-alias) branch for each bucket.
pub(crate) fn bucket_stage_id(bucket: usize) -> &'static str {
    match bucket {
        0 => "preparing",
        1 => "separation",
        2 => "pitch",
        3 => "audio_preprocessing",
        4 => "transcription",
        5 => "alignment",
        _ => "finalizing",
    }
}

/// Bridges a `GraphViewModel` node's blended plan+run-time state onto the
/// existing 3-state `AnalysisGraphStageState` widget
/// (`spawn_analysis_stage_node`) without changing that widget's tested
/// color/layout logic. States the widget has no visual language for
/// (`Frozen`/`Disabled`/`Blocked`/`NotApplicable`) render with the
/// `Waiting` visual treatment but carry a distinct status string the
/// caller should show in place of the node's normal route/model text --
/// real information the old 7-bucket-only UI had nowhere to put, since it
/// never modeled Phase 1 plan states at all.
pub(crate) fn graph_node_state_to_stage_state(
    state: GraphNodeState,
    running_progress: usize,
) -> (AnalysisGraphStageState, Option<&'static str>) {
    match state {
        GraphNodeState::Running => (AnalysisGraphStageState::Running(running_progress), None),
        GraphNodeState::Complete => (AnalysisGraphStageState::Complete, None),
        GraphNodeState::Waiting => (AnalysisGraphStageState::Waiting, None),
        GraphNodeState::Frozen => (
            AnalysisGraphStageState::Waiting,
            Some("Frozen · reusing a protected artifact"),
        ),
        GraphNodeState::Disabled => (
            AnalysisGraphStageState::Waiting,
            Some("Disabled for this run"),
        ),
        GraphNodeState::Blocked => (
            AnalysisGraphStageState::Waiting,
            Some("Blocked · a required input is missing"),
        ),
        GraphNodeState::NotApplicable => (
            AnalysisGraphStageState::Waiting,
            Some("Not applicable to this run's lyrics route"),
        ),
        GraphNodeState::Failed => (
            AnalysisGraphStageState::Waiting,
            Some("Failed · see the inspector for details"),
        ),
        GraphNodeState::Stale => (
            AnalysisGraphStageState::Complete,
            Some("Stale · a newer candidate differs from your saved chart"),
        ),
        GraphNodeState::Bypassed => (
            AnalysisGraphStageState::Waiting,
            Some("Bypassed · using the original mix instead"),
        ),
    }
}

pub(crate) fn analysis_stage_matches(route_stage: &str, selected_stage: &str) -> bool {
    route_stage == selected_stage
        || (selected_stage == "preparing" && route_stage == "key_detection")
        || (selected_stage == "finalizing" && route_stage == "complete")
}

/// The one route recorded for a node, preferring an exact real-node-id
/// match (Phase 3's wire-protocol fix: `AnalysisStageRoute.node_id`) over
/// the legacy coarse-bucket text match. Falls back to bucket matching
/// whenever no route carries a matching `node_id` -- either because the
/// emitting call site hasn't migrated to `progress_node` yet, or (for a
/// compound node's own parent id) because only its children's routes were
/// ever recorded. Shared by the inspector's `selected_route` and the
/// canvas node boxes' `analysis_graph_route_summary`, so both read the same
/// route for the same node.
pub(crate) fn find_matching_route<'a>(
    routes: &'a [app_core::AnalysisStageRoute],
    node_id: &str,
    stage_id: &str,
) -> Option<&'a app_core::AnalysisStageRoute> {
    routes
        .iter()
        .rev()
        .find(|route| route.node_id.as_deref() == Some(node_id))
        .or_else(|| {
            routes
                .iter()
                .rev()
                .find(|route| analysis_stage_matches(&route.stage, stage_id))
        })
}

/// §7.4/§9.3 bug fix: the canvas box and the inspector used to be able to
/// show *different* completion percentages for the same node, because the
/// inspector derived its number from `stage_routes`' historical record,
/// which can be frozen at a stale non-100 value if a stage's last progress
/// event never happened to report exactly 100 before the pipeline moved on.
/// `render_state` (the canvas box's own `GraphNodeState`) derives
/// completion from the plan + real on-disk artifact presence, so it's
/// authoritative -- when it says Complete, the inspector must agree,
/// regardless of the last recorded route percentage. Only overrides the
/// Complete case; Running/Waiting/etc. keep the route/task-derived number,
/// which is already correct and more granular (0-99%) than a node state
/// alone can be.
#[cfg(test)]
pub(crate) fn selected_progress_and_status(
    render_state: Option<GraphNodeState>,
    route_progress: usize,
    route_status: &'static str,
) -> (usize, &'static str) {
    if render_state == Some(GraphNodeState::Complete) {
        (100, "COMPLETE")
    } else {
        (route_progress, route_status)
    }
}

pub(crate) fn analysis_stage_details(
    stage: &str,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match stage {
        "preparing" => (
            "Prepare",
            "Validates the source, resolves analysis settings, and detects musical context before model execution.",
            "Authorized source media and analysis profile",
            "Validated audio, runtime plan, tempo and key context",
        ),
        "separation" => (
            "Separate",
            "Extracts a vocal-focused stem while preserving the original source unchanged.",
            "Validated source audio",
            "Lossless vocal and instrumental analysis stems",
        ),
        "pitch" => (
            "Pitch",
            "Tracks the sung fundamental frequency and converts the contour into editable note guidance.",
            "Separated vocal stem",
            "Pitch contour and note candidates",
        ),
        "audio_preprocessing" => (
            "Preprocess",
            "Normalizes the analysis signal and prepares model-specific audio windows without rewriting source media.",
            "Vocal analysis stem",
            "Model-ready audio windows and vocal regions",
        ),
        "transcription" => (
            "Transcribe",
            "Recognizes lyric text and produces the timing evidence supported by the selected speech model.",
            "Preprocessed vocal regions and language preference",
            "Recognized lyric tokens and provisional timestamps",
        ),
        "alignment" => (
            "Align",
            "Refines recognized or supplied lyrics against the audio into editor-ready character and word timing.",
            "Lyrics, provisional timestamps, and vocal audio",
            "Character and word-level aligned lyrics",
        ),
        "finalizing" => (
            "Finalize",
            "Validates and commits generated analysis assets before the song becomes available for authoring.",
            "Aligned lyrics, pitch data, metadata, and stems",
            "Cached chart analysis and library metadata",
        ),
        _ => (
            "Analysis step",
            "Executes one stage of the configured analysis pipeline.",
            "Previous stage output",
            "Next stage input",
        ),
    }
}
