//! Game trait for user-defined game logic
//!
//! The engine handles low-level systems (window, input, rendering).
//! Users implement the `Game` trait to define their game behavior.

use super::asset::SpriteHandle;
use super::ecs::World;
use super::graphics::{BatchCollector, Camera, SpriteGpuRenderer};
use super::ui::UiContext;

// Re-export from shared context module
pub use super::context::{ContextCore, TimeControl, WindowRequest};

/// Context passed to game callbacks, providing access to engine systems.
///
/// This is a type alias for [`ContextCore`], ensuring the same methods are
/// available in both `GameContext` and [`SceneContext`](super::scene::SceneContext).
pub type GameContext<'a> = ContextCore<'a>;

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

/// Trait for user-defined game logic.
///
/// Implement this trait to create your game. The engine will call your
/// methods at the appropriate times in the game loop.
///
/// # Example
///
/// ```ignore
/// struct MyGame {
///     player: Option<Entity>,
/// }
///
/// impl Game for MyGame {
///     fn init(&mut self, ctx: &mut GameContext) {
///         // Create your entities here
///         let player = ctx.world.spawn();
///         ctx.world.add_component(player, Transform::new(100.0, 100.0));
///         self.player = Some(player);
///     }
///
///     fn update(&mut self, ctx: &mut GameContext, dt: f32) {
///         // Game logic runs here every fixed timestep
///         if ctx.input.key_down(KeyCode::Space) {
///             // Jump!
///         }
///     }
/// }
/// ```
pub trait Game {
  /// Called once when the engine starts, after the window is created.
  ///
  /// Use this to:
  /// - Spawn initial entities
  /// - Set up the camera
  /// - Load any startup state
  fn init(&mut self, ctx: &mut GameContext);

  /// Called every fixed timestep (e.g., 60 times per second).
  ///
  /// Use this for:
  /// - Reading input
  /// - Updating entity positions/velocities
  /// - Game logic (AI, scoring, state machines)
  ///
  /// `dt` is the fixed delta time in seconds (e.g., 1/60 = 0.01667)
  fn update(&mut self, ctx: &mut GameContext, dt: f32);

  /// Called before entity rendering to draw background layers.
  ///
  /// Use this for:
  /// - Parallax scrolling backgrounds
  /// - Static sky/backdrop images
  /// - Any background elements that should be behind entities
  ///
  /// This is called after the frame buffer is cleared but before any
  /// entities (sprites, tiles, etc.) are rendered.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
  ///     self.background.render(frame, width, height, camera);
  /// }
  /// ```
  fn render_background(&mut self, _frame: &mut [u8], _width: u32, _height: u32, _camera: &Camera) {}

  /// Called to populate GPU sprite batches for high-performance rendering.
  ///
  /// Use this for:
  /// - Particle systems (1000+ sprites)
  /// - Bullet hell games
  /// - Any scenario where CPU sprite blitting is too slow
  ///
  /// GPU sprites are rendered on top of CPU-rendered sprites but before UI.
  /// The provided `GpuRenderContext` gives access to batch collection and
  /// texture upload.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
  ///     // Upload textures (cached, safe to call every frame)
  ///     for sprite in &self.particle_sprites {
  ///         gpu.upload_sprite(sprite);
  ///     }
  ///
  ///     // Add particle instances to batches
  ///     for particle in &self.particles {
  ///         let instance = SpriteInstance::new()
  ///             .with_transform(particle.x, particle.y, 0.0, 1.0, 1.0);
  ///         gpu.batch.add(&particle.sprite.id(), false, instance, 10);
  ///     }
  /// }
  /// ```
  fn render_gpu(&mut self, _gpu: &mut GpuRenderContext) {}

  /// Called after sprite rendering to draw UI elements.
  ///
  /// Use this for:
  /// - HUD elements (health bars, score, minimap)
  /// - Debug overlays
  /// - In-game UI (inventory, dialogs)
  ///
  /// UI is drawn in screen space, on top of all world sprites.
  /// The provided `UiContext` offers immediate-mode drawing methods.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_ui(&mut self, ui: &mut UiContext) {
  ///     // Draw health bar
  ///     ui.draw_progress_bar(
  ///         Rect::new(10.0, 10.0, 200.0, 20.0),
  ///         self.health / 100.0,
  ///         Color::RED,
  ///         Color::DARK_GRAY,
  ///     );
  ///
  ///     // Draw score
  ///     ui.draw_text(&format!("Score: {}", self.score), 10.0, 40.0, Color::WHITE);
  /// }
  /// ```
  fn render_ui(&mut self, _ui: &mut UiContext) {}

  /// Return an optional screen overlay for transition effects.
  ///
  /// The engine will blend this color over the entire screen after
  /// rendering all sprites. Used for fade transitions between scenes.
  ///
  /// Returns `Some((alpha, [r, g, b, a]))` to show overlay, `None` for no overlay.
  /// Alpha is 0.0 (transparent) to 1.0 (opaque).
  fn get_overlay(&self) -> Option<(f32, [u8; 4])> {
    None
  }

  /// Whether physics simulation should be paused.
  ///
  /// Override this to pause physics during pause menus, cutscenes, etc.
  /// Default is `false` (physics runs normally).
  fn is_physics_paused(&self) -> bool {
    false
  }

  /// Whether the game wants to quit.
  ///
  /// When this returns `true`, the engine will close the window and exit.
  /// Default is `false` (game keeps running).
  fn should_quit(&self) -> bool {
    false
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct TestGame {
    init_called: bool,
    update_count: u32,
  }

  impl Game for TestGame {
    fn init(&mut self, _ctx: &mut GameContext) {
      self.init_called = true;
    }

    fn update(&mut self, _ctx: &mut GameContext, _dt: f32) {
      self.update_count += 1;
    }
  }

  #[test]
  fn game_trait_is_object_safe() {
    // This test verifies that Game can be used as a trait object
    let game: Box<dyn Game> = Box::new(TestGame {
      init_called: false,
      update_count: 0,
    });
    // If this compiles, the trait is object-safe
    let _ = game;
  }
}
