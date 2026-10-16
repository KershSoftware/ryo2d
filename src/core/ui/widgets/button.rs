//! The button

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::text::{RichText, TextLayoutOptions, TextStyle};
use crate::core::ui::Color;
use crate::core::ui::immediate::{UiContext, text_seed};
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::tree::{DrawKind, Node, RecordSpec, TextLeaf};

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
    node.record = Some(RecordSpec {
      flags: self.opts.flags(Flags::CLICK, true),
      nav: self.opts.nav,
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
    if response.focused() && ui.focus_visible() {
      ui.focus_ring(index, RADIUS);
    }
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

  use crate::core::input::{Direction, InputDevice};
  use crate::core::ui::UiId;

  fn menu(ui: &mut UiContext<'_>, items: &[&str], clicked: &mut Option<usize>) -> Vec<UiId> {
    let mut ids = Vec::new();
    ui.column().focus_scope().wrap_focus().show(|ui| {
      for (i, item) in items.iter().enumerate() {
        let mut b = ui.button(*item).id(i).width(120.0);
        if i == 1 {
          b = b.default_focus();
        }
        let r = b.show();
        if r.clicked() {
          *clicked = Some(i);
        }
        ids.push(r.id);
      }
    });
    ids
  }

  #[test]
  fn a_menu_opens_on_its_default_focus() {
    let mut d = UiDriver::new((300.0, 300.0));
    let ids = d.frame(|ui| menu(ui, &["Resume", "Settings", "Quit"], &mut None));
    d.frame(|ui| menu(ui, &["Resume", "Settings", "Quit"], &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
  }

  #[test]
  fn the_d_pad_moves_down_a_menu_and_confirm_clicks() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["Resume", "Settings", "Quit"];
    for _ in 0..2 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    d.device(InputDevice::Gamepad);
    d.nav(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    d.confirm();
    let mut clicked = None;
    d.frame(|ui| menu(ui, &items, &mut clicked));
    assert_eq!(clicked, Some(2));
    assert!(d.last_outcome().used.contains(&"ui.confirm"));
  }

  #[test]
  fn wrap_focus_loops_a_menu() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..2 {
      d.nav(Direction::Down);
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(ids[0]),
      "B, C, then round to A"
    );
  }

  #[test]
  fn a_held_direction_repeats_after_a_delay() {
    let mut d = UiDriver::new((300.0, 600.0));
    let items = ["0", "1", "2", "3", "4", "5", "6", "7"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.hold(Direction::Down);
    // 0.35 s at 60 fps is 21 frames: one press, then repeats every 6 frames
    for _ in 0..28 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    d.release_nav();
    let at = ids
      .iter()
      .position(|id| Some(*id) == d.frame(|ui| ui.focused()))
      .unwrap();
    assert_eq!(at, 1 + 1 + 2, "default 1, the press, then two repeats");
  }

  #[test]
  fn focus_never_lands_on_disabled_or_clipped_out_widgets() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A clip 100 tall: C (80-120) shows in part, D (120-160) not at all
    let draw = |ui: &mut UiContext<'_>| {
      let mut ids = Vec::new();
      ui.push_clip(crate::core::ui::Rect::new(0.0, 0.0, 300.0, 100.0));
      ui.column().focus_scope().show(|ui| {
        ids.push(ui.button("A").id(0).height(40.0).default_focus().show().id);
        ids.push(ui.button("Off").id(1).height(40.0).enabled(false).show().id);
        ids.push(ui.button("C").id(2).height(40.0).show().id);
        ids.push(ui.button("D").id(3).height(40.0).show().id);
      });
      ui.pop_clip();
      ids
    };
    let ids = d.frame(draw);
    d.frame(draw);
    d.nav(Direction::Down);
    d.frame(draw);
    assert_eq!(
      d.frame(|ui| {
        draw(ui);
        ui.focused()
      }),
      Some(ids[2]),
      "skips the disabled one"
    );
    d.nav(Direction::Down);
    d.frame(draw);
    assert_eq!(
      d.frame(|ui| {
        draw(ui);
        ui.focused()
      }),
      Some(ids[2]),
      "D is clipped out"
    );
  }

  #[test]
  fn navigation_reaches_rows_scrolled_out_of_view() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A view of two 50-tall rows over ten: row 2 starts fully hidden
    let draw = |ui: &mut UiContext<'_>| {
      let mut ids = Vec::new();
      ui.column().focus_scope().show(|ui| {
        ui.scroll().height(100.0).show(|ui| {
          for i in 0..10 {
            let b = ui.button("Row").id(i).height(50.0);
            let b = if i == 0 { b.default_focus() } else { b };
            ids.push(b.show().id);
          }
        });
      });
      ids
    };
    let ids = d.frame(draw);
    d.frame(draw);
    for _ in 0..3 {
      d.nav(Direction::Down);
      d.frame(draw);
    }
    assert_eq!(
      d.frame(|ui| {
        draw(ui);
        ui.focused()
      }),
      Some(ids[3])
    );
  }

  #[test]
  fn an_empty_trapped_scope_keeps_focus_off_the_page() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B"];
    let draw = |ui: &mut UiContext<'_>, open: bool| {
      menu(ui, &items, &mut None);
      if open {
        ui.column().focus_scope().trap().show(|ui| {
          ui.text("Nothing to press");
        });
      }
    };
    for _ in 0..2 {
      d.frame(|ui| draw(ui, false));
    }
    d.frame(|ui| draw(ui, true));
    assert_eq!(
      d.frame(|ui| {
        draw(ui, true);
        ui.focused()
      }),
      None,
      "the trap took focus"
    );
    d.nav(Direction::Down);
    d.frame(|ui| draw(ui, true));
    assert_eq!(
      d.frame(|ui| {
        draw(ui, true);
        ui.focused()
      }),
      None,
      "the page under the trap is not a target"
    );
    assert!(
      d.last_outcome().used.contains(&"ui.down"),
      "the arrow is still the UI's"
    );
  }

  #[test]
  fn a_held_direction_does_not_repeat_into_a_new_scene() {
    let mut d = UiDriver::new((300.0, 600.0));
    let items = ["0", "1", "2", "3", "4", "5"];
    d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.hold(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    d.set_scene(2); // the menu's scene was replaced while Down is held
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..40 {
      d.frame(|ui| menu(ui, &items, &mut None));
    }
    assert_eq!(
      d.frame(|ui| {
        menu(ui, &items, &mut None);
        ui.focused()
      }),
      Some(ids[1]),
      "still on the default"
    );
  }

  #[test]
  fn a_trapped_scope_keeps_focus_in_and_an_open_one_lets_it_out() {
    let draw = |trap: bool| {
      move |ui: &mut UiContext<'_>| {
        let mut ids = Vec::new();
        ui.row().gap(20.0).show(|ui| {
          let mut left = ui.column().focus_scope();
          if trap {
            left = left.trap();
          }
          left.show(|ui| ids.push(ui.button("L").id(0).default_focus().show().id));
          ui.column()
            .focus_scope()
            .show(|ui| ids.push(ui.button("R").id(1).show().id));
        });
        ids
      }
    };
    for trap in [true, false] {
      let mut d = UiDriver::new((400.0, 300.0));
      let ids = d.frame(draw(trap));
      d.frame(draw(trap));
      d.nav(Direction::Right);
      d.frame(draw(trap));
      let expect = if trap { ids[0] } else { ids[1] };
      assert_eq!(d.frame(|ui| ui.focused()), Some(expect), "trap: {trap}");
    }
  }

  #[test]
  fn explicit_neighbours_override_geometry() {
    let mut d = UiDriver::new((300.0, 300.0));
    let mut quit = UiId::NONE;
    let draw = |quit_id: UiId| {
      move |ui: &mut UiContext<'_>| {
        let mut ids = Vec::new();
        ui.column().focus_scope().show(|ui| {
          ids.push(
            ui.button("A")
              .id(0)
              .default_focus()
              .nav_down(quit_id)
              .show()
              .id,
          );
          ids.push(ui.button("B").id(1).show().id);
          ids.push(ui.button("Quit").id(2).show().id);
        });
        ids
      }
    };
    let ids = d.frame(draw(quit));
    quit = ids[2];
    d.frame(draw(quit));
    d.nav(Direction::Down);
    d.frame(draw(quit));
    assert_eq!(d.frame(|ui| ui.focused()), Some(quit));
  }

  #[test]
  fn focus_memory_restores_a_hidden_panel() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.nav(Direction::Down);
    d.frame(|ui| menu(ui, &items, &mut None));
    for _ in 0..3 {
      d.frame(|_| ()); // the menu is hidden
    }
    d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(ids[2]),
      "back on C, not the default B"
    );
  }

  #[test]
  fn focus_follows_the_mouse_but_the_ring_shows_only_for_keys_and_pads() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.device(InputDevice::Mouse);
    d.move_to(10.0, 5.0);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[0]));
    assert!(!d.frame(|ui| ui.focus_visible()));
    d.device(InputDevice::Keyboard);
    assert!(d.frame(|ui| ui.focus_visible()));
  }

  #[test]
  fn focus_follows_a_keyed_item_through_a_reorder() {
    let mut d = UiDriver::new((300.0, 300.0));
    let draw = |order: [usize; 3]| {
      move |ui: &mut UiContext<'_>| {
        ui.column().focus_scope().show(|ui| {
          for key in order {
            let mut b = ui.button(format!("Item {key}")).id(key);
            if key == 2 {
              b = b.default_focus();
            }
            b.show();
          }
        });
      }
    };
    d.frame(draw([0, 1, 2]));
    d.frame(draw([0, 1, 2]));
    let focused = d.frame(|ui| ui.focused());
    d.frame(draw([2, 0, 1]));
    d.frame(draw([2, 0, 1]));
    assert_eq!(d.frame(|ui| ui.focused()), focused);
  }

  #[test]
  fn tab_walks_in_order_and_shift_goes_back() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B", "C"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    d.tab(false);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[2]));
    d.tab(true);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(d.frame(|ui| ui.focused()), Some(ids[1]));
  }

  #[test]
  fn back_is_free_when_no_widget_uses_it() {
    let mut d = UiDriver::new((300.0, 300.0));
    d.frame(|ui| menu(ui, &["A"], &mut None));
    d.back();
    assert!(d.frame(|ui| {
      menu(ui, &["A"], &mut None);
      ui.back()
    }));
  }

  #[test]
  fn arrows_reach_gameplay_when_no_scope_is_shown() {
    let mut d = UiDriver::new((300.0, 300.0));
    // A HUD button outside any focus scope
    let hud = |ui: &mut UiContext<'_>| {
      ui.button("Map")
        .at(crate::core::ui::Rect::new(0.0, 0.0, 60.0, 20.0))
        .show();
    };
    d.frame(hud);
    d.nav(Direction::Down);
    d.confirm();
    d.frame(hud);
    assert!(
      d.last_outcome().used.is_empty(),
      "{:?}",
      d.last_outcome().used
    );
    assert_eq!(d.frame(|ui| ui.focused()), None);
  }

  /// The last frame's focus rings: transparent rects with a border
  fn rings(d: &UiDriver) -> Vec<(Rect, f32, Color)> {
    d.commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect {
          rect,
          fill,
          border: Some((width, color)),
          ..
        } if *fill == Color::TRANSPARENT => Some((*rect, *width, *color)),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_focused_button_draws_its_ring_for_keys_and_pads_only() {
    let mut d = UiDriver::new((300.0, 300.0));
    let items = ["A", "B"];
    let ids = d.frame(|ui| menu(ui, &items, &mut None));
    d.device(InputDevice::Gamepad);
    d.frame(|ui| menu(ui, &items, &mut None));
    let Some(b) = d.memory.current(ids[1]).map(|r| r.rect) else {
      panic!("B was drawn");
    };
    let ring = Rect::new(b.x - 4.0, b.y - 4.0, b.width + 8.0, b.height + 8.0);
    assert_eq!(rings(&d), vec![(ring, 2.0, Color::rgb(100, 140, 200))]);
    d.device(InputDevice::Mouse);
    d.frame(|ui| menu(ui, &items, &mut None));
    assert!(rings(&d).is_empty(), "no ring for the mouse");
  }

  #[test]
  fn requested_focus_moves_and_responses_report_the_change() {
    let mut d = UiDriver::new((300.0, 300.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut responses = Vec::new();
      ui.column().focus_scope().show(|ui| {
        for i in 0..3 {
          let b = ui.button("Item").id(i);
          let b = if i == 1 { b.default_focus() } else { b };
          responses.push(b.show());
        }
      });
      responses
    };
    let ids: Vec<UiId> = d.frame(draw).iter().map(|r| r.id).collect();
    let r = d.frame(draw);
    assert!(r[1].focused() && r[1].gained_focus());
    d.frame(|ui| {
      draw(ui);
      ui.request_focus(ids[0]);
    });
    let r = d.frame(draw);
    assert!(r[0].focused() && r[0].gained_focus());
    assert!(r[1].lost_focus() && !r[1].focused());
    assert!(d.frame(|ui| ui.is_focused(ids[0])));
  }

  #[test]
  fn the_right_stick_scrolls_the_focused_widgets_area() {
    let mut d = UiDriver::new((300.0, 300.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut area = UiId::NONE;
      ui.column().focus_scope().show(|ui| {
        area = ui
          .scroll()
          .height(100.0)
          .show(|ui| {
            for i in 0..10 {
              let b = ui.button("Row").id(i).height(50.0);
              let b = if i == 0 { b.default_focus() } else { b };
              b.show();
            }
          })
          .id;
      });
      area
    };
    let area = d.frame(draw);
    d.frame(draw);
    d.stick(glam::Vec2::new(0.0, -1.0)); // pushed down
    for _ in 0..10 {
      d.frame(draw);
    }
    d.stick(glam::Vec2::ZERO);
    let offset = d.frame(|ui| ui.scroll_offset(area));
    assert!(offset.y > 20.0, "{offset}");
  }
}
