//! Game trait for user-defined game logic
//!
//! The engine handles low-level systems (window, input, rendering).
//! Users implement the `Game` trait to define their game behavior.

use crate::camera::Camera;
use crate::ecs::World;
use crate::input::InputState;

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
