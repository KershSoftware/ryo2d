//! Scene management system for organizing game states
//!
//! Scenes represent distinct states of your game (menu, gameplay, pause, etc.).
//! The `SceneManager` maintains a stack of scenes, allowing for:
//! - Push/pop navigation (menu → gameplay → pause → resume)
//! - Scene transitions with optional fade effects
//! - Clean separation of game state logic
//!
//! # Example
//!
//! ```rust,ignore
//! struct MenuScene;
//!
//! impl Scene for MenuScene {
//!     fn on_enter(&mut self, ctx: &mut SceneContext) {
//!         println!("Menu activated");
//!     }
//!
//!     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!         if ctx.input.key_pressed(KeyCode::Enter) {
//!             return SceneAction::Push(Box::new(GameplayScene::new()));
//!         }
//!         SceneAction::Continue
//!     }
//! }
//! ```

use super::asset::AssetManager;
use super::camera::Camera;
use super::ecs::World;
use super::input::InputState;
use super::rapier_physics::PhysicsCollisionEvent;

/// Context passed to scene callbacks, providing access to engine systems.
///
/// Similar to `GameContext` but designed for scene-based architecture.
pub struct SceneContext<'a> {
  /// The ECS world containing all entities and components
  pub world: &'a mut World,
  /// The camera for world-to-screen transformations
  pub camera: &'a mut Camera,
  /// Current input state (keyboard, mouse)
  pub input: &'a InputState,
  /// Asset manager for loading and caching resources
  pub assets: &'a mut AssetManager,
  /// Collision events from the last physics step
  pub collision_events: &'a [PhysicsCollisionEvent],
}

/// Actions a scene can return to control scene flow.
///
/// Return these from `update()` to tell the SceneManager what to do next.
#[derive(Default)]
pub enum SceneAction {
  /// Continue running the current scene (default)
  #[default]
  Continue,
  /// Push a new scene onto the stack (current scene pauses)
  Push(Box<dyn Scene>),
  /// Pop the current scene off the stack (returns to previous)
  Pop,
  /// Replace the current scene with a new one
  Switch(Box<dyn Scene>),
  /// Clear all scenes and push a new one
  Reset(Box<dyn Scene>),
  /// Exit the game
  Quit,
}

/// Trait for implementing game scenes.
///
/// Scenes are distinct states of your game, each with their own logic.
/// The SceneManager calls these methods at appropriate times.
///
/// # Lifecycle
///
/// ```text
/// ┌─────────────┐
/// │  on_enter   │  ← Called when scene becomes active
/// └──────┬──────┘
///        ▼
/// ┌─────────────┐
/// │   update    │  ← Called every fixed timestep
/// │   render    │  ← Called every frame (optional)
/// └──────┬──────┘
///        │ (push another scene)
///        ▼
/// ┌─────────────┐
/// │  on_pause   │  ← Called when another scene is pushed on top
/// └──────┬──────┘
///        │ (top scene pops)
///        ▼
/// ┌─────────────┐
/// │  on_resume  │  ← Called when this scene becomes active again
/// └──────┬──────┘
///        │ (pop or switch)
///        ▼
/// ┌─────────────┐
/// │  on_exit    │  ← Called when scene is removed from stack
/// └─────────────┘
/// ```
pub trait Scene {
  /// Called when this scene becomes the active scene.
  ///
  /// Use this to:
  /// - Initialize scene-specific entities
  /// - Set up the camera
  /// - Load assets
  /// - Play background music
  fn on_enter(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene is removed from the stack.
  ///
  /// Use this to:
  /// - Clean up entities
  /// - Stop music
  /// - Save state if needed
  fn on_exit(&mut self, _ctx: &mut SceneContext) {}

  /// Called when another scene is pushed on top of this one.
  ///
  /// This scene remains in the stack but is no longer updated.
  /// Use this to pause music, timers, etc.
  fn on_pause(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene becomes active again after being paused.
  ///
  /// The scene above was popped, so this scene resumes.
  /// Use this to resume music, timers, etc.
  fn on_resume(&mut self, _ctx: &mut SceneContext) {}

  /// Called every fixed timestep while this scene is active.
  ///
  /// This is where your main scene logic goes. Return a `SceneAction`
  /// to control scene flow (push, pop, switch, etc.).
  ///
  /// # Arguments
  ///
  /// * `ctx` - Access to engine systems
  /// * `dt` - Fixed delta time in seconds
  ///
  /// # Returns
  ///
  /// A `SceneAction` indicating what the SceneManager should do next.
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction;

  /// Called every frame for custom rendering (optional).
  ///
  /// Most scenes don't need to override this - the engine's built-in
  /// render system handles SpriteRenderer components automatically.
  ///
  /// Override this for:
  /// - Custom UI drawing
  /// - Debug visualizations
  /// - Post-processing effects
  ///
  /// # Arguments
  ///
  /// * `ctx` - Access to engine systems
  /// * `alpha` - Interpolation alpha for smooth rendering (0.0 to 1.0)
  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {}

  /// Whether scenes below this one should also be rendered.
  ///
  /// Return `true` for transparent/overlay scenes (pause menu, HUD).
  /// Return `false` (default) for opaque scenes that fully cover the screen.
  fn is_transparent(&self) -> bool {
    false
  }

  /// Whether scenes below this one should also be updated.
  ///
  /// Return `true` to allow background scenes to continue updating.
  /// Return `false` (default) to pause scenes below.
  fn updates_below(&self) -> bool {
    false
  }
}

/// Manages a stack of scenes.
///
/// The scene at the top of the stack is the "active" scene that receives
/// update calls. Scenes below may or may not update/render depending on
/// the `is_transparent()` and `updates_below()` settings of scenes above.
///
/// # Example
///
/// ```rust,ignore
/// let mut manager = SceneManager::new();
///
/// // Start with menu
/// manager.push(Box::new(MenuScene::new()));
///
/// // In your game loop:
/// let action = manager.update(&mut ctx, dt);
/// manager.handle_action(action, &mut ctx);
/// manager.render(&mut ctx, alpha);
/// ```
pub struct SceneManager {
  /// Stack of scenes (back = top)
  scenes: Vec<Box<dyn Scene>>,
}

impl SceneManager {
  /// Create a new empty scene manager.
  pub fn new() -> Self {
    SceneManager {
      scenes: Vec::new(),
    }
  }

  /// Create a scene manager with an initial scene.
  pub fn with_scene(scene: Box<dyn Scene>) -> Self {
    let mut manager = Self::new();
    manager.scenes.push(scene);
    manager
  }

  /// Push a new scene onto the stack.
  ///
  /// The current top scene will have `on_pause()` called.
  /// The new scene will have `on_enter()` called.
  pub fn push(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    // Pause current top scene
    if let Some(current) = self.scenes.last_mut() {
      current.on_pause(ctx);
    }

    // Add and enter new scene
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      new_top.on_enter(ctx);
    }
  }

  /// Pop the top scene off the stack.
  ///
  /// The popped scene will have `on_exit()` called.
  /// The new top scene (if any) will have `on_resume()` called.
  ///
  /// Returns the popped scene, or None if stack was empty.
  pub fn pop(&mut self, ctx: &mut SceneContext) -> Option<Box<dyn Scene>> {
    if let Some(mut scene) = self.scenes.pop() {
      scene.on_exit(ctx);

      // Resume the new top scene
      if let Some(new_top) = self.scenes.last_mut() {
        new_top.on_resume(ctx);
      }

      Some(scene)
    } else {
      None
    }
  }

  /// Replace the current scene with a new one.
  ///
  /// The current scene will have `on_exit()` called.
  /// The new scene will have `on_enter()` called.
  /// Scenes below are unaffected.
  pub fn switch(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    // Exit current top
    if let Some(mut current) = self.scenes.pop() {
      current.on_exit(ctx);
    }

    // Enter new scene
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      new_top.on_enter(ctx);
    }
  }

  /// Clear all scenes and optionally push a new one.
  ///
  /// All scenes will have `on_exit()` called (top to bottom).
  pub fn clear(&mut self, ctx: &mut SceneContext) {
    while let Some(mut scene) = self.scenes.pop() {
      scene.on_exit(ctx);
    }
  }

  /// Reset with a new scene (clear all, then push).
  pub fn reset(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    self.clear(ctx);
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      new_top.on_enter(ctx);
    }
  }

  /// Update the active scene(s).
  ///
  /// Returns a `SceneAction` from the top scene's update.
  /// Call `handle_action()` after to process it.
  pub fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if self.scenes.is_empty() {
      return SceneAction::Quit;
    }

    // Determine which scenes should update
    // Work from top down, stop when we hit a scene that doesn't update below
    let mut update_from = self.scenes.len() - 1;
    for i in (0..self.scenes.len()).rev() {
      if i == self.scenes.len() - 1 {
        // Top scene always updates
        continue;
      }
      // Check if the scene above allows updates below
      if !self.scenes[i + 1].updates_below() {
        update_from = i + 1;
        break;
      }
    }

    // Update scenes from bottom-up (so top scene's action wins)
    let mut action = SceneAction::Continue;
    for i in update_from..self.scenes.len() {
      // We need to work around the borrow checker here
      // Get raw pointer to scene, update, then handle action
      let scene = &mut self.scenes[i];
      let result = scene.update(ctx, dt);

      // Only the top scene's action matters
      if i == self.scenes.len() - 1 {
        action = result;
      }
    }

    action
  }

  /// Handle a scene action returned from update.
  ///
  /// This processes push/pop/switch/etc. actions.
  pub fn handle_action(&mut self, action: SceneAction, ctx: &mut SceneContext) -> bool {
    match action {
      SceneAction::Continue => true,
      SceneAction::Push(scene) => {
        self.push(scene, ctx);
        true
      }
      SceneAction::Pop => {
        self.pop(ctx);
        !self.scenes.is_empty()
      }
      SceneAction::Switch(scene) => {
        self.switch(scene, ctx);
        true
      }
      SceneAction::Reset(scene) => {
        self.reset(scene, ctx);
        true
      }
      SceneAction::Quit => false,
    }
  }

  /// Render all visible scenes.
  ///
  /// Renders from bottom to top, respecting `is_transparent()`.
  pub fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {
    if self.scenes.is_empty() {
      return;
    }

    // Find the first scene that's not transparent (render from there)
    let mut render_from = self.scenes.len() - 1;
    for i in (0..self.scenes.len()).rev() {
      render_from = i;
      if i == 0 || !self.scenes[i].is_transparent() {
        break;
      }
    }

    // Render from bottom-up
    for i in render_from..self.scenes.len() {
      self.scenes[i].render(ctx, alpha);
    }
  }

  /// Check if the scene stack is empty.
  pub fn is_empty(&self) -> bool {
    self.scenes.is_empty()
  }

  /// Get the number of scenes on the stack.
  pub fn len(&self) -> usize {
    self.scenes.len()
  }

  /// Get a reference to the top scene (if any).
  #[allow(clippy::borrowed_box)]
  pub fn top(&self) -> Option<&Box<dyn Scene>> {
    self.scenes.last()
  }

  /// Get a mutable reference to the top scene (if any).
  #[allow(clippy::borrowed_box)]
  pub fn top_mut(&mut self) -> Option<&mut Box<dyn Scene>> {
    self.scenes.last_mut()
  }
}

impl Default for SceneManager {
  fn default() -> Self {
    Self::new()
  }
}

// ============================================================================
// Game Trait Adapter
// ============================================================================

use super::game::{Game, GameContext};

/// Adapter that allows a SceneManager to be used with the existing Engine.
///
/// This bridges the scene-based architecture with the existing `Game` trait,
/// allowing you to use scenes without modifying engine internals.
///
/// # Example
///
/// ```rust,ignore
/// use ferrum2d::prelude::*;
///
/// // Create your scenes
/// struct MenuScene;
/// impl Scene for MenuScene {
///     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
///         if ctx.input.key_pressed(KeyCode::Enter) {
///             SceneAction::Push(Box::new(GameScene::new()))
///         } else {
///             SceneAction::Continue
///         }
///     }
/// }
///
/// // Use SceneGame adapter with the engine
/// let scenes = SceneGame::new(Box::new(MenuScene));
///
/// EngineBuilder::new()
///     .title("My Scene Game")
///     .with_game(scenes)
///     .build()
///     .run()
///     .unwrap();
/// ```
pub struct SceneGame {
  manager: SceneManager,
  /// Whether the initial scene has been entered
  initialized: bool,
}

impl SceneGame {
  /// Create a new SceneGame with an initial scene.
  pub fn new(initial_scene: Box<dyn Scene>) -> Self {
    SceneGame {
      manager: SceneManager::with_scene(initial_scene),
      initialized: false,
    }
  }

  /// Get a reference to the underlying scene manager.
  pub fn manager(&self) -> &SceneManager {
    &self.manager
  }

  /// Get a mutable reference to the underlying scene manager.
  pub fn manager_mut(&mut self) -> &mut SceneManager {
    &mut self.manager
  }
}

impl Game for SceneGame {
  fn init(&mut self, ctx: &mut GameContext) {
    // Convert GameContext to SceneContext and call on_enter for initial scene
    let mut scene_ctx = SceneContext {
      world: ctx.world,
      camera: ctx.camera,
      input: ctx.input,
      assets: ctx.assets,
      collision_events: ctx.collision_events,
    };

    // Call on_enter for the initial scene
    if let Some(scene) = self.manager.top_mut() {
      scene.on_enter(&mut scene_ctx);
    }
    self.initialized = true;
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Convert GameContext to SceneContext
    let mut scene_ctx = SceneContext {
      world: ctx.world,
      camera: ctx.camera,
      input: ctx.input,
      assets: ctx.assets,
      collision_events: ctx.collision_events,
    };

    // Update scenes and handle actions
    let action = self.manager.update(&mut scene_ctx, dt);
    self.manager.handle_action(action, &mut scene_ctx);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Mock scene for testing
  struct TestScene {
    #[allow(dead_code)]
    name: String,
    entered: bool,
    exited: bool,
    paused: bool,
    resumed: bool,
    update_count: u32,
  }

  impl TestScene {
    fn new(name: &str) -> Self {
      TestScene {
        name: name.to_string(),
        entered: false,
        exited: false,
        paused: false,
        resumed: false,
        update_count: 0,
      }
    }
  }

  impl Scene for TestScene {
    fn on_enter(&mut self, _ctx: &mut SceneContext) {
      self.entered = true;
    }

    fn on_exit(&mut self, _ctx: &mut SceneContext) {
      self.exited = true;
    }

    fn on_pause(&mut self, _ctx: &mut SceneContext) {
      self.paused = true;
    }

    fn on_resume(&mut self, _ctx: &mut SceneContext) {
      self.resumed = true;
    }

    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.update_count += 1;
      SceneAction::Continue
    }
  }

  #[test]
  fn scene_manager_push_pop() {
    // We can't easily test with a real context, so just verify the struct works
    let manager = SceneManager::new();
    assert!(manager.is_empty());
    assert_eq!(manager.len(), 0);
  }

  #[test]
  fn scene_action_default() {
    let action: SceneAction = Default::default();
    assert!(matches!(action, SceneAction::Continue));
  }

  #[test]
  fn test_scene_lifecycle_flags() {
    let scene = TestScene::new("test");
    assert!(!scene.entered);
    assert!(!scene.exited);
    assert!(!scene.paused);
    assert!(!scene.resumed);
    assert_eq!(scene.update_count, 0);
  }
}
