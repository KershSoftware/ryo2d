//! The GPU raycaster, which replaces the 2D scene passes while it is on

use super::scene::clear;
use crate::core::render::{FrameImage, FrameInfo, RenderContext, RenderPass, pass_ids};

/// Clears the game texture to black, then draws walls, floors, ceilings and
/// billboard sprites onto it.
pub(super) struct Raycast;

impl RenderPass for Raycast {
  fn id(&self) -> &str {
    pass_ids::RAYCAST
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.raycast
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (device, game_view) = (ctx.device(), ctx.targets.game_view);
    // Black even when the walls can't draw yet (no wall textures bound)
    clear(ctx.encoder(), game_view, wgpu::Color::BLACK);
    if let Some(raycast) = ctx.scene.raycast {
      raycast.render_walls(device, ctx.encoder(), game_view);
      raycast.render_sprites(device, ctx.encoder(), game_view);
    }
    ctx.image = FrameImage::Game;
  }
}
