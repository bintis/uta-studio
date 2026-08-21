mod chrome;
mod menus;
mod timeline;

pub(crate) use chrome::spawn_editor;
pub(crate) use menus::{
    open_note_from_click, spawn_editor_alignment_guide, spawn_editor_binding_guide,
    spawn_editor_file_menu, spawn_editor_layout_menu, spawn_editor_lyrics,
    spawn_lyric_context_menu, spawn_note_context_menu, update_editor_binding_guides,
    update_editor_geometry, update_editor_playhead,
};
pub(crate) use timeline::{
    clamp_menu_position, spawn_editor_timeline, spawn_menu_check_row, spawn_waveform_context_menu,
};
