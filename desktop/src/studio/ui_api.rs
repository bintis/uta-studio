//! Discoverable, structured API contract for every interactive desktop command.
//!
//! Backend feature APIs remain in `app_core::api_capabilities`; this registry
//! closes the UI layer by assigning every button, menu item, context-menu
//! action, and direct pointer gesture a stable command id and access class.

use std::{collections::HashSet, sync::OnceLock};

use bevy::prelude::{Added, Button, Component, Entity, Query, With};
use serde::Serialize;

use super::{EditorCommand, UiAction, UiCommand};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UiInteractionCapability {
    pub command: String,
    pub access: &'static str,
    pub automated_check: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UiInteractionRequest {
    pub command: String,
    pub access: &'static str,
}

/// Marks a `Button` whose operation is handled by a pointer observer rather
/// than the ordinary `UiAction` dispatcher. This is still a local API: the
/// id is discoverable below and the observer is its in-process handler.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UiPointerApi(pub &'static [&'static str]);

const APP_COMMANDS: &[&str] = &[
    "ui.app.back",
    "ui.app.home",
    "ui.app.toggle_global_search",
    "ui.app.folders",
    "ui.app.settings",
    "ui.app.documentation",
    "ui.app.open_documentation",
    "ui.app.documentation_back",
    "ui.app.documentation_forward",
    "ui.app.toggle_activity",
    "ui.app.close_activity",
    "ui.app.open_about",
    "ui.app.close_about",
    "ui.app.toggle_fullscreen",
    "ui.app.open_log",
    "ui.app.run_diagnostics",
    "ui.app.cancel_leave",
    "ui.app.confirm_leave",
];

const LIBRARY_COMMANDS: &[&str] = &[
    "ui.library.set_library_view",
    "ui.library.set_library_facet",
    "ui.library.load_more_songs",
    "ui.library.apply_library_search",
    "ui.library.clear_library_search",
    "ui.library.toggle_library_layout",
    "ui.library.toggle_export_all_menu",
    "ui.library.export_all_utz",
    "ui.library.export_all_ultra_star",
    "ui.library.open_library_select",
    "ui.library.select_library_value",
    "ui.library.analyze_all",
    "ui.library.rescan_library",
    "ui.library.choose_folder",
    "ui.library.choose_export_folder",
    "ui.library.clear_export_folder",
    "ui.library.select_folder_root",
    "ui.library.folder_up",
    "ui.library.open_folder_entry",
    "ui.library.reveal_folder_entry",
    "ui.library.dismiss_folder_context",
    "ui.library.request_remove_folder",
    "ui.library.cancel_remove_folder",
    "ui.library.confirm_remove_folder",
    "ui.library.open_song",
    "ui.library.analyze_song",
    "ui.library.choose_editor_file",
    "ui.library.open_editor",
    "ui.library.export_utz",
    "ui.library.export_ultra_star",
    "ui.library.open_source",
    "ui.library.reveal_source",
    "ui.library.dismiss_song_context",
    "ui.library.play_library_song",
    "ui.library.toggle_library_playback",
    "ui.library.seek_library_relative",
    "ui.library.previous_library_song",
    "ui.library.next_library_song",
    "ui.library.toggle_library_shuffle",
    "ui.library.cycle_library_repeat",
    "ui.library.adjust_library_volume",
    "ui.library.toggle_library_mute",
    "ui.library.toggle_library_audio_source_menu",
    "ui.library.select_library_audio_source",
    "ui.library.toggle_library_queue",
];

const SETTINGS_COMMANDS: &[&str] = &[
    "ui.settings.settings_tab",
    "ui.settings.refresh_runtime_status",
    "ui.settings.open_model_downloads",
    "ui.settings.close_model_downloads",
    "ui.settings.open_settings_select",
    "ui.settings.select_settings_value",
    "ui.settings.set_model_backend",
    "ui.settings.set_model_device",
    "ui.settings.set_analysis_quality",
    "ui.settings.request_setup",
    "ui.settings.install_audio_model",
    "ui.settings.remove_audio_model",
    "ui.settings.cancel_setup",
    "ui.settings.confirm_setup",
    "ui.settings.toggle_theme",
    "ui.settings.adjust_ui_font_scale",
    "ui.settings.toggle_preserve_continuous_pitch",
    "ui.settings.toggle_analysis_quantization",
    "ui.settings.toggle_auto_analyze",
    "ui.settings.restore_analysis_defaults",
    "ui.settings.request_clear_cache",
    "ui.settings.cancel_clear_cache",
    "ui.settings.confirm_clear_cache",
    "ui.settings.choose_fusion_agent_adapter",
    "ui.settings.clear_fusion_agent_adapter",
];

const ANALYSIS_COMMANDS: &[&str] = &[
    "ui.analysis.start_analysis",
    "ui.analysis.start_queued_analysis",
    "ui.analysis.merge_selected_candidate_phrase",
    "ui.analysis.merge_selected_candidate_range",
    "ui.analysis.keep_authored_chart",
    "ui.analysis.select_analysis_history",
    "ui.analysis.open_song_analysis",
    "ui.analysis.open_song_model_selection",
    "ui.analysis.open_processing_studio",
    "ui.analysis.analyze_now",
    "ui.analysis.open_empty_processing_studio",
    "ui.analysis.select_workflow_node",
    "ui.analysis.move_workflow_node",
    "ui.analysis.duplicate_workflow_node",
    "ui.analysis.remove_workflow_node",
    "ui.analysis.set_workflow_node_model",
    "ui.analysis.set_workflow_separation_strategy",
    "ui.analysis.add_workflow_processor",
    "ui.analysis.add_optional_workflow_card",
    "ui.analysis.set_workflow_parameter",
    "ui.analysis.set_workflow_policy",
    "ui.analysis.adjust_workflow_priority",
    "ui.analysis.rebind_workflow_analyzer",
    "ui.analysis.save_workflow",
    "ui.analysis.preview_workflow",
    "ui.analysis.run_workflow",
    "ui.analysis.open_analysis_inspect",
    "ui.analysis.adjust_analysis_graph_zoom",
    "ui.analysis.toggle_analysis_mini_view",
    "ui.analysis.toggle_analysis_model_panel",
    "ui.analysis.close_analysis_model_panel",
    "ui.analysis.fit_analysis_graph",
    "ui.analysis.dismiss_analysis_node_context",
    "ui.analysis.request_clear_analysis_history",
    "ui.analysis.cancel_clear_analysis_history",
    "ui.analysis.confirm_clear_analysis_history",
    "ui.analysis.compare_node_attempt_with_previous",
    "ui.analysis.close_plan_preview",
    "ui.analysis.queue_exact_preview",
    "ui.analysis.toggle_plan_preview_output",
    "ui.analysis.reset_plan_preview_outputs",
    "ui.analysis.set_plan_preview_quality",
    "ui.analysis.reset_plan_preview_quality",
    "ui.analysis.open_analysis_log_viewer",
    "ui.analysis.close_analysis_log_viewer",
    "ui.analysis.request_delete_song_cache",
    "ui.analysis.cancel_analysis_run",
    "ui.analysis.cancel_delete_song_cache",
    "ui.analysis.confirm_delete_song_cache",
    "ui.analysis.request_delete_authored_chart",
    "ui.analysis.cancel_delete_authored_chart",
    "ui.analysis.confirm_delete_authored_chart",
    "ui.analysis.request_replace_authored_chart",
    "ui.analysis.cancel_replace_authored_chart",
    "ui.analysis.confirm_replace_authored_chart",
];

const EDITOR_COMMANDS: &[&str] = &[
    "ui.editor.open_lyrics_editor",
    "ui.editor.close_lyrics_editor",
    "ui.editor.toggle_lyrics_input_mode",
    "ui.editor.toggle_lyrics_separate_stems",
    "ui.editor.search_lrclib_lyrics",
    "ui.editor.extract_lyrics",
    "ui.editor.previous_lrclib_candidate",
    "ui.editor.next_lrclib_candidate",
    "ui.editor.use_lrclib_plain",
    "ui.editor.use_lrclib_timed",
    "ui.editor.save_lyrics_editor",
    "ui.editor.save_lyrics_editor_and_run_downstream",
    "ui.editor.adjust_transcript_boundary",
    "ui.editor.preview_transcript_at",
    "ui.editor.open_language_editor",
    "ui.editor.close_language_editor",
    "ui.editor.toggle_language_reprocess",
    "ui.editor.toggle_language_picker",
    "ui.editor.select_analysis_language",
    "ui.editor.save_language_editor",
    "ui.editor.open_song_settings",
    "ui.editor.close_song_settings",
    "ui.editor.choose_background_video",
    "ui.editor.clear_background_video",
    "ui.editor.save_song_settings",
    "ui.editor.shift_song_key",
    "ui.editor.shift_song_tempo",
    "ui.editor.focus_chart_problem",
    "ui.editor.open_editor_select",
    "ui.editor.select_editor_value",
    "ui.editor.select_editor_word",
    "ui.editor.select_editor_track",
    "ui.editor.move_selection_to_track",
    "ui.editor.set_note_kind",
    "ui.editor.toggle_editor_file_menu",
    "ui.editor.dismiss_editor_file_menu",
    "ui.editor.save_editor_as_utz",
    "ui.editor.save_editor_as_ultra_star",
    "ui.editor.toggle_editor_layout_menu",
    "ui.editor.dismiss_editor_layout_menu",
    "ui.editor.dismiss_lyric_context",
    "ui.editor.dismiss_note_context",
    "ui.editor.select_waveform_source",
    "ui.editor.select_artifact_audition",
    "ui.editor.activate_artifact_audition",
    "ui.editor.select_artifact_waveform",
    "ui.editor.select_waveform_style",
    "ui.editor.dismiss_waveform_context",
    "ui.editor.toggle_evidence",
    "ui.editor.review_previous",
    "ui.editor.review_next",
    "ui.editor.mark_review_region",
    "ui.editor.accept_suggestion",
    "ui.editor.ignore_suggestion",
    "ui.editor.set_problems_filter",
    "ui.editor.apply_all_lyrics_edit",
    "ui.editor.extend_lyric_over_note",
    "ui.editor.dismiss_problems_panel",
    "ui.editor.dismiss_shortcuts_panel",
];

/// Pointer-observer APIs. Primary and secondary clicks are separate commands
/// because they have different behavior and independent tests. Drag/resize
/// gestures use started/updated/finished commands rather than hiding mutation
/// behind an unregistered `Button`.
const POINTER_COMMANDS: &[&str] = &[
    "ui.pointer.analysis_node.primary",
    "ui.pointer.analysis_node.secondary",
    "ui.pointer.analysis_edge.primary",
    "ui.pointer.song.primary",
    "ui.pointer.song.secondary",
    "ui.pointer.folder_entry.primary",
    "ui.pointer.folder_entry.double_primary",
    "ui.pointer.folder_entry.secondary",
    "ui.pointer.editor_note.secondary",
    "ui.pointer.editor_lyric.primary",
    "ui.pointer.editor_lyric.secondary",
    "ui.pointer.editor_lane.primary",
    "ui.pointer.editor_waveform.secondary",
    "ui.pointer.editor_timeline.primary",
    "ui.pointer.editor_note_drag",
    "ui.pointer.editor_note_resize",
    "ui.pointer.editor_lyric_drag",
    "ui.pointer.editor_lyric_resize",
    "ui.pointer.transcript_boundary_drag",
    "ui.pointer.editor_viewport_pan",
    "ui.pointer.analysis_viewport_pan",
];

fn snake_variant(debug: &str) -> String {
    let name = debug.split(['(', '{']).next().unwrap_or(debug);
    let mut result = String::with_capacity(name.len() + 8);
    for (index, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && index != 0 {
            result.push('_');
        }
        result.extend(ch.to_lowercase());
    }
    result
}

fn classified_access(command: &str) -> &'static str {
    if command.ends_with(".queue_exact_preview") {
        "mutation"
    } else if command.contains("confirm_delete")
        || command.contains("confirm_clear")
        || command.contains("confirm_remove")
        || command.contains("remove_audio_model")
    {
        "destructive"
    } else if command.contains("choose_")
        || command.contains("reveal_")
        || command.contains("open_source")
        || command.contains("open_log")
        || command.contains("search_lrclib")
        || command.contains("request_setup")
        || command.contains("confirm_setup")
        || command.contains("install_audio_model")
    {
        "external"
    } else if command.ends_with(".start_analysis")
        || command.ends_with(".select_library_audio_source")
        || command.contains("preview")
        || command.contains("audition")
        || command.contains("play_")
    {
        "temporary"
    } else if command.contains("open_")
        || command.contains("close_")
        || command.contains("dismiss_")
        || command.contains("select_")
        || command.contains("toggle_")
        || command.contains("focus_")
        || command.contains("fit_")
        || command.contains("adjust_analysis_graph_zoom")
        || command.ends_with(".back")
        || command.ends_with(".home")
        || command.contains("documentation")
    {
        "read"
    } else {
        "mutation"
    }
}

impl UiCommand {
    /// Stable command id used by the same local API whether invoked by mouse,
    /// keyboard, context menu, or an automated contract test.
    pub(crate) fn api_command(&self) -> String {
        match self {
            Self::App(command) => format!("ui.app.{}", snake_variant(&format!("{command:?}"))),
            Self::Library(command) => {
                format!("ui.library.{}", snake_variant(&format!("{command:?}")))
            }
            Self::Settings(command) => {
                format!("ui.settings.{}", snake_variant(&format!("{command:?}")))
            }
            Self::Analysis(command) => {
                format!("ui.analysis.{}", snake_variant(&format!("{command:?}")))
            }
            Self::Editor(EditorCommand::Editor(action)) => {
                format!("ui.editor.action.{}", action.command())
            }
            Self::Editor(command) => {
                format!("ui.editor.{}", snake_variant(&format!("{command:?}")))
            }
        }
    }

    pub(crate) fn api_request(&self) -> UiInteractionRequest {
        let command = self.api_command();
        UiInteractionRequest {
            access: classified_access(&command),
            command,
        }
    }
}

impl UiAction {
    pub(crate) fn api_request(&self) -> UiInteractionRequest {
        self.0.api_request()
    }
}

pub(crate) fn ui_interaction_capabilities() -> Vec<UiInteractionCapability> {
    let editor_actions = app_core::EDITOR_ACTIONS
        .iter()
        .map(|action| format!("ui.editor.action.{}", action.command));
    APP_COMMANDS
        .iter()
        .chain(LIBRARY_COMMANDS)
        .chain(SETTINGS_COMMANDS)
        .chain(ANALYSIS_COMMANDS)
        .chain(EDITOR_COMMANDS)
        .map(|command| (*command).to_string())
        .chain(editor_actions)
        .chain(
            POINTER_COMMANDS
                .iter()
                .map(|command| (*command).to_string()),
        )
        .map(|command| UiInteractionCapability {
            access: classified_access(&command),
            command,
            automated_check: true,
        })
        .collect()
}

fn registered_ui_commands() -> &'static HashSet<String> {
    static REGISTERED: OnceLock<HashSet<String>> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        ui_interaction_capabilities()
            .into_iter()
            .map(|capability| capability.command)
            .collect()
    })
}

#[cfg(test)]
pub(crate) fn ui_interaction_is_registered(command: &str) -> bool {
    registered_ui_commands().contains(command)
}

type AddedUiButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static UiAction>,
        Option<&'static UiPointerApi>,
    ),
    (With<Button>, Added<Button>),
>;

/// Runtime contract audit. Every pickable `Button` must either dispatch a
/// typed `UiAction` or declare the pointer-observer API(s) that handle it.
/// Keeping this in the normal app update catches route-specific controls that
/// a source-only registry test cannot render.
pub(crate) fn audit_ui_api_coverage(buttons: AddedUiButtons) {
    // Audit each newly materialized control once. The previous implementation
    // rescanned every button and rebuilt the full capability vector for every
    // button on every frame, which made debug builds increasingly sluggish as
    // the DAG and its menus grew.
    if !cfg!(debug_assertions) || buttons.is_empty() {
        return;
    }
    let registered = registered_ui_commands();

    for (entity, action, pointer) in &buttons {
        debug_assert!(
            action.is_some() || pointer.is_some(),
            "interactive entity {entity:?} has no local UI API"
        );
        if let Some(action) = action {
            let request = action.api_request();
            debug_assert!(
                registered.contains(&request.command),
                "typed UI command {} is absent from the interaction API registry",
                request.command
            );
        }
        if let Some(pointer) = pointer {
            for command in pointer.0 {
                debug_assert!(
                    registered.contains(*command),
                    "pointer command {command} is absent from the interaction API registry"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn enum_variants(source: &str, enum_name: &str) -> Vec<String> {
        let marker = format!("enum {enum_name} {{");
        let body = source
            .split_once(&marker)
            .unwrap()
            .1
            .split_once("\n}")
            .unwrap()
            .0;
        body.lines()
            .filter_map(|line| {
                if !line.starts_with("    ") || line.starts_with("        ") {
                    return None;
                }
                let name = line.trim_start().split(['(', ',', ' ']).next()?;
                name.chars()
                    .next()?
                    .is_uppercase()
                    .then(|| name.to_string())
            })
            .collect()
    }

    #[test]
    fn every_typed_ui_command_variant_is_a_discoverable_api() {
        let source = include_str!("commands.rs");
        let capabilities = ui_interaction_capabilities()
            .into_iter()
            .map(|capability| capability.command)
            .collect::<BTreeSet<_>>();
        for (enum_name, prefix) in [
            ("AppCommand", "ui.app."),
            ("LibraryCommand", "ui.library."),
            ("SettingsCommand", "ui.settings."),
            ("AnalysisCommand", "ui.analysis."),
            ("EditorCommand", "ui.editor."),
        ] {
            for variant in enum_variants(source, enum_name) {
                if enum_name == "EditorCommand" && variant == "Editor" {
                    continue;
                }
                let command = format!("{prefix}{}", snake_variant(&variant));
                assert!(
                    capabilities.contains(&command),
                    "missing API for {enum_name}::{variant}"
                );
            }
        }
        for action in app_core::EDITOR_ACTIONS {
            assert!(capabilities.contains(&format!("ui.editor.action.{}", action.command)));
        }
    }

    #[test]
    fn ui_api_ids_are_unique_classified_and_automated() {
        let capabilities = ui_interaction_capabilities();
        let commands = capabilities
            .iter()
            .map(|capability| capability.command.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(commands.len(), capabilities.len());
        assert!(capabilities.iter().all(|capability| {
            capability.automated_check
                && matches!(
                    capability.access,
                    "read" | "mutation" | "destructive" | "external" | "temporary"
                )
        }));
    }

    #[test]
    fn every_typed_ui_api_has_a_dispatch_handler() {
        let commands = include_str!("commands.rs");
        let handlers = [
            include_str!("actions_chrome.rs"),
            include_str!("actions_content.rs"),
            include_str!("actions_settings.rs"),
            include_str!("editor/action_input.rs"),
        ]
        .join("\n");
        for enum_name in [
            "AppCommand",
            "LibraryCommand",
            "SettingsCommand",
            "AnalysisCommand",
            "EditorCommand",
        ] {
            for variant in enum_variants(commands, enum_name) {
                let pattern = format!("{enum_name}::{variant}");
                assert!(
                    handlers.contains(&pattern),
                    "{pattern} is registered but has no dispatch handler"
                );
            }
        }
    }

    #[test]
    fn every_spawned_button_declares_its_dispatch_api() {
        fn visit(path: &std::path::Path, failures: &mut Vec<String>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, failures);
                } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                    let lines = std::fs::read_to_string(&path).unwrap();
                    let lines = lines.lines().collect::<Vec<_>>();
                    for (index, line) in lines.iter().enumerate() {
                        if line.trim() != "Button," {
                            continue;
                        }
                        let end = (index + 8).min(lines.len());
                        let declaration = lines[index..end].join("\n");
                        if !declaration.contains("UiAction::")
                            && !declaration.contains("UiPointerApi")
                            && !declaration.contains("action,")
                        {
                            failures.push(format!("{}:{}", path.display(), index + 1));
                        }
                    }
                }
            }
        }

        let mut failures = Vec::new();
        visit(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/studio"),
            &mut failures,
        );
        assert!(
            failures.is_empty(),
            "buttons without a typed or pointer API: {}",
            failures.join(", ")
        );
    }

    #[test]
    fn a_click_builds_a_structured_registered_request() {
        let action = UiAction::from(crate::studio::AppCommand::Back);
        let request = action.api_request();
        assert_eq!(request.command, "ui.app.back");
        assert_eq!(request.access, "read");
        assert!(ui_interaction_is_registered(&request.command));
    }

    #[test]
    fn pointer_and_context_menu_entry_points_are_independent_apis() {
        let capabilities = ui_interaction_capabilities();
        let studio_source = [
            include_str!("analysis_render/nodes.rs"),
            include_str!("analysis_render/overview.rs"),
            include_str!("analysis_edge_selection.rs"),
            include_str!("library/browse.rs"),
            include_str!("folders.rs"),
            include_str!("editor/view/menus.rs"),
            include_str!("editor/view/timeline.rs"),
            include_str!("song_detail/page.rs"),
        ]
        .join("\n");
        for command in POINTER_COMMANDS {
            assert!(capabilities.iter().any(|item| item.command == *command));
            assert!(
                studio_source.contains(command),
                "pointer API {command} has no rendered interaction handler"
            );
        }
        assert_ne!(classified_access("ui.pointer.song.primary"), "destructive");
    }

    #[test]
    fn opening_run_analysis_is_temporary_and_exact_queueing_is_a_mutation() {
        assert_eq!(classified_access("ui.analysis.start_analysis"), "temporary");
        assert_eq!(
            classified_access("ui.analysis.queue_exact_preview"),
            "mutation"
        );
    }

    #[test]
    fn parity_closure_features_have_reachable_typed_ui_actions() {
        for command in [
            "ui.settings.toggle_analysis_quantization",
            "ui.analysis.set_workflow_policy",
            "ui.analysis.merge_selected_candidate_phrase",
            "ui.editor.select_waveform_source",
            "ui.editor.select_editor_track",
            "ui.editor.toggle_evidence",
            "ui.editor.review_previous",
            "ui.editor.review_next",
            "ui.editor.accept_suggestion",
            "ui.editor.action.undo",
        ] {
            assert!(
                ui_interaction_is_registered(command),
                "missing reachable parity action {command}"
            );
        }

        let analysis_settings = include_str!("settings/analysis.rs");
        assert!(analysis_settings.contains("Quantize candidate notes"));
        assert!(analysis_settings.contains("never to continuous PitchEvidence"));

        let plan_preview_source = include_str!("analysis_preview.rs");
        let plan_preview = plan_preview_source
            .split_once("\n#[cfg(test)]")
            .map_or(plan_preview_source, |(production, _)| production);
        for identity_field in ["workflow_id", "workflow_revision"] {
            assert!(plan_preview.contains(identity_field));
        }
        for implementation_detail in ["workflow_schema_version", "definition_digest"] {
            assert!(
                !plan_preview.contains(implementation_detail),
                "run confirmation should not expose {implementation_detail}"
            );
        }

        let technique_timeline = include_str!("editor/view/timeline.rs");
        assert!(technique_timeline.contains("STARS technique"));
        assert!(technique_timeline.contains("Pickable::IGNORE"));
        assert!(technique_timeline.contains("uncal."));
    }
}
