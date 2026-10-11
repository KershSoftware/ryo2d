//! The shell's drawing: top bar, description, settings panel, legend and help

use ryo2d::prelude::*;

use crate::Control;
use crate::settings::Row;

/// Everything the shell draws for one frame
#[allow(dead_code)] // read by `draw` from Task 4
pub(crate) struct View<'a> {
  pub title: &'a str,
  pub index: usize,
  pub count: usize,
  pub page_name: &'a str,
  pub description: &'a str,
  pub rows: &'a [Row],
  pub selected: usize,
  pub controls: &'a [Control],
  pub help_open: bool,
}

pub(crate) fn draw(_ui: &mut UiContext, _view: &View) {}
