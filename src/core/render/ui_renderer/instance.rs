//! One UI draw item on the GPU: a quad (rect, glyph, sprite) with today's
//! sprite-instance lanes, or one triangle of a line with explicit corners

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::core::graphics::SpriteInstance;
use crate::core::render::PixelRect;

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
}

/// A UI draw item: eight vec4 lanes (a quad's are a `SpriteInstance`'s, in
/// order; a line triangle's are its corners), then the clip, kind and flags
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct UiInstance {
  pub(crate) data: [[f32; 4]; 8],
  /// x: x0 | x1 << 16, y: y0 | y1 << 16, z: kind, w: flags | page << 16
  pub(crate) meta: [u32; 4],
}

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

  /// One triangle of a line: each corner's position, uv and linear straight
  /// colour, as the line builder made them
  pub(crate) fn triangle(
    corners: [(Vec2, Vec2, [f32; 4]); 3],
    params: [f32; 4],
    clip: ClipRect,
  ) -> UiInstance {
    let [a, b, c] = corners;
    UiInstance {
      data: [
        [a.0.x, a.0.y, a.1.x, a.1.y],
        [b.0.x, b.0.y, b.1.x, b.1.y],
        [c.0.x, c.0.y, c.1.x, c.1.y],
        a.2,
        b.2,
        c.2,
        params,
        [0.0; 4],
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
