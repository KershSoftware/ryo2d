//! The game texture clear

use crate::core::graphics::renderer::DEFAULT_BACKGROUND_COLOR;
use crate::core::render::{FrameImage, FrameInfo, RenderContext, RenderPass, pass_ids};

/// Clears the game texture to the background colour.
pub(super) struct Clear;

impl RenderPass for Clear {
  fn id(&self) -> &str {
    pass_ids::CLEAR
  }

  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let game_view = ctx.targets.game_view;
    clear(ctx.encoder(), game_view, DEFAULT_BACKGROUND_COLOR);
    ctx.image = FrameImage::Game;
  }
}

/// Record an empty render pass that clears `view` to `color`.
pub(super) fn clear(
  encoder: &mut wgpu::CommandEncoder,
  view: &wgpu::TextureView,
  color: wgpu::Color,
) {
  let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("Clear Game Texture Pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(color),
        store: wgpu::StoreOp::Store,
      },
    })],
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
    multiview_mask: None,
  });
}
