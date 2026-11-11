//! 2D lighting: lights (each holding its own shadow back), then the lit
//! scene

use crate::core::render::{FrameImage, FrameInfo, PassScope, RenderContext, RenderPass, pass_ids};

/// Draws the camera's lights from the batch into its light buffer, each
/// reading its shadow from the frame's shadow atlas, then composes the
/// camera's image with it.
pub(super) struct Lighting;

impl RenderPass for Lighting {
  fn id(&self) -> &str {
    pass_ids::LIGHTING
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.lighting
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(camera) = ctx.current_camera().filter(|camera| camera.lit) else {
      return;
    };
    let (device, queue) = (ctx.device(), ctx.queue());
    let RenderContext {
      encoder,
      targets,
      scene,
      image,
      ..
    } = ctx;
    let Some(lighting) = scene.lighting.as_ref() else {
      return;
    };
    let renderer = &*lighting.renderer;
    let entity = camera.entity;
    let Some(light_view) = renderer.light_view(entity) else {
      return;
    };
    // The frame's lights, through the batch, into the camera's light
    // buffer, in one draw
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Light Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: light_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      if let Some(batch) = scene.batch {
        let shadows = renderer.shadow_group();
        batch.draw_lights(&mut pass, entity, shadows, camera.rect.size());
      }
    }
    // The camera drawing straight into the scene image is lit into the lit
    // image; a camera with an image of its own is lit in place
    let (scene_texture, scene_view, output) = if camera.direct {
      (targets.game, targets.game_view, targets.lit_view)
    } else {
      let images = targets.camera_images;
      let (Some(texture), Some(view)) = (images.texture(entity), images.view(entity)) else {
        return;
      };
      (texture, view, view)
    };
    renderer.compose_for_camera(
      device,
      queue,
      encoder,
      entity,
      scene_texture,
      scene_view,
      output,
      scene.time,
    );
    if camera.direct {
      *image = FrameImage::Lit;
    }
  }
}
