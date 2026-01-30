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

use std::time::Duration;

use super::asset::AssetManager;
use super::audio::AudioManager;
use super::camera::Camera;
use super::ecs::World;
use super::input::InputState;
use super::rapier_physics::PhysicsCollisionEvent;
use super::ui::UiContext;

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
  /// Audio manager for sound effects and music (None if audio unavailable)
  pub audio: &'a mut Option<AudioManager>,
  /// Collision events from the last physics step.
  ///
  /// **Note:** This includes BOTH `Started` and `Stopped` events.
  /// For most game logic (damage, death, scoring), you probably only
  /// want `Started` events. Use [`collisions_started()`](Self::collisions_started)
  /// for a filtered iterator.
  pub collision_events: &'a [PhysicsCollisionEvent],
}

impl<'a> SceneContext<'a> {
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

// ============================================================================
// Transitions
// ============================================================================

/// Visual transition effect when switching between scenes.
///
/// Transitions provide smooth visual effects during scene changes,
/// improving the user experience.
///
/// # Example
///
/// ```rust,ignore
/// // Fade to black over 500ms, then fade in the new scene
/// SceneAction::SwitchWith(
///     Box::new(GameplayScene::new()),
///     Transition::fade(Duration::from_millis(500))
/// )
/// ```
#[derive(Debug, Clone, Default)]
pub enum Transition {
  /// Instant transition (no effect)
  #[default]
  None,
  /// Fade through a solid color (default: black)
  Fade {
    /// Duration of the fade out (old scene)
    duration: Duration,
    /// Color to fade through (RGBA)
    color: [u8; 4],
  },
  /// Slide the new scene in from a direction
  Slide {
    /// Duration of the slide
    duration: Duration,
    /// Direction to slide from
    direction: SlideDirection,
  },
}

impl Transition {
  /// Create a fade transition through black.
  pub fn fade(duration: Duration) -> Self {
    Transition::Fade {
      duration,
      color: [0, 0, 0, 255], // Black
    }
  }

  /// Create a fade transition through a custom color.
  pub fn fade_color(duration: Duration, r: u8, g: u8, b: u8) -> Self {
    Transition::Fade {
      duration,
      color: [r, g, b, 255],
    }
  }

  /// Create a fade transition through white.
  pub fn fade_white(duration: Duration) -> Self {
    Transition::Fade {
      duration,
      color: [255, 255, 255, 255],
    }
  }

  /// Create a slide transition.
  pub fn slide(duration: Duration, direction: SlideDirection) -> Self {
    Transition::Slide {
      duration,
      direction,
    }
  }

  /// Get the total duration of this transition.
  pub fn duration(&self) -> Duration {
    match self {
      Transition::None => Duration::ZERO,
      Transition::Fade { duration, .. } => *duration * 2, // fade out + fade in
      Transition::Slide { duration, .. } => *duration,
    }
  }
}

/// Direction for slide transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
  /// Slide in from the left
  Left,
  /// Slide in from the right
  Right,
  /// Slide in from the top
  Up,
  /// Slide in from the bottom
  Down,
}

/// Actions a scene can return to control scene flow.
///
/// Return these from `update()` to tell the SceneManager what to do next.
///
/// # Transitions
///
/// Actions with `With` suffix (e.g., `SwitchWith`, `PushWith`) accept a
/// `Transition` for smooth visual effects. Actions without transitions
/// change immediately.
#[derive(Default)]
pub enum SceneAction {
  /// Continue running the current scene (default)
  #[default]
  Continue,
  /// Push a new scene onto the stack (current scene pauses)
  Push(Box<dyn Scene>),
  /// Push with a transition effect
  PushWith(Box<dyn Scene>, Transition),
  /// Pop the current scene off the stack (returns to previous)
  Pop,
  /// Pop with a transition effect
  PopWith(Transition),
  /// Replace the current scene with a new one
  Switch(Box<dyn Scene>),
  /// Switch with a transition effect
  SwitchWith(Box<dyn Scene>, Transition),
  /// Clear all scenes and push a new one
  Reset(Box<dyn Scene>),
  /// Reset with a transition effect
  ResetWith(Box<dyn Scene>, Transition),
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

  /// Called after sprite rendering to draw UI elements.
  ///
  /// Use this for HUD elements, debug overlays, and in-game UI.
  /// UI is drawn in screen space, on top of all world sprites.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn render_ui(&mut self, ui: &mut UiContext) {
  ///     ui.draw_text("Press ENTER to start", 300.0, 400.0, Color::WHITE);
  ///     if ui.button("Start", Rect::new(350.0, 450.0, 100.0, 30.0)) {
  ///         self.start_game = true;
  ///     }
  /// }
  /// ```
  fn render_ui(&mut self, _ui: &mut UiContext) {}

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

  /// Human-readable name for debugging and logging.
  ///
  /// Override this to provide a custom name, or leave the default
  /// which uses the type name.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn name(&self) -> &str {
  ///     "MainMenu"
  /// }
  /// ```
  fn name(&self) -> &str {
    std::any::type_name::<Self>()
      .rsplit("::")
      .next()
      .unwrap_or("Unknown")
  }
}

/// Manages a stack of scenes.
///
/// The scene at the top of the stack is the "active" scene that receives
/// update calls. Scenes below may or may not update/render depending on
/// the `is_transparent()` and `updates_below()` settings of scenes above.
///
/// # Transitions
///
/// Scene changes can include visual transitions (fade, slide).
/// During a transition, both scenes may be rendered with effects applied.
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
  /// Active transition state
  transition_state: Option<TransitionState>,
}

/// Internal state for an in-progress transition.
#[derive(Debug)]
struct TransitionState {
  /// The transition effect being applied
  transition: Transition,
  /// Elapsed time in the transition
  elapsed: Duration,
  /// The action to perform at the midpoint (for fades) or end
  pending_action: PendingAction,
  /// Current phase of the transition
  phase: TransitionPhase,
}

enum PendingAction {
  Push(Box<dyn Scene>),
  Pop,
  Switch(Box<dyn Scene>),
  Reset(Box<dyn Scene>),
}

impl std::fmt::Debug for PendingAction {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PendingAction::Push(_) => write!(f, "Push(...)"),
      PendingAction::Pop => write!(f, "Pop"),
      PendingAction::Switch(_) => write!(f, "Switch(...)"),
      PendingAction::Reset(_) => write!(f, "Reset(...)"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPhase {
  /// Fading/sliding out the old scene
  Out,
  /// Fading/sliding in the new scene
  In,
}

impl SceneManager {
  /// Create a new empty scene manager.
  pub fn new() -> Self {
    SceneManager {
      scenes: Vec::new(),
      transition_state: None,
    }
  }

  /// Create a scene manager with an initial scene.
  pub fn with_scene(scene: Box<dyn Scene>) -> Self {
    let mut manager = Self::new();
    manager.scenes.push(scene);
    manager
  }

  /// Check if a transition is currently in progress.
  pub fn is_transitioning(&self) -> bool {
    self.transition_state.is_some()
  }

  /// Get the current transition progress (0.0 to 1.0).
  ///
  /// Returns `None` if no transition is active.
  pub fn transition_progress(&self) -> Option<f32> {
    self.transition_state.as_ref().map(|state| {
      let total = state.transition.duration();
      if total.is_zero() {
        1.0
      } else {
        (state.elapsed.as_secs_f32() / total.as_secs_f32()).min(1.0)
      }
    })
  }

  /// Get the current transition overlay alpha (0.0 to 1.0).
  ///
  /// For fade transitions, this is the opacity of the fade overlay.
  /// Returns 0.0 if no transition or transition doesn't use overlay.
  pub fn transition_overlay_alpha(&self) -> f32 {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::None => 0.0,
        Transition::Fade { duration, .. } => {
          let half_duration = duration.as_secs_f32();
          let elapsed = state.elapsed.as_secs_f32();

          match state.phase {
            TransitionPhase::Out => (elapsed / half_duration).min(1.0),
            TransitionPhase::In => (1.0 - (elapsed / half_duration)).max(0.0),
          }
        }
        Transition::Slide { .. } => 0.0,
      },
      None => 0.0,
    }
  }

  /// Get the transition overlay color (if any).
  pub fn transition_overlay_color(&self) -> Option<[u8; 4]> {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::Fade { color, .. } => Some(*color),
        _ => None,
      },
      None => None,
    }
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
      // Transition variants
      SceneAction::PushWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Push(scene));
        true
      }
      SceneAction::PopWith(transition) => {
        self.start_transition(transition, PendingAction::Pop);
        true
      }
      SceneAction::SwitchWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Switch(scene));
        true
      }
      SceneAction::ResetWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Reset(scene));
        true
      }
      SceneAction::Quit => false,
    }
  }

  /// Start a transition effect.
  fn start_transition(&mut self, transition: Transition, action: PendingAction) {
    if matches!(transition, Transition::None) {
      // No transition, just a marker - shouldn't happen but handle it
      return;
    }

    self.transition_state = Some(TransitionState {
      transition,
      elapsed: Duration::ZERO,
      pending_action: action,
      phase: TransitionPhase::Out,
    });
  }

  /// Update transition state, performing the scene change at the right moment.
  ///
  /// Call this each frame with the delta time to advance transitions.
  /// Returns `true` if the game should continue, `false` if it should quit.
  pub fn update_transition(&mut self, dt: f32, ctx: &mut SceneContext) -> bool {
    let dt_duration = Duration::from_secs_f32(dt);

    // First pass: update elapsed time and check what action to take
    let action_to_execute = if let Some(ref mut state) = self.transition_state {
      state.elapsed += dt_duration;

      match &state.transition {
        Transition::None => {
          // Shouldn't be here, but clean up
          self.transition_state = None;
          None
        }
        Transition::Fade { duration, .. } => {
          let duration = *duration;
          match state.phase {
            TransitionPhase::Out => {
              if state.elapsed >= duration {
                // Need to execute action and switch to In phase
                state.elapsed = Duration::ZERO;
                state.phase = TransitionPhase::In;
                Some(std::mem::replace(
                  &mut state.pending_action,
                  PendingAction::Pop,
                ))
              } else {
                None
              }
            }
            TransitionPhase::In => {
              if state.elapsed >= duration {
                self.transition_state = None;
              }
              None
            }
          }
        }
        Transition::Slide { duration, .. } => {
          let duration = *duration;
          if state.phase == TransitionPhase::Out {
            state.phase = TransitionPhase::In;
            Some(std::mem::replace(
              &mut state.pending_action,
              PendingAction::Pop,
            ))
          } else if state.elapsed >= duration {
            self.transition_state = None;
            None
          } else {
            None
          }
        }
      }
    } else {
      None
    };

    // Second pass: execute any pending action
    if let Some(action) = action_to_execute {
      self.execute_pending_action(action, ctx);
    }

    !self.scenes.is_empty()
  }

  /// Execute a pending action during a transition.
  fn execute_pending_action(&mut self, action: PendingAction, ctx: &mut SceneContext) {
    match action {
      PendingAction::Push(scene) => {
        self.push(scene, ctx);
      }
      PendingAction::Pop => {
        self.pop(ctx);
      }
      PendingAction::Switch(scene) => {
        self.switch(scene, ctx);
      }
      PendingAction::Reset(scene) => {
        self.reset(scene, ctx);
      }
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

  /// Render UI for all visible scenes.
  ///
  /// Called after sprite rendering. Renders from bottom to top.
  pub fn render_ui(&mut self, ui: &mut UiContext) {
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

    // Render UI from bottom-up
    for i in render_from..self.scenes.len() {
      self.scenes[i].render_ui(ui);
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

  /// Get a debug representation of the current scene stack.
  ///
  /// Useful for logging and debugging scene flow.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// println!("{}", manager.debug_stack());
  /// // Output: "[MenuScene] -> [GameplayScene] -> [PauseScene*]"
  /// ```
  pub fn debug_stack(&self) -> String {
    if self.scenes.is_empty() {
      return String::from("(empty)");
    }

    self
      .scenes
      .iter()
      .enumerate()
      .map(|(i, scene)| {
        let name = scene.name();
        if i == self.scenes.len() - 1 {
          format!("[{}*]", name) // Mark active scene
        } else {
          format!("[{}]", name)
        }
      })
      .collect::<Vec<_>>()
      .join(" -> ")
  }

  /// Get the names of all scenes in the stack (bottom to top).
  pub fn scene_names(&self) -> Vec<&str> {
    self.scenes.iter().map(|s| s.name()).collect()
  }

  /// Check if physics should be paused based on scene stack.
  ///
  /// Physics is paused when there's more than one scene and the top scene
  /// doesn't allow updates below (e.g., a pause menu blocking the game).
  pub fn is_physics_paused(&self) -> bool {
    if self.scenes.len() > 1 {
      // If top scene doesn't update below, physics should pause
      if let Some(top) = self.scenes.last() {
        return !top.updates_below();
      }
    }
    false
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
/// use ryo2d::prelude::*;
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
  /// Whether a scene requested to quit
  quit_requested: bool,
}

impl SceneGame {
  /// Create a new SceneGame with an initial scene.
  pub fn new(initial_scene: Box<dyn Scene>) -> Self {
    SceneGame {
      manager: SceneManager::with_scene(initial_scene),
      initialized: false,
      quit_requested: false,
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
      audio: ctx.audio,
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
      audio: ctx.audio,
      collision_events: ctx.collision_events,
    };

    // Update any active transitions first
    self.manager.update_transition(dt, &mut scene_ctx);

    // Only update scenes if not in the middle of a transition
    if !self.manager.is_transitioning() {
      let action = self.manager.update(&mut scene_ctx, dt);
      let should_continue = self.manager.handle_action(action, &mut scene_ctx);
      if !should_continue {
        self.quit_requested = true;
      }
    }
  }

  fn get_overlay(&self) -> Option<(f32, [u8; 4])> {
    let alpha = self.manager.transition_overlay_alpha();
    if alpha > 0.001 {
      let color = self
        .manager
        .transition_overlay_color()
        .unwrap_or([0, 0, 0, 255]);
      Some((alpha, color))
    } else {
      None
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    self.manager.render_ui(ui);
  }

  fn is_physics_paused(&self) -> bool {
    // Physics is paused when the top scene doesn't update below
    // (e.g., a pause menu that blocks the game scene)
    self.manager.is_physics_paused()
  }

  fn should_quit(&self) -> bool {
    self.quit_requested
  }
}

impl SceneGame {
  /// Check if a transition is currently active.
  pub fn is_transitioning(&self) -> bool {
    self.manager.is_transitioning()
  }

  /// Get the transition overlay alpha for rendering fade effects.
  ///
  /// Returns a value from 0.0 (fully transparent) to 1.0 (fully opaque).
  pub fn transition_overlay(&self) -> (f32, [u8; 4]) {
    let alpha = self.manager.transition_overlay_alpha();
    let color = self
      .manager
      .transition_overlay_color()
      .unwrap_or([0, 0, 0, 255]);
    (alpha, color)
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
