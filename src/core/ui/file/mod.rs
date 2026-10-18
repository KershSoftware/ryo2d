//! UI files: screens written in RON that load into `UiNode`
//!
//! `reader` turns the file's text into values that know their line and
//! column, `vocab` is the table of node kinds and options, `build` turns
//! values into `UiNode`s (expanding templates), and `document` is the file:
//! its top level, templates and imports, checked at load.

pub(crate) mod build;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod check;
pub(crate) mod document;
pub(crate) mod error;
pub(crate) mod reader;
pub(crate) mod vocab;

pub use document::{UiArg, UiArgs, UiFile};
pub use error::{UiFileError, UiFileIssue};
