//! Scenes: the stack of game states and the context their callbacks get.
//!
//! Scenes represent distinct states of your game (menu, gameplay, pause, etc.).
//! The [`SceneManager`] keeps them in a stack, which gives:
//! - Push/pop navigation (menu → gameplay → pause → resume)
//! - Scene transitions with optional fade effects
//! - Systems a scene adds with [`SceneContext::add_systems`], which run while
//!   it is active and go away with it
//!
//! # Example
//!
//! ```rust,ignore
//! struct MenuScene;
//!
//! impl Scene for MenuScene {
//!     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!         if ctx.input.key_pressed(KeyCode::Enter) {
//!             return SceneAction::Push(Box::new(GameplayScene::new()));
//!         }
//!         SceneAction::Continue
//!     }
//! }
//! ```

mod cleanup;
mod context;
mod manager;

use std::any::Any;
use std::time::Duration;

use crate::core::ecs::Resource;
use crate::core::graphics::GpuRenderContext;
use crate::core::ui::UiContext;

pub use context::{ConsoleScope, SceneContext};
pub use manager::SceneManager;
pub(crate) use manager::SceneSlot;

// ============================================================================
// Scene Cleanup
// ============================================================================

/// Bitflags controlling which engine state is automatically reset during scene transitions.
///
/// When a scene exits via `Switch` or `Reset`, the engine runs cleanup based on
/// the exiting scene's `cleanup_on_exit()` return value. This gives the
/// incoming scene a clean slate without despawning entities by hand.
///
/// # Default Behavior
///
/// `SceneCleanup::DEFAULT` resets everything **except music**, since music commonly
/// persists across scenes (menu→game, level→level).
///
/// # Customization
///
/// Override `cleanup_on_exit()` on your `Scene` to customize:
///
/// ```rust,ignore
/// fn cleanup_on_exit(&self) -> SceneCleanup {
///     // Keep world entities alive (e.g., for a shared overworld)
///     SceneCleanup::DEFAULT - SceneCleanup::WORLD
/// }
/// ```
///
/// # When Cleanup Runs
///
/// | Action | Cleanup? | Rationale |
/// |--------|:--------:|-----------|
/// | Switch | Yes | Replacing scene, clean slate for new one |
/// | Reset  | Yes | Clearing everything, fresh start |
/// | Push   | No  | Overlay; paused scene needs its state |
/// | Pop    | No  | Resuming scene expects its state intact |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneCleanup(u16);

impl SceneCleanup {
  /// No cleanup: the incoming scene inherits all existing state.
  pub const NONE: Self = Self(0);
  /// Despawn every entity, disabled ones included (resources stay).
  pub const WORLD: Self = Self(1 << 0);
  /// Reset camera position, zoom, and shake (viewport preserved).
  pub const CAMERA: Self = Self(1 << 1);
  /// Clear all post-processing effects.
  pub const EFFECTS: Self = Self(1 << 2);
  /// Reset ambient light, atmosphere and volumetric settings to defaults.
  pub const LIGHTING: Self = Self(1 << 3);
  /// Reset the game clock's speed, pause and hitstop.
  pub const TIME: Self = Self(1 << 4);
  /// Stop any playing music.
  pub const MUSIC: Self = Self(1 << 5);
  /// Drop every buffered message.
  pub const MESSAGES: Self = Self(1 << 6);

  /// All cleanup flags enabled.
  pub const ALL: Self = Self(0b0111_1111);
  /// All cleanup flags except `MUSIC`: the recommended default.
  pub const DEFAULT: Self = Self(0b0101_1111);

  /// Returns `true` if `self` contains all flags in `other`.
  pub fn contains(self, other: Self) -> bool {
    (self.0 & other.0) == other.0
  }

  /// Returns `true` if no flags are set.
  pub fn is_empty(self) -> bool {
    self.0 == 0
  }
}

impl std::ops::BitOr for SceneCleanup {
  type Output = Self;
  fn bitor(self, rhs: Self) -> Self {
    Self(self.0 | rhs.0)
  }
}

impl std::ops::BitAnd for SceneCleanup {
  type Output = Self;
  fn bitand(self, rhs: Self) -> Self {
    Self(self.0 & rhs.0)
  }
}

impl std::ops::Not for SceneCleanup {
  type Output = Self;
  fn not(self) -> Self {
    Self(!self.0)
  }
}

impl std::ops::Sub for SceneCleanup {
  type Output = Self;
  fn sub(self, rhs: Self) -> Self {
    Self(self.0 & !rhs.0)
  }
}

// ============================================================================
// Transitions
// ============================================================================

/// Visual transition effect when switching between scenes.
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
      color: [0, 0, 0, 255],
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
/// Return these from `update()` or `ui()` to tell the [`SceneManager`] what to
/// do next.
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
  /// Pop and return typed data to the scene below
  PopReturn(Box<dyn Any>),
  /// Pop with data and a transition effect
  PopReturnWith(Box<dyn Any>, Transition),
  /// Exit the game
  Quit,
}

/// Convenience helper for extracting typed data from `on_resume`'s returned parameter.
///
/// # Example
///
/// ```rust,ignore
/// fn on_resume(&mut self, _ctx: &mut SceneContext, returned: Option<Box<dyn Any>>) {
///     if let Some(level) = downcast_return::<u32>(returned) {
///         self.selected_level = level;
///     }
/// }
/// ```
pub fn downcast_return<T: Any>(returned: Option<Box<dyn Any>>) -> Option<T> {
  returned.and_then(|data| data.downcast::<T>().ok().map(|b| *b))
}

// ============================================================================
// The Scene trait
// ============================================================================

/// A game state: menu, level, pause screen.
///
/// Every method has a default, so a scene implements only what it needs.
///
/// # Lifecycle
///
/// ```text
/// on_enter   when the scene becomes active
/// ui         every frame, before the fixed steps (visible scenes, bottom-up)
/// update     every fixed step while the scene is active
/// render     every frame, for GPU-batched sprites
/// on_pause   when another scene is pushed on top
/// on_resume  when the scene above pops
/// on_exit    when the scene leaves the stack
/// ```
pub trait Scene: 'static {
  /// Called when this scene becomes the active scene.
  ///
  /// Spawn entities, configure the camera and lighting, load sounds, and add
  /// the scene's systems here.
  fn on_enter(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene is removed from the stack.
  ///
  /// Automatic cleanup (world, camera, effects, lighting, time, messages) runs
  /// after this on Switch/Reset; see [`SceneCleanup`].
  fn on_exit(&mut self, _ctx: &mut SceneContext) {}

  /// Called when another scene is pushed on top of this one.
  fn on_pause(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene becomes active again after the scene above popped.
  ///
  /// If the popped scene used `SceneAction::PopReturn`, the returned data
  /// is passed in `returned`; [`downcast_return`] extracts it.
  fn on_resume(&mut self, _ctx: &mut SceneContext, _returned: Option<Box<dyn Any>>) {}

  /// Called every fixed step while this scene is active.
  ///
  /// `dt` is the constant fixed timestep. Return a `SceneAction` to change
  /// scenes.
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  /// Draw this scene's UI and handle its input. Runs once per frame before
  /// the fixed steps, so input the UI consumes never reaches `update`.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
  ///     ui.draw_text("Press ENTER to start", 300.0, 400.0, Color::WHITE);
  ///     if ui.button("Start", Rect::new(350.0, 450.0, 100.0, 30.0)) {
  ///         return SceneAction::Switch(Box::new(Level::new()));
  ///     }
  ///     SceneAction::Continue
  /// }
  /// ```
  fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    SceneAction::Continue
  }

  /// Add GPU-batched sprites (parallax backgrounds, particles, many-sprite
  /// effects) for this frame.
  fn render(&mut self, _gpu: &mut GpuRenderContext) {}

  /// Whether scenes below this one are also drawn and receive `ui`.
  ///
  /// Return `true` for overlays (pause menu, HUD).
  fn is_transparent(&self) -> bool {
    false
  }

  /// Whether scenes below this one also update and run their systems.
  fn updates_below(&self) -> bool {
    false
  }

  /// Whether physics should be paused while this scene is on top.
  fn is_physics_paused(&self) -> bool {
    false
  }

  /// Which engine state is reset when this scene exits via `Switch` or `Reset`.
  fn cleanup_on_exit(&self) -> SceneCleanup {
    SceneCleanup::DEFAULT
  }

  /// Human-readable name for debugging and logging; defaults to the type name.
  fn name(&self) -> &str {
    std::any::type_name::<Self>()
      .rsplit("::")
      .next()
      .unwrap_or("Unknown")
  }
}

/// Whether physics steps this frame, kept by the scene manager from the
/// stack's shape and the top scene's `is_physics_paused`.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicsPaused(pub bool);

/// Insert the resources scene callbacks and systems expect, for tests.
#[cfg(test)]
pub(crate) fn init_world_resources_for_tests(world: &mut crate::core::ecs::World) {
  use crate::core::asset::AssetManager;
  use crate::core::console::{CommandRegistry, register_builtins};
  use crate::core::cursor::CursorRequest;
  use crate::core::graphics::Camera;
  use crate::core::input::Input;
  use crate::core::lighting::LightingSettings;
  use crate::core::messages::register_engine_messages;
  use crate::core::render::RenderPipeline;
  use crate::core::shaders::EffectsManager;
  use crate::core::time::Time;
  use crate::core::window::{WindowInfo, WindowRequest};

  world.insert_resource(Time::new(60));
  world.insert_resource(Input::new());
  world.insert_resource(Camera::centered_at(400.0, 300.0, 800, 600));
  world.insert_resource(AssetManager::new());
  world.insert_resource(EffectsManager::new());
  world.insert_resource(LightingSettings::default());
  // MIGRATION: Task 16 inserts TilemapManager::new()
  world.insert_resource(WindowRequest::default());
  world.insert_resource(WindowInfo::default());
  world.insert_resource(CursorRequest::default());
  world.insert_resource(RenderPipeline::default_2d());
  let mut commands = CommandRegistry::new();
  register_builtins(&mut commands);
  world.insert_resource(commands);
  world.insert_resource(PhysicsPaused(false));
  register_engine_messages(world);
}

#[cfg(test)]
mod tests {
  use super::*;

  struct TestScene;

  impl Scene for TestScene {}

  #[test]
  fn scene_action_default() {
    let action: SceneAction = Default::default();
    assert!(matches!(action, SceneAction::Continue));
  }

  #[test]
  fn transition_none_default() {
    let transition: Transition = Default::default();
    assert!(matches!(transition, Transition::None));
    assert_eq!(transition.duration(), Duration::ZERO);
  }

  #[test]
  fn transition_fade_black() {
    let transition = Transition::fade(Duration::from_millis(500));
    match transition {
      Transition::Fade { duration, color } => {
        assert_eq!(duration, Duration::from_millis(500));
        assert_eq!(color, [0, 0, 0, 255]);
      }
      _ => panic!("Expected Fade transition"),
    }
    assert_eq!(transition.duration(), Duration::from_millis(1000)); // 2x (out + in)
  }

  #[test]
  fn transition_fade_white() {
    let transition = Transition::fade_white(Duration::from_millis(300));
    match transition {
      Transition::Fade { color, .. } => assert_eq!(color, [255, 255, 255, 255]),
      _ => panic!("Expected Fade transition"),
    }
  }

  #[test]
  fn transition_fade_custom_color() {
    let transition = Transition::fade_color(Duration::from_millis(200), 128, 64, 32);
    match transition {
      Transition::Fade { color, .. } => assert_eq!(color, [128, 64, 32, 255]),
      _ => panic!("Expected Fade transition"),
    }
  }

  #[test]
  fn transition_slide() {
    let transition = Transition::slide(Duration::from_millis(400), SlideDirection::Left);
    match transition {
      Transition::Slide {
        duration,
        direction,
      } => {
        assert_eq!(duration, Duration::from_millis(400));
        assert_eq!(direction, SlideDirection::Left);
      }
      _ => panic!("Expected Slide transition"),
    }
    assert_eq!(transition.duration(), Duration::from_millis(400));
  }

  #[test]
  fn scene_defaults() {
    let mut scene = TestScene;
    assert_eq!(scene.name(), "TestScene");
    assert!(!scene.is_transparent());
    assert!(!scene.updates_below());
    assert!(!scene.is_physics_paused());
    assert_eq!(scene.cleanup_on_exit(), SceneCleanup::DEFAULT);
    let mut world = crate::core::ecs::World::new();
    init_world_resources_for_tests(&mut world);
    let mut audio = crate::core::audio::Audio::new(None);
    let mut slot = SceneSlot::new(Box::new(TestScene));
    let action = SceneContext::scoped(&mut world, &mut audio, &mut slot, |ctx| {
      scene.update(ctx, 1.0 / 60.0)
    });
    assert!(matches!(action, SceneAction::Continue));
  }

  #[test]
  fn scene_cleanup_default_excludes_music() {
    for flag in [
      SceneCleanup::WORLD,
      SceneCleanup::CAMERA,
      SceneCleanup::EFFECTS,
      SceneCleanup::LIGHTING,
      SceneCleanup::TIME,
      SceneCleanup::MESSAGES,
    ] {
      assert!(SceneCleanup::DEFAULT.contains(flag));
      assert!(SceneCleanup::ALL.contains(flag));
    }
    assert!(!SceneCleanup::DEFAULT.contains(SceneCleanup::MUSIC));
    assert!(SceneCleanup::ALL.contains(SceneCleanup::MUSIC));
    assert_eq!(
      SceneCleanup::ALL - SceneCleanup::MUSIC,
      SceneCleanup::DEFAULT
    );
  }

  #[test]
  fn scene_cleanup_none_is_empty() {
    assert!(SceneCleanup::NONE.is_empty());
    assert!(!SceneCleanup::DEFAULT.is_empty());
    assert!(!SceneCleanup::ALL.is_empty());
  }

  #[test]
  fn scene_cleanup_subtraction_and_bitor() {
    let without_world = SceneCleanup::DEFAULT - SceneCleanup::WORLD;
    assert!(!without_world.contains(SceneCleanup::WORLD));
    assert!(without_world.contains(SceneCleanup::CAMERA));
    let combined = SceneCleanup::WORLD | SceneCleanup::CAMERA;
    assert!(combined.contains(SceneCleanup::WORLD));
    assert!(combined.contains(SceneCleanup::CAMERA));
    assert!(!combined.contains(SceneCleanup::EFFECTS));
  }

  #[test]
  fn downcast_return_extracts_the_right_type_only() {
    assert_eq!(downcast_return::<u32>(Some(Box::new(42u32))), Some(42));
    assert_eq!(downcast_return::<String>(Some(Box::new(42u32))), None);
    assert_eq!(downcast_return::<u32>(None), None);
  }
}
