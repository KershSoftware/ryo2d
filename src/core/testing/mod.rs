//! Run the engine without a window, for tests.
//!
//! [`HeadlessEngine`] drives a real scene through the full update and render
//! pipeline at a fixed timestep and reads frames back as pixels:
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use ryo2d::testing::HeadlessEngine;
//!
//! #[test]
//! fn enemy_turns_red_when_hit() {
//!   // None (with a "skipping: no GPU" line) on machines without a GPU
//!   let Some(mut engine) = HeadlessEngine::new_or_skip(
//!     EngineBuilder::new().resolution(320, 180).start_scene(Arena::default()),
//!   ) else { return };
//!
//!   engine.step_frames(10).unwrap();
//!   engine.tap_key(KeyCode::Space);
//!   engine.step_frames(30).unwrap();
//!
//!   let frame = engine.frame().unwrap();
//!   assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
//! }
//! ```
//!
//! Headless runs have no audio (`ctx.audio` is `None`), no gamepads, no hot
//! reload and no dev windows; they never read or write `system.json` user
//! preferences, ignore `RYO_CAPTURE_*`, and keep the FPS, frame-timing and
//! stats overlays off. `Ryo.toml` is honoured as in a normal build. Input
//! injected with `press_key`/`tap_key`/`press_mouse`/`mouse_move` goes through
//! the same handlers as window events; the engine's own debug keys (F1-F5, F9,
//! F10, F12) are inert headless. Seed any randomness your scene uses if you
//! compare frames exactly.
//! Particle emitters use an unseeded thread RNG, so don't compare frames
//! containing particles byte-for-byte.

mod gpu;
#[cfg(test)]
mod render_comparison;

use image::RgbaImage;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

use crate::core::ecs::World;
use crate::core::engine::{Engine, EngineBuilder, FrameOutcome};
use crate::core::graphics::WgpuRenderer;

pub use crate::core::error::HeadlessError;
pub use gpu::shared_gpu;

/// Largest supported render target side, in pixels.
const MAX_DIMENSION: u32 = 8192;

/// Whether the scene is still running after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
  /// The scene is still running
  Running,
  /// The scene (or console) asked to quit; further steps do nothing
  Quit,
}

/// The engine running without a window. See the [module docs](self).
pub struct HeadlessEngine {
  engine: Engine,
  presented: u64,
  quit: bool,
  pending_releases: Vec<KeyCode>,
}

impl HeadlessEngine {
  /// Build the engine headless and enter its initial scene.
  ///
  /// # Panics
  ///
  /// Panics if the builder has no start scene (`EngineBuilder::start_scene`).
  pub fn new(builder: EngineBuilder) -> Result<Self, HeadlessError> {
    let (device, queue) = shared_gpu()?;
    let mut engine = builder.build_headless();
    let (width, height) = engine.game_size();
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
      return Err(HeadlessError::Render(format!(
        "invalid resolution {width}x{height} (each side must be 1..={MAX_DIMENSION})"
      )));
    }
    engine.init_gpu(
      WgpuRenderer::new_offscreen(device, queue, width, height),
      1.0,
    );
    Ok(Self {
      engine,
      presented: 0,
      quit: false,
      pending_releases: Vec::new(),
    })
  }

  /// Like [`new`](Self::new), but returns `None` (and prints a skip line) when
  /// there is no GPU, so tests can `return` early. Panics on any other error.
  pub fn new_or_skip(builder: EngineBuilder) -> Option<Self> {
    match Self::new(builder) {
      Ok(engine) => Some(engine),
      Err(HeadlessError::NoGpu) => {
        eprintln!("skipping: no GPU available for HeadlessEngine");
        None
      }
      Err(error) => panic!("failed to start HeadlessEngine: {error}"),
    }
  }

  /// Run one frame: one fixed update, then render.
  pub fn step(&mut self) -> Result<StepOutcome, HeadlessError> {
    if self.quit {
      return Ok(StepOutcome::Quit);
    }
    let dt = self.engine.fixed_timestep();
    let outcome = self.engine.step_frame(Some(dt));
    self.engine.apply_headless_requests();
    for key in self.pending_releases.drain(..) {
      self
        .engine
        .input_mut()
        .handle_keyboard(key, ElementState::Released);
    }
    match outcome {
      FrameOutcome::Presented => {
        self.presented += 1;
        Ok(StepOutcome::Running)
      }
      FrameOutcome::Skipped => Ok(StepOutcome::Running),
      FrameOutcome::Quit => {
        self.quit = true;
        Ok(StepOutcome::Quit)
      }
      FrameOutcome::Fatal(error) => Err(HeadlessError::Render(error)),
    }
  }

  /// Run up to `frames` frames, stopping early if the scene quits.
  pub fn step_frames(&mut self, frames: u32) -> Result<StepOutcome, HeadlessError> {
    for _ in 0..frames {
      if self.step()? == StepOutcome::Quit {
        return Ok(StepOutcome::Quit);
      }
    }
    Ok(StepOutcome::Running)
  }

  /// Number of frames presented so far.
  pub fn frame_count(&self) -> u64 {
    self.presented
  }

  /// The last presented frame (scene + UI) at game resolution, origin top-left.
  ///
  /// Returns an error if no frame has been presented yet.
  pub fn frame(&self) -> Result<RgbaImage, HeadlessError> {
    if self.presented == 0 {
      return Err(HeadlessError::Render("no frame presented yet".to_string()));
    }
    Ok(self.engine.read_frame()?)
  }

  /// The ECS world, for inspecting entities between steps.
  pub fn world(&self) -> &World {
    self.engine.world()
  }

  /// The ECS world, for spawning or changing entities between steps.
  pub fn world_mut(&mut self) -> &mut World {
    self.engine.world_mut()
  }

  /// Hold `key` down from the next step until [`release_key`](Self::release_key).
  pub fn press_key(&mut self, key: KeyCode) {
    self
      .engine
      .input_mut()
      .handle_keyboard(key, ElementState::Pressed);
  }

  /// Release `key`; the next step sees it as just released.
  pub fn release_key(&mut self, key: KeyCode) {
    self
      .engine
      .input_mut()
      .handle_keyboard(key, ElementState::Released);
  }

  /// Press `key` for the next step only; it is released after that step.
  pub fn tap_key(&mut self, key: KeyCode) {
    self.press_key(key);
    self.pending_releases.push(key);
  }

  /// Hold a mouse button down from the next step.
  pub fn press_mouse(&mut self, button: MouseButton) {
    self
      .engine
      .input_mut()
      .handle_mouse_button(button, ElementState::Pressed);
  }

  /// Release a mouse button.
  pub fn release_mouse(&mut self, button: MouseButton) {
    self
      .engine
      .input_mut()
      .handle_mouse_button(button, ElementState::Released);
  }

  /// Move the mouse to `(x, y)` in game coordinates.
  pub fn mouse_move(&mut self, x: f32, y: f32) {
    self
      .engine
      .input_mut()
      .handle_mouse_move(f64::from(x), f64::from(y));
  }
}
