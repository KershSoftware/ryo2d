//! Recording UI contexts for tests (no GPU)

use glam::Vec2;

use crate::core::render::UiSpace;
use crate::core::text::{Fonts, TextEngine};
use crate::core::ui::UiInput;
use crate::core::ui::immediate::{UiCommands, UiContext};

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
  let space = UiSpace {
    size: Vec2::new(size.0, size.1),
    px_scale: 1.0,
    image: (size.0.ceil() as u32, size.1.ceil() as u32),
    mouse_scale: 1.0,
  };
  UiContext::new(commands, text, space, UiInput::default())
}
