//! `ui.custom`: a widget the game paints itself with a `Painter`

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::text::{RichText, TextEngine, TextLayoutOptions, TextStyle};
use crate::core::ui::immediate::{UiCommand, UiContext, text_seed};
use crate::core::ui::layout::{Length, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::tree::{DrawKind, Node};
use crate::core::ui::{Color, Rect};

/// A custom widget being set up: a box of its own size that the game
/// paints with [`paint`](Self::paint). It is drawn by `paint`, by
/// [`show`](Self::show) (nothing painted) or when it is dropped.
pub struct Custom<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  size: Vec2,
  sense: Sense,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A widget `width` by `height` UI units that the game paints itself,
  /// in its own units: (0, 0) is its top-left wherever the layout puts it.
  /// It senses hovering only unless told ([`sense`](Custom::sense)).
  ///
  /// ```ignore
  /// let health = self.health;
  /// let bar = ui.custom(120.0, 12.0).sense(Sense::click()).paint(|p, response| {
  ///     let size = p.size();
  ///     p.rounded_rect(Rect::new(0.0, 0.0, size.x, size.y), 3.0, Color::DARK_GRAY);
  ///     p.rounded_rect(Rect::new(0.0, 0.0, size.x * health, size.y), 3.0, Color::RED);
  /// });
  /// if bar.clicked() { /* ... */ }
  /// ```
  #[track_caller]
  pub fn custom<'u>(&'u mut self, width: f32, height: f32) -> Custom<'u, 'a> {
    let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    Custom {
      ui: Some(self),
      site: Location::caller(),
      size: Vec2::new(clean(width), clean(height)),
      sense: Sense::hover(),
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Custom<'_, '_>);

impl Custom<'_, '_> {
  /// What it takes from the pointer (default [`Sense::hover`]: it reports
  /// hovering, and clicks reach what is under it)
  pub fn sense(mut self, sense: Sense) -> Self {
    self.sense = sense;
    self
  }

  /// Draw it, painting it with `paint` now (so `paint` may borrow the
  /// scene's data); what the pointer did to it this frame
  pub fn paint(mut self, paint: impl FnOnce(&mut Painter<'_>, &Response)) -> Response {
    self.run(Some(paint)).unwrap_or_default()
  }

  /// Draw it with nothing painted; what the pointer did to it this frame
  pub fn show(mut self) -> Response {
    self
      .run(None::<fn(&mut Painter<'_>, &Response)>)
      .unwrap_or_default()
  }

  /// Record the widget, once (`paint`, `show` and `Drop` all come here)
  fn run(&mut self, paint: Option<impl FnOnce(&mut Painter<'_>, &Response)>) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let mut response = ui.widget_response(id, self.sense, &self.opts);

    let style = self.opts.style.clone();
    // The size it paints at: its own, its `at` rect's, or, when its style
    // fills, where layout put it last frame
    let last = ui
      .memory
      .prev(id)
      .map(|record| Vec2::new(record.rect.width, record.rect.height));
    let fills = style.grow > 0.0;
    let along = |length: Length, own: f32, last: Option<f32>| match length {
      Length::Fit if !fills => own,
      Length::Px(v) if !fills && v.is_finite() && v >= 0.0 => v,
      _ => last.unwrap_or(own),
    };
    let size = match self.opts.at {
      Some(at) => Vec2::new(at.width, at.height),
      None => Vec2::new(
        along(style.width, self.size.x, last.map(|s| s.x)),
        along(style.height, self.size.y, last.map(|s| s.y)),
      ),
    };

    let mut flags = Flags::default();
    if self.sense.click {
      flags |= Flags::CLICK;
    }
    if self.sense.drag {
      flags |= Flags::DRAG;
    }
    if self.sense == Sense::hover() {
      flags |= Flags::HOVER;
    }
    // Focusable only when asked
    let mut node = Node::new(id, style, Measure::Fixed(self.size));
    node.record = Some(self.opts.record(flags, false));
    let index = ui.push_node(node, self.opts.at);
    if let Some(paint) = paint {
      let scale = ui.ui_scale();
      let mut painter = Painter {
        text: ui.text_engine(),
        scale,
        size,
        commands: Vec::new(),
      };
      paint(&mut painter, &response);
      for command in painter.commands {
        ui.push_draw(index, DrawKind::Local(command));
      }
    }
    response.rect = ui.close_node(index).rect;
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Custom<'_, '_> {
  /// A custom widget not shown is drawn (unpainted) when it is dropped
  fn drop(&mut self) {
    self.run(None::<fn(&mut Painter<'_>, &Response)>);
  }
}

/// Paints a custom widget in its own units: (0, 0) is its top-left, and
/// [`size`](Self::size) its size. What it paints is placed with the widget
/// when the layout is done.
pub struct Painter<'p> {
  text: &'p mut TextEngine,
  /// Render pixels per UI unit
  scale: f32,
  size: Vec2,
  /// What was painted, in the widget's units
  commands: Vec<UiCommand>,
}

impl Painter<'_> {
  /// The widget's size in UI units: its own, or, when its style fills, the
  /// size layout gave it last frame
  pub fn size(&self) -> Vec2 {
    self.size
  }

  /// Fill `rect`
  pub fn rect(&mut self, rect: Rect, color: Color) {
    self.bordered(rect, color, 0.0, None);
  }

  /// Fill `rect` with corners of `radius`
  pub fn rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
    self.bordered(rect, color, radius, None);
  }

  /// Fill `rect` with corners of `radius` and a border `border_width` wide
  /// inside its edges
  pub fn bordered_rect(
    &mut self,
    rect: Rect,
    fill: Color,
    radius: f32,
    border_width: f32,
    border_color: Color,
  ) {
    self.bordered(rect, fill, radius, Some((border_width, border_color)));
  }

  fn bordered(&mut self, rect: Rect, fill: Color, radius: f32, border: Option<(f32, Color)>) {
    let radius = if radius.is_finite() {
      radius.max(0.0)
    } else {
      0.0
    };
    let border = border.filter(|(width, _)| width.is_finite() && *width > 0.0);
    self.commands.push(UiCommand::Rect {
      rect,
      fill,
      radius,
      border,
      clip: None,
      layer: 0,
    });
  }

  /// Draw `text` in `style` with its top-left at (x, y)
  pub fn text(&mut self, text: impl Into<RichText>, x: f32, y: f32, style: &TextStyle) {
    let text = text.into();
    let options = TextLayoutOptions::default();
    let Some(layout) = self.text.layout(&text, style, &options, self.scale) else {
      return;
    };
    self.commands.push(UiCommand::Text {
      layout,
      origin: Vec2::new(x, y),
      reveal: None,
      opacity: 1.0,
      seed: text_seed(&text, style, &options),
      clip: None,
      layer: 0,
    });
  }

  /// Draw `sprite` over `rect`, tinted
  pub fn sprite(&mut self, sprite: &SpriteHandle, rect: Rect, tint: Color) {
    self.commands.push(UiCommand::Sprite {
      sprite: sprite.clone(),
      rect,
      uv: [0.0, 0.0, 1.0, 1.0],
      tint,
      filter: Filter::Default,
      clip: None,
      layer: 0,
    });
  }

  /// Draw a line through `points` (at least two, all finite), `width` wide
  pub fn line(&mut self, points: &[Vec2], width: f32, color: Color) {
    if points.len() < 2
      || !width.is_finite()
      || width <= 0.0
      || !points.iter().all(|p| p.is_finite())
    {
      return;
    }
    self.commands.push(UiCommand::Line {
      points: points.to_vec(),
      width,
      color,
      closed: false,
      clip: None,
      layer: 0,
    });
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Color, Rect, Sense};

  use glam::Vec2;

  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::text::TextStyle;
  use crate::core::ui::UiContext;

  #[test]
  fn painting_in_a_column_lands_at_the_widget() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(|ui| {
      ui.column().offset(30.0, 20.0).show(|ui| {
        ui.custom(16.0, 16.0)
          .sense(Sense::click())
          .paint(|p, _| p.rect(Rect::new(2.0, 2.0, 4.0, 4.0), Color::RED));
      });
    });
    let rect = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect { rect, .. } => Some(*rect),
      _ => None,
    });
    assert_eq!(rect, Some(Rect::new(32.0, 22.0, 4.0, 4.0)));
  }

  #[test]
  fn a_drag_reports_its_start_delta_and_release() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 40.0, 40.0);
    let draw = |ui: &mut UiContext<'_>| ui.custom(40.0, 40.0).at(at).sense(Sense::drag()).show();
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    let pressed = d.frame(draw);
    assert!(pressed.pressed() && !pressed.dragging());
    assert_eq!(pressed.pointer(), Some(Vec2::new(20.0, 20.0)));
    d.move_to(30.0, 25.0);
    let started = d.frame(draw);
    assert!(started.drag_started() && started.dragging());
    assert_eq!(started.drag_delta(), Vec2::new(10.0, 5.0));
    d.move_to(31.0, 25.0);
    let moved = d.frame(draw);
    assert!(!moved.drag_started() && moved.dragging());
    assert_eq!(moved.drag_delta(), Vec2::new(1.0, 0.0));
    d.release();
    let released = d.frame(draw);
    assert!(released.drag_released() && !released.dragging());
    assert!(!released.clicked(), "a drag is not a click");
  }

  #[test]
  fn a_sense_masks_what_it_does_not_sense() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 40.0, 40.0);
    let draw = |ui: &mut UiContext<'_>| ui.custom(40.0, 40.0).at(at).sense(Sense::drag()).show();
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(draw);
    d.release();
    let response = d.frame(draw);
    assert!(response.hovered());
    assert!(!response.clicked(), "a drag-only widget reports no click");
    let click = |ui: &mut UiContext<'_>| ui.custom(40.0, 40.0).at(at).sense(Sense::click()).show();
    d.frame(click);
    d.press();
    d.frame(click);
    d.release();
    assert!(d.frame(click).clicked());
  }

  #[test]
  fn a_hover_only_widget_reports_hovered() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 40.0, 40.0);
    let draw = |ui: &mut UiContext<'_>| {
      let hovered = ui.custom(40.0, 40.0).at(at).show().hovered();
      (hovered, ui.wants_pointer())
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    assert_eq!(
      d.frame(draw),
      (true, false),
      "hovered, and the pointer is still gameplay's"
    );
    d.move_to(100.0, 80.0);
    assert_eq!(d.frame(draw), (false, false));
  }

  #[test]
  fn a_hover_only_widget_over_a_button_does_not_take_its_click() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut UiContext<'_>| {
      let clicked = ui.button("Under").at(at).clicked();
      let hovered = ui.custom(80.0, 30.0).at(at).show().hovered();
      (clicked, hovered)
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    assert_eq!(d.frame(draw), (false, true));
    d.press();
    d.frame(draw);
    d.release();
    assert_eq!(
      d.frame(draw),
      (true, true),
      "the click reaches the button under it"
    );
  }

  #[test]
  fn a_filling_custom_widget_paints_at_its_laid_out_size() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut sizes = Vec::new();
    for _ in 0..2 {
      d.frame(|ui| {
        ui.column().width(120.0).show(|ui| {
          ui.custom(10.0, 8.0)
            .fill_width()
            .paint(|p, _| sizes.push(p.size()));
        });
      });
    }
    assert_eq!(
      sizes,
      vec![Vec2::new(10.0, 8.0), Vec2::new(120.0, 8.0)],
      "its own size, then last frame's laid-out size"
    );
  }

  #[test]
  fn a_painter_draws_in_the_widgets_units() {
    let mut d = UiDriver::new((200.0, 100.0));
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "s");
    d.frame(|ui| {
      ui.custom(50.0, 50.0)
        .at(Rect::new(100.0, 40.0, 50.0, 50.0))
        .paint(|p, _| {
          p.rounded_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 3.0, Color::RED);
          p.bordered_rect(
            Rect::new(5.0, 5.0, 10.0, 10.0),
            Color::BLUE,
            2.0,
            1.0,
            Color::WHITE,
          );
          p.sprite(&sprite, Rect::new(1.0, 2.0, 3.0, 4.0), Color::WHITE);
          p.line(&[Vec2::ZERO, Vec2::new(10.0, 0.0)], 2.0, Color::WHITE);
          p.line(&[Vec2::ZERO, Vec2::new(f32::NAN, 0.0)], 2.0, Color::WHITE);
          p.line(&[Vec2::ZERO], 2.0, Color::WHITE);
          p.text("Hi", 4.0, 6.0, &TextStyle::default());
        });
    });
    let mut kinds = Vec::new();
    for (_, command) in d.commands.iter() {
      kinds.push(match command {
        UiCommand::Rect {
          rect,
          radius,
          border,
          ..
        } => format!("rect {} {} {radius} {}", rect.x, rect.y, border.is_some()),
        UiCommand::Sprite { rect, .. } => format!("sprite {} {}", rect.x, rect.y),
        UiCommand::Line { points, .. } => format!("line {} {}", points[0].x, points[1].x),
        UiCommand::Text { origin, .. } => format!("text {} {}", origin.x, origin.y),
      });
    }
    assert_eq!(
      kinds,
      vec![
        "rect 100 40 3 false",
        "rect 105 45 2 true",
        "sprite 101 42",
        "line 100 110",
        "text 104 46",
      ],
      "lines need two finite points"
    );
  }
}
