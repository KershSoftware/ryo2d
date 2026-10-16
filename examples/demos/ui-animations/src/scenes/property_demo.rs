//! Property demo scene - Demonstrates various UI property animations
//!
//! Shows slide-in panels, fading labels, scaling buttons, and color transitions.

use demo_kit::prelude::*;

pub struct PropertyDemo {
  // Slide-in panels
  panel_left: TweenFloat,
  panel_right: TweenFloat,
  panel_top: TweenFloat,

  // Fading labels
  fade_alpha: TweenFloat,

  // Scaling effect
  scale: TweenFloat,

  // Color transitions
  color_tween: TweenColor,

  // Position animation
  position: TweenVec2,

  // Rectangle animation (combined position + size)
  rect_tween: TweenRect,
}

impl PropertyDemo {
  pub fn new() -> Self {
    // Panels slide in from off-screen
    let panel_left = TweenFloat::new(-200.0, 20.0, 0.8)
      .with_ease(Ease::OutBack)
      .with_delay(0.1);

    let panel_right = TweenFloat::new(200.0, -20.0, 0.8)
      .with_ease(Ease::OutBack)
      .with_delay(0.2);

    let panel_top = TweenFloat::new(-100.0, 80.0, 0.6)
      .with_ease(Ease::OutCubic)
      .with_delay(0.3);

    // Fading text pulses
    let fade_alpha = TweenFloat::new(0.3, 1.0, 1.0)
      .with_ease(Ease::InOutSine)
      .with_loop(UiTweenLoop::PingPong);

    // Scale pulses
    let scale = TweenFloat::new(0.8, 1.2, 0.5)
      .with_ease(Ease::InOutSine)
      .with_loop(UiTweenLoop::PingPong);

    // Color cycles through
    let color_tween = TweenColor::new(Color::rgb(255, 100, 100), Color::rgb(100, 100, 255), 2.0)
      .with_ease(Ease::InOutSine)
      .with_loop(UiTweenLoop::PingPong);

    // Position bounces around
    let position = TweenVec2::new((100.0, 400.0), (300.0, 400.0), 1.5)
      .with_ease(Ease::InOutCubic)
      .with_loop(UiTweenLoop::PingPong);

    // Rectangle animates both position and size
    let rect_tween = TweenRect::new(
      Rect::new(500.0, 350.0, 50.0, 50.0),
      Rect::new(550.0, 380.0, 150.0, 80.0),
      1.2,
    )
    .with_ease(Ease::OutElastic)
    .with_loop(UiTweenLoop::PingPong);

    Self {
      panel_left,
      panel_right,
      panel_top,
      fade_alpha,
      scale,
      color_tween,
      position,
      rect_tween,
    }
  }

  fn restart(&mut self) {
    self.panel_left.restart();
    self.panel_right.restart();
    self.panel_top.restart();
    self.fade_alpha.restart();
    self.scale.restart();
    self.color_tween.restart();
    self.position.restart();
    self.rect_tween.restart();
  }
}

impl Default for PropertyDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for PropertyDemo {
  fn name(&self) -> &str {
    "Property Animations"
  }

  fn description(&self) -> &str {
    "Slide-in panels, fades, scale, colour, position and rectangle tweens"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Restart")]
  }

  /// UI tweens run in the UI pass, on real time
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    // Update all tweens
    let dt = ui.dt();
    self.panel_left.update(dt);
    self.panel_right.update(dt);
    self.panel_top.update(dt);
    self.fade_alpha.update(dt);
    self.scale.update(dt);
    self.color_tween.update(dt);
    self.position.update(dt);
    self.rect_tween.update(dt);

    // Manual restart
    if ui.input().key_pressed(KeyCode::Space) {
      ui.consume_key(KeyCode::Space);
      self.restart();
    }

    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;
    let area = content_area(ui);
    // The layout below was written for content starting at y = 55
    let oy = area.y - 55.0;

    // Dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::rgb(20, 20, 30),
    );

    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    // ==========================================
    // Left panel (slides in from left)
    // ==========================================
    let left_x = self.panel_left.value();
    ui.draw_panel(
      Rect::new(left_x, 150.0 + oy, 180.0, 120.0),
      Color::rgb(40, 40, 60),
      Some(Color::rgb(80, 80, 120)),
    );
    ui.draw_text("Left Panel", left_x + 10.0, 160.0 + oy, Color::WHITE);
    ui.draw_text("TweenFloat", left_x + 10.0, 180.0 + oy, text_muted);
    ui.draw_text("Ease: OutBack", left_x + 10.0, 200.0 + oy, text_muted);
    ui.draw_text("Slides from left", left_x + 10.0, 220.0 + oy, text_muted);

    // ==========================================
    // Right panel (slides in from right)
    // ==========================================
    let right_offset = self.panel_right.value();
    let right_x = screen_w - 200.0 + right_offset;
    ui.draw_panel(
      Rect::new(right_x, 150.0 + oy, 180.0, 120.0),
      Color::rgb(60, 40, 40),
      Some(Color::rgb(120, 80, 80)),
    );
    ui.draw_text("Right Panel", right_x + 10.0, 160.0 + oy, Color::WHITE);
    ui.draw_text("TweenFloat", right_x + 10.0, 180.0 + oy, text_muted);
    ui.draw_text("Ease: OutBack", right_x + 10.0, 200.0 + oy, text_muted);
    ui.draw_text("Slides from right", right_x + 10.0, 220.0 + oy, text_muted);

    // ==========================================
    // Top panel (slides down)
    // ==========================================
    let center_x = screen_w / 2.0;
    let top_y = self.panel_top.value() + oy;
    let top_panel_w = 200.0;
    ui.draw_panel(
      Rect::centered_at(center_x, top_y, top_panel_w, 50.0),
      Color::rgb(40, 60, 40),
      Some(Color::rgb(80, 120, 80)),
    );
    ui.draw_text_centered(
      "Top Panel - OutCubic",
      Rect::centered_at(center_x, top_y, top_panel_w, 50.0),
      Color::WHITE,
    );

    // ==========================================
    // Fading text
    // ==========================================
    let alpha = (self.fade_alpha.value() * 255.0) as u8;
    ui.draw_text(
      "Fading Text (PingPong)",
      250.0,
      300.0 + oy,
      Color::rgb(255, 200, 100).with_alpha(alpha),
    );

    // ==========================================
    // Scaling box
    // ==========================================
    let scale = self.scale.value();
    let base_size = 40.0;
    let scaled_size = base_size * scale;
    let scale_x = 400.0;
    let scale_y = 320.0 + oy;
    ui.draw_rect(
      Rect::centered_at(scale_x + 50.0, scale_y + 50.0, scaled_size, scaled_size),
      Color::rgb(100, 200, 255),
    );
    ui.draw_text("Scale Pulse", scale_x, scale_y + 80.0, text_muted);

    // ==========================================
    // Color transition
    // ==========================================
    let color = self.color_tween.value();
    ui.draw_rect(Rect::new(550.0, 280.0 + oy, 80.0, 40.0), color);
    ui.draw_text("Color Tween", 550.0, 330.0 + oy, text_muted);
    ui.draw_text("Red <-> Blue", 550.0, 350.0 + oy, Color::GRAY);

    // ==========================================
    // Position animation (bouncing box)
    // ==========================================
    let (pos_x, pos_y) = self.position.value();
    let pos_y = pos_y + oy;
    ui.draw_rect(Rect::centered_at(pos_x, pos_y, 30.0, 30.0), Color::GREEN);
    ui.draw_text("TweenVec2 Position", 140.0, 440.0 + oy, text_muted);

    // ==========================================
    // Rectangle animation (position + size)
    // ==========================================
    let mut rect = self.rect_tween.value();
    rect.y += oy;
    ui.draw_rect(rect, Color::rgb(255, 150, 50));
    ui.draw_text("TweenRect", 710.0, 400.0 + oy, text_muted);
    ui.draw_text("(pos + size)", 710.0, 418.0 + oy, Color::GRAY);

    // ==========================================
    // Labels for animation types
    // ==========================================
    let panel_y = area.y + area.height - 80.0;
    ui.draw_panel(
      Rect::new(10.0, panel_y, 780.0, 80.0),
      Color::rgb(30, 30, 40),
      Some(Color::rgb(60, 60, 80)),
    );
    ui.draw_text(
      "Animation Types Demonstrated:",
      20.0,
      panel_y + 4.0,
      text_accent,
    );
    ui.draw_text(
      "TweenFloat: Single value (position offset, alpha, scale)",
      20.0,
      panel_y + 23.0,
      Color::WHITE,
    );
    ui.draw_text(
      "TweenVec2: Two values (position coordinates)",
      20.0,
      panel_y + 41.0,
      Color::WHITE,
    );
    ui.draw_text(
      "TweenColor: RGBA color | TweenRect: Position + Size combined",
      20.0,
      panel_y + 59.0,
      Color::WHITE,
    );
  }
}
