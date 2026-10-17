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
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    // Opens on NO; Back (Escape, B) answers no
    ctx.spawn_ui(UiNode::confirm("Quit to Desktop?", "", "YES", "NO"));
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
