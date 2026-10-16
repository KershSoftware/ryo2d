//! Containers: `column`, `row`, `grid`, `stack` and `panel`, laid out by the
//! layout engine when a top-level container's `.show()` returns

use std::hash::Hash;
use std::panic::Location;

use glam::Vec2;

use super::id::key_hash;
use super::immediate::UiContext;
use super::layout::{Align, Edges, Flow, Justify, Length, Tracks, UiStyle};
use super::memory::Flags;
use super::response::Response;
use super::theme::{WidgetKind, WidgetState};
use super::{Anchor, Rect};

/// A container being set up: `.show(|ui| ..)` lays its contents out
#[must_use = "call .show(|ui| ..) to draw the container"]
pub struct Container<'u, 'a> {
  ui: &'u mut UiContext<'a>,
  site: &'static Location<'static>,
  kind: ContainerKind,
  style: UiStyle,
  key: Option<u64>,
  flags: Flags,
  at: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
  Plain,
  Panel,
}

impl<'a> UiContext<'a> {
  /// A column: its children top to bottom, as wide as the widest (and
  /// stretched to it). Sizes to its content unless told otherwise.
  ///
  /// ```ignore
  /// ui.column().gap(8.0).show(|ui| {
  ///     ui.text("Volume");
  ///     ui.text("Music");
  /// });
  /// ```
  #[track_caller]
  pub fn column<'u>(&'u mut self) -> Container<'u, 'a> {
    Container::new(self, Location::caller(), Flow::Column)
  }

  /// A row: its children left to right, as tall as the tallest (and
  /// stretched to it)
  #[track_caller]
  pub fn row<'u>(&'u mut self) -> Container<'u, 'a> {
    Container::new(self, Location::caller(), Flow::Row)
  }

  /// A grid of `columns` equal columns, filled row by row
  /// ([`tracks`](Container::tracks) sets each column's width)
  #[track_caller]
  pub fn grid<'u>(&'u mut self, columns: u16) -> Container<'u, 'a> {
    Container::new(self, Location::caller(), Flow::Grid(Tracks::Equal(columns)))
  }

  /// A stack: its children over each other, each placed by its anchor and
  /// moved by its offset
  #[track_caller]
  pub fn stack<'u>(&'u mut self) -> Container<'u, 'a> {
    Container::new(self, Location::caller(), Flow::Stack)
  }

  /// A panel: a column with the theme's panel look and padding. It blocks
  /// the pointer from what is under it.
  #[track_caller]
  pub fn panel<'u>(&'u mut self) -> Container<'u, 'a> {
    let padding = self.panel_padding();
    let mut panel = Container::new(self, Location::caller(), Flow::Column);
    panel.kind = ContainerKind::Panel;
    panel.style.padding = padding;
    panel.flags = Flags::BLOCKING;
    panel
  }

  /// A panel's padding: its look's own, else the theme's panel padding
  fn panel_padding(&self) -> Edges {
    let theme = self.theme();
    let look = &theme.widgets.panel.normal;
    if look.padding == Edges::ZERO {
      Edges::all(theme.metrics.sizes.panel_padding)
    } else {
      look.padding
    }
  }

  /// Draw the theme's panel look on `node`
  pub(crate) fn panel_fill(&mut self, node: usize) {
    let look = self
      .theme()
      .look(WidgetKind::Panel, None, WidgetState::Normal);
    self.push_look_box(node, &look);
  }
}

impl<'u, 'a> Container<'u, 'a> {
  fn new(ui: &'u mut UiContext<'a>, site: &'static Location<'static>, flow: Flow) -> Self {
    Container {
      ui,
      site,
      kind: ContainerKind::Plain,
      style: UiStyle {
        flow,
        ..UiStyle::default()
      },
      key: None,
      flags: Flags::default(),
      at: None,
    }
  }

  /// Its width: UI units (`120.0`), a fraction of its parent
  /// (`Length::Pct(0.5)`), `Length::Fill` or `Length::Fit` (the default)
  pub fn width(mut self, w: impl Into<Length>) -> Self {
    self.style.width = w.into();
    self
  }

  /// Its height, as [`width`](Self::width)
  pub fn height(mut self, h: impl Into<Length>) -> Self {
    self.style.height = h.into();
    self
  }

  /// Take the width its parent has left (all of the UI's at the top level)
  pub fn fill_width(self) -> Self {
    self.width(Length::Fill)
  }

  /// Take the height its parent has left (all of the UI's at the top level)
  pub fn fill_height(self) -> Self {
    self.height(Length::Fill)
  }

  /// Take the width and height its parent has left
  pub fn fill(self) -> Self {
    self.fill_width().fill_height()
  }

  /// At least this wide
  pub fn min_width(mut self, w: f32) -> Self {
    self.style.min_width = Some(w);
    self
  }

  /// At most this wide
  pub fn max_width(mut self, w: f32) -> Self {
    self.style.max_width = Some(w);
    self
  }

  /// At least this tall
  pub fn min_height(mut self, h: f32) -> Self {
    self.style.min_height = Some(h);
    self
  }

  /// At most this tall
  pub fn max_height(mut self, h: f32) -> Self {
    self.style.max_height = Some(h);
    self
  }

  /// Room between its edges and its children: `8.0`, `(x, y)` or `Edges`
  pub fn padding(mut self, p: impl Into<Edges>) -> Self {
    self.style.padding = p.into();
    self
  }

  /// Room around it, outside its edges
  pub fn margin(mut self, m: impl Into<Edges>) -> Self {
    self.style.margin = m.into();
    self
  }

  /// Room between its children
  pub fn gap(mut self, g: f32) -> Self {
    self.style.gap = g;
    self
  }

  /// Where its children sit across its flow (default `Stretch`)
  pub fn align(mut self, a: Align) -> Self {
    self.style.align = a;
    self
  }

  /// How its children are spread along its flow (default `Start`)
  pub fn justify(mut self, j: Justify) -> Self {
    self.style.justify = j;
    self
  }

  /// Its children centred both ways
  pub fn center(self) -> Self {
    self.align(Align::Center).justify(Justify::Center)
  }

  /// How much of the room its parent has left it takes, shared by weight
  pub fn grow(mut self, g: f32) -> Self {
    self.style.grow = g;
    self
  }

  /// Children that do not fit on its line wrap onto the next
  pub fn wrap_items(mut self) -> Self {
    self.style.wrap = true;
    self
  }

  /// Keep its width to height at `ratio`
  pub fn aspect(mut self, ratio: f32) -> Self {
    self.style.aspect = Some(ratio);
    self
  }

  /// A grid with these column widths
  pub fn tracks(mut self, widths: &[f32]) -> Self {
    self.style.flow = Flow::Grid(Tracks::Fixed(widths.to_vec()));
    self
  }

  /// Where it sits in a stack, or in the UI at the top level (default the
  /// top-left)
  pub fn anchor(mut self, a: Anchor) -> Self {
    self.style.anchor = a;
    self
  }

  /// Moved by this much from where its anchor puts it (in a stack or at the
  /// top level)
  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.style.offset = Vec2::new(x, y);
    self
  }

  /// Placed at `rect`, its size too (at the top level)
  pub fn at(mut self, rect: Rect) -> Self {
    self.at = Some(rect);
    self
  }

  /// Its id from `key` instead of how often this line ran: for containers
  /// made in a loop over items that can reorder
  pub fn id(mut self, key: impl Hash) -> Self {
    self.key = Some(key_hash(&key));
    self
  }

  /// A focus scope: keyboard and pad navigation move between the widgets
  /// inside it
  pub fn focus_scope(mut self) -> Self {
    self.flags |= Flags::FOCUS_SCOPE;
    self
  }

  /// Navigation cannot leave it
  pub fn trap(mut self) -> Self {
    self.flags |= Flags::TRAP;
    self
  }

  /// Navigation past its last widget wraps to its first
  pub fn wrap_focus(mut self) -> Self {
    self.flags |= Flags::WRAP;
    self
  }

  /// It takes the pointer, so what is under it does not
  pub fn block_pointer(mut self) -> Self {
    self.flags |= Flags::BLOCKING;
    self
  }

  /// Draw the container with `contents` inside. A top-level container is
  /// laid out when this returns; its response has this frame's rect.
  pub fn show(self, contents: impl FnOnce(&mut UiContext<'a>)) -> Response {
    let Container {
      ui,
      site,
      kind,
      style,
      key,
      flags,
      at,
    } = self;
    let id = ui.memory.scopes.make(site, key);
    let node = ui.open_node(id, style, at, flags);
    if kind == ContainerKind::Panel {
      ui.panel_fill(node);
    }
    ui.memory.scopes.push(id);
    contents(ui);
    ui.memory.scopes.pop();
    ui.close_node(node)
  }
}

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;
  use crate::core::text::TextStyle;
  use crate::core::ui::immediate::{UiCommand, UiCommands};
  use crate::core::ui::test_util::{context, text_engine};
  use crate::core::ui::{Anchor, Color};

  fn texts(commands: &UiCommands) -> Vec<(u32, Vec2)> {
    commands
      .iter()
      .filter_map(|(order, c)| match c {
        UiCommand::Text { origin, .. } => Some((order, *origin)),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_column_places_texts_top_to_bottom_with_its_gap() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let line = ui.line_height(&TextStyle::default());
    ui.column().gap(10.0).show(|ui| {
      ui.text("first");
      ui.text("second");
    });
    drop(ui);
    let t = texts(&commands);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].1, Vec2::ZERO);
    assert!((t[1].1.y - (line + 10.0)).abs() < 1.0, "{t:?}");
  }

  #[test]
  fn a_centred_panel_sits_in_the_middle() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui
      .panel()
      .width(100.0)
      .height(40.0)
      .anchor(Anchor::Center)
      .show(|_| {});
    drop(ui);
    assert_eq!(response.rect, Rect::new(150.0, 130.0, 100.0, 40.0));
    let rect = commands.iter().find_map(|(_, c)| match c {
      UiCommand::Rect { rect, .. } => Some(*rect),
      _ => None,
    });
    assert_eq!(rect, Some(Rect::new(150.0, 130.0, 100.0, 40.0)));
  }

  #[test]
  fn an_offset_moves_an_anchored_block() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui
      .panel()
      .width(50.0)
      .height(20.0)
      .anchor(Anchor::TopRight)
      .offset(-16.0, 16.0)
      .show(|_| {});
    assert_eq!(response.rect, Rect::new(334.0, 16.0, 50.0, 20.0));
  }

  #[test]
  fn a_grid_places_cells_in_rows() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut ids = Vec::new();
    ui.grid(3).tracks(&[40.0, 40.0, 40.0]).gap(5.0).show(|ui| {
      for i in 0..5 {
        ids.push(ui.panel().width(40.0).height(30.0).id(i).show(|_| {}).id);
      }
    });
    // Inside a container a response carries last frame's rect; this frame's
    // records are written when the grid closes
    let rect = |id| ui.memory_for_test().current(id).map(|r| r.rect).unwrap();
    assert_eq!(rect(ids[0]), Rect::new(0.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[1]), Rect::new(45.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[2]), Rect::new(90.0, 0.0, 40.0, 30.0));
    assert_eq!(rect(ids[3]), Rect::new(0.0, 35.0, 40.0, 30.0));
  }

  #[test]
  fn draws_keep_submission_order_across_absolute_draws() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    ui.column().show(|ui| {
      ui.text("a");
      ui.draw_rect(Rect::new(0.0, 0.0, 5.0, 5.0), Color::RED);
      ui.text("b");
    });
    drop(ui);
    let mut kinds: Vec<(u32, &'static str)> = commands
      .iter()
      .map(|(order, c)| (order, c.kind_name()))
      .collect();
    kinds.sort();
    assert_eq!(
      kinds.iter().map(|k| k.1).collect::<Vec<_>>(),
      vec!["text", "rect", "text"]
    );
  }

  #[test]
  fn text_in_a_narrow_column_wraps() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    ui.column().width(60.0).show(|ui| {
      ui.text("several words that cannot fit on one line");
    });
    drop(ui);
    let lines = commands.iter().find_map(|(_, c)| match c {
      UiCommand::Text { layout, .. } => Some(layout.lines.len()),
      _ => None,
    });
    assert!(lines.unwrap_or(0) > 2);
  }

  #[test]
  fn an_at_rect_container_is_placed_there() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let response = ui
      .panel()
      .at(Rect::new(10.0, 20.0, 80.0, 60.0))
      .show(|_| {});
    assert_eq!(response.rect, Rect::new(10.0, 20.0, 80.0, 60.0));
  }

  #[test]
  fn panels_block_and_scopes_are_explicit() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let panel = ui.panel().width(10.0).height(10.0).show(|_| {});
    let menu = ui.column().focus_scope().trap().show(|_| {});
    let record = |id| ui.memory_for_test().current(id).copied().unwrap();
    assert!(record(panel.id).flags.contains(Flags::BLOCKING));
    assert!(
      !record(panel.id).flags.contains(Flags::FOCUS_SCOPE),
      "panels are not scopes unless marked"
    );
    assert!(
      record(menu.id)
        .flags
        .contains(Flags::FOCUS_SCOPE | Flags::TRAP)
    );
  }

  #[test]
  fn a_scope_keys_the_ids_inside_it() {
    fn panel_in(ui: &mut UiContext<'_>, key: &str) -> crate::core::ui::UiId {
      ui.scope(key, |ui| ui.panel().show(|_| {}).id)
    }
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let (a, b) = (panel_in(&mut ui, "a"), panel_in(&mut ui, "b"));
    drop(ui);
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let (b2, a2) = (panel_in(&mut ui, "b"), panel_in(&mut ui, "a"));
    assert_ne!(a, b);
    assert_eq!(
      (a, b),
      (a2, b2),
      "the key, not the call order, names the scope"
    );
  }

  #[test]
  fn the_same_code_in_two_scenes_gets_two_ids() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui = context(&mut commands, &mut text, (400.0, 300.0));
    let mut ids = Vec::new();
    for scene in [1, 2] {
      ui.begin_scene(scene, scene == 2);
      ids.push(ui.panel().show(|_| {}).id);
      ui.end_scene();
    }
    assert_ne!(ids[0], ids[1]);
  }

  #[test]
  fn rects_snap_to_render_pixels_at_fractional_scale() {
    let (mut commands, mut text) = (UiCommands::default(), text_engine());
    let mut ui =
      crate::core::ui::test_util::context_scaled(&mut commands, &mut text, (400.0, 300.0), 1.25);
    let rect = ui
      .panel()
      .width(33.3)
      .height(10.1)
      .offset(0.3, 0.3)
      .show(|_| {})
      .rect;
    for edge in [rect.x, rect.y, rect.right(), rect.bottom()] {
      let px = edge * 1.25;
      assert!(
        (px - px.round()).abs() < 1e-3,
        "{edge} is not on a render pixel"
      );
    }
  }

  #[test]
  fn a_skinned_panel_draws_a_nine_slice_in_place_of_its_fill() {
    use crate::core::asset::SpriteHandle;
    use crate::core::graphics::Sprite;
    use crate::core::render::batch::Filter;
    use crate::core::ui::test_util::UiDriver;
    use crate::core::ui::theme::{Skin, Theme};
    let sprite =
      SpriteHandle::from_sprite(Sprite::solid_rect(12, 12, 255, 255, 255, 255), "test-skin");
    let mut theme = Theme::dark().skin(
      "frame",
      Skin::from_sprite(sprite, [4.0; 4], Filter::Nearest),
    );
    theme.widgets.panel.normal.skin = Some("frame".to_string());
    let mut d = UiDriver::new((200.0, 100.0)).with_theme(theme);
    d.frame(|ui| {
      ui.panel()
        .at(Rect::new(10.0, 10.0, 80.0, 40.0))
        .show(|_| {});
    });
    let sprites = d
      .commands
      .iter()
      .filter(|(_, c)| matches!(c, UiCommand::Sprite { .. }))
      .count();
    let rects = d
      .commands
      .iter()
      .filter(|(_, c)| matches!(c, UiCommand::Rect { .. }))
      .count();
    assert_eq!(
      (sprites, rects),
      (9, 0),
      "nine sprite pieces and no plain fill"
    );
  }
}
