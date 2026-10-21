//! `HeadlessEngine`: a real engine without a window.

use std::time::Duration;

use image::RgbaImage;
use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::KeyCode;

use super::ui::{UiCacheStats, UiSnapshot, ui_snapshot};
use super::{HeadlessError, shared_gpu};
use crate::core::ecs::World;
use crate::core::ecs::kernel::Messages;
use crate::core::engine::{Engine, EngineBuilder, FrameOutcome};
use crate::core::graphics::{ScreenMapping, WgpuRenderer};
use crate::core::input::{GamepadButton, InputEvent, Stick};
use crate::core::render::UiRenderStats;
use crate::core::ui::nav::nav_targets;
use crate::core::ui::{Rect, UiEvent, UiId};

/// Largest supported render target side, in pixels.
const MAX_DIMENSION: u32 = 8192;

/// How the offscreen window stand-in is made.
#[derive(Debug, Clone, Copy)]
pub struct OffscreenOptions {
  pub(crate) format: wgpu::TextureFormat,
}

impl Default for OffscreenOptions {
  fn default() -> Self {
    Self {
      format: wgpu::TextureFormat::Rgba8UnormSrgb,
    }
  }
}

/// Offscreen options with a chosen target format, for testing output that
/// is not sRGB.
pub fn offscreen_with_format(format: wgpu::TextureFormat) -> OffscreenOptions {
  OffscreenOptions { format }
}

/// Whether the scene is still running after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
  /// The scene is still running
  Running,
  /// The scene (or console) asked to quit; further steps do nothing
  Quit,
}

/// The engine running without a window. See the [module docs](crate::testing).
pub struct HeadlessEngine {
  pub(super) engine: Engine,
  presented: u64,
  quit: bool,
  pending_releases: Vec<KeyCode>,
  pending_pad_releases: Vec<GamepadButton>,
}

impl HeadlessEngine {
  /// Build the engine headless and enter its initial scene.
  ///
  /// # Panics
  ///
  /// Panics if the builder has no start scene (`EngineBuilder::start_scene`).
  pub fn new(builder: EngineBuilder) -> Result<Self, HeadlessError> {
    Self::new_with(builder, OffscreenOptions::default())
  }

  /// [`new`](Self::new) with offscreen options (a different target format).
  pub fn new_with(
    builder: EngineBuilder,
    options: OffscreenOptions,
  ) -> Result<Self, HeadlessError> {
    Self::start(builder, options, 1.0)
  }

  /// A headless engine whose window has `scale_factor` physical pixels per
  /// logical pixel (a HiDPI display at 2.0).
  pub fn with_scale_factor(
    builder: EngineBuilder,
    scale_factor: f64,
  ) -> Result<Self, HeadlessError> {
    Self::start(builder, OffscreenOptions::default(), scale_factor)
  }

  /// [`with_scale_factor`](Self::with_scale_factor), or `None` (with a note)
  /// when this machine has no GPU.
  pub fn new_or_skip_with_scale_factor(builder: EngineBuilder, scale_factor: f64) -> Option<Self> {
    Self::or_skip(Self::with_scale_factor(builder, scale_factor))
  }

  /// The window starts at the game resolution in logical pixels.
  fn start(
    builder: EngineBuilder,
    options: OffscreenOptions,
    scale_factor: f64,
  ) -> Result<Self, HeadlessError> {
    let (device, queue) = shared_gpu()?;
    let mut engine = builder.build_headless();
    let (width, height) = engine.game_size();
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
      return Err(HeadlessError::Render(format!(
        "invalid resolution {width}x{height} (each side must be 1..={MAX_DIMENSION})"
      )));
    }
    let physical = (
      (f64::from(width) * scale_factor).round().max(1.0) as u32,
      (f64::from(height) * scale_factor).round().max(1.0) as u32,
    );
    let mapping = ScreenMapping::new(engine.presentation(), physical, scale_factor);
    let mut renderer = WgpuRenderer::new_offscreen_mapped(device, queue, mapping, options.format);
    renderer.keep_scene_image();
    engine.init_gpu(renderer, scale_factor);
    Ok(Self {
      engine,
      presented: 0,
      quit: false,
      pending_releases: Vec::new(),
      pending_pad_releases: Vec::new(),
    })
  }

  /// Like [`new`](Self::new), but returns `None` (and prints a skip line) when
  /// there is no GPU, so tests can `return` early. Panics on any other error.
  pub fn new_or_skip(builder: EngineBuilder) -> Option<Self> {
    Self::new_or_skip_with(builder, OffscreenOptions::default())
  }

  /// [`new_or_skip`](Self::new_or_skip) with offscreen options.
  pub fn new_or_skip_with(builder: EngineBuilder, options: OffscreenOptions) -> Option<Self> {
    Self::or_skip(Self::new_with(builder, options))
  }

  fn or_skip(engine: Result<Self, HeadlessError>) -> Option<Self> {
    match engine {
      Ok(engine) => Some(engine),
      Err(HeadlessError::NoGpu) => {
        eprintln!("skipping: no GPU available for HeadlessEngine");
        None
      }
      Err(error) => panic!("failed to start HeadlessEngine: {error}"),
    }
  }

  /// Run one frame: exactly one fixed update, then render.
  pub fn step(&mut self) -> Result<StepOutcome, HeadlessError> {
    if self.quit {
      return Ok(StepOutcome::Quit);
    }
    let dt = self.engine.fixed_timestep();
    let outcome = self.engine.frame(Some(dt));
    self.engine.apply_headless_requests();
    for key in self.pending_releases.drain(..) {
      self.engine.input_mut().queue(InputEvent::Key {
        key,
        pressed: false,
      });
    }
    for button in self.pending_pad_releases.drain(..) {
      self.engine.input_mut().queue(InputEvent::PadButton {
        button,
        pressed: false,
      });
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

  /// Resize as if the window became `width x height`; the game is scaled
  /// into it.
  pub fn resize(&mut self, width: u32, height: u32) {
    self.engine.resize_window(width, height);
  }

  /// The last presented window, bars included (`frame` is the scene image
  /// inside them).
  pub fn window_frame(&self) -> Result<RgbaImage, HeadlessError> {
    if self.presented == 0 {
      return Err(HeadlessError::Render("no frame presented yet".to_string()));
    }
    Ok(self.engine.read_window()?)
  }

  /// Move the pointer to `(x, y)` physical window pixels, mapped to screen
  /// space like a real cursor (over the bars too).
  pub fn mouse_move_window(&mut self, x: f32, y: f32) {
    self.engine.cursor_moved(f64::from(x), f64::from(y));
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
      .queue(InputEvent::Key { key, pressed: true });
  }

  /// Release `key`; the next step sees it as just released.
  pub fn release_key(&mut self, key: KeyCode) {
    self.engine.input_mut().queue(InputEvent::Key {
      key,
      pressed: false,
    });
  }

  /// Deliver typed text, as a window's text event does alongside a key press
  /// (`press_key` and `tap_key` type nothing).
  pub fn type_text(&mut self, text: &str) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::Text(text.to_string()));
  }

  /// Press `key` for the next step only; it is released after that step.
  pub fn tap_key(&mut self, key: KeyCode) {
    self.press_key(key);
    self.pending_releases.push(key);
  }

  /// Hold a mouse button down from the next step.
  pub fn press_mouse(&mut self, button: MouseButton) {
    self.engine.input_mut().queue(InputEvent::MouseButton {
      button,
      pressed: true,
    });
  }

  /// Release a mouse button.
  pub fn release_mouse(&mut self, button: MouseButton) {
    self.engine.input_mut().queue(InputEvent::MouseButton {
      button,
      pressed: false,
    });
  }

  /// Move the mouse to `(x, y)` in game coordinates.
  pub fn mouse_move(&mut self, x: f32, y: f32) {
    self.engine.input_mut().queue(InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    });
  }

  /// Hold a gamepad button down from the next step (a virtual pad connects if none is).
  pub fn press_pad(&mut self, button: GamepadButton) {
    self.engine.input_mut().queue(InputEvent::PadButton {
      button,
      pressed: true,
    });
  }

  /// Release a gamepad button.
  pub fn release_pad(&mut self, button: GamepadButton) {
    self.engine.input_mut().queue(InputEvent::PadButton {
      button,
      pressed: false,
    });
  }

  /// Press a gamepad button for the next step only; it is released after that step.
  pub fn tap_pad(&mut self, button: GamepadButton) {
    self.press_pad(button);
    self.pending_pad_releases.push(button);
  }

  /// Hold a stick at `(x, y)` (-1..1 each axis, +y up) until set again.
  pub fn set_stick(&mut self, stick: Stick, x: f32, y: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::PadStick { stick, x, y });
  }

  /// Turn the mouse wheel by `(dx, dy)` lines in the next step.
  pub fn wheel(&mut self, dx: f32, dy: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::Scroll(MouseScrollDelta::LineDelta(dx, dy)));
  }

  /// Move a trackpad by `(dx, dy)` logical pixels in the next step (+y
  /// scrolls up); the logical form, so the window's scale factor is not
  /// applied (queue `InputEvent::Scroll` with a `PixelDelta` for winit's
  /// physical pixels).
  pub fn trackpad(&mut self, dx: f32, dy: f32) {
    self
      .engine
      .input_mut()
      .queue(InputEvent::ScrollPixels { x: dx, y: dy });
  }

  /// The UI widget with the keyboard and gamepad focus, as the last step
  /// resolved it (`None` when nothing has it).
  pub fn ui_focused(&self) -> Option<UiId> {
    let focused = self.engine.ui_memory().interact.focused;
    (focused != UiId::NONE).then_some(focused)
  }

  /// What directional navigation can reach in the top scene, as last drawn:
  /// each focusable widget (enabled, shown, in a focus scope, not
  /// `nav_skip`) and its rect in UI units, by draw order. While a modal or
  /// another trapping scope is open, only the widgets inside it.
  pub fn ui_focusables(&self) -> Vec<(UiId, Rect)> {
    nav_targets(self.engine.ui_memory(), self.engine.top_scope())
  }

  /// Give widget `id` the focus at the next step's focus resolve, as
  /// `UiContext::request_focus` does (when it can take it then).
  pub fn focus_ui(&mut self, id: UiId) {
    self.engine.ui_memory_mut().request_focus(id);
  }

  /// Everything about the last frame's UI that a player or the game can see,
  /// in `Debug` form, as `UiHarness::frame_snapshot`: the draw commands,
  /// every record, the focus, the node events (those not yet dropped by a
  /// message update), the interaction, the widget states and their values,
  /// the open modals, the UI actions navigation used, the captured rebinds,
  /// the input the UI took and every UI node's components. Two runs that
  /// differ in any of these differ here.
  #[doc(hidden)]
  pub fn ui_snapshot(&self) -> String {
    let engine = &self.engine;
    let events: Vec<UiEvent> = engine
      .world()
      .get_resource::<Messages<UiEvent>>()
      .map(|messages| messages.iter_current_update_messages().cloned().collect())
      .unwrap_or_default();
    let log = engine.ui_pass_log();
    ui_snapshot(&UiSnapshot {
      commands: engine.ui_commands(),
      memory: engine.ui_memory(),
      events: &events,
      outcome: &log.used,
      rebinds: &log.rebinds,
      consumed: &log.consumed.consumed(),
      world: engine.world(),
    })
  }

  /// What the node walk did in the last UI pass: the nodes it built (and
  /// which), and how many tree nodes it replayed from records
  #[doc(hidden)]
  pub fn ui_cache_stats(&self) -> UiCacheStats {
    UiCacheStats::of(self.engine.world())
  }

  /// What the UI's render step did in the last frame: whether it kept the
  /// UI image, the texts it built and reused
  #[doc(hidden)]
  pub fn ui_render_stats(&self) -> UiRenderStats {
    self.engine.ui_render_stats()
  }

  /// The draws the last frame's `ui` pass was given: none when the UI image
  /// was kept or the UI drew nothing
  #[doc(hidden)]
  pub fn ui_draw_count(&self) -> u32 {
    self.engine.ui_draw_count()
  }

  /// The engine's debug switches, for a test to turn an overlay on
  #[cfg(test)]
  pub(crate) fn debug_config_mut(&mut self) -> &mut crate::core::config::DebugConfig {
    self.engine.debug_config_mut()
  }

  /// The last frame's render step on the CPU (building, uploading and
  /// encoding the frame; not the UI pass)
  #[doc(hidden)]
  pub fn render_time(&self) -> Duration {
    self.engine.render_time()
  }
}
