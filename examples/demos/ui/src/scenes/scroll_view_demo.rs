//! Scroll view demo scene - demonstrates the scroll view widget

use ryo2d::prelude::*;

pub struct ScrollViewDemo {
  /// Main content scroll view (vertical)
  content_scroll: ScrollViewState,
  /// Secondary list scroll view (vertical)
  list_scroll: ScrollViewState,
  /// Horizontal scroll view
  horizontal_scroll: ScrollViewState,
  /// Number of items in the list
  item_count: usize,
  /// Selected item index
  selected_item: Option<usize>,
  /// Delta time from last frame
  last_dt: f32,
}

impl ScrollViewDemo {
  pub fn new() -> Self {
    Self {
      content_scroll: ScrollViewState::new(),
      list_scroll: ScrollViewState::new(),
      horizontal_scroll: ScrollViewState::new(),
      item_count: 50,
      selected_item: None,
      last_dt: 0.016,
    }
  }
}

impl Scene for ScrollViewDemo {
  fn name(&self) -> &str {
    "Scroll View Demo"
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    self.last_dt = dt;

    // Press Escape to go back to main menu
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Pop;
    }

    // Add/remove items with +/- keys
    if ctx.input.key_pressed(KeyCode::Equal) || ctx.input.key_pressed(KeyCode::NumpadAdd) {
      self.item_count = (self.item_count + 10).min(200);
    }
    if ctx.input.key_pressed(KeyCode::Minus) || ctx.input.key_pressed(KeyCode::NumpadSubtract) {
      self.item_count = self.item_count.saturating_sub(10).max(5);
    }

    // Home/End to scroll to top/bottom (or left/right for horizontal)
    if ctx.input.key_pressed(KeyCode::Home) {
      self.content_scroll.scroll_to_top();
      self.list_scroll.scroll_to_top();
      self.horizontal_scroll.scroll_to_top(); // Scrolls to left
    }
    if ctx.input.key_pressed(KeyCode::End) {
      let content_height = 800.0; // Approximate
      self.content_scroll.scroll_to_bottom(content_height, 250.0);
      let list_height = self.item_count as f32 * 28.0;
      self.list_scroll.scroll_to_bottom(list_height, 200.0);
      // Scroll horizontal to right
      self.horizontal_scroll.set_scroll_x(800.0); // Content width - view width
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let dt = self.last_dt;
    let padding = 8.0; // Consistent padding for all scroll views

    // Title
    ui.draw_text("Scroll View Demo", 20.0, 15.0, Color::WHITE);
    ui.draw_text("Press ESC to return | +/- items | Home/End jump", 20.0, 35.0, Color::GRAY);

    // --- Row 1: Vertical scroll views ---

    // Left panel - text content scroll view (vertical)
    ui.draw_text("Vertical Text Scroll:", 20.0, 60.0, Color::rgb(100, 180, 255));

    let text_content = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\nDuis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\n\nCurabitur pretium tincidunt lacus. Nulla gravida orci a odio. Nullam varius, turpis et commodo pharetra, est eros bibendum elit, nec luctus magna felis sollicitudin mauris. Integer in mauris eu nibh euismod gravida.\n\nDuis ac tellus et risus vulputate vehicula. Donec lobortis risus a elit. Etiam tempor. Ut ullamcorper, ligula eu tempor congue, eros est euismod turpis, id tincidunt sapien risus a quam.\n\nMaecenas fermentum consequat mi. Donec fermentum. Pellentesque malesuada nulla a mi. Duis sapien sem, aliquet sed, vulputate eget, feugiat non, lacus. Morbi interdum mollis sapien. Sed ac risus.\n\nPraesent nec nisl a purus blandit viverra. Praesent ac massa at ligula laoreet iaculis. Nulla neque dolor, sagittis eget, iaculis quis, molestie non, velit.";

    let text_rect = Rect::new(20.0, 78.0, 350.0, 250.0);
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
      &format!("Item List ({} items):", self.item_count),
      400.0,
      60.0,
      Color::rgb(100, 180, 255),
    );

    let list_rect = Rect::new(400.0, 78.0, 280.0, 250.0);
    let item_height = 28.0;
    let list_content_height = self.item_count as f32 * item_height + padding * 2.0;

    ui.scroll_view(
      &mut self.list_scroll,
      list_rect,
      list_content_height,
      dt,
      |ui, scroll_y| {
        for i in 0..self.item_count {
          let y = list_rect.y + padding + i as f32 * item_height - scroll_y;

          // Only process visible items
          if y > list_rect.y - item_height && y < list_rect.y + list_rect.height {
            let item_rect = Rect::new(list_rect.x + padding, y, list_rect.width - 30.0 - padding * 2.0, item_height - 2.0);

            // Alternate row colors
            let bg_color = if Some(i) == self.selected_item {
              Color::rgb(60, 100, 160)
            } else if i % 2 == 0 {
              Color::rgb(40, 40, 55)
            } else {
              Color::rgb(35, 35, 48)
            };

            ui.draw_rect(item_rect, bg_color);
            ui.draw_text(&format!("  Item #{:03}", i + 1), item_rect.x, y + 6.0, Color::WHITE);
          }
        }
      },
    );

    // --- Row 2: Horizontal scroll view ---
    ui.draw_text("Horizontal Scroll (X-axis):", 20.0, 345.0, Color::rgb(100, 180, 255));

    let horiz_rect = Rect::new(20.0, 363.0, 660.0, 120.0);
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
          let card_rect = Rect::new(x, horiz_rect.y + padding, card_width, horiz_rect.height - padding * 2.0 - 16.0);

          // Only draw visible cards
          if x > horiz_rect.x - card_width && x < horiz_rect.x + horiz_rect.width {
            // Card background
            let hue = (i as f32 / num_cards as f32 * 360.0) as u8;
            let card_color = Color::rgb(40 + hue / 4, 50 + (255 - hue) / 6, 80 + hue / 3);
            ui.draw_rect(card_rect, card_color);
            ui.draw_rect_outline(card_rect, Color::rgb(100, 100, 120), 1.0);

            // Card title
            ui.draw_text(&format!("Card {}", i + 1), x + 10.0, card_rect.y + 10.0, Color::WHITE);
            ui.draw_text("Drag to scroll", x + 10.0, card_rect.y + 30.0, Color::LIGHT_GRAY);
            ui.draw_text("horizontally", x + 10.0, card_rect.y + 48.0, Color::LIGHT_GRAY);
          }
        }
      },
    );

    // Debug info
    let debug_y = 500.0;
    ui.draw_text("Scroll State:", 20.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text(
      &format!("Text Y: {:.1}  |  List Y: {:.1}  |  Horiz X: {:.1}",
        self.content_scroll.scroll_y(),
        self.list_scroll.scroll_y(),
        self.horizontal_scroll.scroll_x()),
      30.0,
      debug_y + 18.0,
      Color::WHITE,
    );

    ui.draw_text("Controls:", 520.0, debug_y, Color::rgb(100, 180, 255));
    ui.draw_text("Mouse wheel | Drag scrollbar | Drag content", 530.0, debug_y + 18.0, Color::LIGHT_GRAY);
  }
}
