//! The trait every demo page implements

use ryo2d::prelude::*;

use crate::{Control, Settings};

/// One page of a demo. Its feature code is plain ryo2d; the shell supplies
/// page switching, the settings panel, help and layout.
///
/// A page is built fresh from its factory whenever it is shown or reset, and
/// runs inside its own engine scene. Leaving it resets the world, camera,
/// post-processing effects, ambient light and atmosphere, time scale, music,
/// events and raycasting. Anything else a page changes (the cursor, UI scale,
/// render pipeline passes, other lighting settings, sound effects, tilemaps,
/// `Resources`) carries over unless the page undoes it in `exit`.
pub trait DemoPage: 'static {
  /// Shown in the top bar, e.g. `Glow`
  fn name(&self) -> &str;

  /// What the page shows; the first line appears under the top bar, all of it in help
  fn description(&self) -> &str {
    ""
  }

  /// Declare the settings panel's rows (called every frame, before `update`)
  fn settings(&mut self, _settings: &mut Settings) {}

  /// The page's own keys and mouse controls, for the legend and help.
  /// Reserved shell keys are rejected when the demo starts.
  fn controls(&self) -> Vec<Control> {
    Vec::new()
  }

  /// The page uses Escape itself right now, such as to close its own dialog.
  /// While this is true the shell leaves Escape to the page instead of
  /// quitting (an open help overlay still takes it first), drops `Esc quit`
  /// from the top bar, and lets [`controls`](Self::controls) list Escape.
  fn wants_escape(&self) -> bool {
    false
  }

  /// Set up the page (spawn entities, configure the camera and lighting)
  fn enter(&mut self, _ctx: &mut SceneContext) {}

  /// Undo engine state the page changed that the scene reset does not cover
  /// (see the trait docs); called when the page is left or reset
  fn exit(&mut self, _ctx: &mut SceneContext) {}

  /// Fixed-timestep update; read settings fields here
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) {}

  /// Called every frame before rendering (see `Scene::render`)
  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {}

  /// The page's own UI, drawn under the shell's
  fn render_ui(&mut self, _ui: &mut UiContext) {}

  /// GPU drawing (particles, `render_gpu` sprites)
  fn render_gpu(&mut self, _ctx: &mut GpuRenderContext) {}
}
