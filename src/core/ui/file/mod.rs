//! UI files: screens written in RON that load into `UiNode`
//!
//! `reader` turns the file's text into values that know their line and
//! column. Later modules check those values against the vocabulary and
//! build `UiNode`s from them.

pub(crate) mod reader;
