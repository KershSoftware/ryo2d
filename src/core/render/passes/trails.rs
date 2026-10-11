//! Trails and lines: lit ones before lighting, unlit ones after it

use crate::core::render::{FrameInfo, RenderContext, RenderPass, pass_ids};
use crate::core::trail::TrailStyle;

/// Draws the trails and lines that lighting applies to onto the game texture.
pub(super) struct Trails;

impl RenderPass for Trails {
  fn id(&self) -> &str {
    pass_ids::TRAILS
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast && frame.trails
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(trails) = ctx.scene.trails else {
      return;
    };
    let (camera_pos, camera_zoom, time) =
      (ctx.scene.camera_pos, ctx.scene.camera_zoom, ctx.scene.time);
    let (queue, game_view) = (ctx.queue(), ctx.targets.game_view);
    trails.render(
      queue,
      ctx.encoder(),
      game_view,
      camera_pos,
      camera_zoom,
      time,
      TrailStyle::Solid,
    );
  }
}

/// Draws the `with_lighting(false)` trails and lines onto the latest image
/// (the lit scene when lighting ran).
pub(super) struct UnlitTrails;

impl RenderPass for UnlitTrails {
  fn id(&self) -> &str {
    pass_ids::UNLIT_TRAILS
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast && frame.trails
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(trails) = ctx.scene.trails else {
      return;
    };
    let (camera_pos, camera_zoom, time) =
      (ctx.scene.camera_pos, ctx.scene.camera_zoom, ctx.scene.time);
    let queue = ctx.queue();
    let (encoder, view) = ctx.encoder_and_current_view();
    trails.render_unlit(queue, encoder, view, camera_pos, camera_zoom, time);
  }
}
