//! Recording UI contexts for tests (no GPU)

use glam::Vec2;

use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{UiCommands, UiContext};
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
