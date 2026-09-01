mod copy;
mod dialogs;
mod jobs;
mod lyrics_workbench;
mod page;
mod types;

pub(crate) use copy::*;
// Test helpers live in copy.rs as crate-visible functions.
pub(crate) use dialogs::*;
pub(crate) use jobs::*;
pub(crate) use lyrics_workbench::*;
pub(crate) use page::*;
pub(crate) use types::*;

#[cfg(test)]
include!("tests.rs");
