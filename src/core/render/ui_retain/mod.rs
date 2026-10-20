#![cfg_attr(not(test), allow(dead_code))] // wired into the frame in the next task

//! UI render retention: a frame whose UI draws exactly what the UI image
//! holds keeps the image, building, uploading and drawing no UI (see the
//! module notes, "UI render retention")

use std::hash::Hasher;
use std::sync::Arc;

use crate::core::hash::FastHasher;
use crate::core::render::UiSpace;
use crate::core::text::{Icon, TextIcons};
use crate::core::ui::Rect;
use crate::core::ui::immediate::{UiCommand, UiCommands};

#[cfg(test)]
mod tests;

/// What the UI's render step did in a frame
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiRenderStats {
  /// The UI image was kept from the last frame: no UI was built, uploaded
  /// or drawn
  pub kept: bool,
  /// Texts whose glyphs were built
  pub texts_built: u32,
  /// Texts drawn from glyphs built in an earlier frame
  pub texts_reused: u32,
  /// Glyph pages cleared to make room since the renderer started
  pub glyph_evictions: u64,
}

/// What a frame's UI pixels depend on besides its commands
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiRenderEnv {
  pub(crate) space: UiSpace,
  /// Rects snap to whole pixels of the pixel-art target
  pub(crate) on_target: bool,
  /// Text is drawn (the game kept the text resources)
  pub(crate) text: bool,
  /// The text engine's font generation (faces and render modes)
  pub(crate) fonts: u64,
}

/// Whether the UI image can be shown again as it is, and what it holds
#[derive(Debug, Default)]
pub(crate) struct UiRetain {
  /// The commands the UI image holds or is being drawn with: copies, so the
  /// layouts and points they share stay alive and no address in them is
  /// reused while compared
  drawn: Vec<(u32, UiCommand)>,
  /// Each drawn command's stamp (`stamp_of`)
  stamps: Vec<u64>,
  env: Option<UiRenderEnv>,
  /// The epoch of the UI image `drawn` is in, once a presented frame's `ui`
  /// pass drew it
  image: Option<u64>,
  /// The last frame's
  #[allow(dead_code)] // read by the frame in the next task
  pub(crate) stats: UiRenderStats,
}

impl UiRetain {
  /// Whether this frame shows the UI image as it is: its commands draw what
  /// the image holds (the same commands in the same order, the same sprite
  /// and icon versions, none time-driven), in the same environment, and
  /// the image (epoch `image`) is the one they were drawn into. When not,
  /// the commands become what the image is to hold, once drawn.
  pub(crate) fn keep(
    &mut self,
    commands: &UiCommands,
    icons: Option<&TextIcons>,
    env: UiRenderEnv,
    image: u64,
  ) -> bool {
    let now = commands.as_slice();
    let same = !now.is_empty()
      && self.image == Some(image)
      && self.env == Some(env)
      && now.len() == self.drawn.len()
      && now.iter().zip(&self.drawn).zip(&self.stamps).all(
        |(((order, command), (was_order, was)), stamp)| {
          order == was_order
            && !animated(command)
            && same(command, was)
            && stamp_of(command, icons) == *stamp
        },
      );
    if same {
      return true;
    }
    self.drawn.clear();
    self.drawn.extend_from_slice(now);
    self.stamps.clear();
    self
      .stamps
      .extend(now.iter().map(|(_, command)| stamp_of(command, icons)));
    self.env = Some(env);
    self.image = None;
    false
  }

  /// The frame was presented. Unless it kept the image, the image now holds
  /// its commands if its `ui` pass drew them (`drew`) into the image of
  /// epoch `image`, and is unknown otherwise.
  pub(crate) fn presented(&mut self, kept: bool, drew: bool, image: u64) {
    if !kept {
      self.image = drew.then_some(image);
    }
  }

  /// The frame was not presented: what the image holds is unknown
  pub(crate) fn lost(&mut self) {
    self.image = None;
  }

  /// Forget the image (retention was turned off)
  #[allow(dead_code)] // called by the frame in the next task
  pub(crate) fn clear(&mut self) {
    self.drawn.clear();
    self.stamps.clear();
    self.env = None;
    self.image = None;
  }
}

/// Whether drawing `command` reads the clock: a text with an effect
fn animated(command: &UiCommand) -> bool {
  matches!(command, UiCommand::Text { layout, .. } if layout.has_effects())
}

fn bits(rect: Rect) -> [u32; 4] {
  [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits)
}

/// Whether `a` draws what `b` does: shared layouts and points by address,
/// numbers by their bits, sprites by handle (their versions are stamps)
fn same(a: &UiCommand, b: &UiCommand) -> bool {
  let clips = |a: &Option<Rect>, b: &Option<Rect>| a.map(bits) == b.map(bits);
  match (a, b) {
    (
      UiCommand::Rect {
        rect,
        fill,
        radius,
        border,
        clip,
        layer,
      },
      UiCommand::Rect {
        rect: rect_b,
        fill: fill_b,
        radius: radius_b,
        border: border_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      bits(*rect) == bits(*rect_b)
        && fill == fill_b
        && radius.to_bits() == radius_b.to_bits()
        && border.map(|(width, color)| (width.to_bits(), color))
          == border_b.map(|(width, color)| (width.to_bits(), color))
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Sprite {
        sprite,
        rect,
        uv,
        tint,
        filter,
        clip,
        layer,
      },
      UiCommand::Sprite {
        sprite: sprite_b,
        rect: rect_b,
        uv: uv_b,
        tint: tint_b,
        filter: filter_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      sprite.serial() == sprite_b.serial()
        && bits(*rect) == bits(*rect_b)
        && uv.map(f32::to_bits) == uv_b.map(f32::to_bits)
        && tint == tint_b
        && filter == filter_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Line {
        points,
        at,
        width,
        color,
        closed,
        clip,
        layer,
      },
      UiCommand::Line {
        points: points_b,
        at: at_b,
        width: width_b,
        color: color_b,
        closed: closed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      Arc::ptr_eq(points, points_b)
        && at.to_array().map(f32::to_bits) == at_b.to_array().map(f32::to_bits)
        && width.to_bits() == width_b.to_bits()
        && color == color_b
        && closed == closed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    (
      UiCommand::Text {
        layout,
        origin,
        reveal,
        opacity,
        seed,
        clip,
        layer,
      },
      UiCommand::Text {
        layout: layout_b,
        origin: origin_b,
        reveal: reveal_b,
        opacity: opacity_b,
        seed: seed_b,
        clip: clip_b,
        layer: layer_b,
      },
    ) => {
      Arc::ptr_eq(layout, layout_b)
        && origin.to_array().map(f32::to_bits) == origin_b.to_array().map(f32::to_bits)
        && reveal.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
          == reveal_b.map(|(shown, fade)| (shown.to_bits(), fade.to_bits()))
        && opacity.to_bits() == opacity_b.to_bits()
        && seed == seed_b
        && clips(clip, clip_b)
        && layer == layer_b
    }
    _ => false,
  }
}

/// What `command` draws from besides its own value: a sprite's version, or
/// a text's icons (each sprite's version; a name's sprite and region in
/// `icons`); 0 for neither
fn stamp_of(command: &UiCommand, icons: Option<&TextIcons>) -> u64 {
  match command {
    UiCommand::Sprite { sprite, .. } => sprite.generation(),
    UiCommand::Text { layout, .. } if !layout.icons.is_empty() => {
      let mut hasher = FastHasher::default();
      for placed in &layout.icons {
        match &placed.icon {
          Icon::Sprite { sprite, .. } => {
            hasher.write_usize(sprite.serial());
            hasher.write_u64(sprite.generation());
          }
          Icon::Named { name, .. } => match icons.and_then(|icons| icons.get(name.as_str())) {
            Some(source) => {
              hasher.write_usize(source.sprite.serial());
              hasher.write_u64(source.sprite.generation());
              for bit in source.region.map_or([u32::MAX; 4], bits) {
                hasher.write_u32(bit);
              }
            }
            None => hasher.write_u64(u64::MAX),
          },
        }
      }
      hasher.finish()
    }
    _ => 0,
  }
}
