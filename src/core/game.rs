//! Game trait for user-defined game logic
//!
//! The engine handles low-level systems (window, input, rendering).
//! Users implement the `Game` trait to define their game behavior.

use super::asset::{AssetManager, SpriteHandle};
use super::audio::AudioManager;
use super::camera::Camera;
use super::ecs::World;
use super::input::InputState;
use super::rapier_physics::PhysicsCollisionEvent;
use super::shaders::EffectsManager;
use super::sprite_gpu::{BatchCollector, SpriteGpuRenderer};
use super::ui::UiContext;

/// Context passed to game callbacks, providing access to engine systems.
///
/// This is your "toolkit" for building games - it contains everything
/// you need to create entities, handle input, and control the camera.
pub struct GameContext<'a> {
  /// The ECS world containing all entities and components
  pub world: &'a mut World,
  /// The camera for world-to-screen transformations
  pub camera: &'a mut Camera,
  /// Current input state (keyboard, mouse)
  pub input: &'a InputState,
  /// Asset manager for loading and caching resources
  pub assets: &'a mut AssetManager,
  /// Audio manager for sound effects and music (None if audio unavailable)
  pub audio: &'a mut Option<AudioManager>,
  /// Post-process effects manager for screen-wide visual effects
  pub effects: &'a mut EffectsManager,
  /// Collision events from the last physics step.
  ///
  /// **Note:** This includes BOTH `Started` and `Stopped` events.
  /// For most game logic (damage, death, scoring), you probably only
  /// want `Started` events. Use [`collisions_started()`](Self::collisions_started)
  /// for a filtered iterator.
  pub collision_events: &'a [PhysicsCollisionEvent],
}

impl<'a> GameContext<'a> {
  /// Iterate over collision events where objects started touching.
  ///
  /// This is the most common case for game logic - reacting when
  /// objects first collide (damage, pickups, death triggers, etc.).
  ///
  /// # Example
  ///
  /// ```ignore
  /// for event in ctx.collisions_started() {
  ///     if event.involves(player) {
  ///         // Player hit something!
  ///     }
  /// }
  /// ```
  pub fn collisions_started(&self) -> impl Iterator<Item = &PhysicsCollisionEvent> {
    self.collision_events.iter().filter(|e| e.is_started())
  }

  /// Iterate over collision events where objects stopped touching.
  ///
  /// Useful for "exit trigger" logic or resetting state when
  /// objects separate.
  pub fn collisions_stopped(&self) -> impl Iterator<Item = &PhysicsCollisionEvent> {
    self.collision_events.iter().filter(|e| e.is_stopped())
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
  pub camera: &'a super::camera::Camera,
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
    camera: &'a super::camera::Camera,
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
    self.gpu_renderer.upload_sprite(self.device, self.queue, sprite);
  }

  /// Check if a sprite texture is already uploaded
  pub fn has_texture(&self, id: &str) -> bool {
    self.gpu_renderer.has_texture(id)
  }

  /// Get screen dimensions (width, height)
  pub fn screen_size(&self) -> (f32, f32) {
    self.camera.viewport_size()
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
