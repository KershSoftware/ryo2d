//! The world: entities, sprites and trails/lines drawn together in z order

use crate::core::graphics::SpriteGpuRenderer;
use crate::core::render::{FrameInfo, PassScope, RenderContext, RenderPass, pass_ids};

/// Draws the lit entities, sprites, trails and lines a camera sees onto its
/// image in one render pass, interleaved by z.
pub(super) struct World;

impl RenderPass for World {
  fn id(&self) -> &str {
    pass_ids::WORLD
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.world
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    replay(ctx, true, "World Render Pass");
  }
}

/// Draws what a camera sees `with_lighting(false)` (sprites, particles,
/// trails and lines), by z, onto its latest image (the lit one when lighting
/// ran).
pub(super) struct UnlitWorld;

impl RenderPass for UnlitWorld {
  fn id(&self) -> &str {
    pass_ids::UNLIT_WORLD
  }

  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    replay(ctx, false, "Unlit World Render Pass");
  }
}

/// Draw the current camera's `lit` (or unlit) draws onto its image in one
/// render pass. The engine wrote the instances and triangles when it laid
/// out the frame (buffer writes can't happen inside the pass).
fn replay(ctx: &mut RenderContext<'_>, lit: bool, label: &str) {
  let sprites: &SpriteGpuRenderer = ctx.scene.sprites;
  let (Some(camera), Some(batch)) = (ctx.camera_entity(), ctx.scene.batch) else {
    return;
  };
  let draws = batch.draws(camera, lit);
  if draws.is_empty() {
    return;
  }
  let image = ctx
    .current_camera()
    .map_or(ctx.render_size(), |camera| camera.rect.size());
  let (encoder, view) = ctx.encoder_and_current_view();
  let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some(label),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Load,
        store: wgpu::StoreOp::Store,
      },
    })],
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
    multiview_mask: None,
  });
  for draw in draws {
    batch.draw(&mut pass, draw, camera, sprites, image);
  }
}
