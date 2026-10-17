//! The slider

use std::ops::RangeInclusive;
use std::panic::Location;

use glam::Vec2;

use super::common::{WidgetOpts, widget_builders};
use crate::core::input::Direction;
use crate::core::text::{RichText, TextLayoutOptions};
use crate::core::ui::immediate::{UiContext, text_seed};
use crate::core::ui::layout::{Align, Edges, Flow, Measure, UiStyle};
use crate::core::ui::memory::Flags;
use crate::core::ui::response::{Response, Sense};
use crate::core::ui::theme::{WidgetKind, warn_once};
use crate::core::ui::tree::{DrawKind, Node, RecordSpec, TextLeaf};

/// The track's thickness inside the slider's node
const TRACK: f32 = 6.0;

/// A slider being set up. It is drawn by [`show`](Self::show) or when it is
/// dropped.
pub struct Slider<'u, 'a> {
  ui: Option<&'u mut UiContext<'a>>,
  site: &'static Location<'static>,
  value: &'u mut f32,
  range: (f32, f32),
  step: Option<f32>,
  show_value: bool,
  label: Option<RichText>,
  opts: WidgetOpts,
}

impl<'a> UiContext<'a> {
  /// A slider writing `value` within `range`. Dragging the track (or a press
  /// on it) sets the value, and Left and Right move it by a twentieth of
  /// the range, or by [`step`](Slider::step), while it has the focus.
  ///
  /// ```ignore
  /// let volume = ui.slider(&mut settings.volume, 0.0..=1.0).step(0.05).show();
  /// if volume.changed() {
  ///     ctx.audio().set_master_volume(settings.volume);
  /// }
  /// ```
  #[track_caller]
  pub fn slider<'u>(
    &'u mut self,
    value: &'u mut f32,
    range: RangeInclusive<f32>,
  ) -> Slider<'u, 'a> {
    Slider {
      ui: Some(self),
      site: Location::caller(),
      value,
      range: (*range.start(), *range.end()),
      step: None,
      show_value: false,
      label: None,
      opts: WidgetOpts::default(),
    }
  }
}

widget_builders!(Slider<'_, '_>);

impl Slider<'_, '_> {
  /// Snap the value to multiples of `step` from the range's start, and move
  /// by `step` with Left and Right
  pub fn step(mut self, step: f32) -> Self {
    self.step = Some(step);
    self
  }

  /// Show the value as text after the track
  pub fn show_value(mut self) -> Self {
    self.show_value = true;
    self
  }

  /// A label before the track
  pub fn label(mut self, label: impl Into<RichText>) -> Self {
    self.label = Some(label.into());
    self
  }

  /// Draw the slider; what happened to it this frame
  pub fn show(mut self) -> Response {
    self.run().unwrap_or_default()
  }

  /// Record the slider, once (`show` and `Drop` both come here)
  fn run(&mut self) -> Option<Response> {
    let ui = self.ui.take()?;
    let (min, max) = bounds(self.range);
    let step = self.step.filter(|step| step.is_finite() && *step > 0.0);
    let theme = ui.theme_arc();
    let sizes = &theme.metrics.sizes;
    let [handle_w, handle_h] = sizes.slider_handle;
    let site = self.site;
    let disabled = self.opts.disabled;

    // A label or a value makes it a row around the track, which keeps the
    // widget's id and record
    let boxed = self.label.is_some() || self.show_value;
    let (row_id, id) = if boxed {
      let row = ui.memory.scopes.make(site, self.opts.key);
      ui.memory.scopes.push(row);
      (Some(row), ui.memory.scopes.make(site, None))
    } else {
      (None, ui.memory.scopes.make(site, self.opts.key))
    };
    let mut response = ui.response(id, Sense::click_and_drag(), disabled);

    // The value: repaired, then moved by the pointer or the keys
    if !self.value.is_finite() {
      *self.value = min;
    }
    let before = *self.value;
    let snap = |v: f32| {
      let v = match step {
        Some(step) => min + ((v - min) / step).round() * step,
        None => v,
      };
      v.clamp(min, max)
    };
    let mut value = before;
    if !disabled {
      let track = ui.memory.prev(id).map(|record| record.rect);
      let left_button = ui.memory.interact.active_button == 0;
      if response.pressed()
        && left_button
        && let (Some(pointer), Some(track)) = (response.pointer, track)
        && track.width > handle_w
      {
        let t =
          ((pointer.x - (track.x + handle_w / 2.0)) / (track.width - handle_w)).clamp(0.0, 1.0);
        value = snap(min + t * (max - min));
      }
      let by = step.unwrap_or((max - min) / 20.0);
      match ui.nav_step_for(id) {
        Some(Direction::Left) => value = snap(value - by),
        Some(Direction::Right) => value = snap(value + by),
        _ => {}
      }
    }
    if value != before {
      *self.value = value;
      response.mark_changed();
    }
    let t = if max > min {
      ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
      0.0
    };

    let state = ui.widget_state(&response, disabled);
    let look = ui.look_of(WidgetKind::Slider, &self.opts, state);
    let dim = f32::from(look.text.a) / 255.0;
    let height = sizes.slider[1].max(handle_h);

    if let Some(row) = row_id {
      let style = UiStyle {
        flow: Flow::Row,
        align: Align::Center,
        gap: theme.metrics.spacing,
        ..self.opts.style.clone()
      };
      let row_node = Node::new(row, style, Measure::None);
      let row_index = ui.push_node(row_node, self.opts.at);
      let body = theme.text.body.style();
      let options = TextLayoutOptions::default();
      if let Some(label) = self.label.take() {
        let seed = text_seed(&label, &body, &options);
        let leaf = TextLeaf {
          text: label,
          style: body.clone(),
          options,
        };
        ui.text_leaf(site, leaf, (dim, None, seed), Vec2::ZERO);
      }
      let track = self.track_node(ui, id, [sizes.slider[0], height], true);
      draw_track(ui, track, &look, (handle_w, handle_h), t);
      if response.focused() && ui.focus_visible() {
        ui.focus_ring(track, look.radius);
      }
      response.rect = ui.close_node(track).rect;
      if self.show_value {
        let text = RichText::new(format!(
          "{:.*}",
          decimals(step.unwrap_or((max - min) / 20.0)),
          value
        ));
        let seed = text_seed(&text, &body, &options);
        let leaf = TextLeaf {
          text,
          style: body,
          options,
        };
        ui.text_leaf(site, leaf, (dim, None, seed), Vec2::ZERO);
      }
      ui.memory.scopes.pop();
      ui.close_node(row_index);
    } else {
      let track = self.track_node(ui, id, [sizes.slider[0], height], false);
      draw_track(ui, track, &look, (handle_w, handle_h), t);
      if response.focused() && ui.focus_visible() {
        ui.focus_ring(track, look.radius);
      }
      response.rect = ui.close_node(track).rect;
    }
    ui.tooltip_tick(self.opts.tooltip.as_ref(), &response);
    Some(response)
  }

  /// The track's node: its own size, recorded for clicks, drags and the
  /// horizontal arrows. Inside a row the row takes the widget's layout
  /// options.
  fn track_node(
    &self,
    ui: &mut UiContext<'_>,
    id: crate::core::ui::UiId,
    size: [f32; 2],
    in_row: bool,
  ) -> usize {
    let style = if in_row {
      UiStyle::default()
    } else {
      self.opts.style.clone()
    };
    let mut node = Node::new(id, style, Measure::Fixed(Vec2::new(size[0], size[1])));
    node.record = Some(RecordSpec {
      flags: self
        .opts
        .flags(Flags::CLICK | Flags::DRAG | Flags::TAKES_HORIZONTAL, true),
      nav: self.opts.nav,
    });
    ui.push_node(node, if in_row { None } else { self.opts.at })
  }
}

impl Drop for Slider<'_, '_> {
  /// A slider not shown is drawn when it is dropped
  fn drop(&mut self) {
    self.run();
  }
}

/// The track's draws: the groove, the filled part up to the handle, and the
/// handle. The handle's centre runs from half its width in from the left
/// edge to half its width in from the right.
fn draw_track(
  ui: &mut UiContext<'_>,
  node: usize,
  look: &crate::core::ui::Look,
  handle: (f32, f32),
  t: f32,
) {
  let (handle_w, handle_h) = handle;
  let radius = look.radius.min(TRACK / 2.0);
  // The groove and its filled part are TRACK thick, centred, whatever the height
  ui.push_draw(
    node,
    DrawKind::Fraction {
      inset: Edges::ZERO,
      to: 1.0,
      band: Some(TRACK),
      fill: look.fill,
      radius,
    },
  );
  ui.push_draw(
    node,
    DrawKind::Fraction {
      inset: Edges::new(handle_w / 2.0, 0.0, handle_w / 2.0, 0.0),
      to: t,
      band: Some(TRACK),
      fill: look.border,
      radius,
    },
  );
  ui.push_draw(
    node,
    DrawKind::Box {
      inset: Edges::ZERO,
      frac: Vec2::new(t, 0.5),
      size: Vec2::new(handle_w, handle_h),
      fill: look.text,
      radius: look.radius.min(handle_w / 2.0),
      border: None,
    },
  );
}

/// `range` with its ends in order; a bound that is not a number makes it
/// 0 to 1 (warned once)
fn bounds((start, end): (f32, f32)) -> (f32, f32) {
  if !(start.is_finite() && end.is_finite()) {
    warn_once("slider-range".to_string(), || {
      format!("a slider's range {start}..={end} is not finite; using 0..=1")
    });
    return (0.0, 1.0);
  }
  (start.min(end), start.max(end))
}

/// How many decimals show a multiple of `step`: none for whole steps
fn decimals(step: f32) -> usize {
  (0..=6)
    .find(|&places| {
      let scaled = step * 10f32.powi(places as i32);
      (scaled - scaled.round()).abs() < 1e-3 * scaled.abs().max(1.0)
    })
    .unwrap_or(6)
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use crate::core::input::Direction;
  use crate::core::ui::test_util::UiDriver;
  use crate::core::ui::{Rect, UiContext};

  const TRACK: Rect = Rect::new(20.0, 20.0, 112.0, 20.0);

  fn slider(ui: &mut UiContext<'_>, value: &mut f32) -> bool {
    let mut changed = false;
    // `at` takes the slider out of its column's layout, but not out of its focus scope
    ui.column().focus_scope().show(|ui| {
      changed = ui
        .slider(value, 0.0..=1.0)
        .at(TRACK)
        .default_focus()
        .show()
        .changed();
    });
    changed
  }

  #[test]
  fn dragging_a_slider_sets_its_value() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.0;
    d.frame(|ui| slider(ui, &mut v));
    // The handle is 12 wide: x 26 is t = 0, x 126 is t = 1
    d.move_to(76.0, 30.0);
    d.press();
    assert!(d.frame(|ui| slider(ui, &mut v)));
    assert!((v - 0.5).abs() < 0.01, "{v}");
    d.move_to(500.0, 30.0);
    d.frame(|ui| slider(ui, &mut v));
    assert_eq!(v, 1.0, "clamped");
  }

  #[test]
  fn left_and_right_step_a_focused_slider() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.5;
    d.frame(|ui| slider(ui, &mut v));
    d.frame(|ui| slider(ui, &mut v));
    d.nav(Direction::Right);
    d.frame(|ui| slider(ui, &mut v));
    assert!((v - 0.55).abs() < 1e-5, "a twentieth: {v}");
    d.nav(Direction::Left);
    d.frame(|ui| slider(ui, &mut v));
    assert!((v - 0.5).abs() < 1e-5);
    assert!(
      d.last_outcome().used.contains(&"ui.left"),
      "the slider took Left, so gameplay does not see it"
    );
  }

  #[test]
  fn a_slider_snaps_to_its_step() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.0;
    let draw = |ui: &mut UiContext<'_>, v: &mut f32| {
      ui.slider(v, 0.0..=10.0).step(2.5).at(TRACK).show();
    };
    d.frame(|ui| draw(ui, &mut v));
    d.move_to(60.0, 30.0); // t = 0.34
    d.press();
    d.frame(|ui| draw(ui, &mut v));
    assert_eq!(v, 2.5);
  }

  #[test]
  fn a_bad_range_or_value_is_made_safe() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = f32::NAN;
    d.frame(|ui| {
      ui.slider(&mut v, 5.0..=1.0).at(TRACK).show();
    });
    assert_eq!(v, 1.0, "NaN becomes the minimum of the swapped range");
  }

  #[test]
  fn a_disabled_slider_ignores_input() {
    let mut d = UiDriver::new((200.0, 100.0));
    let mut v = 0.2;
    let draw = |ui: &mut UiContext<'_>, v: &mut f32| {
      ui.slider(v, 0.0..=1.0).at(TRACK).enabled(false).show();
    };
    d.frame(|ui| draw(ui, &mut v));
    d.move_to(100.0, 30.0);
    d.press();
    d.frame(|ui| draw(ui, &mut v));
    assert_eq!(v, 0.2);
  }

  #[test]
  fn the_slider_groove_is_six_tall_at_any_height() {
    use crate::core::ui::immediate::UiCommand;
    for height in [20.0, 8.0] {
      let mut d = UiDriver::new((300.0, 100.0));
      let mut v = 0.5;
      d.frame(|ui| {
        ui.slider(&mut v, 0.0..=1.0)
          .at(Rect::new(0.0, 10.0, 100.0, height))
          .show();
      });
      let rects: Vec<Rect> = d
        .commands
        .iter()
        .filter_map(|(_, c)| match c {
          UiCommand::Rect { rect, .. } => Some(*rect),
          _ => None,
        })
        .collect();
      // The groove, the filled part, the handle
      assert_eq!(rects.len(), 3, "{height}: {rects:?}");
      for groove in &rects[..2] {
        assert_eq!(groove.height, 6.0, "{height}: {rects:?}");
        assert_eq!(groove.y + 3.0, 10.0 + height / 2.0, "centred: {rects:?}");
      }
    }
  }

  #[test]
  fn a_fill_width_slider_in_a_fit_column_keeps_its_width() {
    use crate::core::ui::Theme;
    let width = Theme::dark().metrics.sizes.slider[0];
    let mut d = UiDriver::new((300.0, 100.0));
    let mut v = 0.5;
    let mut draw = |ui: &mut UiContext<'_>| {
      let mut track = Rect::default();
      let column = ui
        .column()
        .show(|ui| track = ui.slider(&mut v, 0.0..=1.0).fill_width().show().rect)
        .rect;
      (column, track)
    };
    d.frame(&mut draw);
    let (column, track) = d.frame(&mut draw);
    assert_eq!(column.width, width, "the column fits the slider");
    assert_eq!(track.width, width, "the slider keeps its own width");
  }

  #[test]
  fn a_slider_can_show_its_label_and_value() {
    use crate::core::ui::immediate::UiCommand;
    let mut d = UiDriver::new((300.0, 100.0));
    let mut v = 0.5;
    d.frame(|ui| {
      ui.slider(&mut v, 0.0..=1.0)
        .step(0.05)
        .label("Volume")
        .show_value()
        .at(Rect::new(0.0, 0.0, 300.0, 30.0))
        .show();
    });
    let texts: Vec<String> = d
      .commands
      .iter()
      .filter_map(|(_, c)| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, vec!["Volume".to_string(), "0.50".to_string()]);
  }
}
