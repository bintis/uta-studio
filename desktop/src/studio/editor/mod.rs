//! The chart editor: UI-facing state, rendering, input, and commands built on
//! the UI-agnostic `app_core::EditorDocument`.

mod audition;
mod commands;
mod input;
mod panels;
mod state;
mod view;

pub(crate) use audition::*;
pub(crate) use commands::*;
pub(crate) use input::*;
pub(crate) use panels::*;
pub(crate) use state::*;
pub(crate) use view::*;
