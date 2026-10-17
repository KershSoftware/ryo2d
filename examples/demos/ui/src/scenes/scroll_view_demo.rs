//! Scroll area demo page - demonstrates `ui.scroll()`

use demo_kit::prelude::*;

pub struct ScrollViewDemo {
  /// The text area (vertical)
  content_scroll: UiId,
  /// The item list (vertical)
  list_scroll: UiId,
  /// The card strip (horizontal)
  horizontal_scroll: UiId,
  /// Number of items in the list
  item_count: i32,
  /// Selected item index
  selected_item: Option<usize>,
}

impl ScrollViewDemo {
  pub fn new() -> Self {
    Self {
      content_scroll: UiId::NONE,
      list_scroll: UiId::NONE,
      horizontal_scroll: UiId::NONE,
      item_count: 50,
      selected_item: None,
    }
  }

  fn scrolls(&self) -> [UiId; 3] {
    [
      self.content_scroll,
      self.list_scroll,
      self.horizontal_scroll,
    ]
  }
}

impl Default for ScrollViewDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for ScrollViewDemo {
  fn name(&self) -> &str {
    "Scroll Views"
  }

  fn description(&self) -> &str {
    "Scrollable content in both directions: the wheel scrolls the area under the pointer, Shift scrolls sideways."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("Items", &mut self.item_count, 5..=200);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Home, "Scroll to the top"),
      Control::key(KeyCode::End, "Scroll to the bottom"),
    ]
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Home/End jump every area to its start or end (offsets are clamped)
    if ui.input().key_pressed(KeyCode::Home) {
      ui.consume_key(KeyCode::Home);
      for id in self.scrolls() {
        ui.set_scroll_offset(id, Vec2::ZERO);
      }
    }
    if ui.input().key_pressed(KeyCode::End) {
      ui.consume_key(KeyCode::End);
      for id in self.scrolls() {
        ui.set_scroll_offset(id, Vec2::splat(1.0e6));
      }
    }

    let padding = 8.0;
    let top = content_area(ui).y;
    let item_count = self.item_count.max(0) as usize;

    // --- Row 1: Vertical scroll areas ---

    // Left panel - text content
    ui.draw_text(
      "Vertical Text Scroll:",
      20.0,
      top,
      Color::rgb(100, 180, 255),
    );

    let text_content = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\nDuis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\n\nCurabitur pretium tincidunt lacus. Nulla gravida orci a odio. Nullam varius, turpis et commodo pharetra, est eros bibendum elit, nec luctus magna felis sollicitudin mauris. Integer in mauris eu nibh euismod gravida.\n\nDuis ac tellus et risus vulputate vehicula. Donec lobortis risus a elit. Etiam tempor. Ut ullamcorper, ligula eu tempor congue, eros est euismod turpis, id tincidunt sapien risus a quam.\n\nMaecenas fermentum consequat mi. Donec fermentum. Pellentesque malesuada nulla a mi. Duis sapien sem, aliquet sed, vulputate eget, feugiat non, lacus. Morbi interdum mollis sapien. Sed ac risus.\n\nPraesent nec nisl a purus blandit viverra. Praesent ac massa at ligula laoreet iaculis. Nulla neque dolor, sagittis eget, iaculis quis, molestie non, velit.";

    let text_rect = Rect::new(20.0, top + 18.0, 350.0, 250.0);
    ui.draw_rect(text_rect, Color::rgb(30, 30, 40));
    self.content_scroll = ui
      .scroll()
      .at(text_rect)
      .padding(padding)
      .show(|ui| {
        ui.text(text_content).color(Color::WHITE);
      })
      .id;

    // Right panel - item list
    ui.draw_text(
      &format!("Item List ({} items):", item_count),
      400.0,
      top,
      Color::rgb(100, 180, 255),
    );

    let list_rect = Rect::new(400.0, top + 18.0, 280.0, 250.0);
    let item_height = 28.0;
    let selected = self.selected_item;
    let mut clicked = None;
    let label = TextStyle::default();
    ui.draw_rect(list_rect, Color::rgb(30, 30, 40));
    self.list_scroll = ui
      .scroll()
      .at(list_rect)
      .padding(padding)
      .gap(2.0)
      .show(|ui| {
        for i in 0..item_count {
          let response = ui
            .custom(0.0, item_height - 2.0)
            .id(i)
            .fill_width()
            .sense(Sense::click())
            .paint(|p, response| {
              let size = p.size();
              // Alternate row colors
              let bg_color = if Some(i) == selected {
                Color::rgb(60, 100, 160)
              } else if response.hovered() {
                Color::rgb(55, 55, 75)
              } else if i % 2 == 0 {
                Color::rgb(40, 40, 55)
              } else {
                Color::rgb(35, 35, 48)
              };
              p.rect(Rect::new(0.0, 0.0, size.x, size.y), bg_color);
              p.text(format!("  Item #{:03}", i + 1), 0.0, 4.0, &label);
            });
          if response.clicked() {
            clicked = Some(i);
          }
        }
      })
      .id;
    if clicked.is_some() {
      self.selected_item = clicked;
    }

    // --- Row 2: Horizontal scroll area ---
    ui.draw_text(
      "Horizontal Scroll (X-axis):",
      20.0,
      top + 285.0,
      Color::rgb(100, 180, 255),
    );

    let horiz_rect = Rect::new(20.0, top + 303.0, 660.0, 120.0);
    let card_width = 150.0;
    let num_cards = 12;
    let card_height = horiz_rect.height - padding * 2.0 - 10.0;
    let card_title = TextStyle::default();
    let card_note = TextStyle::default().color(Color::LIGHT_GRAY);
    ui.draw_rect(horiz_rect, Color::rgb(30, 30, 40));
    self.horizontal_scroll = ui
      .scroll()
      .horizontal()
      .at(horiz_rect)
      .padding(padding)
      .gap(10.0)
      .show(|ui| {
        for i in 0..num_cards {
          ui.custom(card_width, card_height).id(i).paint(|p, _| {
            let size = p.size();
            let card = Rect::new(0.0, 0.0, size.x, size.y);
            let hue = (i as f32 / num_cards as f32 * 360.0) as u8;
            let card_color = Color::rgb(40 + hue / 4, 50 + (255 - hue) / 6, 80 + hue / 3);
            p.bordered_rect(card, card_color, 0.0, 1.0, Color::rgb(100, 100, 120));
            p.text(format!("Card {}", i + 1), 10.0, 10.0, &card_title);
            p.text("Shift + wheel", 10.0, 30.0, &card_note);
            p.text("scrolls sideways", 10.0, 48.0, &card_note);
          });
        }
      })
      .id;

    // Debug info
    let debug_y = top + 440.0;
    ui.draw_text("Scroll State:", 20.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      &format!(
        "Text Y: {:.1}  |  List Y: {:.1}  |  Horiz X: {:.1}",
        ui.scroll_offset(self.content_scroll).y,
        ui.scroll_offset(self.list_scroll).y,
        ui.scroll_offset(self.horizontal_scroll).x
      ),
      30.0,
      debug_y + 18.0,
      Color::WHITE,
    );

    ui.draw_text("Controls:", 520.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      "Mouse wheel | Shift + wheel | Drag scrollbar",
      530.0,
      debug_y + 18.0,
      Color::LIGHT_GRAY,
    );
  }
}
