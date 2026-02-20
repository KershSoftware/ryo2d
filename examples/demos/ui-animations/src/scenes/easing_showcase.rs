//! Easing showcase scene - Grid showing all 25 easing functions

use ryo2d::prelude::*;

/// Data for a single easing demo box
struct EasingBox {
  name: &'static str,
  ease: Ease,
  tween: TweenFloat,
  color: Color,
}

impl EasingBox {
  fn new(name: &'static str, ease: Ease, color: Color) -> Self {
    Self {
      name,
      ease,
      tween: TweenFloat::new(0.0, 1.0, 1.5).with_ease(ease),
      color,
    }
  }

  fn restart(&mut self) {
    self.tween = TweenFloat::new(0.0, 1.0, 1.5).with_ease(self.ease);
  }
}

pub struct EasingShowcase {
  boxes: Vec<EasingBox>,
  auto_restart: bool,
}

impl EasingShowcase {
  pub fn new() -> Self {
    let boxes = vec![
      // Row 1: Linear
      EasingBox::new("Linear", Ease::Linear, Color::rgb(200, 200, 200)),
      // Row 2: Quadratic
      EasingBox::new("InQuad", Ease::InQuad, Color::rgb(255, 100, 100)),
      EasingBox::new("OutQuad", Ease::OutQuad, Color::rgb(100, 255, 100)),
      EasingBox::new("InOutQuad", Ease::InOutQuad, Color::rgb(100, 100, 255)),
      // Row 3: Cubic
      EasingBox::new("InCubic", Ease::InCubic, Color::rgb(255, 150, 100)),
      EasingBox::new("OutCubic", Ease::OutCubic, Color::rgb(100, 255, 150)),
      EasingBox::new("InOutCubic", Ease::InOutCubic, Color::rgb(150, 100, 255)),
      // Row 4: Quartic
      EasingBox::new("InQuart", Ease::InQuart, Color::rgb(255, 200, 100)),
      EasingBox::new("OutQuart", Ease::OutQuart, Color::rgb(100, 255, 200)),
      EasingBox::new("InOutQuart", Ease::InOutQuart, Color::rgb(200, 100, 255)),
      // Row 5: Sine
      EasingBox::new("InSine", Ease::InSine, Color::rgb(255, 100, 150)),
      EasingBox::new("OutSine", Ease::OutSine, Color::rgb(150, 255, 100)),
      EasingBox::new("InOutSine", Ease::InOutSine, Color::rgb(100, 150, 255)),
      // Row 6: Exponential
      EasingBox::new("InExpo", Ease::InExpo, Color::rgb(255, 150, 150)),
      EasingBox::new("OutExpo", Ease::OutExpo, Color::rgb(150, 255, 150)),
      EasingBox::new("InOutExpo", Ease::InOutExpo, Color::rgb(150, 150, 255)),
      // Row 7: Back (overshoot)
      EasingBox::new("InBack", Ease::InBack, Color::rgb(255, 200, 150)),
      EasingBox::new("OutBack", Ease::OutBack, Color::rgb(150, 255, 200)),
      EasingBox::new("InOutBack", Ease::InOutBack, Color::rgb(200, 150, 255)),
      // Row 8: Elastic
      EasingBox::new("InElastic", Ease::InElastic, Color::rgb(255, 100, 200)),
      EasingBox::new("OutElastic", Ease::OutElastic, Color::rgb(200, 255, 100)),
      EasingBox::new(
        "InOutElastic",
        Ease::InOutElastic,
        Color::rgb(100, 200, 255),
      ),
      // Row 9: Bounce
      EasingBox::new("InBounce", Ease::InBounce, Color::rgb(255, 255, 100)),
      EasingBox::new("OutBounce", Ease::OutBounce, Color::rgb(100, 255, 255)),
      EasingBox::new("InOutBounce", Ease::InOutBounce, Color::rgb(255, 100, 255)),
    ];

    Self {
      boxes,
      auto_restart: true,
    }
  }

  fn restart_all(&mut self) {
    for b in &mut self.boxes {
      b.restart();
    }
  }
}

impl Default for EasingShowcase {
  fn default() -> Self {
    Self::new()
  }
}

impl Scene for EasingShowcase {
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if ctx.input.key_pressed(KeyCode::Escape) || ctx.input.key_pressed(KeyCode::Backspace) {
      return SceneAction::Pop;
    }

    // Update all tweens
    let mut all_finished = true;
    for b in &mut self.boxes {
      b.tween.update(dt);
      if b.tween.is_playing() {
        all_finished = false;
      }
    }

    // Auto-restart when all finished
    if all_finished && self.auto_restart {
      self.restart_all();
    }

    // Manual restart
    if ctx.input.key_pressed(KeyCode::Space) {
      self.restart_all();
    }

    // Toggle auto-restart
    if ctx.input.key_pressed(KeyCode::KeyA) {
      self.auto_restart = !self.auto_restart;
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;

    // Dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::rgb(20, 20, 30),
    );

    // Title and instructions
    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    ui.draw_text(
      "Easing Showcase - All 25 Functions",
      10.0,
      10.0,
      text_accent,
    );
    ui.draw_text(
      "Space: Restart | A: Toggle Auto | ESC/Backspace: Back",
      10.0,
      30.0,
      text_muted,
    );

    let auto_text = if self.auto_restart {
      "Auto: ON"
    } else {
      "Auto: OFF"
    };
    ui.draw_text(auto_text, screen_w - 70.0, 30.0, text_muted);

    // Grid layout
    let cols = 5;
    let margin_x = 20.0;
    let margin_top = 60.0;
    let margin_bottom = 20.0;

    let available_w = screen_w - margin_x * 2.0;
    let available_h = screen_h - margin_top - margin_bottom;

    let rows = self.boxes.len().div_ceil(cols);
    let cell_w = available_w / cols as f32;
    let cell_h = available_h / rows as f32;

    let box_width = cell_w - 20.0;
    let track_height = 6.0;
    let indicator_size = 12.0;

    for (i, b) in self.boxes.iter().enumerate() {
      let col = i % cols;
      let row = i / cols;

      let cell_x = margin_x + col as f32 * cell_w;
      let cell_y = margin_top + row as f32 * cell_h;

      // Label
      let (label_w, _) = ui.text_size(b.name);
      let label_x = cell_x + (cell_w - label_w as f32) / 2.0;
      ui.draw_text(b.name, label_x, cell_y + 5.0, Color::WHITE);

      // Track background
      let track_x = cell_x + (cell_w - box_width) / 2.0;
      let track_y = cell_y + 30.0;
      ui.draw_rect(
        Rect::new(track_x, track_y, box_width, track_height),
        Color::rgb(50, 50, 60),
      );

      // Moving indicator
      let progress = b.tween.value();
      let indicator_x = track_x + progress * (box_width - indicator_size);
      let indicator_y = track_y - (indicator_size - track_height) / 2.0;
      ui.draw_rect(
        Rect::new(indicator_x, indicator_y, indicator_size, indicator_size),
        b.color,
      );

      // Progress percentage
      let pct_text = format!("{:.0}%", progress * 100.0);
      let (pct_w, _) = ui.text_size(&pct_text);
      let pct_x = cell_x + (cell_w - pct_w as f32) / 2.0;
      ui.draw_text(&pct_text, pct_x, track_y + 15.0, Color::GRAY);
    }
    SceneAction::Continue
  }
}
