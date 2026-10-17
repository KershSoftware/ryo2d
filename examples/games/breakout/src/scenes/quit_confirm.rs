//! Quit Confirmation Scene
//!
//! Overlay shown when the user presses Escape from the main menu.

use ryo2d::prelude::*;

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
  fn on_enter(&mut self, _ctx: &mut SceneContext) {
    // Overlay - no world changes needed
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    // Opens on NO; Back (Escape, B) answers no
    match ui.confirm_with("Quit to Desktop?", "", "YES", "NO") {
      Some(true) => SceneAction::Quit,
      Some(false) => SceneAction::Pop,
      None => SceneAction::Continue,
    }
  }

  fn is_transparent(&self) -> bool {
    true // Show menu behind overlay
  }

  fn name(&self) -> &str {
    "QuitConfirmScene"
  }
}
