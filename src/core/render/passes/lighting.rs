//! 2D lighting: lights, shadows, then the lit scene

use crate::core::ecs::GlobalTransform;
use crate::core::lighting::{LightOccluder2D, generate_shadow_geometry};
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
    // Only the camera drawing straight into the scene image is lit for now
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
    let lights = lighting.lights;
    let light_refs = || lights.iter().map(|(position, light)| (position, light));

    let (camera, uniforms) = (camera.entity, scene.camera_uniforms);
    lighting.renderer.render_lights_for_camera(
      device,
      queue,
      encoder,
      light_refs(),
      camera,
      uniforms,
    );

    // Shadows subtract from the light buffer
    let casts_shadows = lights.iter().any(|(_, light)| light.casts_shadows());
    if casts_shadows && !lighting.occluders.is_empty() {
      let occluders: Vec<(&GlobalTransform, &LightOccluder2D)> = lighting
        .occluders
        .iter()
        .map(|(transform, occluder)| (transform, *occluder))
        .collect();
      let geometry = generate_shadow_geometry(light_refs(), &occluders);
      lighting
        .renderer
        .render_shadows(device, queue, encoder, &geometry, camera, uniforms);
    }

    lighting.renderer.compose_from_texture(
      device,
      queue,
      encoder,
      targets.game,
      targets.lit_view,
      scene.time,
    );
    *image = FrameImage::Lit;
  }
}
