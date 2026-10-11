//! Game types used by the engine and scene system.
//!
//! Contains shared types like WindowRequest, TimeControl, TimeInfo,
//! and GpuRenderContext that are used by both the engine and scenes.

use super::asset::SpriteHandle;
use super::config::save::DisplayMode;
use super::ecs::World;
use super::graphics::{BatchCollector, SpriteGpuRenderer};

/// Pending window configuration change requested by the game.
///
/// Set via [`SceneContext::request_display_change`].
/// The engine will apply these changes after the current update.
#[derive(Debug, Clone, Default)]
pub struct WindowRequest {
  /// New resolution width (None = no change)
  pub width: Option<u32>,
  /// New resolution height (None = no change)
  pub height: Option<u32>,
  /// New display mode (None = no change)
  pub display_mode: Option<DisplayMode>,
  /// New UI scale factor (None = no change)
  /// Use Some(0) to reset to auto-detect mode
  pub ui_scale: Option<u32>,
}

/// Time control state for game juice effects.
///
/// Allows game code to control time scale and hitstop effects.
/// Changes are applied immediately and persist until changed again.
#[derive(Debug, Clone)]
pub struct TimeControl {
  /// Time scale multiplier (0.0 = frozen, 1.0 = normal, >1.0 = fast)
  pub time_scale: f32,
  /// Remaining hitstop duration in seconds
  pub hitstop_remaining: f32,
}

impl Default for TimeControl {
  fn default() -> Self {
    Self {
      time_scale: 1.0,
      hitstop_remaining: 0.0,
    }
  }
}

/// Timing information exposed to game code.
///
/// Provides access to all timing values for different use cases:
/// - `fixed_dt`: Unscaled fixed timestep (for physics math unaffected by slow-mo)
/// - `scaled_dt`: Fixed timestep × time_scale (for gameplay that should slow down)
/// - `real_dt`: Actual time since last frame (for UI, particles, camera that should stay smooth)
/// - `render_alpha`: Interpolation factor 0.0-1.0 (for smooth rendering between updates)
///
/// # Example
///
/// ```ignore
/// fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
///     let time = ctx.time;
///
///     // Physics calculations (unaffected by slow-mo)
///     let gravity = 9.8 * time.fixed_dt;
///
///     // Gameplay (affected by slow-mo)
///     player.velocity += input * time.scaled_dt;
///
///     // UI animations (always smooth, regardless of update rate)
///     menu.animate(time.real_dt);
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TimeInfo {
  /// Unscaled fixed timestep in seconds (e.g., 1/60 = 0.01667 at 60hz).
  ///
  /// Use for physics calculations that shouldn't be affected by time_scale.
  pub fixed_dt: f32,

  /// Fixed timestep scaled by time_scale, zero during hitstop.
  ///
  /// This is the same value passed as `dt` to `update()`.
  /// Use for gameplay that should slow down with slow-mo effects.
  pub scaled_dt: f32,

  /// Actual time since last frame in seconds (variable).
  ///
  /// Use for UI animations, particles, and effects that should
  /// remain smooth regardless of the fixed update rate.
  pub real_dt: f32,

  /// Interpolation factor between 0.0 and 1.0.
  ///
  /// Represents progress between the last fixed update and the next.
  /// Use for interpolating positions during rendering to achieve
  /// smooth motion when render FPS exceeds physics rate.
  ///
  /// Example: At 60hz physics on a 144hz display, this will be
  /// ~0.0, ~0.42, ~0.83 across the render frames between updates.
  pub render_alpha: f32,

  /// Current time scale multiplier (0.0 = frozen, 1.0 = normal, >1.0 = fast).
  pub time_scale: f32,

  /// Whether the game is currently in hitstop (freeze frame effect).
  pub is_hitstop: bool,

  /// Current frame number since engine start.
  pub frame_count: u64,
}

impl WindowRequest {
  /// Create a new empty window request
  pub fn new() -> Self {
    Self::default()
  }

  /// Request a resolution change
  pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
    self.width = Some(width);
    self.height = Some(height);
    self
  }

  /// Request a display mode change
  pub fn with_display_mode(mut self, mode: DisplayMode) -> Self {
    self.display_mode = Some(mode);
    self
  }

  /// Check if any change is requested
  pub fn has_changes(&self) -> bool {
    self.width.is_some()
      || self.height.is_some()
      || self.display_mode.is_some()
      || self.ui_scale.is_some()
  }

  /// Clear all pending requests
  pub fn clear(&mut self) {
    self.width = None;
    self.height = None;
    self.display_mode = None;
    self.ui_scale = None;
  }
}

/// Context for GPU sprite rendering.
///
/// This context is passed to the optional `render_gpu` callback,
/// allowing games to use GPU-accelerated sprite batching for
/// high-performance particle systems and effects.
///
/// # Example
///
/// ```ignore
/// fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
///     for particle in &self.particles {
///         // Upload texture if not cached
///         gpu.upload_sprite(&particle.sprite);
///
///         // Create instance with transform
///         let instance = SpriteInstance::new()
///             .with_transform(particle.x, particle.y, particle.rotation, 1.0, 1.0);
///
///         // Add to batch
///         gpu.batch.add(particle.sprite.id(), false, instance, particle.z_order);
///     }
/// }
/// ```
pub struct GpuRenderContext<'a> {
  /// The batch collector for grouping sprites by texture
  pub batch: &'a mut BatchCollector,
  /// The GPU sprite renderer (for uploading textures)
  gpu_renderer: &'a mut SpriteGpuRenderer,
  /// Device for GPU operations
  device: &'a wgpu::Device,
  /// Queue for GPU operations
  queue: &'a wgpu::Queue,
  /// Camera for coordinate transforms
  pub camera: &'a super::graphics::Camera,
  /// The ECS world (read-only for rendering)
  pub world: &'a World,
}

impl<'a> GpuRenderContext<'a> {
  /// Create a new GPU render context
  pub fn new(
    batch: &'a mut BatchCollector,
    gpu_renderer: &'a mut SpriteGpuRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    camera: &'a super::graphics::Camera,
    world: &'a World,
  ) -> Self {
    Self {
      batch,
      gpu_renderer,
      device,
      queue,
      camera,
      world,
    }
  }

  /// Upload a sprite texture to the GPU cache.
  ///
  /// Call this for each unique sprite before adding instances to the batch.
  /// Sprites are cached by ID, so subsequent calls with the same sprite are no-ops.
  pub fn upload_sprite(&mut self, sprite: &SpriteHandle) {
    self
      .gpu_renderer
      .upload_sprite(self.device, self.queue, sprite);
  }

  /// Check if a sprite texture is already uploaded
  pub fn has_texture(&self, id: &str) -> bool {
    self.gpu_renderer.has_texture(id)
  }

  /// Add a sprite drawn with a game shader.
  ///
  /// `texture_id` is the id the texture was uploaded under. `params` are
  /// matched by name to the fields of the shader's `Params` struct (the
  /// sprite's own effect parameters are overwritten with them). A shader that
  /// fails to compile is logged and the sprite is drawn plain.
  pub fn add_custom_sprite(
    &mut self,
    texture_id: &str,
    shader: &super::ecs::ShaderHandle,
    params: &super::ecs::ShaderParams,
    mut instance: super::graphics::sprite_gpu::SpriteInstance,
    z_order: i32,
  ) {
    let shading = match self
      .gpu_renderer
      .shade_custom(self.device, shader, params, &mut instance)
    {
      Some(key) => crate::core::render::SpriteShading::Custom(key),
      None => crate::core::render::SpriteShading::Plain,
    };
    self.batch.add_shaded(texture_id, shading, instance, z_order);
  }

  /// Get screen dimensions (width, height)
  pub fn screen_size(&self) -> (f32, f32) {
    self.camera.viewport_size()
  }

  /// Get screen width
  pub fn screen_width(&self) -> u32 {
    let (w, _) = self.camera.viewport_size();
    w as u32
  }

  /// Get screen height
  pub fn screen_height(&self) -> u32 {
    let (_, h) = self.camera.viewport_size();
    h as u32
  }

  /// Get the camera reference
  pub fn camera(&self) -> &super::graphics::Camera {
    self.camera
  }

  /// Add a sprite instance to the batch collector.
  ///
  /// This is the low-level API for adding GPU sprites. Most games should use
  /// the higher-level APIs (e.g., GpuParallaxBackground) instead.
  pub fn add_sprite_instance(
    &mut self,
    texture_id: &str,
    instance: super::graphics::sprite_gpu::SpriteInstance,
    z_order: i32,
    has_effects: bool,
  ) {
    self.batch.add(texture_id, has_effects, instance, z_order);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // ===========================================================================
  // TimeControl Tests
  // ===========================================================================

  #[test]
  fn time_control_default_values() {
    let tc = TimeControl::default();
    assert_eq!(tc.time_scale, 1.0);
    assert_eq!(tc.hitstop_remaining, 0.0);
  }

  #[test]
  fn time_control_time_scale_clamped_positive() {
    let tc = TimeControl {
      time_scale: -1.0,
      ..Default::default()
    };
    // Note: Clamping is done in the context methods, not in the struct itself
    // This test just verifies the field can be set
    assert_eq!(tc.time_scale, -1.0);
  }

  #[test]
  fn time_control_hitstop_sets_value() {
    let tc = TimeControl {
      hitstop_remaining: 0.05,
      ..Default::default()
    };
    assert!((tc.hitstop_remaining - 0.05).abs() < 0.001);
  }

  #[test]
  fn time_control_is_hitstop_active() {
    let tc = TimeControl::default();
    assert!(tc.hitstop_remaining <= 0.0);

    let tc = TimeControl {
      hitstop_remaining: 0.05,
      ..Default::default()
    };
    assert!(tc.hitstop_remaining > 0.0);

    let tc = TimeControl {
      hitstop_remaining: 0.0,
      ..Default::default()
    };
    assert!(tc.hitstop_remaining <= 0.0);
  }

  #[test]
  fn window_request_time_scale_integration() {
    // Verify WindowRequest and TimeControl can coexist in the same context
    let wr = WindowRequest::default();
    let tc = TimeControl::default();

    assert!(!wr.has_changes());
    assert_eq!(tc.time_scale, 1.0);
  }

  // ===========================================================================
  // TimeInfo Tests
  // ===========================================================================

  #[test]
  fn time_info_struct_fields() {
    let time = TimeInfo {
      fixed_dt: 1.0 / 60.0,
      scaled_dt: 1.0 / 120.0,
      real_dt: 0.017,
      render_alpha: 0.5,
      time_scale: 0.5,
      is_hitstop: false,
      frame_count: 100,
    };

    assert!((time.fixed_dt - 1.0 / 60.0).abs() < 0.0001);
    assert!((time.scaled_dt - 1.0 / 120.0).abs() < 0.0001);
    assert!((time.real_dt - 0.017).abs() < 0.0001);
    assert!((time.render_alpha - 0.5).abs() < 0.0001);
    assert_eq!(time.time_scale, 0.5);
    assert!(!time.is_hitstop);
    assert_eq!(time.frame_count, 100);
  }

  #[test]
  fn time_info_is_copy() {
    let time = TimeInfo {
      fixed_dt: 1.0 / 60.0,
      scaled_dt: 1.0 / 60.0,
      real_dt: 0.016,
      render_alpha: 0.0,
      time_scale: 1.0,
      is_hitstop: false,
      frame_count: 0,
    };

    // TimeInfo is Copy, so this should work without move
    let time2 = time;
    let time3 = time;
    assert_eq!(time2.frame_count, time3.frame_count);
  }

  #[test]
  fn time_info_hitstop_flag() {
    let time_normal = TimeInfo {
      fixed_dt: 1.0 / 60.0,
      scaled_dt: 1.0 / 60.0,
      real_dt: 0.016,
      render_alpha: 0.0,
      time_scale: 1.0,
      is_hitstop: false,
      frame_count: 0,
    };

    let time_hitstop = TimeInfo {
      is_hitstop: true,
      scaled_dt: 0.0, // During hitstop, scaled_dt is 0
      ..time_normal
    };

    assert!(!time_normal.is_hitstop);
    assert!(time_hitstop.is_hitstop);
    assert_eq!(time_hitstop.scaled_dt, 0.0);
  }
}
