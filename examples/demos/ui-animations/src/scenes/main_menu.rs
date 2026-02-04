//! Main menu scene - navigation hub for all UI animation demos

use ryo2d::prelude::*;

use super::{EasingShowcase, InteractiveDemo, PropertyDemo};

pub struct MainMenu {
  selected_demo: Option<usize>,
  title_alpha: TweenFloat,
  button_offsets: Vec<TweenFloat>,
}

impl MainMenu {
  pub fn new() -> Self {
    // Title fades in
    let title_alpha = TweenFloat::new(0.0, 1.0, 0.8).with_ease(Ease::OutQuad);

    // Buttons slide in with staggered delays
    let button_offsets = vec![
      TweenFloat::new(-300.0, 0.0, 0.5)
        .with_ease(Ease::OutBack)
        .with_delay(0.2),
      TweenFloat::new(-300.0, 0.0, 0.5)
        .with_ease(Ease::OutBack)
        .with_delay(0.3),
      TweenFloat::new(-300.0, 0.0, 0.5)
        .with_ease(Ease::OutBack)
        .with_delay(0.4),
    ];

    Self {
      selected_demo: None,
      title_alpha,
      button_offsets,
    }
  }
}

impl Default for MainMenu {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for MainMenu {
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    // Update animations
    self.title_alpha.update(dt);
    for offset in &mut self.button_offsets {
      offset.update(dt);
    }

    // Handle keyboard navigation
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Handle demo selection
    if let Some(demo_index) = self.selected_demo.take() {
      match demo_index {
        0 => return SceneAction::Push(Box::new(EasingShowcase::new())),
        1 => return SceneAction::Push(Box::new(PropertyDemo::new())),
        2 => return SceneAction::Push(Box::new(InteractiveDemo::new())),
        _ => {}
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let center_x = screen_w as f32 / 2.0;

    // Dark background
    let bg_rect = Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32);
    ui.draw_rect(bg_rect, Color::rgb(25, 25, 35));

    // Animated title
    let title_alpha = (self.title_alpha.value() * 255.0) as u8;
    ui.draw_text_anchored(
      "UI Animations Demo",
      Anchor::TopCenter,
      0.0,
      40.0,
      Color::WHITE.with_alpha(title_alpha),
    );

    // Subtitle
    ui.draw_text_anchored(
      "UiTween system for smooth UI animations",
      Anchor::TopCenter,
      0.0,
      70.0,
      Color::GRAY.with_alpha(title_alpha),
    );

    // Demo buttons with slide-in animation
    let button_width = 300.0;
    let button_height = 36.0;
    let desc_gap = 6.0;
    let item_spacing = 20.0;
    let start_y = 140.0;

    let demos = [
      ("1. Easing Showcase", "All 25 easing functions visualized"),
      (
        "2. Property Animations",
        "Position, opacity, scale, color tweens",
      ),
      ("3. Interactive Demo", "Button hover and click effects"),
    ];

    let desc_height = 12.0;
    let item_height = button_height + desc_gap + desc_height + item_spacing;

    for (i, (title, description)) in demos.iter().enumerate() {
      let y = start_y + item_height * i as f32;
      let x_offset = self.button_offsets[i].value();

      let button_rect = Rect::centered_at(
        center_x + x_offset,
        y + button_height / 2.0,
        button_width,
        button_height,
      );

      if ui.button(title, button_rect) {
        self.selected_demo = Some(i);
      }

      // Description text below each button
      let desc_y = y + button_height + desc_gap;
      let (desc_w, _) = ui.text_size(description);
      let desc_x = center_x - desc_w as f32 / 2.0 + x_offset;
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
      "Ryo2D Engine - UI Tween System",
      Anchor::BottomRight,
      -10.0,
      -10.0,
      Color::rgb(60, 60, 80),
    );
  }
}
