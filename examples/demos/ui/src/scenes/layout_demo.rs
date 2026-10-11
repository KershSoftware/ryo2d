//! Layout demo page - demonstrates HBox, VBox, and Stack layout containers

use demo_kit::prelude::*;

pub struct LayoutDemo {
  // Demo state for interactive elements
  slider_values: [f32; 3],
  checkbox_states: [bool; 3],
  toggle_state: bool,
  hbox_spacing: f32,
  vbox_spacing: f32,
}

impl LayoutDemo {
  pub fn new() -> Self {
    Self {
      slider_values: [0.3, 0.6, 0.9],
      checkbox_states: [true, false, true],
      toggle_state: true,
      hbox_spacing: 8.0,
      vbox_spacing: 6.0,
    }
  }
}

impl Default for LayoutDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for LayoutDemo {
  fn name(&self) -> &str {
    "Layout"
  }

  fn description(&self) -> &str {
    "HBox, VBox and Stack positioning."
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();

    // Background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      Color::rgb(25, 25, 35),
    );

    // =====================================================================
    // Section 1: HBox Demo
    // =====================================================================
    let section1_y = 90.0;
    ui.draw_text("HBox - Horizontal Layout", 20.0, section1_y, Color::WHITE);

    // Simple row of buttons
    let mut row = HBox::new(20.0, section1_y + 25.0, self.hbox_spacing);
    ui.button("Button A", row.next(80.0, 30.0));
    ui.button("Button B", row.next(80.0, 30.0));
    ui.button("Button C", row.next(80.0, 30.0));
    ui.button("Button D", row.next(80.0, 30.0));

    // Show bounds
    let bounds = row.bounds();
    ui.draw_rect_outline(bounds.expand(4.0), Color::rgb(60, 60, 80), 1.0);
    ui.draw_text(
      &format!("bounds: {}x{}", bounds.width as i32, bounds.height as i32),
      bounds.right() + 20.0,
      bounds.y + 8.0,
      Color::GRAY,
    );

    // Row with mixed widget sizes
    let mut row2 = HBox::new(20.0, section1_y + 70.0, 10.0).with_valign(VAlign::Center);
    ui.draw_text(
      "Label:",
      row2.next(50.0, 20.0).x,
      row2.y() + 6.0,
      Color::WHITE,
    );
    ui.slider(row2.next(150.0, 8.0), &mut self.slider_values[0], 0.0, 1.0);
    ui.checkbox(row2.next(20.0, 20.0), &mut self.checkbox_states[0], None);
    ui.toggle(row2.next(44.0, 24.0), &mut self.toggle_state);
    ui.button("Action", row2.next(70.0, 28.0));

    // =====================================================================
    // Section 2: VBox Demo
    // =====================================================================
    let section2_y = 200.0;
    ui.draw_text("VBox - Vertical Layout", 20.0, section2_y, Color::WHITE);

    // Settings-style column
    let mut col = VBox::new(20.0, section2_y + 25.0, self.vbox_spacing);

    // Each row is an HBox inside the VBox with vertical centering
    let row_rect = col.next(300.0, 24.0);
    let mut setting_row = HBox::new(row_rect.x, row_rect.y, 10.0).with_valign(VAlign::Center);
    let label_rect = setting_row.next(80.0, 24.0);
    ui.draw_text(
      "Volume:",
      label_rect.x,
      label_rect.y + 4.0,
      Color::LIGHT_GRAY,
    );
    ui.slider(
      setting_row.next(150.0, 8.0),
      &mut self.slider_values[1],
      0.0,
      1.0,
    );
    let pct_rect = setting_row.next(45.0, 24.0);
    ui.draw_text(
      &format!("{:.0}%", self.slider_values[1] * 100.0),
      pct_rect.x,
      pct_rect.y + 4.0,
      Color::GRAY,
    );

    let row_rect = col.next(300.0, 24.0);
    let mut setting_row = HBox::new(row_rect.x, row_rect.y, 10.0).with_valign(VAlign::Center);
    let label_rect = setting_row.next(80.0, 24.0);
    ui.draw_text(
      "Brightness:",
      label_rect.x,
      label_rect.y + 4.0,
      Color::LIGHT_GRAY,
    );
    ui.slider(
      setting_row.next(150.0, 8.0),
      &mut self.slider_values[2],
      0.0,
      1.0,
    );
    let pct_rect = setting_row.next(45.0, 24.0);
    ui.draw_text(
      &format!("{:.0}%", self.slider_values[2] * 100.0),
      pct_rect.x,
      pct_rect.y + 4.0,
      Color::GRAY,
    );

    let row_rect = col.next(300.0, 24.0);
    ui.checkbox(
      Rect::new(row_rect.x, row_rect.y, 20.0, 20.0),
      &mut self.checkbox_states[1],
      Some("Enable Sound Effects"),
    );

    let row_rect = col.next(300.0, 24.0);
    ui.checkbox(
      Rect::new(row_rect.x, row_rect.y, 20.0, 20.0),
      &mut self.checkbox_states[2],
      Some("Show FPS Counter"),
    );

    // Show bounds
    let vbox_bounds = col.bounds();
    ui.draw_rect_outline(vbox_bounds.expand(4.0), Color::rgb(60, 60, 80), 1.0);

    // =====================================================================
    // Section 3: Stack Demo
    // =====================================================================
    let section3_x = 380.0;
    let section3_y = 200.0;
    ui.draw_text(
      "Stack - Layered Layout",
      section3_x,
      section3_y,
      Color::WHITE,
    );

    // Create a stack for a card-like UI
    let stack = Stack::new(section3_x, section3_y + 25.0, 220.0, 150.0);

    // Background layer
    ui.draw_panel(
      stack.full(),
      Color::rgb(40, 40, 55),
      Some(Color::rgb(70, 70, 90)),
    );

    // Header area (top portion)
    let (header, content) = stack.split_vertical(35.0);
    ui.draw_rect(header, Color::rgb(50, 50, 70));
    ui.draw_text_centered("Card Title", header, Color::WHITE);

    // Content with padding
    let content_stack = Stack::from_rect(content);
    let content_padded = content_stack.padded(10.0);
    ui.draw_text(
      "Stack allows layering",
      content_padded.x,
      content_padded.y,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "and alignment within",
      content_padded.x,
      content_padded.y + 18.0,
      Color::LIGHT_GRAY,
    );
    ui.draw_text(
      "a bounding rect.",
      content_padded.x,
      content_padded.y + 36.0,
      Color::LIGHT_GRAY,
    );

    // Button at bottom-right of content
    let btn_rect = Stack::from_rect(content).aligned_with_padding(
      70.0,
      25.0,
      HAlign::Right,
      VAlign::Bottom,
      8.0,
    );
    ui.button("OK", btn_rect);

    // =====================================================================
    // Section 4: Nested Layout Demo
    // =====================================================================
    let section4_x = 380.0;
    let section4_y = 410.0;
    ui.draw_text("Nested Layouts", section4_x, section4_y, Color::WHITE);

    // Outer stack for the whole panel
    let outer = Stack::new(section4_x, section4_y + 25.0, 220.0, 100.0);
    ui.draw_panel(
      outer.full(),
      Color::rgb(35, 35, 50),
      Some(Color::rgb(60, 60, 80)),
    );

    // Inner content with padding
    let inner = outer.padded(10.0);

    // VBox inside the stack
    let mut inner_col = VBox::new(inner.x, inner.y, 8.0);

    // Row 1: Label + buttons
    let r1 = inner_col.next(200.0, 25.0);
    let mut r1_row = HBox::new(r1.x, r1.y, 6.0);
    ui.draw_text(
      "Actions:",
      r1_row.next(60.0, 20.0).x,
      r1.y + 4.0,
      Color::LIGHT_GRAY,
    );
    ui.button("A", r1_row.next(40.0, 24.0));
    ui.button("B", r1_row.next(40.0, 24.0));
    ui.button("C", r1_row.next(40.0, 24.0));

    // Row 2: Progress bar
    let r2 = inner_col.next(200.0, 20.0);
    ui.draw_progress_bar(
      Rect::new(r2.x, r2.y, 180.0, 12.0),
      0.65,
      Color::rgb(80, 150, 255),
      Color::rgb(40, 40, 60),
    );

    // =====================================================================
    // Spacing Controls (bottom of screen)
    // =====================================================================
    let controls_y = screen_h as f32 - 90.0;
    ui.draw_panel(
      Rect::new(10.0, controls_y - 10.0, 420.0, 55.0),
      Color::new(0, 0, 0, 150),
      Some(Color::rgb(50, 50, 70)),
    );

    let mut control_row = HBox::new(20.0, controls_y, 16.0).with_valign(VAlign::Center);
    let lbl = control_row.next(100.0, 20.0);
    ui.draw_text("HBox spacing:", lbl.x, lbl.y, Color::LIGHT_GRAY);
    ui.slider(
      control_row.next(80.0, 8.0),
      &mut self.hbox_spacing,
      0.0,
      20.0,
    );
    let lbl = control_row.next(100.0, 20.0);
    ui.draw_text("VBox spacing:", lbl.x, lbl.y, Color::LIGHT_GRAY);
    ui.slider(
      control_row.next(80.0, 8.0),
      &mut self.vbox_spacing,
      0.0,
      20.0,
    );
  }
}
