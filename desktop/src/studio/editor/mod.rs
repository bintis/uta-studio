//! The chart editor: UI-facing state, rendering, input, and commands built on
//! the UI-agnostic `app_core::EditorDocument`.

mod action_input;
pub(crate) mod actions;
pub(crate) mod audition;
pub(crate) mod commands;
mod input;
mod panels;
mod state;
mod tracks;
mod view;

pub(crate) use action_input::{handle_editor_keyboard, handle_editor_ui_action};
pub(crate) use actions::{EditorAction, EditorActionContext, spawn_editor_action_button};
#[cfg(test)]
pub(crate) use audition::confirm_waveform_status;
pub(crate) use audition::{
    activate_editor_artifact_audition, poll_editor_load_job, select_editor_artifact_audition,
    set_editor_artifact_waveform, set_editor_waveform_source, start_editor_load_job,
    start_editor_merge_load_job, sync_editor_audio,
};
pub(crate) use commands::{
    can_extend_editor_lyric, extend_editor_lyric, next_extendable_editor_note,
    try_save_editor_chart,
};
pub(crate) use input::{
    finish_inline_lyric_edit, flush_editor_viewport_rebuild, handle_editor_pointer_capture,
    handle_editor_wheel, handle_tap_release, sync_editor_word_input,
};
pub(crate) use panels::{
    handle_problems_panel_scroll, handle_shortcuts_panel_scroll, refresh_editor_problems_cache,
    spawn_all_lyrics_panel, spawn_editor_inspector, spawn_problems_panel, spawn_shortcuts_panel,
    sync_editor_phrase_input, update_editor_shortcuts_panel_visibility,
};
pub(crate) use state::*;
pub(crate) use tracks::{spawn_editor_tracks, sync_editor_singer_input};
pub(crate) use view::{
    spawn_editor, spawn_editor_file_menu, spawn_editor_layout_menu, spawn_lyric_context_menu,
    spawn_menu_check_row, spawn_note_context_menu, spawn_waveform_context_menu,
    update_editor_binding_guides, update_editor_geometry, update_editor_playhead,
};
