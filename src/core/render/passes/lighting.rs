//! Lighting pass - renders 2D lights and composes the lit scene
//!
//! This pass collects lights from the ECS world and renders them to a light
//! accumulation buffer, then composes the result with the scene.

use crate::core::ecs::{Transform, Vec2};
use crate::core::lighting::{Light2D, LightOccluder2D, generate_shadow_geometry};
use crate::core::render::pass_ids;
use crate::core::render::{RenderContext, RenderPass, RenderPassId};

/// Collected light data for rendering
#[derive(Clone)]
pub struct LightData {
  pub transform: Transform,
  pub light: Light2D,
}

/// Collected occluder data for shadow casting
#[derive(Clone)]
pub struct OccluderData {
  pub transform: Transform,
  pub occluder: LightOccluder2D,
}

/// Render pass that handles 2D lighting.
///
/// This pass:
/// 1. Renders all lights to an accumulation buffer
/// 2. Composes the lit scene by multiplying scene colors by light values
///
/// # Usage
///
/// The engine collects Light2D entities and passes them to this pass before
/// pipeline execution:
///
/// ```ignore
/// // In engine, before pipeline.execute():
/// let lights: Vec<_> = world.query_mut::<(&Transform, &Light2D)>()
///     .into_iter()
///     .map(|(t, l)| LightData { transform: t.clone(), light: l.clone() })
///     .collect();
///
/// if let Some(lighting_pass) = pipeline.get_pass_mut::<LightingPass>("lighting") {
///     lighting_pass.set_lights(lights);
/// }
/// ```
pub struct LightingPass {
  /// Collected lights for this frame
  lights: Vec<LightData>,
  /// Collected occluders for shadow casting
  occluders: Vec<OccluderData>,
  /// Camera position (set before execute)
  camera_pos: Vec2,
  /// Camera zoom (set before execute)
  camera_zoom: f32,
  /// Whether this pass is enabled
  enabled: bool,
}

impl Default for LightingPass {
  fn default() -> Self {
    Self::new()
  }
}

impl LightingPass {
  /// Create a new lighting pass
  pub fn new() -> Self {
    Self {
      lights: Vec::new(),
      occluders: Vec::new(),
      camera_pos: Vec2::ZERO,
      camera_zoom: 1.0,
      enabled: true,
    }
  }

  /// Set the lights to render this frame.
  ///
  /// This should be called by the engine before pipeline execution.
  pub fn set_lights(&mut self, lights: Vec<LightData>) {
    self.lights = lights;
  }

  /// Set the occluders for shadow casting this frame.
  ///
  /// This should be called by the engine before pipeline execution.
  pub fn set_occluders(&mut self, occluders: Vec<OccluderData>) {
    self.occluders = occluders;
  }

  /// Set camera parameters for light positioning
  pub fn set_camera(&mut self, pos: Vec2, zoom: f32) {
    self.camera_pos = pos;
    self.camera_zoom = zoom;
  }

  /// Get the number of lights
  pub fn light_count(&self) -> usize {
    self.lights.len()
  }

  /// Get the number of occluders
  pub fn occluder_count(&self) -> usize {
    self.occluders.len()
  }

  /// Clear lights after rendering
  pub fn clear_lights(&mut self) {
    self.lights.clear();
  }

  /// Clear occluders after rendering
  pub fn clear_occluders(&mut self) {
    self.occluders.clear();
  }

  /// Check if there are any lights to render
  pub fn has_lights(&self) -> bool {
    !self.lights.is_empty()
  }

  /// Check if there are any occluders
  pub fn has_occluders(&self) -> bool {
    !self.occluders.is_empty()
  }

  /// Check if there are any shadow-casting lights
  pub fn has_shadow_casting_lights(&self) -> bool {
    self.lights.iter().any(|ld| ld.light.casts_shadows())
  }
}

impl RenderPass for LightingPass {
  fn id(&self) -> RenderPassId {
    pass_ids::LIGHTING
  }

  fn dependencies(&self) -> &[RenderPassId] {
    // Must run after GPU sprites are rendered and CPU content is uploaded
    &[pass_ids::GPU_SPRITE]
  }

  fn execute(&mut self, ctx: &mut RenderContext) {
    // Get the lighting renderer
    let Some(lighting) = ctx.lighting.as_mut() else {
      return;
    };

    // Update camera from context
    self.camera_pos = Vec2::new(ctx.camera.x, ctx.camera.y);
    self.camera_zoom = ctx.camera.zoom;

    // Auto-enable lighting when lights exist
    if !self.lights.is_empty() && !lighting.is_enabled() {
      lighting.set_enabled(true);
    }

    // Skip if lighting is disabled and no lights
    if !lighting.is_enabled() && self.lights.is_empty() {
      return;
    }

    // Step 1: Render lights to the accumulation buffer
    lighting.render_lights(
      ctx.device,
      ctx.queue,
      ctx.encoder,
      self.lights.iter().map(|ld| (&ld.transform, &ld.light)),
      self.camera_pos,
      self.camera_zoom,
    );

    // Step 2: Render shadows to subtract from light buffer
    if self.has_shadow_casting_lights() && self.has_occluders() {
      // Prepare occluder references for shadow generation
      let occluder_refs: Vec<(&Transform, &LightOccluder2D)> = self
        .occluders
        .iter()
        .map(|od| (&od.transform, &od.occluder))
        .collect();

      // Generate shadow geometry
      let shadow_geometry = generate_shadow_geometry(
        self.lights.iter().map(|ld| (&ld.transform, &ld.light)),
        &occluder_refs,
      );

      // Render shadows
      lighting.render_shadows(ctx.device, ctx.queue, ctx.encoder, &shadow_geometry);
    }

    // Step 3: Compose the lit scene
    // Use compose_from_texture to read from game texture (which has GPU + CPU content)
    // and write lit result back to game texture view.
    lighting.compose_from_texture(
      ctx.device,
      ctx.queue,
      ctx.encoder,
      ctx.game_texture,
      ctx.game_texture_view,
      ctx.time,
    );

    // Mark that lighting was applied
    ctx.lighting_applied = true;

    // Clear lights and occluders for next frame
    self.lights.clear();
    self.occluders.clear();
  }

  fn should_execute(&self, ctx: &RenderContext) -> bool {
    // Execute if we have lights or if lighting is enabled (for ambient)
    let lighting_enabled = ctx
      .lighting
      .as_ref()
      .map(|l| l.is_enabled())
      .unwrap_or(false);
    !self.lights.is_empty() || lighting_enabled
  }

  fn is_enabled(&self) -> bool {
    self.enabled
  }

  fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_pass_id() {
    let pass = LightingPass::new();
    assert_eq!(pass.id(), "lighting");
  }

  #[test]
  fn test_dependencies() {
    let pass = LightingPass::new();
    assert_eq!(pass.dependencies(), &["gpu_sprite"]);
  }

  #[test]
  fn test_light_collection() {
    let mut pass = LightingPass::new();
    assert!(!pass.has_lights());
    assert_eq!(pass.light_count(), 0);

    pass.set_lights(vec![LightData {
      transform: Transform::new(100.0, 100.0),
      light: Light2D::point(crate::core::ui::Color::WHITE, 50.0),
    }]);

    assert!(pass.has_lights());
    assert_eq!(pass.light_count(), 1);

    pass.clear_lights();
    assert!(!pass.has_lights());
  }
}
