//! Scroll view demo page - demonstrates the scroll view widget

use demo_kit::prelude::*;

pub struct ScrollViewDemo {
  /// Main content scroll view (vertical)
  content_scroll: ScrollViewState,
  /// Secondary list scroll view (vertical)
  list_scroll: ScrollViewState,
  /// Horizontal scroll view
  horizontal_scroll: ScrollViewState,
  /// Number of items in the list
  item_count: i32,
  /// Selected item index
  selected_item: Option<usize>,
}

impl ScrollViewDemo {
  pub fn new() -> Self {
    Self {
      content_scroll: ScrollViewState::new(),
      list_scroll: ScrollViewState::new(),
      horizontal_scroll: ScrollViewState::new(),
      item_count: 50,
      selected_item: None,
    }
  }
}

impl DemoPage for ScrollViewDemo {
  fn name(&self) -> &str {
    "Scroll Views"
  }

  fn description(&self) -> &str {
    "Scrollable content in both directions."
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
    // Scroll momentum runs on the UI's real frame time
    let dt = ui.dt();

    // Home/End to scroll to top/bottom (or left/right for horizontal)
    if ui.input().key_pressed(KeyCode::Home) {
      ui.consume_key(KeyCode::Home);
      self.content_scroll.scroll_to_top();
      self.list_scroll.scroll_to_top();
      self.horizontal_scroll.scroll_to_top(); // Scrolls to left
    }
    if ui.input().key_pressed(KeyCode::End) {
      ui.consume_key(KeyCode::End);
      let content_height = 800.0; // Approximate
      self.content_scroll.scroll_to_bottom(content_height, 250.0);
      let list_height = self.item_count.max(0) as f32 * 28.0;
      self.list_scroll.scroll_to_bottom(list_height, 200.0);
      // Scroll horizontal to right
      self.horizontal_scroll.set_scroll_x(800.0); // Content width - view width
    }

    let padding = 8.0; // Consistent padding for all scroll views
    let top = content_area(ui).y;
    let item_count = self.item_count.max(0) as usize;

    // --- Row 1: Vertical scroll views ---

    // Left panel - text content scroll view (vertical)
    ui.draw_text(
      "Vertical Text Scroll:",
      20.0,
      top,
      Color::rgb(100, 180, 255),
    );

    let text_content = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\nDuis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\n\nCurabitur pretium tincidunt lacus. Nulla gravida orci a odio. Nullam varius, turpis et commodo pharetra, est eros bibendum elit, nec luctus magna felis sollicitudin mauris. Integer in mauris eu nibh euismod gravida.\n\nDuis ac tellus et risus vulputate vehicula. Donec lobortis risus a elit. Etiam tempor. Ut ullamcorper, ligula eu tempor congue, eros est euismod turpis, id tincidunt sapien risus a quam.\n\nMaecenas fermentum consequat mi. Donec fermentum. Pellentesque malesuada nulla a mi. Duis sapien sem, aliquet sed, vulputate eget, feugiat non, lacus. Morbi interdum mollis sapien. Sed ac risus.\n\nPraesent nec nisl a purus blandit viverra. Praesent ac massa at ligula laoreet iaculis. Nulla neque dolor, sagittis eget, iaculis quis, molestie non, velit.";

    let text_rect = Rect::new(20.0, top + 18.0, 350.0, 250.0);
    let wrap_width = text_rect.width - 30.0 - padding * 2.0; // Account for scrollbar and padding
    let wrapped_lines = ui.wrap_text(text_content, wrap_width as u32);
    let line_height = 18.0;
    let content_height = wrapped_lines.len() as f32 * line_height + padding * 2.0;

    ui.scroll_view(
      &mut self.content_scroll,
      text_rect,
      content_height,
      dt,
      |ui, scroll_y| {
        let mut y = text_rect.y + padding - scroll_y;
        for line in &wrapped_lines {
          if y > text_rect.y - line_height && y < text_rect.y + text_rect.height {
            ui.draw_text(line, text_rect.x + padding, y, Color::WHITE);
          }
          y += line_height;
        }
      },
    );

    // Right panel - item list scroll view (vertical)
    ui.draw_text(
      &format!("Item List ({} items):", item_count),
      400.0,
      top,
      Color::rgb(100, 180, 255),
    );

    let list_rect = Rect::new(400.0, top + 18.0, 280.0, 250.0);
    let item_height = 28.0;
    let list_content_height = item_count as f32 * item_height + padding * 2.0;

    ui.scroll_view(
      &mut self.list_scroll,
      list_rect,
      list_content_height,
      dt,
      |ui, scroll_y| {
        for i in 0..item_count {
          let y = list_rect.y + padding + i as f32 * item_height - scroll_y;

          // Only process visible items
          if y > list_rect.y - item_height && y < list_rect.y + list_rect.height {
            let item_rect = Rect::new(
              list_rect.x + padding,
              y,
              list_rect.width - 30.0 - padding * 2.0,
              item_height - 2.0,
            );

            // Alternate row colors
            let bg_color = if Some(i) == self.selected_item {
              Color::rgb(60, 100, 160)
            } else if i % 2 == 0 {
              Color::rgb(40, 40, 55)
            } else {
              Color::rgb(35, 35, 48)
            };

            ui.draw_rect(item_rect, bg_color);
            ui.draw_text(
              &format!("  Item #{:03}", i + 1),
              item_rect.x,
              y + 6.0,
              Color::WHITE,
            );
          }
        }
      },
    );

    // --- Row 2: Horizontal scroll view ---
    ui.draw_text(
      "Horizontal Scroll (X-axis):",
      20.0,
      top + 285.0,
      Color::rgb(100, 180, 255),
    );

    let horiz_rect = Rect::new(20.0, top + 303.0, 660.0, 120.0);
    let card_width = 150.0;
    let card_spacing = 10.0;
    let num_cards = 12;
    let horiz_content_width = num_cards as f32 * (card_width + card_spacing) + padding * 2.0;

    // Draw horizontal scroll view using scroll_x
    ui.scroll_view_horizontal(
      &mut self.horizontal_scroll,
      horiz_rect,
      horiz_content_width,
      dt,
      |ui, scroll_x| {
        for i in 0..num_cards {
          let x = horiz_rect.x + padding + i as f32 * (card_width + card_spacing) - scroll_x;
          let card_rect = Rect::new(
            x,
            horiz_rect.y + padding,
            card_width,
            horiz_rect.height - padding * 2.0 - 16.0,
          );

          // Only draw visible cards
          if x > horiz_rect.x - card_width && x < horiz_rect.x + horiz_rect.width {
            // Card background
            let hue = (i as f32 / num_cards as f32 * 360.0) as u8;
            let card_color = Color::rgb(40 + hue / 4, 50 + (255 - hue) / 6, 80 + hue / 3);
            ui.draw_rect(card_rect, card_color);
            ui.draw_rect_outline(card_rect, Color::rgb(100, 100, 120), 1.0);

            // Card title
            ui.draw_text(
              &format!("Card {}", i + 1),
              x + 10.0,
              card_rect.y + 10.0,
              Color::WHITE,
            );
            ui.draw_text(
              "Drag to scroll",
              x + 10.0,
              card_rect.y + 30.0,
              Color::LIGHT_GRAY,
            );
            ui.draw_text(
              "horizontally",
              x + 10.0,
              card_rect.y + 48.0,
              Color::LIGHT_GRAY,
            );
          }
        }
      },
    );

    // Debug info
    let debug_y = top + 440.0;
    ui.draw_text("Scroll State:", 20.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      &format!(
        "Text Y: {:.1}  |  List Y: {:.1}  |  Horiz X: {:.1}",
        self.content_scroll.scroll_y(),
        self.list_scroll.scroll_y(),
        self.horizontal_scroll.scroll_x()
      ),
      30.0,
      debug_y + 18.0,
      Color::WHITE,
    );

    ui.draw_text("Controls:", 520.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      "Mouse wheel | Drag scrollbar | Drag content",
      530.0,
      debug_y + 18.0,
      Color::LIGHT_GRAY,
    );
  }
}
