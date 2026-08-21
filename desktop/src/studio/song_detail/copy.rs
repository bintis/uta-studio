use crate::studio::*;

pub(crate) fn spawn_song_primary_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    song: &Song,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let state = app_core::resolve_song_authoring_state(&song.file_hash)
        .unwrap_or(app_core::SongAuthoringState::AnalyzeSong);
    match state {
        app_core::SongAuthoringState::InProgress => {
            let label = session
                .analysis_tasks
                .iter()
                .find(|task| task.file_hash == song.file_hash)
                .map(|task| match task.status {
                    app_core::QueuedStatus::Queued => "Queued for analysis".to_string(),
                    app_core::QueuedStatus::Analyzing(progress) => {
                        format!("Analyzing · {progress}%")
                    }
                    app_core::QueuedStatus::Failed(_) => "Analyzing".to_string(),
                })
                .unwrap_or_else(|| "Analyzing".to_string());
            spawn_action_button(
                parent,
                font,
                theme,
                label,
                UiAction::from(AppCommand::ToggleActivity),
            );
        }
        app_core::SongAuthoringState::RetryFailedNode => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Retry failed analysis",
                UiAction::from(LibraryCommand::AnalyzeSong(song.file_hash.clone())),
            );
        }
        app_core::SongAuthoringState::AnalyzeSong => {
            if app_core::analysis_runtime_status().ready {
                spawn_action_button(
                    parent,
                    font,
                    theme,
                    "Analyze song",
                    UiAction::from(LibraryCommand::AnalyzeSong(song.file_hash.clone())),
                );
            } else {
                spawn_action_button(
                    parent,
                    font,
                    theme,
                    "Set up analysis",
                    UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
                );
            }
        }
        app_core::SongAuthoringState::OpenEditor => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Open editor",
                UiAction::from(LibraryCommand::OpenEditor(song.file_hash.clone())),
            );
        }
        app_core::SongAuthoringState::FixChartIssues => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Fix chart issues",
                UiAction::from(LibraryCommand::OpenEditor(song.file_hash.clone())),
            );
        }
        app_core::SongAuthoringState::EditChart => {
            spawn_action_button(
                parent,
                font,
                theme,
                "Edit chart",
                UiAction::from(LibraryCommand::OpenEditor(song.file_hash.clone())),
            );
        }
    }
}

pub(crate) fn spawn_detail_heading(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                padding: UiRect::axes(px(16), px(13)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|header| {
            spawn_text(header, font.clone(), eyebrow, 8.0, theme.primary);
            spawn_text(header, font, title, 13.0, theme.foreground);
        });
}

/// One of the phase plan §8.2's 6 independent, named section cards
/// (Overview/Analysis/Lyrics & Timing/Audio & Pitch/Authoring &
/// Export/Artifacts & History) -- same bordered-card style every card on
/// this page already uses (`BackgroundColor(theme.card.with_alpha(0.32))` +
/// `BorderColor`), factored out since the page now has 6 of them instead of
/// the 2 (one wide "Production controls" card with subheadings crammed into
/// a single scrolling column, one "Overview" card) it used to. `min_width`
/// also doubles as `flex_basis` so cards keep a sensible starting width
/// before the row's `FlexWrap::Wrap` reflows them.
pub(crate) fn spawn_song_detail_section_card(
    columns: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    min_width: f32,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    columns
        .spawn((
            Node {
                min_width: px(min_width),
                flex_basis: px(min_width),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.32)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(build);
}

pub(crate) fn spawn_detail_value(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    value: String,
) {
    parent
        .spawn((
            Node {
                min_height: px(48),
                padding: UiRect::axes(px(14), px(10)),
                flex_direction: FlexDirection::Column,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.3)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), label, 9.0, theme.muted_foreground);
            spawn_wrapped_text(row, font, value, 11.0, theme.foreground);
        });
}

/// Overview section rows (phase plan §8.2's Overview list: authoring
/// readiness, detected key/confidence, musical BPM/confidence, beat count,
/// vocal/instrumental/pitch-evidence availability, active analysis
/// profile, timed-lyrics source, chart assets). Reads real on-disk/DB
/// state directly (`load_music_analysis`, `cached_artifact_presence_for_song`,
/// `get_song_analysis_profile`) rather than only `Song`'s cached summary
/// fields, same accepted pattern as this file's other direct app-core
/// reads during render. Deliberately **not** included: a chart issue
/// count -- that needs the full `ChartDocument`'s `ChartProblem` list,
/// which only exists once the chart is loaded into the editor, not
/// something to load and parse on every Song Detail render.
pub(crate) fn song_overview_rows(song: &Song) -> Vec<(&'static str, String)> {
    let media = song
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("media")
        .to_ascii_uppercase();
    let transcript = song
        .transcript_source
        .as_ref()
        .map(|source| format!("{source:?}"))
        .unwrap_or_else(|| "Not generated".to_string());
    let music_analysis = app_core::load_music_analysis(&app_core::CacheDir::new(), &song.file_hash);
    let presence = app_core::cached_artifact_presence_for_song(&song.file_hash);
    let has_artifact = |kind: app_core::ArtifactKind| app_core::artifact_present(&presence, kind);
    let profile_source = if app_core::get_song_analysis_profile(&song.file_hash).is_some() {
        "Song override"
    } else {
        "Global defaults"
    };

    let mut rows = vec![
        (
            "Media",
            format!(
                "{media} · {}",
                if song.is_video { "Video" } else { "Audio" }
            ),
        ),
        (
            "Analysis",
            if song.is_analyzed {
                "Analyzed"
            } else {
                "Not analyzed"
            }
            .to_string(),
        ),
        ("Active analysis profile", profile_source.to_string()),
        ("Lyrics source", transcript),
    ];

    // §8.2 Overview's "Last successful run" -- previously recorded as
    // blocked on "the Phase 3 history writer," which was stale: the
    // `analysis_history` table this reads has existed since before this
    // pass and already carries everything needed (`file_hash`, `status`,
    // `finished_at_ms`); the only actual gap was that nothing queried it
    // for this row yet.
    rows.push((
        "Last successful run",
        last_successful_run_copy(&app_core::load_analysis_history(200), &song.file_hash),
    ));

    // Phase 5 §5.5 "New candidate analysis is available" -- real, mtime-based
    // staleness comparison (`app_core::candidate_chart_status`), not a
    // placeholder. Omitted entirely (not just "N/A") for a song that has
    // never been authored yet: `chart_readiness`'s own missing-assets copy
    // already covers that case, and "candidate" isn't a meaningful concept
    // until there's an Authored Chart to compare it against.
    if let Some(copy) =
        candidate_availability_copy(&app_core::candidate_chart_status(&song.file_hash))
    {
        rows.push(("Candidate availability", copy));
    }

    // Phase 8 "Chart issue count" -- previously deferred as "needs a full
    // `ChartDocument` load, too expensive per render," which turned out to
    // be a false premise: `EditorDocument::new` only needs the chart's
    // structural data (lyrics/notes), not `ChartAudio`/`playable_audio`
    // resolution, so `app_core::chart_problem_count` is cheap enough to
    // call directly here.
    if let Some(copy) = chart_issue_count_copy(app_core::chart_problem_count(&song.file_hash)) {
        rows.push(("Chart issues", copy));
    }

    if let Some(analysis) = music_analysis.as_ref() {
        // §9.2 Music Analysis acceptance: "Unknown Key shows as Warning, not
        // Failure" -- this row has no error/failure styling to begin with
        // (plain informational text), so an undetected key already renders
        // as the same "Unknown" text it always has, never as a failure.
        rows.push(("Detected key", detected_key_copy(&analysis.key)));
        // "BPM-only fallback correctly displayed" -- `beats` is empty
        // whenever `analyze_rhythm` (rhythm.py) could only estimate a global
        // tempo via autocorrelation, without Essentia's full beat tracker
        // (see the doc comment on `MusicRhythmAnalysis::beats`). Previously
        // this rendered as "· 0 beats", indistinguishable from a bug; now
        // it's named explicitly.
        rows.push(("Musical BPM", musical_bpm_copy(&analysis.rhythm)));
        // "Descriptors unavailable shows Not Applicable" -- previously there
        // was no row for this at all, so the gap was that it was silently
        // absent rather than explicitly N/A.
        rows.push((
            "Extra descriptors",
            extra_descriptors_copy(analysis.descriptors.as_ref()),
        ));
    }

    rows.push((
        "Vocal / instrumental stems",
        match (
            has_artifact(app_core::ArtifactKind::VocalStem),
            has_artifact(app_core::ArtifactKind::InstrumentalStem),
        ) {
            (true, true) => "Both available".to_string(),
            (true, false) => "Vocal only".to_string(),
            (false, true) => "Instrumental only".to_string(),
            (false, false) if song.no_stems => "Original mix".to_string(),
            (false, false) => "Pending".to_string(),
        },
    ));
    rows.push((
        "Pitch evidence",
        if has_artifact(app_core::ArtifactKind::PitchTrack) {
            "Available"
        } else {
            "Pending"
        }
        .to_string(),
    ));
    rows.push((
        "Chart assets",
        if song.authoring_ready {
            "Complete".to_string()
        } else if song.authoring_missing.is_empty() {
            "Waiting for chart".to_string()
        } else {
            song.authoring_missing.join(" · ").replace('_', " ")
        },
    ));
    rows.push((
        "Export",
        if song.authoring_ready {
            "UTZ · UltraStar"
        } else {
            "Waiting for chart"
        }
        .to_string(),
    ));
    rows
}

/// Pure lookup behind `song_overview_rows`'s "Last successful run" row,
/// separated out so it's testable without a real DB fixture -- same pattern
/// as `resolve_song_authoring_state`/`overlay_failed_node_attempts`.
/// `history` is assumed newest-first (`analysis_history_load`'s real
/// ordering, `ORDER BY finished_at_ms DESC, id DESC`), so the first match
/// is genuinely the most recent completed run for this song, not merely *a*
/// completed run.
pub(crate) fn last_successful_run_copy(
    history: &[app_core::AnalysisRunHistory],
    file_hash: &str,
) -> String {
    history
        .iter()
        .find(|run| run.file_hash == file_hash && run.status == "completed")
        .map(|run| format_epoch_ms(run.finished_at_ms))
        .unwrap_or_else(|| "None yet".to_string())
}

/// Pure formatter behind the Overview panel's "Candidate availability" row
/// -- same "pure decision function separated from IO" pattern as
/// `last_successful_run_copy`. `None` means the row should be omitted
/// entirely (nothing authored yet, so "candidate" isn't a meaningful
/// concept for this song).
pub(crate) fn candidate_availability_copy(
    status: &app_core::CandidateChartStatus,
) -> Option<String> {
    match status {
        app_core::CandidateChartStatus::NotAuthoredYet => None,
        app_core::CandidateChartStatus::UpToDate => Some("Up to date".to_string()),
        app_core::CandidateChartStatus::CandidateAvailable(summary) => {
            let mut changed = Vec::new();
            if summary.lyrics_changed {
                changed.push("lyrics");
            }
            if summary.pitch_evidence_changed {
                changed.push("pitch");
            }
            Some(format!(
                "New candidate available ({} · {} notes vs {} authored)",
                changed.join(" & "),
                summary.candidate_note_count,
                summary.authored_note_count,
            ))
        }
    }
}

/// Pure formatter behind the Overview panel's "Chart issues" row -- same
/// "pure decision function separated from IO" pattern as
/// `candidate_availability_copy`. `None` means the row should be omitted
/// entirely (no transcript/pitch/authored chart data exists yet for this
/// song, so a problem count isn't a meaningful concept).
pub(crate) fn chart_issue_count_copy(count: Option<usize>) -> Option<String> {
    match count? {
        0 => Some("None".to_string()),
        1 => Some("1 issue".to_string()),
        n => Some(format!("{n} issues")),
    }
}

/// Pure formatter behind the Overview panel's "Detected key" row. §9.2
/// Music Analysis acceptance: "Unknown Key shows as Warning, not Failure."
pub(crate) fn detected_key_copy(key: &app_core::MusicKeyAnalysis) -> String {
    let key_name = key
        .tonic
        .as_deref()
        .map(|tonic| match key.scale.as_deref() {
            Some(scale) => format!("{tonic} {scale}"),
            None => tonic.to_string(),
        })
        .unwrap_or_else(|| "Unknown".to_string());
    format!("{key_name} (confidence {:.2})", key.confidence)
}

/// Pure formatter behind the Overview panel's "Musical BPM" row. §9.2 Music
/// Analysis acceptance: "BPM-only fallback correctly displayed" -- named
/// explicitly rather than rendering as an unexplained "0 beats".
pub(crate) fn musical_bpm_copy(rhythm: &app_core::MusicRhythmAnalysis) -> String {
    let Some(bpm) = rhythm.bpm else {
        return "Unavailable".to_string();
    };
    if rhythm.beats.is_empty() {
        format!(
            "{bpm:.1} (confidence {:.2}) · BPM-only, no beat grid",
            rhythm.confidence
        )
    } else {
        format!(
            "{bpm:.1} (confidence {:.2}) · {} beats",
            rhythm.confidence,
            rhythm.beats.len()
        )
    }
}

/// Pure formatter behind the Overview panel's "Extra descriptors" row. §9.2
/// Music Analysis acceptance: "Descriptors unavailable shows Not
/// Applicable" -- Essentia has no Windows wheel, so this is a real,
/// expected state, not an error.
pub(crate) fn extra_descriptors_copy(
    descriptors: Option<&app_core::MusicAnalysisDescriptors>,
) -> String {
    match descriptors {
        None => "Not Applicable".to_string(),
        Some(d) => format!(
            "Danceability {:.2} · Dynamic range {:.1} dB · Loudness {:.1} dB",
            d.danceability, d.dynamic_complexity_db, d.loudness_db
        ),
    }
}

pub(crate) fn album_art_handle(
    song: &Song,
    asset_server: &AssetServer,
    images: &mut Assets<Image>,
    local_images: &mut LocalImages,
) -> Handle<Image> {
    let Some(path) = song.album_art_path.as_ref() else {
        return asset_server.load(MUSIC_PLACEHOLDER_PATH);
    };
    if let Some(handle) = local_images.covers.get(path) {
        return handle.clone();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return asset_server.load(MUSIC_PLACEHOLDER_PATH);
    };
    let extension = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else {
        "jpg"
    };
    let Ok(decoded) = Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    ) else {
        return asset_server.load(MUSIC_PLACEHOLDER_PATH);
    };
    let Ok(dynamic) = decoded.try_into_dynamic() else {
        return asset_server.load(MUSIC_PLACEHOLDER_PATH);
    };
    // Library artwork can be several thousand pixels wide while its largest
    // presentation in the desktop UI is a small cover. Bounding retained
    // textures prevents a route change from uploading another full-resolution
    // image while the analyzer has recently held several gigabytes of models.
    let bounded = dynamic.thumbnail(512, 512);
    let image = Image::from_dynamic(bounded, true, RenderAssetUsages::default());
    let handle = images.add(image);
    local_images.covers.insert(path.clone(), handle.clone());
    handle
}
