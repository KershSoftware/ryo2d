//! Recording UI contexts and a headless UI driver for tests (no GPU)

use std::sync::Arc;

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::core::input::{Direction, InputDevice, InputView};
use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{UiCommands, UiContext};
use crate::core::ui::interact::{
  PointerInput, apply_scroll_requests, resolve_pointer, route_wheel,
};
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::UiMemory;
use crate::core::ui::nav::{NavInput, NavOutcome, resolve_focus};
use crate::core::ui::theme::Theme;

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
  let mut ui = UiContext::new(commands, text, memory, layout, space, input, theme);
  ui.begin_scene(1, true);
  ui
}

/// How long each driven frame lasts (seconds)
const FRAME: f64 = 1.0 / 60.0;

/// A headless UI over several frames: it owns a memory, a layout engine, a
/// text engine and the commands, scripts the pointer and the navigation
/// actions and resolves them before each frame, as the engine does
pub(crate) struct UiDriver {
  pub(crate) memory: UiMemory,
  pub(crate) layout: LayoutEngine,
  pub(crate) text: TextEngine,
  /// The last frame's commands
  pub(crate) commands: UiCommands,
  size: (f32, f32),
  pointer: PointerInput,
  /// A wheel step for the next frame, in lines
  wheel: Vec2,
  /// The navigation input for the next frame
  nav: NavInput,
  /// Keys pressed in the next frame
  keys: Vec<KeyCode>,
  /// Text typed in the next frame
  typed: String,
  /// The IME's composition in the next frame
  preedit: String,
  /// Shift is held until `shift(false)`
  shift: bool,
  /// Directions held until `release_nav`
  holding: [bool; 4],
  /// The scene `frame` runs as the top scene
  scene: u64,
  /// The outcome of the last frame that had navigation input
  outcome: NavOutcome,
  time: f64,
  /// What the widgets and the interaction rules read
  theme: Arc<Theme>,
}

impl UiDriver {
  /// A UI `size` UI units at scale 1, the pointer at the origin
  pub(crate) fn new(size: (f32, f32)) -> UiDriver {
    UiDriver {
      memory: UiMemory::new(),
      layout: LayoutEngine::new(),
      text: text_engine(),
      commands: UiCommands::default(),
      size,
      pointer: PointerInput {
        position: Vec2::ZERO,
        pressed: [false; 2],
        released: [false; 2],
        down: [false; 2],
        time: 0.0,
        double_click: 0.3,
      },
      wheel: Vec2::ZERO,
      nav: NavInput::default(),
      keys: Vec::new(),
      typed: String::new(),
      preedit: String::new(),
      shift: false,
      holding: [false; 4],
      scene: 1,
      outcome: NavOutcome::default(),
      time: 0.0,
      theme: Arc::new(Theme::dark()),
    }
  }

  /// Draw with `theme` (and take its interaction timings) from the next frame
  pub(crate) fn with_theme(mut self, theme: Theme) -> UiDriver {
    self.theme = Arc::new(theme);
    self
  }

  /// Move the pointer to (x, y) in UI units
  pub(crate) fn move_to(&mut self, x: f32, y: f32) {
    self.pointer.position = Vec2::new(x, y);
  }

  /// Press the left button in the next frame; it stays down until `release`
  pub(crate) fn press(&mut self) {
    self.pointer.pressed[0] = true;
    self.pointer.down[0] = true;
  }

  /// Release the left button in the next frame
  pub(crate) fn release(&mut self) {
    self.pointer.released[0] = true;
    self.pointer.down[0] = false;
  }

  /// A wheel step of (dx, dy) lines in the next frame, routed without Shift
  pub(crate) fn wheel(&mut self, dx: f32, dy: f32) {
    self.wheel = Vec2::new(dx, dy);
  }

  /// Press `key` in the next frame (and release it after)
  pub(crate) fn key(&mut self, key: KeyCode) {
    self.keys.push(key);
  }

  /// Type `text` in the next frame
  pub(crate) fn type_text(&mut self, text: &str) {
    self.typed.push_str(text);
  }

  /// Show `text` as the IME's composition in the next frame
  pub(crate) fn preedit(&mut self, text: &str) {
    self.preedit = text.to_string();
  }

  /// Hold Shift from the next frame (`true`) or let it go
  pub(crate) fn shift(&mut self, held: bool) {
    self.shift = held;
  }

  /// Press `direction` for the next frame (and release it after)
  pub(crate) fn nav(&mut self, direction: Direction) {
    let k = direction_index(direction);
    self.nav.pressed[k] = true;
    self.nav.held[k] = true;
  }

  /// Press `direction` in the next frame and hold it until `release_nav`
  pub(crate) fn hold(&mut self, direction: Direction) {
    self.nav(direction);
    self.holding[direction_index(direction)] = true;
  }

  /// Let go of every held direction
  pub(crate) fn release_nav(&mut self) {
    self.holding = [false; 4];
    self.nav.pressed = [false; 4];
    self.nav.held = [false; 4];
  }

  /// Press Confirm in the next frame
  pub(crate) fn confirm(&mut self) {
    self.nav.confirm = true;
  }

  /// Press Back in the next frame
  pub(crate) fn back(&mut self) {
    self.nav.back = true;
  }

  /// Press Tab in the next frame, with Shift when `shift`
  pub(crate) fn tab(&mut self, shift: bool) {
    self.nav.next = true;
    self.nav.shift = shift;
  }

  /// Press `ui.tab_next` (E, the right shoulder) in the next frame
  pub(crate) fn tab_next(&mut self) {
    self.nav.tab_next = true;
  }

  /// Hold the right stick at `at` (+y up) until it is moved again
  pub(crate) fn stick(&mut self, at: Vec2) {
    self.nav.right_stick = at;
  }

  /// The player's last device, from the next frame on
  pub(crate) fn device(&mut self, device: InputDevice) {
    self.nav.device = device;
  }

  /// Run the next frames as scene `scene` (its scope id; 1 by default), the
  /// top scene
  pub(crate) fn set_scene(&mut self, scene: u64) {
    self.scene = scene;
  }

  /// What the navigation took of the input in the last frame that had any
  /// (a direction pressed or held, Confirm, Back, Tab or a tab switch); a
  /// frame without navigation input leaves it
  pub(crate) fn last_outcome(&self) -> &NavOutcome {
    &self.outcome
  }

  /// One frame: resolve the scripted pointer, the wheel and navigation, run
  /// `ui` as the top scene (scope 1, or `set_scene`'s), 1/60 s after the
  /// last, reading the scripted keys, text and pointer. A frame that draws
  /// nothing leaves the records and widget state of the last frame that did.
  pub(crate) fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R {
    self.time += FRAME;
    self.pointer.time = self.time;
    self.commands.clear();
    self.memory.begin_frame();
    self.layout.begin_frame();
    let scene = Some(self.scene);
    let metrics = self.theme.metrics.clone();
    self.pointer.double_click = f64::from(metrics.double_click);
    let pointer = resolve_pointer(
      &mut self.memory,
      &self.pointer,
      scene,
      &[],
      metrics.drag_threshold,
    );
    apply_scroll_requests(&mut self.memory);
    route_wheel(
      &mut self.memory,
      self.pointer.position,
      self.wheel,
      false,
      scene,
      metrics.scroll_line,
    );
    let mut nav = self.nav;
    for (held, holding) in nav.held.iter_mut().zip(self.holding) {
      *held |= holding;
    }
    nav.shift |= self.shift;
    nav.dt = FRAME as f32;
    nav.repeat_delay = metrics.repeat_delay;
    nav.repeat_rate = metrics.repeat_rate;
    let moved = self.memory.interact.pointer != self.memory.interact.prev_pointer;
    let outcome = resolve_focus(&mut self.memory, &nav, scene, moved);
    let scripted = nav.held.contains(&true)
      || nav.confirm
      || nav.back
      || nav.next
      || nav.tab_next
      || nav.tab_prev;
    if scripted {
      self.outcome = outcome;
    }
    let space = UiSpace {
      size: Vec2::new(self.size.0, self.size.1),
      px_scale: 1.0,
      image: (self.size.0.ceil() as u32, self.size.1.ceil() as u32),
      mouse_scale: 1.0,
    };
    // The keys, text and pointer as the UI reads them; a press over the UI
    // is the UI's, as in the engine
    let mut input = UiInput::from_view(self.view());
    for (button, consume) in [MouseButton::Left, MouseButton::Right]
      .into_iter()
      .zip(pointer.consume_press)
    {
      if consume {
        input.consume_mouse(button);
      }
    }
    let result = {
      let mut context = UiContext::new(
        &mut self.commands,
        &mut self.text,
        &mut self.memory,
        &mut self.layout,
        space,
        input,
        Arc::clone(&self.theme),
      );
      context.set_dt(FRAME as f32);
      context.begin_scene(self.scene, true);
      let result = ui(&mut context);
      context.end_scene();
      result
    };
    // A frame that drew nothing is a read of the UI's state (`scroll_offset`):
    // the UI stays as the last drawn frame left it
    if self.memory.drew_nothing() {
      self.memory.rewind_frame();
    } else {
      self.memory.end_frame();
      self.layout.end_frame();
    }
    // A press and a release last one frame, and so do navigation presses
    self.pointer.pressed = [false; 2];
    self.pointer.released = [false; 2];
    self.wheel = Vec2::ZERO;
    self.keys.clear();
    self.typed.clear();
    self.preedit.clear();
    self.nav = NavInput {
      right_stick: self.nav.right_stick,
      device: self.nav.device,
      ..NavInput::default()
    };
    result
  }
}

impl UiDriver {
  /// The next frame's scripted keys, text and pointer as the input sees them
  fn view(&self) -> InputView {
    let mut view = InputView {
      mouse: (self.pointer.position.x, self.pointer.position.y),
      text: self.typed.clone(),
      ime_preedit: self.preedit.clone(),
      ime_composing: !self.preedit.is_empty(),
      ..InputView::default()
    };
    for (k, button) in [MouseButton::Left, MouseButton::Right]
      .into_iter()
      .enumerate()
    {
      if self.pointer.down[k] {
        view.mouse_down.insert(button);
      }
      if self.pointer.pressed[k] {
        view.mouse_pressed.insert(button);
      }
      if self.pointer.released[k] {
        view.mouse_released.insert(button);
      }
    }
    for &key in &self.keys {
      view.keys_down.insert(key);
      view.keys_pressed.insert(key);
      view.keys_repeated.insert(key);
    }
    if self.shift {
      view.keys_down.insert(KeyCode::ShiftLeft);
    }
    view
  }
}

/// A direction's place in `NavInput`'s arrays
fn direction_index(direction: Direction) -> usize {
  match direction {
    Direction::Up => 0,
    Direction::Down => 1,
    Direction::Left => 2,
    Direction::Right => 3,
  }
}
