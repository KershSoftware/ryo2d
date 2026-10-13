//! The window cameras' images, placed in their viewports

use crate::core::graphics::renderer::DEFAULT_BACKGROUND_COLOR;
use crate::core::render::{FrameImage, RenderContext, RenderPass, pass_ids};

use super::scene::clear;

/// Draws each window camera's own image over the scene image in its
/// viewport, in render order; a camera that does not clear blends over the
/// ones below. When no camera drew straight into the scene image, it is
/// cleared to the background colour first.
pub(super) struct Cameras;

impl RenderPass for Cameras {
  fn id(&self) -> &str {
    pass_ids::CAMERAS
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let cameras = ctx.scene.cameras;
    if !cameras.iter().any(|camera| camera.direct) {
      let game_view = ctx.targets.game_view;
      clear(ctx.encoder(), game_view, DEFAULT_BACKGROUND_COLOR);
      ctx.image = FrameImage::Game;
    }
    let images = ctx.targets.camera_images;
    let (encoder, view) = ctx.encoder_and_current_view();
    images.composite(encoder, view, cameras);
  }
}
