//! The world: entities, sprites and trails/lines drawn together in z order

use crate::core::graphics::SpriteGpuRenderer;
use crate::core::render::{
  DrawPayload, FrameInfo, PassScope, RenderContext, RenderPass, WorldRun, pass_ids,
};
use crate::core::trail::TrailRenderer;

/// Draws the entities, sprites and lit trails and lines a camera sees onto
/// its image in one render pass, interleaved by z.
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
    let sprites: &SpriteGpuRenderer = ctx.scene.sprites;
    let trails: Option<&TrailRenderer> = ctx.scene.trails;
    let Some(camera) = ctx.camera_entity() else {
      return;
    };
    // The engine wrote the instances and trail vertices when it laid out
    // the frame (buffer writes can't happen inside the pass)
    let runs = sprites.world_runs(camera);
    if runs.is_empty() {
      return;
    }
    let (encoder, view) = ctx.encoder_and_current_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("World Render Pass"),
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
    for run in runs {
      match run {
        WorldRun::Sprites { .. } => sprites.draw_world_run(&mut pass, run, camera),
        WorldRun::Trail { z, layers } => {
          if let Some(trails) = trails {
            trails.draw_group(&mut pass, true, (*z, *layers), camera);
          }
        }
      }
    }
  }
}

/// Draws the `with_lighting(false)` trails and lines a camera sees, by z,
/// onto its latest image (the lit one when lighting ran).
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
    let Some(trails) = ctx.scene.trails else {
      return;
    };
    let Some(camera) = ctx.current_camera() else {
      return;
    };
    let list = ctx.scene.draw_list;
    if list.unlit_items_for(camera).next().is_none() {
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
    for item in list.unlit_items_for(camera) {
      if let DrawPayload::Trail { z, layers } = item.payload {
        trails.draw_group(&mut pass, false, (z, layers), camera.entity);
      }
    }
  }
}
