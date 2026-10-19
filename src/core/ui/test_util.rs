//! Recording UI contexts and a headless UI driver for tests (no GPU)

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use glam::Vec2;
use winit::keyboard::KeyCode;

use crate::core::input::{Binding, Direction, GamepadButton, InputDevice};
use crate::core::render::UiSpace;
use crate::core::testing::UiHarness;
use crate::core::testing::ui::direction_index;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{PendingRebind, PromptContext, UiCommands, UiContext};
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::{UiMemory, UiSound};
use crate::core::ui::nav::NavOutcome;
use crate::core::ui::theme::Theme;

/// How many captured warnings contain `needle`
pub(crate) fn warnings_containing(needle: &str) -> usize {
  crate::core::dev_log::get_entries()
    .iter()
    .filter(|w| w.message.contains(needle))
    .count()
}

/// A text engine over the built-in fonts
pub(crate) fn text_engine() -> TextEngine {
  let mut engine = TextEngine::new();
  engine.sync(&Fonts::with_builtins());
  engine
}

/// A context recording into `commands`, `size` UI units at scale 1
pub(crate) fn context<'a>(
  commands: &'a mut UiCommands,
  text: &'a mut TextEngine,
  size: (f32, f32),
) -> UiContext<'a> {
  context_scaled(commands, text, size, 1.0)
}

/// A context recording into `commands`, `size` UI units at `px_scale`
/// render pixels per UI unit
pub(crate) fn context_scaled<'a>(
  commands: &'a mut UiCommands,
  text: &'a mut TextEngine,
  size: (f32, f32),
  px_scale: f32,
) -> UiContext<'a> {
  let space = UiSpace {
    size: Vec2::new(size.0, size.1),
    px_scale,
    image: (
      (size.0 * px_scale).ceil() as u32,
      (size.1 * px_scale).ceil() as u32,
    ),
    mouse_scale: 1.0,
  };
  context_in(commands, text, space, UiInput::default())
}

/// A context over `space` reading `input`, drawing scene 1 (the top scene)
/// with a fresh memory and layout engine of its own (leaked: tests only)
pub(crate) fn context_in<'a>(
  commands: &'a mut UiCommands,
  text: &'a mut TextEngine,
  space: UiSpace,
  input: UiInput,
) -> UiContext<'a> {
  let memory = Box::leak(Box::new(UiMemory::new()));
  let layout = Box::leak(Box::new(LayoutEngine::new()));
  let theme = Arc::new(Theme::dark());
  let mut ui = UiContext::new(commands, text, memory, layout, space, input, theme, 0);
  ui.begin_scene(1, true);
  ui
}

/// A prompt context for `device` with `actions` bound (Xbox labels, no
/// glyphs)
pub(crate) fn prompts_with(device: InputDevice, actions: &[(&str, Vec<Binding>)]) -> PromptContext {
  PromptContext {
    device,
    bindings: actions
      .iter()
      .map(|(name, bindings)| ((*name).to_string(), bindings.clone()))
      .collect(),
    ..PromptContext::default()
  }
}

/// A headless UI over several frames: a [`UiHarness`] with the crate's
/// extra scripting (keys, text, pad buttons, held directions, the stick, the
/// device, scenes) and reads of its memory, commands and outcomes. It derefs
/// to the harness, so `d.memory` and `d.commands` reach the harness's.
pub(crate) struct UiDriver {
  harness: UiHarness,
}

impl Deref for UiDriver {
  type Target = UiHarness;

  fn deref(&self) -> &UiHarness {
    &self.harness
  }
}

impl DerefMut for UiDriver {
  fn deref_mut(&mut self) -> &mut UiHarness {
    &mut self.harness
  }
}

impl UiDriver {
  /// A UI `size` UI units at scale 1, the pointer at the origin
  pub(crate) fn new(size: (f32, f32)) -> UiDriver {
    UiDriver {
      harness: UiHarness::new(size.0, size.1),
    }
  }

  /// Prompts read `prompts` from the next frame
  pub(crate) fn prompts(mut self, prompts: PromptContext) -> UiDriver {
    self.harness.prompts = Arc::new(prompts);
    self
  }

  /// Press a pad `button` in the next frame (and release it after)
  pub(crate) fn pad(&mut self, button: GamepadButton) {
    self.harness.pads.push(button);
  }

  /// The presses rebinding rows captured in the last frame, whole
  pub(crate) fn rebinds(&self) -> &[PendingRebind] {
    &self.harness.rebinds
  }

  /// The presses rebinding rows captured in the last frame, as (action,
  /// slot, binding)
  pub(crate) fn pending_rebinds(&self) -> Vec<(String, usize, Binding)> {
    self
      .harness
      .rebinds
      .iter()
      .map(|r| (r.action.clone(), r.slot, r.binding))
      .collect()
  }

  /// Draw with `theme` (and take its interaction timings) from the next frame
  pub(crate) fn with_theme(self, theme: Theme) -> UiDriver {
    UiDriver {
      harness: self.harness.with_theme(theme),
    }
  }

  /// Move the pointer to (x, y) in UI units
  pub(crate) fn move_to(&mut self, x: f32, y: f32) {
    self.harness.move_to(x, y);
  }

  /// Press the left button in the next frame; it stays down until `release`
  pub(crate) fn press(&mut self) {
    self.harness.press();
  }

  /// Release the left button in the next frame
  pub(crate) fn release(&mut self) {
    self.harness.release();
  }

  /// A wheel step of (dx, dy) lines in the next frame, routed without Shift
  pub(crate) fn wheel(&mut self, dx: f32, dy: f32) {
    self.harness.wheel(dx, dy);
  }

  /// Press `key` in the next frame (and release it after)
  pub(crate) fn key(&mut self, key: KeyCode) {
    self.harness.keys.push(key);
  }

  /// Show `text` as the IME's composition in the next frame
  pub(crate) fn preedit(&mut self, text: &str) {
    self.harness.preedit = text.to_string();
  }

  /// Hold Shift from the next frame (`true`) or let it go
  pub(crate) fn shift(&mut self, held: bool) {
    self.harness.shift = held;
  }

  /// Press `direction` for the next frame (and release it after)
  pub(crate) fn nav(&mut self, direction: Direction) {
    self.harness.press_direction(direction);
  }

  /// Press `direction` in the next frame and hold it until `release_nav`
  pub(crate) fn hold(&mut self, direction: Direction) {
    self.nav(direction);
    self.harness.holding[direction_index(direction)] = true;
  }

  /// Let go of every held direction
  pub(crate) fn release_nav(&mut self) {
    let h = &mut self.harness;
    h.holding = [false; 4];
    h.nav.pressed = [false; 4];
    h.nav.held = [false; 4];
  }

  /// Press Confirm in the next frame
  pub(crate) fn confirm(&mut self) {
    self.harness.nav.confirm = true;
  }

  /// Press Back in the next frame
  pub(crate) fn back(&mut self) {
    self.harness.nav.back = true;
  }

  /// Press Tab in the next frame, with Shift when `shift`
  pub(crate) fn tab(&mut self, shift: bool) {
    self.harness.nav.next = true;
    self.harness.nav.shift = shift;
  }

  /// Press `ui.tab_next` (E, the right shoulder) in the next frame
  pub(crate) fn tab_next(&mut self) {
    self.harness.nav.tab_next = true;
  }

  /// Hold the right stick at `at` (+y up) until it is moved again
  pub(crate) fn stick(&mut self, at: Vec2) {
    self.harness.nav.right_stick = at;
  }

  /// The player's last device, from the next frame on
  pub(crate) fn device(&mut self, device: InputDevice) {
    self.harness.nav.device = device;
  }

  /// Run the next frames as scene `scene` (its scope id; 1 by default), the
  /// top scene
  pub(crate) fn set_scene(&mut self, scene: u64) {
    self.harness.scene = scene;
  }

  /// The sounds the last frame asked for
  pub(crate) fn last_sounds(&self) -> Vec<UiSound> {
    self.harness.memory.interact.sounds.clone()
  }

  /// What the navigation took of the input in the last frame that had any
  /// (a direction pressed or held, Confirm, Back, Tab or a tab switch); a
  /// frame without navigation input leaves it
  pub(crate) fn last_outcome(&self) -> &NavOutcome {
    &self.harness.outcome
  }

  /// One frame: resolve the scripted pointer, the wheel and navigation, run
  /// `ui` as the top scene (scope 1, or `set_scene`'s), 1/60 s after the
  /// last, reading the scripted keys, text and pointer. A frame that draws
  /// nothing leaves the records and widget state of the last frame that did.
  pub(crate) fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R {
    self.harness.frame(ui)
  }
}
