//! 2D lighting: lights, shadows, then the lit scene

use crate::core::render::{FrameImage, FrameInfo, PassScope, RenderContext, RenderPass, pass_ids};

/// Renders the lights (and shadows from shadow-casting lights and occluders),
/// then composes the game texture with them into the lit texture.
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
    let Some(lighting) = scene.lighting.as_mut() else {
      return;
    };
    let (entity, uniforms) = (camera.entity, scene.camera_uniforms);
    // The lights and shadows were uploaded once for the frame
    lighting
      .renderer
      .draw_for_camera(device, encoder, entity, uniforms);

    // The camera drawing straight into the scene image is lit into the lit
    // image; a camera with an image of its own is lit in place
    let (scene_texture, output) = if camera.direct {
      (targets.game, targets.lit_view)
    } else {
      let images = targets.camera_images;
      let (Some(texture), Some(view)) = (images.texture(entity), images.view(entity)) else {
        return;
      };
      (texture, view)
    };
    lighting.renderer.compose_for_camera(
      device,
      queue,
      encoder,
      entity,
      scene_texture,
      output,
      scene.time,
    );
    if camera.direct {
      *image = FrameImage::Lit;
    }
  }
}
