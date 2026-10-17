//! The button

use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::asset::SpriteHandle;
use crate::core::render::batch::Filter;
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::Color;
use crate::core::ui::immediate::{UiContext, text_seed};
use crate::core::ui::layout::{Edges, Measure};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::WidgetKind;
use crate::core::ui::tree::{DrawKind, Node, RecordSpec, TextLeaf};

/// Room between an icon and the label
const ICON_GAP: f32 = 6.0;

/// A button being set up. It is drawn by [`show`](Self::show) (its
/// [`Response`]), by [`clicked`](Self::clicked), or when it is dropped, so
/// `ui.button("Quit");` is a statement.
pub struct Button<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  label: RichText,
  icon: Option<SpriteHandle>,
  opts: WidgetOpts,
  /// The looks it is drawn in (a tab, a choice and a dropdown's option are
  /// buttons in their own looks)
  kind: WidgetKind,
  /// Recorded besides `CLICK` and the focus options
  flags: Flags,
  /// Where the label sits across the content box, 0 (left) to 1
  label_x: f32,
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
      kind: WidgetKind::Button,
      flags: Flags::default(),
      label_x: 0.5,
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

  /// Drawn in `kind`'s looks
  pub(crate) fn kind(mut self, kind: WidgetKind) -> Self {
    self.kind = kind;
    self
  }

  /// Also records `flags`
  pub(crate) fn also(mut self, flags: Flags) -> Self {
    self.flags |= flags;
    self
  }

  /// Its label at the left of its content box
  pub(crate) fn label_left(mut self) -> Self {
    self.label_x = 0.0;
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

    let state = ui.widget_state(&response, disabled);
    let look = ui.look_of(self.kind, &self.opts, state);
    let theme = ui.theme_arc();
    let sizes = &theme.metrics.sizes;
    // Its look's own padding, else the theme's button padding
    let pad = if look.padding == Edges::ZERO {
      Edges::xy(sizes.button_padding[0], sizes.button_padding[1])
    } else {
      look.padding
    };
    // The label is laid out opaque and faded through its opacity, so every
    // look shares one cached layout
    let label = std::mem::take(&mut self.label);
    let style = theme.text.button.style().color(look.text.with_alpha(255));
    let options = TextLayoutOptions::default();
    let text = ui.measure_rich(&label, &style, &options);
    let line = ui.line_height(&style);
    let icon_room = if self.icon.is_some() {
      line + ICON_GAP
    } else {
      0.0
    };
    let size = Vec2::new(
      text.x + icon_room + pad.width(),
      (text.y.max(line) + pad.height()).max(sizes.button_min_height),
    );

    // The look's offset moves the contents: the centre shifts by half of what
    // one side gains over the other
    let [dx, dy] = look.offset;
    let (left, right) = (
      pad.left + (2.0 * dx).max(0.0),
      pad.right + (-2.0 * dx).max(0.0),
    );
    let (top, bottom) = (
      pad.top + (2.0 * dy).max(0.0),
      pad.bottom + (-2.0 * dy).max(0.0),
    );
    let mut style_of_node = self.opts.style.clone();
    // The icon's room is padding: the label centres in what is left
    style_of_node.padding = Edges::new(left + icon_room, top, right, bottom);
    let mut node = Node::new(id, style_of_node, Measure::Fixed(size));
    node.record = Some(RecordSpec {
      flags: self.opts.flags(Flags::CLICK | self.flags, true),
      nav: self.opts.nav,
    });
    let index = ui.push_node(node, self.opts.at);

    ui.push_look_box(index, &look);
    if let Some(sprite) = self.icon.take() {
      // Centred with the label: its room is the content box but the label
      ui.push_draw(
        index,
        DrawKind::Sprite {
          inset: Edges::new(left, top, right + ICON_GAP + text.x, bottom),
          frac: Vec2::splat(0.5),
          size: Some(Vec2::splat(line)),
          sprite,
          uv: [0.0, 0.0, 1.0, 1.0],
          tint: Color::WHITE.with_alpha(look.text.a),
          filter: Filter::Default,
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
        frac: Vec2::new(self.label_x, 0.5),
        wrap: false,
        opacity: f32::from(look.text.a) / 255.0,
        reveal: None,
        seed,
      },
    );
    if response.focused() && ui.focus_visible() {
      ui.focus_ring(index, look.radius);
    }
    response.rect = ui.close_node(index).rect;
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }
}

impl Drop for Button<'_, '_> {
  /// A button not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
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
    // The menu is hidden: only a HUD button outside any scope is drawn
    let hidden = |ui: &mut UiContext<'_>| {
      ui.button("HUD").at(Rect::new(250.0, 250.0, 40.0, 20.0));
      ui.focused()
    };
    for _ in 0..3 {
      d.frame(hidden);
    }
    assert_eq!(
      d.frame(hidden),
      None,
      "nothing has focus while it is hidden"
    );
    d.frame(|ui| menu(ui, &items, &mut None));
    d.frame(|ui| menu(ui, &items, &mut None));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(ids[2]),
      "back on C, not the default B"
    );
  }

  #[test]
  fn a_direction_held_from_before_a_scope_appears_does_not_move_in_it() {
    let mut d = UiDriver::new((300.0, 600.0));
    let items = ["0", "1", "2", "3", "4", "5"];
    let hud = |ui: &mut UiContext<'_>| {
      ui.button("HUD").at(Rect::new(250.0, 550.0, 40.0, 20.0));
    };
    d.frame(hud);
    d.hold(Direction::Down); // pressed for gameplay: no scope is shown
    d.frame(hud);
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
    assert!(
      d.last_outcome().used.is_empty(),
      "the held key stays the game's: {:?}",
      d.last_outcome().used
    );
  }

  #[test]
  fn navigation_leaves_a_scroll_list_and_skips_its_hidden_rows() {
    // Ten 50-tall rows in a 100-tall view, then a Back button
    let draw = |wrap: bool| {
      move |ui: &mut UiContext<'_>| {
        let mut ids = Vec::new();
        let mut scope = ui.column().gap(10.0).focus_scope();
        if wrap {
          scope = scope.wrap_focus();
        }
        scope.show(|ui| {
          ui.scroll().height(100.0).show(|ui| {
            for i in 0..10 {
              ids.push(ui.button("Row").id(i).height(50.0).show().id);
            }
          });
          ids.push(ui.button("Back").height(30.0).default_focus().show().id);
        });
        ids
      }
    };
    for wrap in [false, true] {
      let mut d = UiDriver::new((300.0, 300.0));
      let ids = d.frame(draw(wrap));
      d.frame(draw(wrap));
      let back = ids[10];
      assert_eq!(d.frame(|ui| ui.focused()), Some(back));
      d.nav(Direction::Down);
      d.frame(draw(wrap));
      let expect = if wrap { ids[0] } else { back };
      assert_eq!(
        d.frame(|ui| ui.focused()),
        Some(expect),
        "wrap: {wrap}: the hidden rows below Back are not targets"
      );
    }
    let mut d = UiDriver::new((300.0, 300.0));
    let ids = d.frame(draw(false));
    d.frame(draw(false));
    d.nav(Direction::Up);
    d.frame(draw(false));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(ids[1]),
      "the last row shown"
    );
    d.nav(Direction::Down);
    d.frame(draw(false));
    assert_eq!(
      d.frame(|ui| ui.focused()),
      Some(ids[2]),
      "from inside the list, its hidden rows are reachable"
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

  #[test]
  fn a_button_takes_its_colours_from_the_theme() {
    use crate::core::ui::Color;
    use crate::core::ui::immediate::UiCommand;
    use crate::core::ui::theme::{LookPatch, Theme, Variant, WidgetKind};
    let theme = Theme::dark().variant(
      "go",
      Variant::of(WidgetKind::Button).normal(LookPatch::default().fill(Color::GREEN)),
    );
    let mut d = UiDriver::new((200.0, 100.0)).with_theme(theme.clone());
    let at = crate::core::ui::Rect::new(10.0, 10.0, 80.0, 30.0);
    d.frame(|ui| {
      ui.button("Plain").at(at).show();
      ui.button("Go").at(at).variant("go").show();
    });
    let fills: Vec<Color> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Rect { fill, .. } => Some(*fill),
        _ => None,
      })
      .collect();
    let plain = theme
      .look(
        WidgetKind::Button,
        None,
        crate::core::ui::theme::WidgetState::Normal,
      )
      .fill;
    assert_eq!(fills, vec![plain, Color::GREEN]);
  }

  #[test]
  fn a_per_call_style_wins_in_every_state() {
    use crate::core::ui::theme::LookPatch;
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    let draw = |ui: &mut UiContext<'_>| {
      ui.button("A")
        .at(at)
        .variant("danger")
        .style(LookPatch::default().fill(Color::BLUE))
        .show();
    };
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::BLUE]);
    d.move_to(20.0, 20.0);
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::BLUE], "hovered");
    d.press();
    d.frame(draw);
    assert_eq!(fills(&d), vec![Color::BLUE], "pressed");
  }

  #[test]
  fn a_focused_button_shows_its_focused_look_only_with_the_ring() {
    use crate::core::input::InputDevice;
    use crate::core::ui::theme::{Theme, WidgetKind, WidgetState};
    let focused = Theme::dark()
      .look(WidgetKind::Button, None, WidgetState::Focused)
      .fill;
    let normal = Theme::dark()
      .look(WidgetKind::Button, None, WidgetState::Normal)
      .fill;
    assert_ne!(focused, normal);
    let mut d = UiDriver::new((200.0, 100.0));
    let draw = |ui: &mut UiContext<'_>| {
      ui.column().focus_scope().show(|ui| {
        ui.button("A").default_focus().show();
      });
    };
    // Away from the button, so only focus can change its look
    d.move_to(190.0, 90.0);
    d.device(InputDevice::Keyboard);
    d.frame(draw);
    d.frame(draw);
    d.frame(draw);
    assert_eq!(fills(&d)[0], focused);
    d.device(InputDevice::Mouse);
    d.frame(draw);
    assert_eq!(fills(&d)[0], normal, "no ring, no focused look");
  }

  #[test]
  fn the_focus_ring_follows_the_themes_metrics() {
    use crate::core::input::InputDevice;
    use crate::core::ui::theme::Theme;
    let mut theme = Theme::dark();
    theme.metrics.focus_ring.width = 3.0;
    theme.metrics.focus_ring.color = Color::YELLOW;
    let mut d = UiDriver::new((200.0, 100.0)).with_theme(theme);
    let draw = |ui: &mut UiContext<'_>| {
      ui.column().focus_scope().show(|ui| {
        ui.button("A").default_focus().show();
      });
    };
    d.device(InputDevice::Keyboard);
    for _ in 0..3 {
      d.frame(draw);
    }
    let ring = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect {
        border: Some((width, color)),
        fill,
        ..
      } if fill.a == 0 => Some((*width, *color)),
      _ => None,
    });
    assert_eq!(ring, Some((3.0, Color::YELLOW)));
  }

  #[test]
  fn a_disabled_variant_button_draws_the_disabled_look() {
    let mut d = UiDriver::new((200.0, 100.0));
    let at = Rect::new(10.0, 10.0, 80.0, 30.0);
    d.frame(|ui| {
      ui.button("A").at(at).variant("danger").show();
    });
    let enabled = fills(&d);
    d.frame(|ui| {
      ui.button("A")
        .at(at)
        .variant("danger")
        .enabled(false)
        .show();
    });
    let disabled = fills(&d);
    assert_ne!(disabled, enabled);
    assert_eq!(disabled[0].a, 120, "dimmed like any disabled button");
    let text = d.commands.iter().find_map(|(_, c)| match c {
      UiCommand::Text { opacity, .. } => Some(*opacity),
      _ => None,
    });
    assert!(text.is_some_and(|o| o < 0.6), "{text:?}");
  }
}

/// `.at(rect)` on a widget or container inside another container
#[cfg(all(test, feature = "default-fonts"))]
mod at_tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext, UiId};

  const AT: Rect = Rect::new(100.0, 50.0, 40.0, 20.0);

  /// The rects of a column's buttons, the middle one placed at `AT` when `with_at`
  fn column(ui: &mut UiContext<'_>, with_at: bool) -> (Rect, Rect, Rect) {
    let (mut a, mut b, mut c) = (Rect::default(), Rect::default(), Rect::default());
    ui.column().offset(10.0, 10.0).show(|ui| {
      a = ui.button("A").show().rect;
      if with_at {
        b = ui.button("B").at(AT).show().rect;
      }
      c = ui.button("C").show().rect;
    });
    (a, b, c)
  }

  fn settled(with_at: bool) -> (Rect, Rect, Rect) {
    let mut d = UiDriver::new((300.0, 200.0));
    d.frame(|ui| column(ui, with_at));
    d.frame(|ui| column(ui, with_at))
  }

  #[test]
  fn an_at_widget_inside_a_column_is_placed_there_and_takes_no_space() {
    let (_, b, c) = settled(true);
    let (_, _, c_alone) = settled(false);
    assert_eq!(b, AT);
    assert_eq!(
      c, c_alone,
      "the sibling after it sits where it would without it"
    );
  }

  #[test]
  fn an_at_widget_stays_in_its_column_s_focus_scope() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut id = UiId::NONE;
      ui.column().focus_scope().show(|ui| {
        ui.button("A").default_focus().show();
        id = ui.button("B").at(AT).show().id;
      });
      id
    };
    d.frame(draw);
    let b = d.frame(draw);
    d.nav(Direction::Down);
    d.frame(draw);
    assert_eq!(d.memory.interact.focused, b);
  }

  #[test]
  fn a_fill_width_button_respects_a_max_width_below_its_label() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let mut rect = Rect::default();
      ui.column().width(200.0).show(|ui| {
        rect = ui
          .button("A label wider than forty")
          .fill_width()
          .max_width(40.0)
          .show()
          .rect;
      });
      rect
    };
    d.frame(draw);
    let button = d.frame(draw);
    assert_eq!(
      button.width, 40.0,
      "the cap the caller set holds: {button:?}"
    );
  }

  #[test]
  fn an_at_container_inside_a_column_is_placed_there() {
    let mut d = UiDriver::new((300.0, 200.0));
    let draw = |ui: &mut UiContext<'_>| {
      let (mut outer, mut inner) = (Rect::default(), Rect::default());
      ui.column().show(|ui| {
        ui.button("A").show();
        outer = ui
          .column()
          .at(AT)
          .show(|ui| inner = ui.button("In").show().rect)
          .rect;
      });
      (outer, inner)
    };
    d.frame(draw);
    let (outer, inner) = d.frame(draw);
    assert_eq!(outer, AT);
    assert!(
      inner.x >= AT.x && inner.y >= AT.y && inner.y < AT.bottom(),
      "{inner:?}"
    );
  }
}
