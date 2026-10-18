//! Quit Confirmation Scene
//!
//! Overlay shown when the user presses Escape from the main menu.

use ryo2d::prelude::*;

use crate::look::spawn_ui_file;

const QUIT_CONFIRM: &str = "assets/ui/quit_confirm.ui.ron";
const QUIT_CONFIRM_EMBEDDED: &str = include_str!("../../assets/ui/quit_confirm.ui.ron");

/// Quit confirmation overlay
pub struct QuitConfirmScene;

impl QuitConfirmScene {
  /// Create a new quit confirmation scene
  pub fn new() -> Self {
    Self
  }
}

impl Default for QuitConfirmScene {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for QuitConfirmScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Opens on NO; Back (Escape, B) answers no
    spawn_ui_file(ctx, QUIT_CONFIRM, QUIT_CONFIRM_EMBEDDED);
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    for event in ctx.ui_events() {
      if event.kind != UiEventKind::Clicked {
        continue;
      }
      match event.action.as_deref() {
        Some("yes") => return SceneAction::Quit,
        Some("no") => return SceneAction::Pop,
        _ => {}
      }
    }
    SceneAction::Continue
  }

  fn is_transparent(&self) -> bool {
    true // Show menu behind overlay
  }

  fn name(&self) -> &str {
    "QuitConfirmScene"
  }
}
