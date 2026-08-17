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

    pub(crate) fn left_port(self) -> Vec2 {
        Vec2::new(self.x, self.y + self.height / 2.0)
    }

    pub(crate) fn right_port(self) -> Vec2 {
        Vec2::new(self.x + self.width, self.y + self.height / 2.0)
    }
}

/// DAG canvas zoom bounds (§7.8/§9.3 "DAG 支持 Pan、Zoom、Fit"). 1.0 is
/// unscaled. Kept tight enough that node text (fixed-size, not scaled --
/// see `zoomed_box`) stays legible at the low end and panning stays
/// practical at the high end.
pub(crate) const ANALYSIS_GRAPH_ZOOM_MIN: f32 = 0.5;
pub(crate) const ANALYSIS_GRAPH_ZOOM_MAX: f32 = 1.75;
pub(crate) const ANALYSIS_GRAPH_ZOOM_STEP: f32 = 0.15;

pub(crate) fn clamp_analysis_graph_zoom(zoom: f32) -> f32 {
    zoom.clamp(ANALYSIS_GRAPH_ZOOM_MIN, ANALYSIS_GRAPH_ZOOM_MAX)
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
    let rect = layout?.rect(id)?;
    let bucket = analysis_node_stage_index(id.as_str()).unwrap_or(0);
    let scroll = (rect.x * zoom - 60.0).max(0.0);
    Some((scroll.round() as i32, bucket_stage_id(bucket).to_string()))
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
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseActivity,
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
                        UiAction::CloseActivity,
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
                                        UiAction::CancelAnalysisRun(task.file_hash.clone()),
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
                UiAction::SetLibraryView(LibraryView::Queue),
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
        "stems.separate" => Some(1),
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
        &revision.config_hash.chars().take(12).collect::<String>(),
        &revision.content_hash.chars().take(12).collect::<String>(),
        input_summary,
        format_epoch_ms(revision.created_at_ms),
    )
}

/// §7.6 "Compare revisions": renders
/// `app_core::compare_artifact_revisions`'s result as readable copy, same
/// "session.notice, not a new diff panel" choice as
/// `format_node_attempt_comparison`.
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
/// `analysis_stage_matches`/`UiAction::SelectAnalysisStage` already key
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

pub(crate) fn spawn_analysis_session_overview(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let active_task = session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .find(|task| matches!(task.status, app_core::QueuedStatus::Queued))
        });
    let history = session
        .selected_analysis_history
        .and_then(|id| {
            session
                .analysis_history
                .iter()
                .find(|history| history.id == id)
        })
        .or_else(|| {
            active_task
                .is_none()
                .then(|| session.analysis_history.first())
                .flatten()
        });
    let history_task = history.map(|history| app_core::AnalysisTask {
        file_hash: history.file_hash.clone(),
        title: history.title.clone(),
        artist: history.artist.clone(),
        status: app_core::QueuedStatus::Analyzing(if history.status == "completed" {
            100
        } else {
            0
        }),
        live: Some(history.snapshot.clone()),
    });
    let Some(task) = history_task.as_ref().or(active_task) else {
        return;
    };
    let viewing_history = history_task.is_some();

    let progress = match &task.status {
        app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
        _ => 0,
    };
    let stage = task
        .live
        .as_ref()
        .map(|live| live.stage.as_str())
        .unwrap_or("preparing");
    let live_node_id = task.live.as_ref().and_then(|live| live.node_id.as_deref());
    let stage_index = resolve_live_stage_index(stage, live_node_id);
    let operation = task
        .live
        .as_ref()
        .map(|live| live.operation.as_str())
        .unwrap_or("Waiting for the analysis runtime");
    let detail = task
        .live
        .as_ref()
        .map(|live| live.detail.as_str())
        .unwrap_or("The task is queued and will start when the current analysis completes.");
    let selected_stage = session.selected_analysis_stage.as_deref().unwrap_or(stage);
    let selected_stage_index = analysis_stage_index(selected_stage);
    // Real node id for the selected bucket (Phase 3/7 wire-protocol fix):
    // computed here, ahead of the plan/artifact block below that used to be
    // the only place deriving it, because `selected_route` now needs it
    // too. Every top-level compute node maps 1:1 with a bucket today (no
    // compound child is individually clickable yet -- see
    // `expanded_compound_nodes` further down), so this is already the
    // precise node id for anything actually selectable right now.
    let (selected_node_id, _) = stage_primary_node_and_artifact(selected_stage_index);
    let selected_route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, selected_node_id, selected_stage));
    let selected_is_current = analysis_stage_matches(stage, selected_stage);
    let selected_progress = selected_route
        .map(|route| route.stage_progress.clamp(0, 100))
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.stage_progress.clamp(0, 100))
                    .unwrap_or(0)
            })
        })
        .unwrap_or_else(|| {
            if selected_stage_index < stage_index {
                100
            } else {
                0
            }
        });
    let selected_trace_missing = selected_route.is_none() && selected_progress >= 100;
    let selected_pending_copy = if selected_trace_missing {
        "Not recorded in this analysis session"
    } else {
        "Pending"
    };
    let (selected_label, selected_purpose, selected_input, selected_output) =
        analysis_stage_details(selected_stage);
    let selected_status = if selected_progress >= 100 {
        "COMPLETE"
    } else if selected_is_current {
        "RUNNING"
    } else if selected_stage_index < stage_index {
        "COMPLETE"
    } else {
        "WAITING"
    };
    let selected_operation = selected_route
        .map(|route| route.operation.as_str())
        .or_else(|| selected_is_current.then_some(operation))
        .unwrap_or("This step has not started yet.");
    let selected_implementation = selected_route
        .map(|route| route.implementation.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.implementation.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_model = selected_route
        .map(|route| route.model.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.model.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_requested_device = selected_route
        .map(|route| route.requested_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.requested_device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_actual_device = selected_route
        .map(|route| route.actual_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_device_fallback = selected_route.and_then(|route| {
        route
            .fallback_from
            .as_deref()
            .zip(route.fallback_reason.as_deref())
    });
    let selected_backend_fallback = selected_route.and_then(|route| {
        route
            .backend_fallback_from
            .as_deref()
            .zip(route.backend_fallback_reason.as_deref())
    });
    let history_error = history.and_then(|history| history.error_message.as_deref());

    // Real Phase 1/2 domain-model data for the selected stage, grounding
    // the inspector panel in the actual DAG plan and on-disk artifact
    // state instead of only the static per-stage copy above
    // (docs/analysis-dag-redesign.md Phase 7 "node inspector" item).
    let plan_node_id = selected_node_id;
    let (_, plan_artifact_kind) = stage_primary_node_and_artifact(selected_stage_index);
    let plan_preview = app_core::preview_full_analysis_plan(&task.file_hash)
        .ok()
        .map(|plan| {
            let attempts = history
                .map(|history| app_core::load_analysis_node_attempts(history.id))
                .unwrap_or_default();
            let plan = overlay_failed_node_attempts(plan, &attempts);
            let candidate_status = app_core::candidate_chart_status(&task.file_hash);
            overlay_stale_candidate_chart(plan, &candidate_status)
        });
    // Single real read of on-disk artifact presence for this render, reused
    // by both the inspector panel below and the DAG canvas's node/edge
    // readiness -- was previously two separate calls to the same function.
    let artifact_presence = app_core::cached_artifact_presence_for_song(&task.file_hash);
    let planned_node = plan_preview
        .as_ref()
        .and_then(|plan| plan.node(&app_core::AnalysisNodeId::new(plan_node_id)));
    let plan_state_copy = planned_node
        .map(|node| node_state_copy(node.state))
        .unwrap_or("Not planned in this run");
    let plan_will_run_copy = planned_node.map_or("Unknown", |node| {
        if node.will_run {
            "Will run this pass"
        } else {
            "Reused or skipped"
        }
    });
    let plan_reason_copy = planned_node.and_then(|node| node.reason.as_deref());
    let plan_artifact_copy = plan_artifact_kind.map(|kind| {
        if app_core::artifact_present(&artifact_presence, kind) {
            "Present on disk"
        } else {
            "Not yet generated"
        }
    });
    // Cheap SQL read, not a file scan -- safe to call every render. The
    // table itself only fills in once `SyncArtifactRevisions` (or a future
    // live-run writer) has recorded something for this song/kind.
    let artifact_revisions = plan_artifact_kind
        .map(|kind| app_core::load_artifact_revisions(&task.file_hash, kind))
        .unwrap_or_default();

    // Remaining Phase 7 §7.4 inspector facts (Cache Signature, Algorithm
    // Version, Last Attempt, Fallback, Error, Parameters, Parameter source)
    // -- all backed by real data that was already being loaded/computed
    // above for other purposes, just not surfaced as facts yet.
    let active_revision = artifact_revisions.iter().find(|revision| revision.active);
    let selected_cache_signature = active_revision
        .map(|revision| revision.config_hash.chars().take(12).collect::<String>())
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_algorithm_version = active_revision
        .map(|revision| revision.algorithm_version.clone())
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_last_attempt = active_revision
        .map(|revision| format_epoch_ms(revision.created_at_ms))
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_fallback_text = selected_device_fallback
        .map(|(from, reason)| format!("Device: {from} -> current ({reason})"))
        .or_else(|| {
            selected_backend_fallback
                .map(|(from, reason)| format!("Backend: {from} -> current ({reason})"))
        })
        .unwrap_or_else(|| "None".to_string());
    let selected_duration_text = node_duration_copy(selected_route);
    let selected_error_text = if viewing_history {
        history_error
            .map(str::to_string)
            .unwrap_or_else(|| "None recorded".to_string())
    } else {
        "None recorded".to_string()
    };
    let selected_parameter = plan_preview
        .as_ref()
        .and_then(|plan| selected_stage_parameter(plan_node_id, &plan.profile_snapshot));
    // Phase 8 §8.4: a real three-tier resolution (Global Defaults -> Song
    // Profile -> Run Override), replacing the old binary "song profile
    // exists at all? y/n" check -- backed by the identical
    // `resolve_profile_field` real execution uses (`process_song`), so this
    // fact row and what actually runs can never disagree.
    let selected_parameter_source = node_parameter_source_copy(
        node_config_profile_field(plan_node_id),
        &app_core::AnalysisProfileSnapshot::from_app_config(
            &app_core::AppConfig::load(),
            &task.file_hash,
        ),
        app_core::get_song_analysis_profile(&task.file_hash).as_ref(),
        app_core::pending_run_override_for(&task.file_hash, plan_node_id).as_deref(),
    );

    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(30), px(26)),
                row_gap: px(16),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|session_card| {
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(20),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                if viewing_history {
                                    "ANALYSIS SESSION HISTORY"
                                } else {
                                    "LIVE ANALYSIS SESSION"
                                },
                                9.0,
                                theme.primary,
                            );
                            copy.spawn((
                                Button,
                                UiAction::OpenSong(task.file_hash.clone()),
                                Node {
                                    align_items: AlignItems::FlexStart,
                                    ..default()
                                },
                            ))
                            .with_children(|title| {
                                spawn_text(
                                    title,
                                    font.clone(),
                                    task.title.clone(),
                                    25.0,
                                    theme.foreground,
                                );
                            });
                            spawn_text(
                                copy,
                                font.clone(),
                                task.artist.clone(),
                                11.0,
                                theme.muted_foreground,
                            );
                        });
                    if viewing_history && active_task.is_some() {
                        spawn_text_button(
                            header,
                            font.clone(),
                            theme,
                            "View live",
                            9.0,
                            UiAction::SelectAnalysisHistory(None),
                        );
                    }
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{progress:02}%"),
                        30.0,
                        theme.foreground,
                    );
                });

            session_card
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|current| {
                    spawn_text(
                        current,
                        font.clone(),
                        "CURRENT OPERATION",
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_text(current, font.clone(), operation, 18.0, theme.foreground);
                    spawn_wrapped_text(current, font.clone(), detail, 10.0, theme.muted_foreground);
                    if let Some(live) = task.live.as_ref() {
                        if let Some(fallback_from) = live.fallback_from.as_deref() {
                            current
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    margin: UiRect::top(px(8)),
                                    ..default()
                                })
                                .with_children(|route| {
                                    spawn_text(
                                        route,
                                        font.clone(),
                                        "EXECUTION FALLBACK",
                                        8.0,
                                        theme.editor_warning,
                                    );
                                    route
                                        .spawn((
                                            Node {
                                                min_width: px(58),
                                                padding: UiRect::axes(px(10), px(6)),
                                                justify_content: JustifyContent::Center,
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(theme.editor_warning.with_alpha(0.08)),
                                            BorderColor::all(theme.editor_warning.with_alpha(0.48)),
                                        ))
                                        .with_children(|source| {
                                            spawn_text(
                                                source,
                                                font.clone(),
                                                fallback_from.to_ascii_uppercase(),
                                                9.0,
                                                theme.editor_warning,
                                            );
                                        });
                                    route.spawn((
                                        Node {
                                            width: px(34),
                                            height: px(2),
                                            ..default()
                                        },
                                        BackgroundColor(theme.editor_warning.with_alpha(0.68)),
                                    ));
                                    spawn_text(
                                        route,
                                        font.clone(),
                                        ">",
                                        10.0,
                                        theme.editor_warning,
                                    );
                                    route
                                        .spawn((
                                            Node {
                                                min_width: px(58),
                                                padding: UiRect::axes(px(10), px(6)),
                                                justify_content: JustifyContent::Center,
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(theme.pitch_contour.with_alpha(0.09)),
                                            BorderColor::all(theme.pitch_contour.with_alpha(0.52)),
                                        ))
                                        .with_children(|destination| {
                                            spawn_text(
                                                destination,
                                                font.clone(),
                                                live.device.to_ascii_uppercase(),
                                                9.0,
                                                theme.pitch_contour,
                                            );
                                        });
                                    if let Some(reason) = live.fallback_reason.as_deref() {
                                        spawn_wrapped_text(
                                            route,
                                            font.clone(),
                                            reason,
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                    }
                                });
                        }
                    }
                });

            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(5),
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.72)),
                ))
                .with_children(|rail| {
                    rail.spawn((
                        Node {
                            width: percent(progress as f32),
                            height: percent(100),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                    ));
                });

            let active_stage_progress = task
                .live
                .as_ref()
                .map(|live| live.stage_progress.clamp(0, 100))
                .unwrap_or(0);
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|heading| {
                    spawn_text(
                        heading,
                        font.clone(),
                        "DATA DEPENDENCY GRAPH",
                        8.0,
                        theme.primary,
                    );
                    spawn_text(
                        heading,
                        font.clone(),
                        "Stages consume the connected artifacts · Drag canvas to pan · Ctrl + wheel to zoom, Ctrl + Shift + wheel to pan",
                        8.0,
                        theme.muted_foreground,
                    );
                });

            // GraphViewModel + auto-layout (docs/analysis-dag-redesign.md
            // Phase 7 §7.1/§7.2): every node's state now comes from the
            // real Phase 1 plan blended with the existing bucket-based
            // run-time completion signal, and every position is computed,
            // not hand-placed. Compound-node expand/collapse
            // (`session.expanded_compound_nodes`) is toggled from the Node
            // Context Menu's "Expand sub-checks"/"Collapse sub-checks"
            // action -- music.analysis renders collapsed with a "N
            // sub-checks not shown" note by default, and as separate boxes
            // once expanded.
            let stage_complete = |index: usize| {
                index < stage_index
                    || (index == stage_index && active_stage_progress >= 100)
                    || progress >= 100
            };
            let graph_spec = app_core::baseline_graph_spec();
            // MINI view (`session.analysis_mini_view`, toggled by the "VIEW"
            // row's MINI/Full button) shows only the top-level, model-backed
            // nodes: build the graph as if nothing were expanded, regardless
            // of what the user individually expanded in the full view. That
            // per-node state (`expanded_compound_nodes`) is left untouched
            // either way, so switching back to Full restores it exactly.
            let no_expanded = std::collections::BTreeSet::new();
            let expanded = if session.analysis_mini_view {
                &no_expanded
            } else {
                &session.expanded_compound_nodes
            };
            let graph_view = build_graph_view_model(
                &graph_spec,
                plan_preview.as_ref(),
                live_node_id,
                stage_index,
                expanded,
                &analysis_node_stage_index,
                &stage_complete,
            );
            let render_graph = build_render_graph(&graph_spec, &graph_view, &|kind| {
                app_core::artifact_present(&artifact_presence, kind)
            });
            // MINI view drops the synthetic Artifact/Export boxes too --
            // "只显示以模型为基础的大节点" means the real compute/model
            // stages only, not the data-file decoration `build_render_graph`
            // adds around them. Filtering here (rather than skipping the
            // `RenderNodeKind::Artifact`/`::Export` spawn arms below) means
            // the layout algorithm never lays those boxes out in the first
            // place, and the corner mini-map -- which reads the same
            // `render_graph.nodes` -- gets the same filtering for free.
            let render_graph = if session.analysis_mini_view {
                let compute_ids: std::collections::BTreeSet<app_core::AnalysisNodeId> = render_graph
                    .nodes
                    .iter()
                    .filter(|node| node.kind == RenderNodeKind::Compute)
                    .map(|node| node.id.clone())
                    .collect();
                crate::studio::analysis_model::RenderGraph {
                    nodes: render_graph
                        .nodes
                        .into_iter()
                        .filter(|node| node.kind == RenderNodeKind::Compute)
                        .collect(),
                    edges: render_graph
                        .edges
                        .into_iter()
                        .filter(|(from, to)| {
                            compute_ids.contains(from) && compute_ids.contains(to)
                        })
                        .collect(),
                }
            } else {
                render_graph
            };
            // Confirmed real bug fix -- see `selected_progress_and_status`'s
            // doc comment: the canvas box and inspector used to be able to
            // show different completion percentages for the same node.
            let selected_render_state = render_graph
                .node(&app_core::AnalysisNodeId::new(selected_node_id))
                .map(|node| node.state);
            let (selected_progress, selected_status) = selected_progress_and_status(
                selected_render_state,
                selected_progress,
                selected_status,
            );
            let render_ids: Vec<app_core::AnalysisNodeId> =
                render_graph.nodes.iter().map(|n| n.id.clone()).collect();
            let layout =
                layered_layout_from_edges(&render_ids, &render_graph.edges, LayoutSpacing::default());
            let canvas_width = layout.as_ref().map_or(900.0, |l| l.canvas_width).max(900.0);
            let canvas_height = layout.as_ref().map_or(430.0, |l| l.canvas_height).max(300.0);
            let zoom = clamp_analysis_graph_zoom(session.analysis_graph_zoom);
            let scaled_canvas_width = canvas_width * zoom;
            let scaled_canvas_height = canvas_height * zoom;

            // Focus targets for §7.8/§9.3's "Focus Current/Failed/Stale" --
            // real per-node `NodeState::Failed`/`::Stale` from the Phase 1
            // planner (`plan_preview`), not `GraphNodeState` (the render
            // state the canvas boxes use below), which doesn't carry those
            // two variants yet. A button is only spawned when a matching
            // node genuinely exists this pass, per the phase plan's own
            // "菜单项必须按状态和节点能力启用或禁用".
            let current_focus = live_node_id
                .map(app_core::AnalysisNodeId::new)
                .and_then(|id| analysis_graph_focus_target(layout.as_ref(), &id, zoom));
            let failed_focus = plan_preview
                .as_ref()
                .and_then(|plan| {
                    plan.nodes
                        .iter()
                        .find(|node| node.state == app_core::NodeState::Failed)
                })
                .and_then(|node| analysis_graph_focus_target(layout.as_ref(), &node.id, zoom));
            let stale_focus = plan_preview
                .as_ref()
                .and_then(|plan| {
                    plan.nodes
                        .iter()
                        .find(|node| node.state == app_core::NodeState::Stale)
                })
                .and_then(|node| analysis_graph_focus_target(layout.as_ref(), &node.id, zoom));

            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|controls| {
                    spawn_text(controls, font.clone(), "VIEW", 7.0, theme.muted_foreground);
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "−",
                        11.0,
                        UiAction::AdjustAnalysisGraphZoom(
                            -((ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32),
                        ),
                    );
                    spawn_text(
                        controls,
                        font.clone(),
                        format!("{:.0}%", zoom * 100.0),
                        9.0,
                        theme.foreground,
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "+",
                        11.0,
                        UiAction::AdjustAnalysisGraphZoom(
                            (ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32,
                        ),
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "Fit",
                        9.0,
                        UiAction::FitAnalysisGraph(canvas_width.round() as i32),
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        if session.analysis_mini_view {
                            "Full view"
                        } else {
                            "MINI view"
                        },
                        9.0,
                        UiAction::ToggleAnalysisMiniView,
                    );
                    if let Some((scroll, stage_id)) = current_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus current",
                            9.0,
                            UiAction::FocusAnalysisGraphNode(scroll, stage_id),
                        );
                    }
                    if let Some((scroll, stage_id)) = failed_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus failed",
                            9.0,
                            UiAction::FocusAnalysisGraphNode(scroll, stage_id),
                        );
                    }
                    if let Some((scroll, stage_id)) = stale_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus stale",
                            9.0,
                            UiAction::FocusAnalysisGraphNode(scroll, stage_id),
                        );
                    }
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "Plan Preview",
                        9.0,
                        UiAction::OpenPlanPreview(task.file_hash.clone()),
                    );
                });

            session_card
                .spawn((
                    AnalysisGraphViewport,
                    ScrollPosition(Vec2::new(session.analysis_graph_scroll_offset, 0.0)),
                    Node {
                        width: percent(100),
                        height: px(445),
                        overflow: Overflow::scroll_x(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.34)),
                    BorderColor::all(theme.border.with_alpha(0.5)),
                ))
                .with_children(|viewport| {
                    viewport
                        .spawn(Node {
                            position_type: PositionType::Relative,
                            width: px(scaled_canvas_width),
                            height: px(scaled_canvas_height),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|graph| {
                            let Some(layout) = layout.as_ref() else {
                                return;
                            };
                            let spacing = LayoutSpacing::default();
                            let column_step = spacing.node_width + spacing.column_gap;
                            for (from, to) in &render_graph.edges {
                                let (Some(from_rect), Some(to_rect)) =
                                    (layout.rect(from), layout.rect(to))
                                else {
                                    continue;
                                };
                                let ready = render_graph
                                    .node(to)
                                    .is_some_and(|n| n.state == GraphNodeState::Complete);
                                let from_box = zoomed_box(from_rect, zoom);
                                let to_box = zoomed_box(to_rect, zoom);
                                let from_port = from_box.right_port();
                                let to_port = to_box.left_port();
                                // With no crossing-minimization pass, an
                                // edge spanning more than one column's step
                                // naively elbowed straight through whatever
                                // sat in the column(s) it skipped -- lines
                                // cutting across unrelated node cards and
                                // their icons. Route those above every row
                                // instead, along a rail in the canvas's own
                                // top margin (clear of every node's `y`,
                                // which starts at `spacing.margin`); an
                                // adjacent-column edge has no column to
                                // skip, so it keeps the plain elbow.
                                let points = if to_rect.x - from_rect.x > column_step * 1.5 {
                                    let rail_y = spacing.margin * 0.5 * zoom;
                                    [
                                        from_port,
                                        Vec2::new(from_port.x, rail_y),
                                        Vec2::new(to_port.x, rail_y),
                                        to_port,
                                    ]
                                } else {
                                    let mid_x = (from_port.x + to_port.x) / 2.0;
                                    [
                                        from_port,
                                        Vec2::new(mid_x, from_port.y),
                                        Vec2::new(mid_x, to_port.y),
                                        to_port,
                                    ]
                                };
                                spawn_analysis_graph_path(graph, theme, &points, ready);
                            }
                            for node in &render_graph.nodes {
                                let Some(rect) = layout.rect(&node.id) else {
                                    continue;
                                };
                                let bounds = zoomed_box(rect, zoom);
                                match node.kind {
                                    RenderNodeKind::Compute => {
                                        let bucket = analysis_node_stage_index(node.id.as_str())
                                            .unwrap_or(0);
                                        let stage_id = bucket_stage_id(bucket);
                                        let (state, override_text) =
                                            graph_node_state_to_stage_state(
                                                node.state,
                                                active_stage_progress,
                                            );
                                        let (mut route, mut warning) =
                                            analysis_graph_route_summary(
                                                task,
                                                node.id.as_str(),
                                                stage_id,
                                                stage_complete(bucket),
                                            );
                                        if let Some(text) = override_text {
                                            route = text.to_string();
                                            warning = matches!(
                                                node.state,
                                                GraphNodeState::Blocked
                                                    | GraphNodeState::Failed
                                                    | GraphNodeState::Stale
                                            );
                                        }
                                        if node.collapsed_child_count > 0 {
                                            route = format!(
                                                "{route} · {} sub-check{} not shown",
                                                node.collapsed_child_count,
                                                if node.collapsed_child_count == 1 {
                                                    ""
                                                } else {
                                                    "s"
                                                }
                                            );
                                        }
                                        spawn_analysis_stage_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            bounds,
                                            bucket,
                                            stage_id,
                                            node.id.as_str(),
                                            &task.file_hash,
                                            &node.label,
                                            state,
                                            selected_stage == stage_id,
                                            &route,
                                            warning,
                                        );
                                    }
                                    RenderNodeKind::Artifact => {
                                        spawn_analysis_artifact_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            bounds,
                                            "ARTIFACT",
                                            &node.label,
                                            &node.detail,
                                            node.state == GraphNodeState::Complete,
                                            false,
                                            None,
                                        );
                                    }
                                    RenderNodeKind::Export => {
                                        // Both export boxes ("UTZ package",
                                        // "UltraStar chart") are produced by
                                        // the same real node -- see
                                        // `build_render_graph`'s Export
                                        // construction.
                                        let owner_id = "chart.build_candidate";
                                        let owner_bucket =
                                            analysis_node_stage_index(owner_id).unwrap_or(0);
                                        let owner_label = graph_spec
                                            .node(&app_core::AnalysisNodeId::new(owner_id))
                                            .map(|spec| spec.label.clone())
                                            .unwrap_or_else(|| owner_id.to_string());
                                        spawn_analysis_artifact_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            bounds,
                                            "OUTPUT",
                                            &node.label,
                                            &node.detail,
                                            node.state == GraphNodeState::Complete,
                                            true,
                                            Some((
                                                owner_id.to_string(),
                                                owner_label,
                                                task.file_hash.clone(),
                                                bucket_stage_id(owner_bucket).to_string(),
                                            )),
                                        );
                                    }
                                }
                            }
                        });
                })
                .observe(
                    |mut drag: On<Pointer<Drag>>,
                     ui_scale: Res<UiScale>,
                     mut session: ResMut<StudioSession>,
                     mut viewports: Query<
                        (&ComputedNode, &mut ScrollPosition),
                        With<AnalysisGraphViewport>,
                    >| {
                        if drag.button != PointerButton::Primary {
                            return;
                        }
                        drag.propagate(false);
                        let Ok((computed, mut position)) = viewports.single_mut() else {
                            return;
                        };
                        let size = computed.size() * computed.inverse_scale_factor();
                        let content = computed.content_size() * computed.inverse_scale_factor();
                        let delta = drag.delta / ui_scale.0;
                        position.x = (position.x - delta.x)
                            .clamp(0.0, (content.x - size.x).max(0.0));
                        session.analysis_graph_scroll_offset = position.x;
                    },
                );

            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(16)),
                        row_gap: px(12),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.34)),
                    BorderColor::all(theme.primary.with_alpha(0.38)),
                ))
                .with_children(|inspector| {
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(5),
                            ..default()
                        })
                        .with_children(|header| {
                            spawn_text(
                                header,
                                font.clone(),
                                format!(
                                    "STEP {:02} · {}",
                                    selected_stage_index + 1,
                                    selected_label.to_ascii_uppercase()
                                ),
                                9.0,
                                theme.primary,
                            );
                            header.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                header,
                                font.clone(),
                                format!("{selected_status} · {selected_progress}%"),
                                9.0,
                                if selected_status == "WAITING" {
                                    theme.muted_foreground
                                } else {
                                    theme.pitch_contour
                                },
                            );
                        });
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_purpose,
                        10.0,
                        theme.muted_foreground,
                    );
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_operation,
                        13.0,
                        theme.foreground,
                    );
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(9),
                            row_gap: px(9),
                            ..default()
                        })
                        .with_children(|facts| {
                            // §7.4 lists 14 facts; ALGORITHM VERSION / CACHE
                            // SIGNATURE / LAST ATTEMPT come from the active
                            // artifact revision, FALLBACK from the same
                            // route data the "current operation" banner
                            // above already uses, ERROR from the selected
                            // history run, DURATION from the same route's
                            // real `started_at_ms`/`finished_at_ms`
                            // (`server.py::_progress_payload`, Phase 7's
                            // "Duration 检查器字段" gap closed), and the
                            // node's one profile parameter (+ its source)
                            // only when the selected node actually has one
                            // (`selected_stage_parameter`) -- PARAMETER
                            // SOURCE-without-a-parameter is intentionally
                            // omitted rather than faked: a source with no
                            // parameter to source would be meaningless.
                            let mut fact_rows: Vec<(&str, String)> = vec![
                                ("IMPLEMENTATION", selected_implementation.to_string()),
                                ("MODEL / ALGORITHM", selected_model.to_string()),
                                ("REQUESTED DEVICE", selected_requested_device.to_string()),
                                ("ACTUAL DEVICE", selected_actual_device.to_string()),
                                ("INPUT", selected_input.to_string()),
                                ("OUTPUT", selected_output.to_string()),
                                ("ALGORITHM VERSION", selected_algorithm_version.clone()),
                                ("CACHE SIGNATURE", selected_cache_signature.clone()),
                                ("LAST ATTEMPT", selected_last_attempt.clone()),
                                ("DURATION", selected_duration_text.clone()),
                                ("FALLBACK", selected_fallback_text.clone()),
                                ("ERROR", selected_error_text.clone()),
                            ];
                            if let Some((label, value)) = selected_parameter.clone() {
                                fact_rows.push((label, value));
                                fact_rows
                                    .push(("PARAMETER SOURCE", selected_parameter_source.to_string()));
                            }
                            for (label, value) in fact_rows {
                                let value_color = if label == "ERROR" && value != "None recorded" {
                                    theme.destructive
                                } else if label == "FALLBACK" && value != "None" {
                                    theme.editor_warning
                                } else {
                                    theme.foreground
                                };
                                facts
                                    .spawn((
                                        Node {
                                            min_width: px(205),
                                            flex_basis: px(240),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::all(px(10)),
                                            row_gap: px(3),
                                            overflow: Overflow::clip(),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.34)),
                                        BorderColor::all(theme.border.with_alpha(0.4)),
                                    ))
                                    .with_children(|fact| {
                                        spawn_text(
                                            fact,
                                            font.clone(),
                                            label,
                                            7.0,
                                            theme.muted_foreground,
                                        );
                                        spawn_bounded_wrapped_text(
                                            fact,
                                            font.clone(),
                                            value,
                                            9.0,
                                            value_color,
                                        );
                                    });
                            }
                        });
                    inspector
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(10)),
                                row_gap: px(4),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.34)),
                            BorderColor::all(theme.border.with_alpha(0.4)),
                        ))
                        .with_children(|plan_box| {
                            plan_box
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    ..default()
                                })
                                .with_children(|plan_header| {
                                    spawn_text(
                                        plan_header,
                                        font.clone(),
                                        "PLAN & ARTIFACTS",
                                        7.0,
                                        theme.muted_foreground,
                                    );
                                    plan_header.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    spawn_text_button(
                                        plan_header,
                                        font.clone(),
                                        theme,
                                        "Sync from disk",
                                        8.0,
                                        UiAction::SyncArtifactRevisions(task.file_hash.clone()),
                                    );
                                });
                            spawn_text(
                                plan_box,
                                font.clone(),
                                format!("{plan_state_copy} · {plan_will_run_copy}"),
                                9.0,
                                theme.foreground,
                            );
                            if let Some(artifact_copy) = plan_artifact_copy {
                                spawn_text(
                                    plan_box,
                                    font.clone(),
                                    artifact_copy,
                                    9.0,
                                    theme.muted_foreground,
                                );
                            }
                            if let Some(reason) = plan_reason_copy {
                                spawn_wrapped_text(
                                    plan_box,
                                    font.clone(),
                                    reason,
                                    9.0,
                                    theme.editor_warning,
                                );
                            }
                            for revision in &artifact_revisions {
                                let file_name = revision
                                    .path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().to_string())
                                    .unwrap_or_else(|| revision.id.clone());
                                plan_box
                                    .spawn(Node {
                                        width: percent(100),
                                        align_items: AlignItems::Center,
                                        column_gap: px(8),
                                        flex_wrap: FlexWrap::Wrap,
                                        row_gap: px(4),
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        spawn_text(
                                            row,
                                            font.clone(),
                                            match (revision.active, revision.invalidated) {
                                                (_, true) => format!("✕ {file_name} · invalidated"),
                                                (true, false) => format!("● {file_name}"),
                                                (false, false) => format!("○ {file_name}"),
                                            },
                                            9.0,
                                            if revision.invalidated {
                                                theme.destructive
                                            } else if revision.active {
                                                theme.pitch_contour
                                            } else {
                                                theme.muted_foreground
                                            },
                                        );
                                        row.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        if artifact_kind_is_playable(revision.kind) {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Play",
                                                8.0,
                                                UiAction::PlayArtifactRevision(
                                                    revision.path.clone(),
                                                ),
                                            );
                                        } else {
                                            // §7.6 "Preview": the JSON/text
                                            // counterpart to "Play" above --
                                            // the two are mutually exclusive
                                            // by artifact kind, never both
                                            // shown for the same revision.
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Preview",
                                                8.0,
                                                UiAction::PreviewArtifactRevision(
                                                    revision.path.clone(),
                                                ),
                                            );
                                        }
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Open",
                                            8.0,
                                            UiAction::OpenArtifactRevision(revision.path.clone()),
                                        );
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Reveal",
                                            8.0,
                                            UiAction::RevealArtifactRevision(
                                                revision.path.clone(),
                                            ),
                                        );
                                        if !revision.active && !revision.invalidated {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Set active",
                                                8.0,
                                                UiAction::SetActiveArtifactRevision(
                                                    revision.clone(),
                                                ),
                                            );
                                        }
                                        // Phase 6 `invalidate_artifact_revision` /
                                        // §7.6 "Invalidate": omitted once a
                                        // revision is already invalidated --
                                        // there's nothing further to invalidate,
                                        // and no "restore" action exists yet
                                        // (a fresh rerun or Sync from disk is
                                        // the intended way back).
                                        if !revision.invalidated {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Invalidate",
                                                8.0,
                                                UiAction::RequestInvalidateArtifactRevision(
                                                    revision.clone(),
                                                ),
                                            );
                                        }
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Inspect provenance",
                                            8.0,
                                            UiAction::InspectArtifactProvenance(revision.clone()),
                                        );
                                        // §7.6 "Compare revisions": against
                                        // whichever revision is Active for
                                        // this kind -- omitted for the
                                        // Active revision itself (nothing to
                                        // compare it to) and when this song's
                                        // kind has no Active revision at all.
                                        if !revision.active
                                            && let Some(active) = active_revision
                                        {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Compare revisions",
                                                8.0,
                                                UiAction::CompareArtifactRevisions(
                                                    revision.clone(),
                                                    active.id.clone(),
                                                ),
                                            );
                                        }
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Delete",
                                            8.0,
                                            UiAction::RequestDeleteArtifactRevision(
                                                revision.clone(),
                                            ),
                                        );
                                    });
                            }
                        });
                    for (label, from, to, reason) in selected_device_fallback
                        .map(|(from, reason)| {
                            ("COMPUTE FALLBACK", from, selected_actual_device, reason)
                        })
                        .into_iter()
                        .chain(selected_backend_fallback.map(|(from, reason)| {
                            ("MODEL FALLBACK", from, selected_implementation, reason)
                        }))
                    {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!(
                                "{label} · {} > {} · {reason}",
                                from.to_ascii_uppercase(),
                                to.to_ascii_uppercase()
                            ),
                            9.0,
                            theme.editor_warning,
                        );
                    }
                    if let Some(error) = history_error {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!("SESSION ERROR · {error}"),
                            9.0,
                            theme.destructive,
                        );
                    }
                });

            if let Some(live) = task.live.as_ref() {
                session_card
                    .spawn(Node {
                        width: percent(100),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(10),
                        row_gap: px(10),
                        ..default()
                    })
                    .with_children(|details| {
                        let device_route = live
                            .fallback_from
                            .as_ref()
                            .map(|from| {
                                format!(
                                    "{} > {}",
                                    from.to_ascii_uppercase(),
                                    live.device.to_ascii_uppercase()
                                )
                            })
                            .unwrap_or_else(|| live.device.to_ascii_uppercase());
                        for (label, value) in [
                            ("IMPLEMENTATION", live.implementation.clone()),
                            ("MODEL / ALGORITHM", live.model.clone()),
                            ("ACTUAL COMPUTE ROUTE", device_route),
                        ] {
                            details
                                .spawn((
                                    Node {
                                        min_width: px(230),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(px(12)),
                                        row_gap: px(3),
                                        overflow: Overflow::clip(),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.26)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                ))
                                .with_children(|item| {
                                    spawn_text(
                                        item,
                                        font.clone(),
                                        label,
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                    spawn_bounded_wrapped_text(
                                        item,
                                        font.clone(),
                                        value,
                                        10.0,
                                        theme.foreground,
                                    );
                                });
                        }
                    });
            }
        });

    if let Some(context) = session.analysis_node_context.as_ref() {
        spawn_analysis_node_context_menu(parent, font.clone(), theme, context);
    }
    if let Some(dialog) = session.node_config_dialog.as_ref() {
        spawn_node_config_dialog(
            parent,
            font.clone(),
            theme,
            dialog,
            session.config.compute_backend.as_deref() == Some("intel"),
            session.notice.as_deref(),
        );
    }
    if let Some(draft) = session.plan_preview_draft.as_ref() {
        spawn_plan_preview_dialog(
            parent,
            font.clone(),
            theme,
            draft,
            session.notice.as_deref(),
        );
    }
    if let Some(state) = session.app_log_viewer.as_ref() {
        spawn_app_log_viewer(
            parent,
            font.clone(),
            theme,
            state,
            session.selected_analysis_history,
        );
    }
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    node_id: &str,
    stage_id: &str,
    completed: bool,
) -> (String, bool) {
    let route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, node_id, stage_id));
    let Some(route) = route else {
        return (
            if completed {
                "Complete · no runtime trace".to_string()
            } else {
                "Awaiting connected inputs".to_string()
            },
            false,
        );
    };
    let warning = route.fallback_from.is_some() || route.backend_fallback_from.is_some();
    let implementation = route
        .backend_fallback_from
        .as_ref()
        .map(|from| {
            format!(
                "{} > {}",
                from.to_ascii_uppercase(),
                route.implementation.to_ascii_uppercase()
            )
        })
        .unwrap_or_else(|| route.implementation.clone());
    let model = (!route.model.trim().is_empty())
        .then(|| route.model.as_str())
        .unwrap_or("default");
    (format!("{implementation} · {model}"), warning)
}

pub(crate) fn spawn_analysis_stage_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    index: usize,
    stage_id: &str,
    node_id: &str,
    file_hash: &str,
    label: &str,
    state: AnalysisGraphStageState,
    selected: bool,
    route: &str,
    warning: bool,
) {
    let (status, progress, status_color) = match state {
        AnalysisGraphStageState::Waiting => ("WAITING", 0, theme.muted_foreground),
        AnalysisGraphStageState::Running(progress) => ("RUNNING", progress, theme.primary),
        AnalysisGraphStageState::Complete => ("COMPLETE", 100, theme.pitch_contour),
    };
    let running = matches!(state, AnalysisGraphStageState::Running(_));
    let complete = matches!(state, AnalysisGraphStageState::Complete);
    let context_node_id = node_id.to_string();
    let context_stage_id = stage_id.to_string();
    let context_file_hash = file_hash.to_string();
    let context_label = label.to_string();
    parent
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(7),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(if running {
                theme.primary.with_alpha(0.16)
            } else if selected {
                theme.card.with_alpha(0.9)
            } else {
                theme.card.with_alpha(0.68)
            }),
            BorderColor::all(if selected {
                theme.primary.with_alpha(0.92)
            } else if running {
                theme.primary.with_alpha(0.62)
            } else if complete {
                theme.pitch_contour.with_alpha(0.42)
            } else {
                theme.border.with_alpha(0.68)
            }),
            ZIndex(2),
        ))
        .with_children(|node| {
            spawn_analysis_graph_ports(node, theme, complete || running);
            if selected {
                node.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(9),
                        bottom: px(9),
                        width: px(2),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.primary),
                    Pickable::IGNORE,
                ));
            }
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|heading| {
                heading
                    .spawn((
                        Node {
                            width: px(22),
                            height: px(22),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(if running {
                            theme.primary
                        } else if complete {
                            theme.pitch_contour
                        } else {
                            theme.muted
                        }),
                    ))
                    .with_children(|badge| {
                        spawn_text(
                            badge,
                            font.clone(),
                            format!("{:02}", index + 1),
                            7.0,
                            if running || complete {
                                theme.background
                            } else {
                                theme.muted_foreground
                            },
                        );
                    });
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), label, 9.0, theme.foreground);
                        spawn_text(copy, font.clone(), status, 7.0, status_color);
                    });
            });
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|meter| {
                meter
                    .spawn((
                        Node {
                            min_width: px(0),
                            height: px(3),
                            flex_grow: 1.0,
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.muted.with_alpha(0.72)),
                    ))
                    .with_children(|rail| {
                        rail.spawn((
                            Node {
                                width: percent(progress as f32),
                                height: percent(100),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(if complete {
                                theme.pitch_contour
                            } else {
                                theme.primary
                            }),
                        ));
                    });
                spawn_text(
                    meter,
                    font.clone(),
                    format!("{progress}%"),
                    7.0,
                    status_color,
                );
            });
            spawn_bounded_wrapped_text(
                node,
                font,
                route,
                7.0,
                if warning {
                    theme.editor_warning
                } else {
                    theme.muted_foreground
                },
            );
        })
        .observe(
            move |mut event: On<Pointer<Click>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>,
                  lists: Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>| {
                event.propagate(false);
                let menu_position = analysis_context_menu_position(
                    event.pointer_location.position,
                    session.library_scroll_offset,
                    &lists,
                );
                open_analysis_node_from_click(
                    &event,
                    menu_position,
                    &context_node_id,
                    &context_label,
                    &context_file_hash,
                    &context_stage_id,
                    &mut session,
                    &mut invalidated,
                );
            },
        );
}

/// The click position `open_analysis_node_from_click` needs, converted from
/// raw window pixels into `LibrarySongList`'s own local space -- the
/// analysis node context menu is spawned as a direct absolute-positioned
/// child of that same list (`spawn_analysis_node_context_menu`), so that is
/// the coordinate space its `left`/`top` need. Falls back to the raw window
/// position if the list isn't found (defensive only -- every caller of this
/// only runs from inside that list's own subtree).
fn analysis_context_menu_position(
    window_position: Vec2,
    scroll_offset: f32,
    lists: &Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>,
) -> Vec2 {
    let Ok((computed, transform)) = lists.single() else {
        return window_position;
    };
    // `UiGlobalTransform` maps into space centered on the node (matching
    // `ui_node_contains_pointer`'s own use of it), not space anchored at its
    // top-left corner -- which is what `Node::left`/`Node::top` on the
    // absolute-positioned menu actually need. Add back half the list's own
    // size to shift the origin from its center to its corner.
    let center_relative = transform
        .affine()
        .inverse()
        .transform_point2(window_position);
    let half_size = computed.size() * computed.inverse_scale_factor() / 2.0;
    let mut local = center_relative + half_size;
    // Bevy's UI layout subtracts a node's *parent's* scroll position from
    // its rendered spot regardless of position type -- an absolute child of
    // a scrolling node still moves with that scroll (unlike CSS, where
    // `position: absolute` normally opts out of that). The menu is a direct
    // child of `LibrarySongList`, so without adding the list's current
    // scroll back in here, the menu would render offset from the node by
    // however far the list had been scrolled at click time. The list only
    // scrolls vertically (`ScrollPosition(Vec2::new(0.0, ...))`), so only Y
    // needs it.
    local.y += scroll_offset;
    local
}

/// `click_target`, when set, is `(node_id, label, file_hash, stage_id)` of
/// the *real* compute node this virtual box's output belongs to (an
/// Artifact/Export box is never itself a real `AnalysisGraphSpec` node --
/// see `build_render_graph`'s doc comment) -- clicking it opens the same
/// node context menu (right-click) / inspector selection (left-click) as
/// that real node, via `open_analysis_node_from_click`, so "Run this node
/// only" etc. on an output box runs the compute step that actually produces
/// it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_artifact_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    eyebrow: &str,
    title: &str,
    detail: &str,
    ready: bool,
    output: bool,
    click_target: Option<(String, String, String, String)>,
) {
    let accent = if output {
        theme.primary
    } else {
        theme.pitch_contour
    };
    let clickable = click_target.is_some();
    let mut entity = parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(bounds.x),
            top: px(bounds.y),
            width: px(bounds.width),
            height: px(bounds.height),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            row_gap: px(2),
            overflow: Overflow::clip(),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(if output { 8 } else { 18 })),
            ..default()
        },
        BackgroundColor(if ready {
            accent.with_alpha(0.1)
        } else {
            theme.background.with_alpha(0.72)
        }),
        BorderColor::all(if ready {
            accent.with_alpha(0.62)
        } else {
            theme.border.with_alpha(0.62)
        }),
        ZIndex(2),
    ));
    if clickable {
        entity.insert(Button);
    }
    entity.with_children(|node| {
        spawn_analysis_graph_ports(node, theme, ready);
        spawn_text(
            node,
            font.clone(),
            format!(
                "{eyebrow} · {}",
                if ready {
                    if output { "AVAILABLE" } else { "READY" }
                } else {
                    "PENDING"
                }
            ),
            6.5,
            if ready { accent } else { theme.muted_foreground },
        );
        spawn_text(node, font.clone(), title, 9.0, theme.foreground);
        spawn_bounded_wrapped_text(node, font, detail, 7.0, theme.muted_foreground);
    });
    if let Some((node_id, label, file_hash, stage_id)) = click_target {
        entity.observe(
            move |mut event: On<Pointer<Click>>,
                  mut session: ResMut<StudioSession>,
                  mut invalidated: ResMut<UiInvalidated>,
                  lists: Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>| {
                event.propagate(false);
                let menu_position = analysis_context_menu_position(
                    event.pointer_location.position,
                    session.library_scroll_offset,
                    &lists,
                );
                open_analysis_node_from_click(
                    &event,
                    menu_position,
                    &node_id,
                    &label,
                    &file_hash,
                    &stage_id,
                    &mut session,
                    &mut invalidated,
                );
            },
        );
    }
}

pub(crate) fn spawn_analysis_graph_ports(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    ready: bool,
) {
    for (left, right) in [(Some(px(-5)), None), (None, Some(px(-5)))] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: left.unwrap_or_default(),
                right: right.unwrap_or_default(),
                top: percent(50),
                width: px(10),
                height: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            UiTransform::from_xy(px(0), px(-5)),
            BackgroundColor(if ready {
                theme.pitch_contour
            } else {
                theme.muted
            }),
            BorderColor::all(theme.background.with_alpha(0.9)),
            Pickable::IGNORE,
        ));
    }
}

pub(crate) fn spawn_analysis_graph_path(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    points: &[Vec2],
    ready: bool,
) {
    let color = if ready {
        theme.pitch_contour.with_alpha(0.68)
    } else {
        theme.border.with_alpha(0.64)
    };
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let horizontal = (from.y - to.y).abs() <= 0.5;
        let left = from.x.min(to.x);
        let top = from.y.min(to.y);
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(if horizontal {
                    (to.x - from.x).abs().max(2.0)
                } else {
                    2.0
                }),
                height: px(if horizontal {
                    2.0
                } else {
                    (to.y - from.y).abs().max(2.0)
                }),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            ZIndex(0),
            Pickable::IGNORE,
        ));
    }
}
