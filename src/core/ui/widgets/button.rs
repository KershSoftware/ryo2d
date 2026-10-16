//! The button

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::text::{RichText, TextLayoutOptions, TextStyle};
use crate::core::ui::immediate::{UiContext, text_seed};
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::tree::{DrawKind, Node, RecordSpec, TextLeaf};
use crate::core::ui::{Color, UiId};

/// Room between a button's edges and its label, across and down
const PADDING: Vec2 = Vec2::new(12.0, 6.0);
/// The least a button is tall
const MIN_HEIGHT: f32 = 28.0;
/// Room between an icon and the label
const ICON_GAP: f32 = 6.0;
const RADIUS: f32 = 4.0;
const BORDER_WIDTH: f32 = 1.0;
/// A disabled button's alpha, fill, border and label alike
const DISABLED_ALPHA: u8 = 120;

/// A button being set up. It is drawn by [`show`](Self::show) (its
/// [`Response`]), by [`clicked`](Self::clicked), or when it is dropped, so
/// `ui.button("Quit");` is a statement.
pub struct Button<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  label: RichText,
  icon: Option<SpriteHandle>,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A button labelled `label`, as wide as its label plus padding (or as
  /// its style or `at` rect say). It clicks when the left button is pressed
  /// on it and released over it.
  ///
  /// ```ignore
  /// if ui.button("Play").clicked() {
  ///     return SceneAction::Switch(Box::new(Level::default()));
  /// }
  /// let quit = ui.button("Quit").at(Rect::new(10.0, 10.0, 120.0, 32.0)).show();
  /// ```
  #[track_caller]
  pub fn button<'u>(&'u mut self, label: impl Into<RichText>) -> Button<'u, 'a> {
    Button {
      ui: Some(self),
      site: Location::caller(),
      label: label.into(),
      icon: None,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Button<'_, '_>);

impl Button<'_, '_> {
  /// A sprite left of the label, as tall as a line of it
  pub fn icon(mut self, sprite: &SpriteHandle) -> Self {
    self.icon = Some(sprite.clone());
    self
  }

  /// Draw the button; what the pointer did to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Draw the button; whether it was clicked this frame
  pub fn clicked(self) -> bool {
    self.show().clicked()
  }

  /// Record the button, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let id = ui.memory.scopes.make(self.site, self.opts.key);
    let disabled = self.opts.disabled;
    let mut response = ui.response(id, Sense::click(), disabled);

    let label = std::mem::take(&mut self.label);
    let style = TextStyle::default();
    let options = TextLayoutOptions::default();
    let text = ui.measure_rich(&label, &style, &options);
    let line = ui.line_height(&style);
    let icon_room = if self.icon.is_some() {
      line + ICON_GAP
    } else {
      0.0
    };
    let size = Vec2::new(
      text.x + icon_room + 2.0 * PADDING.x,
      (text.y.max(line) + 2.0 * PADDING.y).max(MIN_HEIGHT),
    );

    let mut style_of_node = self.opts.style.clone();
    // The icon's room is padding: the label centres in what is left
    style_of_node.padding = Edges::new(PADDING.x + icon_room, PADDING.y, PADDING.x, PADDING.y);
    let mut node = Node::new(id, style_of_node, Measure::Fixed(size));
    let mut flags = Flags::CLICK;
    if disabled {
      flags |= Flags::DISABLED;
    }
    node.record = Some(RecordSpec {
      flags,
      nav: [UiId::NONE; 4],
    });
    let index = ui.push_node(node, self.opts.at);

    let (fill, border) = look(&response, disabled);
    ui.push_draw(
      index,
      DrawKind::Fill {
        inset: Edges::ZERO,
        fill,
        radius: RADIUS,
        border: Some((BORDER_WIDTH, border)),
      },
    );
    let alpha = if disabled { DISABLED_ALPHA } else { 255 };
    if let Some(sprite) = self.icon.take() {
      // Centred with the label: its room is the content box but the label
      ui.push_draw(
        index,
        DrawKind::Sprite {
          inset: Edges::new(
            PADDING.x,
            PADDING.y,
            PADDING.x + ICON_GAP + text.x,
            PADDING.y,
          ),
          frac: Vec2::splat(0.5),
          size: Some(Vec2::splat(line)),
          sprite,
          uv: [0.0, 0.0, 1.0, 1.0],
          tint: Color::WHITE.with_alpha(alpha),
        },
      );
    }
    let seed = text_seed(&label, &style, &options);
    ui.push_draw(
      index,
      DrawKind::Text {
        leaf: TextLeaf {
          text: label,
          style,
          options,
        },
        frac: Vec2::splat(0.5),
        wrap: false,
        // Faded through opacity, so both looks share one cached layout
        opacity: f32::from(alpha) / 255.0,
        reveal: None,
        seed,
      },
    );
    response.rect = ui.close_node(index).rect;
    Some(response)
  }
}

impl Drop for Button<'_, '_> {
  /// A button not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

/// A button's fill and border for its state (the theme's from Task 9)
fn look(response: &Response, disabled: bool) -> (Color, Color) {
  if disabled {
    (
      Color::new(40, 40, 60, DISABLED_ALPHA),
      Color::new(80, 80, 100, DISABLED_ALPHA),
    )
  } else if response.pressed() && response.hovered() {
    (Color::rgb(60, 60, 80), Color::rgb(100, 100, 130))
  } else if response.hovered() {
    (Color::rgb(50, 50, 70), Color::rgb(100, 100, 130))
  } else {
    (Color::rgb(40, 40, 60), Color::rgb(80, 80, 100))
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::ui::Rect;
  use crate::core::ui::test_util::UiDriver;

  use crate::core::asset::SpriteHandle;
  use crate::core::graphics::Sprite;
  use crate::core::text::TextStyle;
  use crate::core::ui::immediate::UiCommand;
  use crate::core::ui::{Color, UiContext};

  #[test]
  fn a_button_clicks_on_release() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    // One call site, so it is the same button every frame
    let go = |ui: &mut UiContext<'_>| ui.button("Go").at(at).clicked();
    d.frame(go);
    d.move_to(20.0, 20.0);
    d.press();
    assert!(!d.frame(go));
    d.release();
    assert!(d.frame(go));
    assert!(!d.frame(go), "once");
  }

  #[test]
  fn a_panel_drawn_over_a_button_takes_its_clicks() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut crate::core::ui::UiContext<'_>| {
      let clicked = ui.button("Under").at(at).clicked();
      ui.panel()
        .at(Rect::new(0.0, 0.0, 200.0, 100.0))
        .show(|_| {});
      clicked
    };
    d.frame(draw);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(!d.frame(draw));
  }

  #[test]
  fn a_button_in_a_column_is_hit_where_it_was_laid_out() {
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut crate::core::ui::UiContext<'_>| {
      let mut clicked = false;
      ui.column()
        .offset(50.0, 40.0)
        .show(|ui| clicked = ui.button("In").clicked());
      clicked
    };
    d.frame(draw);
    d.move_to(60.0, 50.0);
    d.press();
    d.frame(draw);
    d.release();
    assert!(d.frame(draw));
  }

  /// The fills of the last frame's rect commands
  fn fills(d: &UiDriver) -> Vec<Color> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { fill, .. } => Some(*fill),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_button_is_its_label_plus_padding() {
    let mut d = UiDriver::new((200.0, 100.0));
    let rect = d.frame(|ui| {
      let label = ui.measure_text("Wide label", &TextStyle::default());
      let rect = ui.button("Wide label").show().rect;
      (label, rect)
    });
    let (label, rect) = rect;
    assert!((rect.width - (label.x + 24.0)).abs() < 1.0, "{rect:?}");
    assert!(
      (rect.height - (label.y + 12.0).max(28.0)).abs() < 1.0,
      "{rect:?}"
    );
  }

  #[test]
  fn a_button_looks_hovered_pressed_and_disabled() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut UiContext<'_>| {
      ui.button("A").at(at).show();
    };
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::rgb(40, 40, 60)]);
    d.move_to(20.0, 20.0);
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::rgb(50, 50, 70)]);
    d.press();
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::rgb(60, 60, 80)]);
    d.release();
    d.frame(|ui| {
      ui.button("A").at(at).enabled(false).show();
    });
    assert_eq!(fills(&d), vec![Color::new(40, 40, 60, 120)]);
  }

  #[test]
  fn an_icon_sits_left_of_the_label_and_both_are_centred() {
    let mut d = UiDriver::new((300.0, 100.0));
    let sprite = SpriteHandle::from_sprite(Sprite::solid_rect(4, 4, 255, 255, 255, 255), "i");
    let at = Rect::new(10.0, 10.0, 200.0, 40.0);
    let (label, line) = d.frame(|ui| {
      ui.button("Save").at(at).icon(&sprite).show();
      let style = TextStyle::default();
      (ui.measure_text("Save", &style), ui.line_height(&style))
    });
    let icon = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Sprite { rect, .. } => Some(*rect),
      _ => None,
    });
    let text = d.commands.iter().find_map(|(_, c)| c.text_origin());
    let (Some(icon), Some(text)) = (icon, text) else {
      panic!("an icon and a label");
    };
    assert!((icon.width - line).abs() < 1.0 && (icon.height - line).abs() < 1.0);
    assert!(
      (text.x - (icon.right() + 6.0)).abs() < 1.0,
      "{icon:?} {text}"
    );
    let group = line + 6.0 + label.x;
    assert!(
      (icon.x - (at.x + (at.width - group) / 2.0)).abs() < 1.0,
      "the icon and label are centred together: {icon:?}"
    );
    assert!((icon.y + icon.height / 2.0 - (at.y + at.height / 2.0)).abs() < 1.0);
  }

  #[test]
  fn a_dropped_button_is_drawn() {
    let mut d = UiDriver::new((200.0, 100.0));
    d.frame(|ui| {
      ui.button("Statement");
    });
    assert_eq!(fills(&d).len(), 1);
    assert_eq!(
      d.commands
        .iter()
        .filter(|(_, c)| c.text_origin().is_some())
        .count(),
      1
    );
  }

  #[test]
  fn a_disabled_button_never_clicks() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    // One call site, so it is the same button every frame
    let no = |ui: &mut UiContext<'_>| ui.button("No").at(at).enabled(false).clicked();
    d.frame(no);
    d.move_to(20.0, 20.0);
    d.press();
    d.frame(no);
    d.release();
    assert!(!d.frame(no));
  }
}
