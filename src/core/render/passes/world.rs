//! The world: entities, sprites and trails/lines drawn together in z order

use crate::core::graphics::SpriteGpuRenderer;
use crate::core::render::{FrameInfo, RenderContext, RenderPass, WorldRun, pass_ids};
use crate::core::trail::TrailRenderer;

/// Draws the frame's entities, sprites and lit trails and lines onto the game texture
/// in one render pass, interleaved by z.
pub(super) struct World;

impl RenderPass for World {
  fn id(&self) -> &str {
    pass_ids::WORLD
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.world
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let sprites: &SpriteGpuRenderer = ctx.scene.sprites;
    let trails: Option<&TrailRenderer> = ctx.scene.trails;
    let (queue, game_view) = (ctx.queue(), ctx.targets.game_view);

    // Buffer writes can't happen inside the pass (the engine already wrote
    // the trail vertices when it laid out the frame)
    sprites.upload_world(queue);

    let runs = sprites.world_runs();
    if runs.is_empty() {
      return;
    }
    let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("World Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: game_view,
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
    for run in runs {
      match run {
        WorldRun::Sprites { .. } => sprites.draw_world_run(&mut pass, run),
        WorldRun::Trail { z } => {
          if let Some(trails) = trails {
            trails.draw_group(&mut pass, true, *z);
          }
        }
      }
    }
  }
}

/// Draws the `with_lighting(false)` trails and lines, by z, onto the latest
/// image (the lit scene when lighting ran).
pub(super) struct UnlitWorld;

impl RenderPass for UnlitWorld {
  fn id(&self) -> &str {
    pass_ids::UNLIT_WORLD
  }

  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(trails) = ctx.scene.trails else {
      return;
    };
    let list = ctx.scene.draw_list;
    if list.unlit_items().is_empty() {
      return;
    }
    let (encoder, view) = ctx.encoder_and_current_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Unlit World Render Pass"),
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
    for item in list.unlit_items() {
      if let crate::core::render::DrawPayload::Trail { z } = item.payload {
        trails.draw_group(&mut pass, false, z);
      }
    }
  }
}
