//! What `UiContext` records: drawn by the engine's UI pass through the 2D
//! batch, by layer, then in the order the draws were recorded

use std::borrow::Cow;
use std::sync::Arc;

use glam::Vec2;

use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
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
    /// How the texture is sampled (`Filter::Default` for absolute draws)
    filter: Filter,
    clip: Option<Rect>,
    layer: u8,
  },
  Line {
    points: Arc<[Vec2]>,
    /// Added to every point where it is drawn: a node's local line is
    /// placed so, sharing its points with the draw it was made from
    at: Vec2,
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

  fn clip_mut(&mut self) -> &mut Option<Rect> {
    match self {
      Self::Rect { clip, .. }
      | Self::Sprite { clip, .. }
      | Self::Line { clip, .. }
      | Self::Text { clip, .. } => clip,
    }
  }

  /// The same draw moved by `by` (UI units): its rect, a line's offset, a
  /// text's origin and its own clip. A line's points and a text's layout are
  /// shared, not copied.
  pub(crate) fn moved(&self, by: Vec2) -> UiCommand {
    let shift = |rect: Rect| Rect::new(rect.x + by.x, rect.y + by.y, rect.width, rect.height);
    let mut command = match self {
      Self::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      } => Self::Rect {
        rect: shift(*rect),
        fill: *fill,
        radius: *radius,
        border: *border,
        clip: *clip,
        layer: *layer,
      },
      Self::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      } => Self::Sprite {
        sprite: sprite.clone(),
        rect: shift(*rect),
        uv: *uv,
        tint: *tint,
        filter: *filter,
        clip: *clip,
        layer: *layer,
      },
      Self::Line {
        points,
        at,
        width,
        color,
        closed,
        clip,
        layer,
      } => Self::Line {
        points: Arc::clone(points),
        at: *at + by,
        width: *width,
        color: *color,
        closed: *closed,
        clip: *clip,
        layer: *layer,
      },
      Self::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip,
        layer,
      } => Self::Text {
        layout: Arc::clone(layout),
        origin: *origin + by,
        reveal: *reveal,
        opacity: *opacity,
        seed: *seed,
        clip: *clip,
        layer: *layer,
      },
    };
    let clip = command.clip_mut();
    *clip = clip.map(shift);
    command
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

/// A scroll area's slot in a frame: what the area shows is placed relative
/// to it, so moving the slot moves its contents exactly
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollSlot {
  /// Where the contents' origin is drawn, in whole render pixels
  pub(crate) offset: [i32; 2],
  /// The clip the contents draw under (UI units, absolute)
  pub(crate) clip: Option<Rect>,
}

impl ScrollSlot {
  /// Slot 0: no scroll area, so positions and clips are absolute
  pub(crate) const NONE: ScrollSlot = ScrollSlot {
    offset: [0, 0],
    clip: None,
  };

  /// Its offset in UI units, at `px_scale` render pixels per UI unit
  pub(crate) fn offset_units(&self, px_scale: f32) -> Vec2 {
    Vec2::new(self.offset[0] as f32, self.offset[1] as f32) / px_scale
  }
}

/// One UI draw: its place in the painter's order, the slot its positions
/// and clip are relative to (0: none, positions absolute), and the command
#[derive(Debug, Clone)]
pub(crate) struct UiDraw {
  pub(crate) order: u32,
  pub(crate) slot: u16,
  pub(crate) command: UiCommand,
}

impl UiDraw {
  /// The command as it is drawn: one in a slot moved by the slot's offset
  /// (at `px_scale` render pixels per UI unit), its own clip moved with it
  /// and met with the slot's. `None` when its slot is not in `slots`.
  pub(crate) fn placed(&self, slots: &[ScrollSlot], px_scale: f32) -> Option<Cow<'_, UiCommand>> {
    if self.slot == 0 {
      return Some(Cow::Borrowed(&self.command));
    }
    let slot = slots.get(usize::from(self.slot))?;
    let mut command = self.command.moved(slot.offset_units(px_scale));
    let clip = command.clip_mut();
    *clip = meet(slot.clip, *clip);
    Some(Cow::Owned(command))
  }
}

/// The overlap of two rects (empty when they do not meet)
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
  let (x, y) = (a.x.max(b.x), a.y.max(b.y));
  let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
  Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Two clips together: their overlap, either one alone, or none
pub(crate) fn meet(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
  match (a, b) {
    (Some(a), Some(b)) => Some(intersect(a, b)),
    (a, b) => a.or(b),
  }
}

/// The frame's UI draws (cleared each UI pass), each with its place in the
/// painter's order and its slot, and the frame's slot table. A container's
/// draws are pushed when it is laid out, after absolute draws recorded
/// later, so the order (taken when a draw was recorded), not the position in
/// the list, is what orders them.
#[derive(Debug)]
pub(crate) struct UiCommands {
  draws: Vec<UiDraw>,
  /// Slot 0 (no scroll area), then each scroll area's, in the order they
  /// were laid out
  slots: Vec<ScrollSlot>,
}

impl Default for UiCommands {
  fn default() -> UiCommands {
    UiCommands {
      draws: Vec::new(),
      slots: vec![ScrollSlot::NONE],
    }
  }
}

impl UiCommands {
  /// Each draw, in the order they were pushed
  pub(crate) fn as_slice(&self) -> &[UiDraw] {
    &self.draws
  }
  /// The slot table: slot 0, then the scroll areas'
  pub(crate) fn slots(&self) -> &[ScrollSlot] {
    &self.slots
  }
  pub(crate) fn clear(&mut self) {
    self.draws.clear();
    self.slots.clear();
    self.slots.push(ScrollSlot::NONE);
  }
  pub(crate) fn push(&mut self, order: u32, slot: u16, command: UiCommand) {
    self.draws.push(UiDraw {
      order,
      slot,
      command,
    });
  }
  /// Add a scroll area's slot: its index, or `u16::MAX` when the table is
  /// full (its contents are then placed absolute)
  pub(crate) fn push_slot(&mut self, slot: ScrollSlot) -> u16 {
    let Ok(index) = u16::try_from(self.slots.len()) else {
      return u16::MAX;
    };
    if index == u16::MAX {
      return u16::MAX;
    }
    self.slots.push(slot);
    index
  }
  pub(crate) fn is_empty(&self) -> bool {
    self.draws.is_empty()
  }
  /// How many commands there are (the UI harness)
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn len(&self) -> usize {
    self.draws.len()
  }
}

#[cfg(test)]
impl UiCommands {
  /// Each draw, in the order they were pushed
  pub(crate) fn iter(&self) -> impl Iterator<Item = &UiDraw> {
    self.draws.iter()
  }
  /// The commands as stored (relative to their slots), in the order they
  /// were pushed
  pub(crate) fn commands(&self) -> impl Iterator<Item = &UiCommand> {
    self.draws.iter().map(|draw| &draw.command)
  }
  /// The commands where they are drawn at `px_scale` render pixels per UI
  /// unit (each moved by its slot and under its slot's clip), in the order
  /// they were pushed
  pub(crate) fn placed(&self, px_scale: f32) -> Vec<UiCommand> {
    self
      .draws
      .iter()
      .filter_map(|draw| draw.placed(&self.slots, px_scale))
      .map(Cow::into_owned)
      .collect()
  }
}
