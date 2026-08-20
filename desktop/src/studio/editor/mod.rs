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
pub(crate) use audition::{
    poll_editor_load_job, set_editor_waveform_source, start_editor_load_job,
    start_editor_merge_load_job, start_editor_revision_load_job, sync_editor_audio,
};
pub(crate) use commands::{
    can_extend_editor_lyric, extend_editor_lyric, next_extendable_editor_note,
};
pub(crate) use input::{
    finish_inline_lyric_edit, handle_editor_pointer_capture, handle_editor_wheel,
    handle_tap_release, sync_editor_word_input,
};
pub(crate) use panels::{
    handle_problems_panel_scroll, handle_shortcuts_panel_scroll, refresh_editor_problems_cache,
    spawn_all_lyrics_panel, spawn_editor_inspector, spawn_problems_panel, spawn_shortcuts_panel,
    sync_editor_phrase_input, update_editor_shortcuts_panel_visibility,
};
pub(crate) use state::*;
pub(crate) use tracks::{spawn_editor_tracks, sync_editor_singer_input};
pub(crate) use view::{
    spawn_editor, spawn_menu_check_row, update_editor_binding_guides, update_editor_geometry,
    update_editor_playhead,
};
