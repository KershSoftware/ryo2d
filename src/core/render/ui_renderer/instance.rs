//! One UI draw item on the GPU: a quad (rect, glyph, sprite) with today's
//! sprite-instance lanes, or two triangles of a line's piece with the
//! points they are built from

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::core::graphics::SpriteInstance;
use crate::core::render::PixelRect;
use crate::core::render::batch::PieceKind;
use crate::core::ui::Color;

/// What an instance draws, and which fragment body draws it (`ui.wgsl`'s
/// `KIND_` constants)
pub(crate) struct UiKind;

impl UiKind {
  pub(crate) const RECT: u32 = 0;
  pub(crate) const MASK: u32 = 1;
  pub(crate) const COLOR: u32 = 2;
  pub(crate) const MSDF: u32 = 3;
  pub(crate) const SPRITE: u32 = 4;
  /// A sprite of a render-target page (linear, premultiplied)
  pub(crate) const SPRITE_LINEAR: u32 = 5;
  pub(crate) const LINE: u32 = 6;
}

/// Sample with the nearest sampler (`ui.wgsl`'s `FLAG_NEAREST`)
const NEAREST: u32 = 1;

/// Its clip is its own, relative to its slot: the shader moves it by the
/// slot's offset and meets it with the slot's clip (`ui.wgsl`'s
/// `FLAG_OWN_CLIP`)
const OWN_CLIP: u32 = 2;

/// Where an instance's scroll slot sits in its flags word, below the glyph
/// page: 8 bits (`ui.wgsl`'s `SLOT_SHIFT`)
const SLOT_SHIFT: u32 = 8;
const SLOT_BITS: u32 = 0xFF << SLOT_SHIFT;

/// A clip rect in UI image pixels: pixels with centres in [x0, x1) x [y0, y1)
/// are drawn (the scissor test)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipRect {
  pub(crate) x0: u16,
  pub(crate) y0: u16,
  pub(crate) x1: u16,
  pub(crate) y1: u16,
}

impl ClipRect {
  /// `scissor` (as `scissor_of` makes it), or the whole image
  pub(crate) fn of(scissor: Option<PixelRect>, image: (u32, u32)) -> ClipRect {
    let narrow = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
    match scissor {
      Some(rect) => ClipRect {
        x0: narrow(rect.x),
        y0: narrow(rect.y),
        x1: narrow(rect.x.saturating_add(rect.width)),
        y1: narrow(rect.y.saturating_add(rect.height)),
      },
      None => ClipRect {
        x0: 0,
        y0: 0,
        x1: narrow(image.0),
        y1: narrow(image.1),
      },
    }
  }

  /// This clip, relative to a slot, where the slot draws it: moved by the
  /// slot's `offset` (whole pixels) and met with the slot's clip, as the
  /// shader moves an instance's own clip (`ui.wgsl`'s `vs_ui`)
  pub(crate) fn in_slot(self, offset: [i32; 2], slot: ClipRect) -> ClipRect {
    let at = |v: u16, by: i32| {
      let moved = i64::from(v) + i64::from(by);
      u16::try_from(moved.max(0)).unwrap_or(u16::MAX)
    };
    ClipRect {
      x0: slot.x0.max(at(self.x0, offset[0])),
      y0: slot.y0.max(at(self.y0, offset[1])),
      x1: slot.x1.min(at(self.x1, offset[0])),
      y1: slot.y1.min(at(self.y1, offset[1])),
    }
  }

  /// Whether no pixel passes it
  pub(crate) fn is_empty(self) -> bool {
    self.x1 <= self.x0 || self.y1 <= self.y0
  }
}

/// Where a line instance's piece kind, pair and edge sit in its bits word,
/// above the window's present and seam bits (`ui.wgsl`'s line path)
const LINE_KIND_SHIFT: u32 = 12;
const LINE_PAIR_SHIFT: u32 = 14;
const LINE_EDGE_SHIFT: u32 = 18;

/// The points a line instance carries: the six about its piece's point
/// (slot 2 the point itself, slot k the point k - 2 along the line from
/// it), each its position before the line's offset and its `u`, and which slots hold a point (bit k) and
/// which is a closed line's first point, its seam (bit k)
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UiLineWindow {
  pub(crate) points: [(Vec2, f32); 6],
  pub(crate) present: u32,
  pub(crate) seam: u32,
}

/// What every piece of a UI line shares: the offset its points are moved
/// by (the GPU adds it, with a scroll slot's), one half width and one
/// colour (sRGB, straight alpha), the half length between a two-point
/// line's caps, its params, its miter limit and its edge (as the line
/// table numbers it, `edge_code`)
#[derive(Debug, Clone, Copy)]
pub(crate) struct UiLine {
  pub(crate) at: Vec2,
  pub(crate) half_width: f32,
  pub(crate) along_half: f32,
  pub(crate) color: Color,
  pub(crate) params: [f32; 4],
  pub(crate) miter_limit: f32,
  pub(crate) edge: u32,
}

/// A UI draw item: eight vec4 lanes (a quad's are a `SpriteInstance`'s, in
/// order; a line's are two triangles of one of its pieces with the points
/// they are built from), then the clip, kind and flags.
///
/// In a scroll slot (`slot()` above 0) its positions are relative to the
/// slot: the shader adds the slot's offset, and its clip is the slot's
/// (read from the slot; the instance's is not read, and is built as the
/// whole image), or its own relative to the slot met with the slot's
/// (`own_clip()`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct UiInstance {
  pub(crate) data: [[f32; 4]; 8],
  /// x: x0 | x1 << 16, y: y0 | y1 << 16, z: kind, w: flags | slot << 8 |
  /// page << 16
  pub(crate) meta: [u32; 4],
}

// A quad's eight lanes are a sprite instance, cast as it is
const _: () = assert!(std::mem::size_of::<SpriteInstance>() == 128);

impl UiInstance {
  /// A quad of `kind` drawn as `instance` (its transform, uv rect, tint,
  /// effect lanes and uv bounds), sampling glyph page `page` (glyph kinds)
  pub(crate) fn quad(
    kind: u32,
    instance: &SpriteInstance,
    clip: ClipRect,
    nearest: bool,
    page: u16,
  ) -> UiInstance {
    UiInstance {
      data: bytemuck::cast(*instance),
      meta: Self::meta(clip, kind, if nearest { NEAREST } else { 0 }, page),
    }
  }

  /// Two triangles of a line's piece (`pair` 0 the first two, 1 the next
  /// two), with the points the GPU builds them from (`line_pieces.wgsl`'s
  /// `line_piece_vertex`, as for a world line). Lanes: the window's six
  /// positions (a0-a2), their `u` (a3, a4.xy), the half width and a
  /// two-point line's half length between its caps (a4.zw), the line's
  /// params (a5), then its sRGB colour as two exact 16-bit halves, the miter
  /// limit and the window's bits with its edge (a6), and the offset added
  /// to every point (a7.xy)
  pub(crate) fn line_piece(
    window: &UiLineWindow,
    piece: PieceKind,
    pair: u32,
    line: &UiLine,
    clip: ClipRect,
  ) -> UiInstance {
    let [p0, p1, p2, p3, p4, p5] = window.points;
    let kind = match piece {
      PieceKind::Segment => 0,
      PieceKind::Join => 1,
      PieceKind::StartCap => 2,
      PieceKind::EndCap => 3,
    };
    let bits = (window.present & 0x3F)
      | (window.seam & 0x3F) << 6
      | kind << LINE_KIND_SHIFT
      | (pair & 0xF) << LINE_PAIR_SHIFT
      | (line.edge & 0x1) << LINE_EDGE_SHIFT;
    let Color { r, g, b, a } = line.color;
    let half = |low: u8, high: u8| f32::from(u16::from(low) | u16::from(high) << 8);
    UiInstance {
      data: [
        [p0.0.x, p0.0.y, p1.0.x, p1.0.y],
        [p2.0.x, p2.0.y, p3.0.x, p3.0.y],
        [p4.0.x, p4.0.y, p5.0.x, p5.0.y],
        [p0.1, p1.1, p2.1, p3.1],
        [p4.1, p5.1, line.half_width, line.along_half],
        line.params,
        // Below 2^24, so exact as f32
        [half(r, g), half(b, a), line.miter_limit, bits as f32],
        [line.at.x, line.at.y, 0.0, 0.0],
      ],
      meta: Self::meta(clip, UiKind::LINE, 0, 0),
    }
  }

  fn meta(clip: ClipRect, kind: u32, flags: u32, page: u16) -> [u32; 4] {
    [
      u32::from(clip.x0) | u32::from(clip.x1) << 16,
      u32::from(clip.y0) | u32::from(clip.y1) << 16,
      kind,
      flags | u32::from(page) << 16,
    ]
  }

  /// The same instance in scroll slot `slot` (0: none, its positions and
  /// clip absolute)
  pub(crate) fn with_slot(mut self, slot: u8) -> UiInstance {
    self.meta[3] = self.meta[3] & !SLOT_BITS | u32::from(slot) << SLOT_SHIFT;
    self
  }

  /// The same instance, its clip its own, relative to its slot
  pub(crate) fn with_own_clip(mut self) -> UiInstance {
    self.meta[3] |= OWN_CLIP;
    self
  }

  /// Its scroll slot (0: none)
  pub(crate) fn slot(&self) -> u8 {
    ((self.meta[3] & SLOT_BITS) >> SLOT_SHIFT) as u8
  }

  /// Whether its clip is its own, relative to its slot
  pub(crate) fn own_clip(&self) -> bool {
    self.meta[3] & OWN_CLIP != 0
  }

  pub(crate) fn kind(&self) -> u32 {
    self.meta[2]
  }

  pub(crate) fn page(&self) -> u16 {
    (self.meta[3] >> 16) as u16
  }

  pub(crate) fn clip(&self) -> ClipRect {
    ClipRect {
      x0: self.meta[0] as u16,
      x1: (self.meta[0] >> 16) as u16,
      y0: self.meta[1] as u16,
      y1: (self.meta[1] >> 16) as u16,
    }
  }
}
