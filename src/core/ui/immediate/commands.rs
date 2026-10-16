//! What `UiContext` records: drawn by the engine's UI pass through the 2D
//! batch, in submission order (popups last)

use std::sync::Arc;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::text::TextLayout;
use crate::core::ui::{Color, Rect};

/// One UI draw, in UI units
#[derive(Debug, Clone)]
pub(crate) enum UiCommand {
  Rect {
    rect: Rect,
    fill: Color,
    radius: f32,
    border: Option<(f32, Color)>,
    clip: Option<Rect>,
    layer: u8,
  },
  Sprite {
    sprite: SpriteHandle,
    rect: Rect,
    uv: [f32; 4],
    tint: Color,
    clip: Option<Rect>,
    layer: u8,
  },
  Line {
    points: Vec<Vec2>,
    width: f32,
    color: Color,
    closed: bool,
    clip: Option<Rect>,
    layer: u8,
  },
  Text {
    layout: Arc<TextLayout>,
    origin: Vec2,
    reveal: Option<(f32, f32)>,
    opacity: f32,
    /// Seeds the text's effects; made from the text alone, so it does not
    /// change with what else was drawn
    seed: u64,
    clip: Option<Rect>,
    layer: u8,
  },
}

impl UiCommand {
  /// Its draw layer (higher layers draw over lower ones)
  pub(crate) fn draw_layer(&self) -> u8 {
    match self {
      Self::Rect { layer, .. }
      | Self::Sprite { layer, .. }
      | Self::Line { layer, .. }
      | Self::Text { layer, .. } => *layer,
    }
  }

  /// The clip it was recorded under
  pub(crate) fn clip_rect(&self) -> Option<Rect> {
    match self {
      Self::Rect { clip, .. }
      | Self::Sprite { clip, .. }
      | Self::Line { clip, .. }
      | Self::Text { clip, .. } => *clip,
    }
  }
}

#[cfg(test)]
impl UiCommand {
  pub(crate) fn kind_name(&self) -> &'static str {
    match self {
      Self::Rect { .. } => "rect",
      Self::Sprite { .. } => "sprite",
      Self::Line { .. } => "line",
      Self::Text { .. } => "text",
    }
  }

  pub(crate) fn clip(&self) -> Option<Rect> {
    self.clip_rect()
  }

  pub(crate) fn text_origin(&self) -> Option<Vec2> {
    match self {
      Self::Text { origin, .. } => Some(*origin),
      _ => None,
    }
  }
}

/// The frame's UI draws (cleared each UI pass)
#[derive(Debug, Default)]
pub(crate) struct UiCommands {
  commands: Vec<UiCommand>,
}

impl UiCommands {
  pub(crate) fn clear(&mut self) {
    self.commands.clear();
  }
  pub(crate) fn push(&mut self, command: UiCommand) {
    self.commands.push(command);
  }
  pub(crate) fn is_empty(&self) -> bool {
    self.commands.is_empty()
  }
  pub(crate) fn iter(&self) -> impl Iterator<Item = &UiCommand> {
    self.commands.iter()
  }
}
