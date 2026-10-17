//! The image

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::ui::Color;
use crate::core::ui::immediate::UiContext;
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, WidgetState};
use crate::core::ui::tree::{DrawKind, Node};

/// An image being set up. It is drawn by [`show`](Self::show) or when it is
/// dropped.
pub struct Image<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  sprite: SpriteHandle,
  size: Option<Vec2>,
  tint: Color,
  filter: Filter,
  uv: [f32; 4],
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A sprite as a widget, the sprite's size unless told ([`size`](Image::size),
  /// or the layout options). It senses nothing, unless it has a tooltip.
  ///
  /// ```ignore
  /// ui.image(&self.portrait).size(64.0, 64.0).filter(Filter::Nearest);
  /// ```
  #[track_caller]
  pub fn image<'u>(&'u mut self, sprite: &SpriteHandle) -> Image<'u, 'a> {
    Image {
      ui: Some(self),
      site: Location::caller(),
      sprite: sprite.clone(),
      size: None,
      tint: Color::WHITE,
      filter: Filter::Default,
      uv: [0.0, 0.0, 1.0, 1.0],
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Image<'_, '_>);

impl Image<'_, '_> {
  /// Its size in UI units
  pub fn size(mut self, width: f32, height: f32) -> Self {
    self.size = Some(Vec2::new(width, height));
    self
  }

  /// Multiply its colours by `color` (white leaves them)
  pub fn tint(mut self, color: Color) -> Self {
    self.tint = color;
    self
  }

  /// How the texture is sampled when it is not drawn at its own size
  /// (`Filter::Default`: nearest in pixel-art presentation, else linear)
  pub fn filter(mut self, filter: Filter) -> Self {
    self.filter = filter;
    self
  }

  /// Draw part of the sprite: left, top, right and bottom as fractions of it
  pub fn uv(mut self, uv: [f32; 4]) -> Self {
    self.uv = uv;
    self
  }

  /// Draw the image; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the image, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.widget_response(id, Sense::hover(), &self.opts);
    let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let size = self
      .size
      .map(|s| Vec2::new(clean(s.x), clean(s.y)))
      .unwrap_or_else(|| Vec2::new(self.sprite.width() as f32, self.sprite.height() as f32));

    let mut node = Node::new(id, self.opts.style.clone(), Measure::Fixed(size));
    if self.opts.tooltip.is_some() || self.opts.drag.is_set() {
      node.record = Some(self.opts.record(Flags::HOVER, false));
    }
    let mut tint = self.tint;
    if disabled {
      // Dimmed as far as a disabled button's label is
      let dim = ui.look_of(WidgetKind::Button, &self.opts, WidgetState::Disabled);
      tint = tint.with_alpha((u16::from(tint.a) * u16::from(dim.text.a) / 255) as u8);
    }
    let index = ui.push_node(node, self.opts.at);
    ui.push_draw(
      index,
      DrawKind::Sprite {
        inset: Edges::ZERO,
        frac: Vec2::splat(0.5),
        size: None,
        sprite: self.sprite.clone(),
        uv: self.uv,
        tint,
        filter: self.filter,
      },
    );
    response.rect = ui.close_node(index).rect;
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Image<'_, '_> {
  /// An image not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::render::batch::Filter;
  use crate::core::ui::Rect;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;

  #[test]
  fn an_image_draws_its_sprite_with_its_filter() {
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(8, 8, 255, 255, 255, 255), "test-image");
    let mut d = UiDriver::new((100.0, 100.0));
    d.frame(|ui| {
      ui.image(&sprite)
        .filter(Filter::Nearest)
        .at(Rect::new(4.0, 4.0, 32.0, 32.0))
        .show();
    });
    let drawn: Vec<(Rect, Filter)> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Sprite { rect, filter, .. } => Some((*rect, *filter)),
        _ => None,
      })
      .collect();
    assert_eq!(
      drawn,
      vec![(Rect::new(4.0, 4.0, 32.0, 32.0), Filter::Nearest)]
    );
  }
}
