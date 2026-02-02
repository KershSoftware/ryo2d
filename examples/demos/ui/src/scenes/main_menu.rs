//! Main menu scene - navigation hub for all UI demos

use ryo2d::prelude::*;

use super::{ClippingDemo, LayoutDemo, TextDemo, TextInputDemo, ThemeDemo};

pub struct MainMenu {
  selected_demo: Option<usize>,
}

impl MainMenu {
  pub fn new() -> Self {
    Self {
      selected_demo: None,
    }
  }
}

impl Default for MainMenu {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MainMenu {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Handle keyboard navigation
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Handle demo selection
    if let Some(demo_index) = self.selected_demo.take() {
      match demo_index {
        0 => return SceneAction::Push(Box::new(ThemeDemo::new())),
        1 => return SceneAction::Push(Box::new(LayoutDemo::new())),
        2 => return SceneAction::Push(Box::new(ClippingDemo::new())),
        3 => return SceneAction::Push(Box::new(TextDemo::new())),
        4 => return SceneAction::Push(Box::new(TextInputDemo::new())),
        5 => return SceneAction::Push(Box::new(super::ScrollViewDemo::new())),
        6 => return SceneAction::Push(Box::new(super::DropdownDemo::new())),
        // Future demos will be added here
        _ => {}
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let center_x = screen_w as f32 / 2.0;

    // Dark background - draw a full screen rect
    let bg_rect = Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32);
    ui.draw_rect(bg_rect, Color::rgb(25, 25, 35));

    // Title
    ui.draw_text_anchored("Ryo2D UI Demo", Anchor::TopCenter, 0.0, 40.0, Color::WHITE);

    // Subtitle
    ui.draw_text_anchored(
      "Explore the UI system features",
      Anchor::TopCenter,
      0.0,
      70.0,
      Color::GRAY,
    );

    // Demo buttons
    let button_width = 300.0;
    let button_height = 36.0;
    let desc_gap = 6.0; // Gap between button and description
    let item_spacing = 20.0; // Padding between each category
    let start_y = 120.0;

    let demos = [
      (
        "1. Widgets & Themes",
        "Buttons, sliders, toggles, hot-reload",
      ),
      ("2. Layout Containers", "HBox, VBox, Stack positioning"),
      (
        "3. Clipping & Modals",
        "Scissor clipping and modal overlays",
      ),
      ("4. Text Wrapping", "Word wrap and text layout"),
      ("5. Text Input", "Editable text fields with cursor"),
      ("6. Scroll Views", "Scrollable content containers"),
      ("7. Dropdowns", "Select from list of options"),
    ];

    // Calculate item height (button + gap + description text height + spacing)
    let desc_height = 12.0; // Approximate text height
    let item_height = button_height + desc_gap + desc_height + item_spacing;

    for (i, (title, description)) in demos.iter().enumerate() {
      let y = start_y + item_height * i as f32;
      let button_rect = Rect::centered_at(
        center_x,
        y + button_height / 2.0,
        button_width,
        button_height,
      );

      // All demos are now enabled
      let enabled = true;

      if enabled {
        if ui.button(title, button_rect) {
          self.selected_demo = Some(i);
        }
      } else {
        // Draw disabled button
        ui.draw_panel(
          button_rect,
          Color::rgb(30, 30, 40),
          Some(Color::rgb(50, 50, 60)),
        );
        ui.draw_text_centered(title, button_rect, Color::GRAY);
      }

      // Description text below each button
      let desc_y = y + button_height + desc_gap;
      let (desc_w, _) = ui.text_size(description);
      let desc_x = center_x - desc_w as f32 / 2.0;
      ui.draw_text(description, desc_x, desc_y, Color::rgb(100, 100, 120));
    }

    // Footer
    ui.draw_text_anchored(
      "Press ESC to quit",
      Anchor::BottomCenter,
      0.0,
      -20.0,
      Color::rgb(80, 80, 100),
    );

    // Version info
    ui.draw_text_anchored(
      "Ryo2D Engine - UI System",
      Anchor::BottomRight,
      -10.0,
      -10.0,
      Color::rgb(60, 60, 80),
    );
  }
}
