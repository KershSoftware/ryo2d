//! Recording UI contexts and a headless UI driver for tests (no GPU)

use glam::Vec2;

use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{UiCommands, UiContext};
use crate::core::ui::interact::{PointerInput, resolve_pointer};
use crate::core::ui::layout::engine::LayoutEngine;
use crate::core::ui::memory::UiMemory;

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
  let mut ui = UiContext::new(commands, text, memory, layout, space, input);
  ui.begin_scene(1, true);
  ui
}

/// A headless UI over several frames: it owns a memory, a layout engine, a
/// text engine and the commands, scripts the pointer and resolves it before
/// each frame, as the engine does
pub(crate) struct UiDriver {
  pub(crate) memory: UiMemory,
  pub(crate) layout: LayoutEngine,
  pub(crate) text: TextEngine,
  /// The last frame's commands
  pub(crate) commands: UiCommands,
  size: (f32, f32),
  pointer: PointerInput,
  time: f64,
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
      },
      time: 0.0,
    }
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

  /// One frame: resolve the scripted pointer, run `ui` as the top scene (scope 1)
  pub(crate) fn frame<R>(&mut self, ui: impl FnOnce(&mut UiContext<'_>) -> R) -> R {
    self.time += 1.0 / 60.0;
    self.pointer.time = self.time;
    self.commands.clear();
    self.memory.begin_frame();
    self.layout.begin_frame();
    resolve_pointer(&mut self.memory, &self.pointer, Some(1), &[], 4.0);
    let space = UiSpace {
      size: Vec2::new(self.size.0, self.size.1),
      px_scale: 1.0,
      image: (self.size.0.ceil() as u32, self.size.1.ceil() as u32),
      mouse_scale: 1.0,
    };
    let result = {
      let mut context = UiContext::new(
        &mut self.commands,
        &mut self.text,
        &mut self.memory,
        &mut self.layout,
        space,
        UiInput::default(),
      );
      context.begin_scene(1, true);
      let result = ui(&mut context);
      context.end_scene();
      result
    };
    self.memory.end_frame();
    self.layout.end_frame();
    // A press and a release last one frame
    self.pointer.pressed = [false; 2];
    self.pointer.released = [false; 2];
    result
  }
}
