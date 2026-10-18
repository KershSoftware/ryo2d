//! A UI with no window or GPU, driven frame by frame over scripted input

use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::core::asset::AssetManager;
use crate::core::ecs::kernel::{Entity, Message, Messages, World};
use crate::core::error::AssetResult;
use crate::core::input::{Direction, GamepadButton, InputView};
use crate::core::messages::{register_message, update_messages};
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
use crate::core::ui::{
  BindValue, Theme, UiContext, UiEvent, UiFileError, UiId, UiInput, UiNode, UiReloaded, nodes,
};
#[cfg(test)]
use crate::core::ui::{
  memory::Record,
  widgets::list::{ListState, item_id, state_id},
  widgets::scroll::ScrollState,
};

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
  /// The entities of spawned UI nodes, and the asset manager UI files load
  /// through
  world: World,
  /// The query `reload_trees` keeps over the trees built from UI files
  sources: nodes::SourceQuery,
  /// The last frame's node events
  last_events: Vec<UiEvent>,
}

impl UiHarness {
  /// A UI `width` by `height` UI units with the built-in fonts, at scale 1,
  /// in the dark theme, the pointer at the origin, loading files relative to
  /// the working directory
  pub fn new(width: f32, height: f32) -> UiHarness {
    let mut text = TextEngine::new();
    text.sync(&Fonts::with_builtins());
    let mut world = World::new();
    register_message::<UiEvent>(&mut world);
    register_message::<UiReloaded>(&mut world);
    world.insert_resource(AssetManager::new());
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
      sources: Default::default(),
      last_events: Vec::new(),
    }
  }

  /// Spawn a UI node tree as a root of the harness's scene
  pub fn spawn_ui(&mut self, tree: UiNode) -> Entity {
    nodes::spawn_ui(&mut self.world, self.scene, tree)
  }

  /// Bring a spawned tree in line with `tree` in place, as `ctx.update_ui`
  pub fn update_ui(&mut self, node: Entity, tree: UiNode) -> Entity {
    nodes::update_ui(&mut self.world, node, tree)
  }

  /// Load UI files (and their images) relative to `dir`
  pub fn with_assets(mut self, dir: impl Into<PathBuf>) -> UiHarness {
    self.assets().set_base_path(dir.into());
    self
  }

  /// The asset manager the harness loads UI files through
  pub fn assets(&mut self) -> &mut AssetManager {
    self
      .world
      .get_resource_or_insert_with(AssetManager::new)
      .into_inner()
  }

  /// Load a UI file, build its root and spawn it, as `ctx.spawn_ui_file`
  pub fn spawn_ui_file(&mut self, path: &str) -> Result<Entity, UiFileError> {
    let file = self.assets().load_ui(path)?;
    let tree = file.root(self.assets())?;
    Ok(self.spawn_ui(tree))
  }

  /// Read a loaded UI file from disk again, as `ctx.assets.reload_ui`: the
  /// next frame brings the trees built from it in line before drawing
  pub fn reload_ui(&mut self, path: &str) -> AssetResult<()> {
    self.assets().reload_ui(path)
  }

  /// The `M` messages written in the last frame, `UiReloaded` among them
  pub fn messages<M: Message + Clone>(&self) -> impl Iterator<Item = M> {
    self
      .world
      .get_resource::<Messages<M>>()
      .into_iter()
      .flat_map(|messages| messages.iter_current_update_messages().cloned())
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

  /// Set a named binding on the tree `node` belongs to, as `ctx.ui_set`
  pub fn ui_set(&mut self, node: Entity, name: &str, value: impl Into<BindValue>) {
    nodes::bindings::set(&mut self.world, node, name, value.into());
  }

  /// A named binding's value on the tree `node` belongs to, as `ctx.ui_get`
  pub fn ui_get<T: TryFrom<BindValue>>(&self, node: Entity, name: &str) -> Option<T> {
    nodes::bindings::get(&self.world, node, name).and_then(|value| T::try_from(value).ok())
  }

  /// The events the node trees sent in the last frame, in walk order
  pub fn events(&self) -> &[UiEvent] {
    &self.last_events
  }

  /// A node's id once it has been drawn: what `ctx.ui_id` returns
  pub fn ui_id(&self, node: Entity) -> Option<UiId> {
    self
      .world
      .get::<nodes::UiInteraction>(node)
      .and_then(nodes::UiInteraction::id)
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

  /// What the last frame did to the layout's tree
  #[cfg(test)]
  pub(crate) fn layout_stats(&self) -> crate::core::ui::layout::engine::LayoutStats {
    self.layout.stats()
  }

  /// Where the scroll area `scroll` shows its contents from after the last
  /// frame, as `ui.scroll_offset` (zero for an area never drawn)
  #[cfg(test)]
  pub(crate) fn scroll_offset(&self, scroll: UiId) -> Vec2 {
    self
      .memory
      .peek_state::<ScrollState>(scroll)
      .map_or(Vec2::ZERO, |state| state.offset)
  }

  /// The size of the scroll area `scroll`'s contents as the last frame laid
  /// them out: never less than its view, unless it was given its height (a
  /// list's is its length)
  #[cfg(test)]
  pub(crate) fn scroll_content(&self, scroll: UiId) -> Vec2 {
    self
      .memory
      .peek_state::<ScrollState>(scroll)
      .expect("a scroll area drawn in the last frame")
      .content
  }

  /// The last frame's record of item `key`'s container in the list `list`
  #[cfg(test)]
  pub(crate) fn record_of_key(&self, list: UiId, key: usize) -> Option<Record> {
    let columns = self.memory.peek_state::<ListState>(state_id(list))?.columns;
    self.memory.drawn(item_id(list, columns, key)).copied()
  }

  /// What the list `list` remembers after the last frame (`None` for a
  /// list never drawn)
  #[cfg(test)]
  pub(crate) fn list_state(&self, list: UiId) -> Option<ListState> {
    self.memory.peek_state::<ListState>(state_id(list)).cloned()
  }

  /// The last frame's record of the widget or container `id`
  #[cfg(test)]
  pub(crate) fn record_of(&self, id: UiId) -> Option<Record> {
    self.memory.drawn(id).copied()
  }

  /// Press `direction` for the next frame (and release it after)
  pub(crate) fn press_direction(&mut self, direction: Direction) {
    let k = direction_index(direction);
    self.nav.pressed[k] = true;
    self.nav.held[k] = true;
  }

  /// One frame at 60 fps: resolve the scripted input (the pointer, the wheel
  /// and navigation) against the last frame, run `ui` as the top scene,
  /// reading the scripted keys, text and pointer, and finish the frame. The
  /// harness scene's node trees draw before `ui`, as a scene's do before its
  /// `ui`.
  pub fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R {
    self.frame_with_world(|context, _| ui(context))
  }

  /// [`frame`](Self::frame), with `ui` given the world the node trees live
  /// in too. The harness scene's node trees draw before `ui`, as a scene's
  /// do before its `ui`.
  pub fn frame_with_world<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>, &mut World) -> R) -> R {
    update_messages(&mut self.world);
    nodes::reload_trees(&mut self.world, &mut self.sources);
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
      nodes::begin_pass(&mut self.world);
      let events = nodes::draw_nodes(&mut self.world, &mut context, self.scene);
      self.last_events.clone_from(&events);
      nodes::deliver(&mut self.world, events);
      let result = ui(&mut context, &mut self.world);
      let late = nodes::deliver_pending(&mut self.world);
      self.last_events.extend(late);
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
