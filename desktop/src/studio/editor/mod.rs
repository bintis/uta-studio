//! The chart editor: UI-facing state, rendering, input, and commands built on
//! the UI-agnostic `app_core::EditorDocument`.

mod actions;
mod audition;
mod commands;
mod input;
mod panels;
mod state;
mod tracks;
mod view;

pub(crate) use actions::*;
pub(crate) use audition::*;
pub(crate) use commands::*;
pub(crate) use input::*;
pub(crate) use panels::*;
pub(crate) use state::*;
pub(crate) use tracks::*;
pub(crate) use view::*;
