//! A UI with no window or GPU, driven frame by frame over scripted input

use std::sync::Arc;

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::core::ecs::kernel::{Entity, World};
use crate::core::input::{Direction, GamepadButton, InputView};
use crate::core::messages::register_message;
use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::immediate::{PendingRebind, PromptContext, UiCommands};
use crate::core::ui::interact::{
  PointerInput, apply_scroll_requests, resolve_pointer, route_wheel,
};
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::UiMemory;
use crate::core::ui::nav::{NavInput, NavOutcome, resolve_focus};
use crate::core::ui::theme::warn_once;
use crate::core::ui::{Theme, UiContext, UiEvent, UiId, UiInput, UiNode, nodes};

/// How long each frame lasts (seconds)
const FRAME: f64 = 1.0 / 60.0;

/// A UI with no window or GPU: memory, layout, text and the resolve step over
/// scripted input, one call per frame. For unit-testing game UI and for
/// benchmarks.
///
/// Each [`frame`](UiHarness::frame) resolves the input scripted since the
/// last one against the last frame's widgets, as the engine does before the
/// scenes' `ui` hooks, then runs the closure as the top scene's `ui`. A press
/// lands on what the last frame drew, so draw a frame before scripting input
/// for it. A frame that draws nothing (a read such as `ui.focused()`) leaves
/// the UI as the last frame that drew left it.
///
/// ```
/// use ryo2d::prelude::*;
/// use ryo2d::testing::UiHarness;
///
/// fn menu(ui: &mut UiContext<'_>) -> bool {
///   let mut quit = false;
///   ui.column().focus_scope().show(|ui| {
///     ui.button("Play").default_focus();
///     quit = ui.button("Quit").clicked();
///   });
///   quit
/// }
///
/// let mut ui = UiHarness::new(320.0, 240.0);
/// ui.frame(menu); // drawn once, so the next frame can resolve against it
/// ui.frame(menu); // the scope takes focus on its default
/// ui.action("ui.down");
/// ui.frame(menu);
/// ui.action("ui.confirm");
/// assert!(ui.frame(menu));
/// ```
pub struct UiHarness {
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
  pub(crate) nav: NavInput,
  /// Keys pressed in the next frame
  pub(crate) keys: Vec<KeyCode>,
  /// Pad buttons pressed in the next frame
  pub(crate) pads: Vec<GamepadButton>,
  /// Text typed in the next frame
  pub(crate) typed: String,
  /// The IME's composition in the next frame
  pub(crate) preedit: String,
  /// Shift is held until it is let go
  pub(crate) shift: bool,
  /// Directions held until they are let go
  pub(crate) holding: [bool; 4],
  /// The scene `frame` runs as the top scene
  pub(crate) scene: u64,
  /// The outcome of the last frame that had navigation input
  pub(crate) outcome: NavOutcome,
  time: f64,
  /// What the widgets and the interaction rules read
  theme: Arc<Theme>,
  /// What prompts read
  pub(crate) prompts: Arc<PromptContext>,
  /// The presses rebinding rows captured in the last frame
  pub(crate) rebinds: Vec<PendingRebind>,
  /// The entities of spawned UI nodes
  world: World,
}

impl UiHarness {
  /// A UI `width` by `height` UI units with the built-in fonts, at scale 1,
  /// in the dark theme, the pointer at the origin
  pub fn new(width: f32, height: f32) -> UiHarness {
    let mut text = TextEngine::new();
    text.sync(&Fonts::with_builtins());
    let mut world = World::new();
    register_message::<UiEvent>(&mut world);
    UiHarness {
      memory: UiMemory::new(),
      layout: LayoutEngine::new(),
      text,
      commands: UiCommands::default(),
      size: (width, height),
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
      pads: Vec::new(),
      typed: String::new(),
      preedit: String::new(),
      shift: false,
      holding: [false; 4],
      scene: 1,
      outcome: NavOutcome::default(),
      time: 0.0,
      theme: Arc::new(Theme::dark()),
      prompts: Arc::new(PromptContext::default()),
      rebinds: Vec::new(),
      world,
    }
  }

  /// Spawn a UI node tree as a root of the harness's scene
  pub fn spawn_ui(&mut self, tree: UiNode) -> Entity {
    nodes::spawn_ui(&mut self.world, self.scene, tree)
  }

  /// The world UI nodes live in
  pub fn world(&self) -> &World {
    &self.world
  }

  /// The world UI nodes live in, to change
  pub fn world_mut(&mut self) -> &mut World {
    &mut self.world
  }

  /// The first node named `name` in the harness's UI trees
  pub fn find_ui(&self, name: &str) -> Option<Entity> {
    nodes::find_ui(&self.world, self.scene, name)
  }

  /// Draw with `theme` (and take its interaction timings) from the next frame
  pub fn with_theme(mut self, theme: Theme) -> UiHarness {
    self.theme = Arc::new(theme);
    self
  }

  /// Move the pointer to (x, y) in UI units
  pub fn move_to(&mut self, x: f32, y: f32) {
    self.pointer.position = Vec2::new(x, y);
  }

  /// Press the left button in the next frame; it stays down until `release`
  pub fn press(&mut self) {
    self.pointer.pressed[0] = true;
    self.pointer.down[0] = true;
  }

  /// Release the left button in the next frame
  pub fn release(&mut self) {
    self.pointer.released[0] = true;
    self.pointer.down[0] = false;
  }

  /// A wheel step of (dx, dy) lines in the next frame (+y scrolls up),
  /// routed without Shift
  pub fn wheel(&mut self, dx: f32, dy: f32) {
    self.wheel = Vec2::new(dx, dy);
  }

  /// Press a UI action in the next frame (and release it after): `"ui.up"`,
  /// `"ui.down"`, `"ui.left"`, `"ui.right"`, `"ui.confirm"`, `"ui.back"`,
  /// `"ui.next"` (Tab), `"ui.tab_next"` or `"ui.tab_prev"`. Another name
  /// warns once and does nothing.
  pub fn action(&mut self, name: &str) {
    match name {
      "ui.up" => self.press_direction(Direction::Up),
      "ui.down" => self.press_direction(Direction::Down),
      "ui.left" => self.press_direction(Direction::Left),
      "ui.right" => self.press_direction(Direction::Right),
      "ui.confirm" => self.nav.confirm = true,
      "ui.back" => self.nav.back = true,
      "ui.next" => self.nav.next = true,
      "ui.tab_next" => self.nav.tab_next = true,
      "ui.tab_prev" => self.nav.tab_prev = true,
      _ => warn_once(format!("harness-action:{name}"), || {
        format!("UiHarness::action: \"{name}\" is not a UI action; ignored")
      }),
    }
  }

  /// The widget with the keyboard and gamepad focus after the last frame
  pub fn focused(&self) -> Option<UiId> {
    let focused = self.memory.interact.focused;
    (focused != UiId::NONE).then_some(focused)
  }

  /// The UI commands the last frame recorded
  pub fn command_count(&self) -> usize {
    self.commands.len()
  }

  /// Press `direction` for the next frame (and release it after)
  pub(crate) fn press_direction(&mut self, direction: Direction) {
    let k = direction_index(direction);
    self.nav.pressed[k] = true;
    self.nav.held[k] = true;
  }

  /// One frame at 60 fps: resolve the scripted input (the pointer, the wheel
  /// and navigation) against the last frame, run `ui` as the top scene,
  /// reading the scripted keys, text and pointer, and finish the frame
  pub fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R {
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
      context.set_prompts(Arc::clone(&self.prompts));
      context.begin_scene(self.scene, true);
      let result = ui(&mut context);
      context.end_scene();
      self.rebinds = context.take_rebinds();
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
    self.text.end_frame();
    // A press and a release last one frame, and so do navigation presses
    self.pointer.pressed = [false; 2];
    self.pointer.released = [false; 2];
    self.wheel = Vec2::ZERO;
    self.keys.clear();
    self.pads.clear();
    self.typed.clear();
    self.preedit.clear();
    self.nav = NavInput {
      right_stick: self.nav.right_stick,
      device: self.nav.device,
      ..NavInput::default()
    };
    result
  }

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
    view.gamepad_pressed.extend(self.pads.iter().copied());
    view
  }
}

/// A direction's place in `NavInput`'s arrays
pub(crate) fn direction_index(direction: Direction) -> usize {
  match direction {
    Direction::Up => 0,
    Direction::Down => 1,
    Direction::Left => 2,
    Direction::Right => 3,
  }
}
