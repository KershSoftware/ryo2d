//! Interactive demo scene - Button hover/click effects with visual feedback

use demo_kit::prelude::*;

/// The layout was written for a title above y = 55; the shell's header ends higher
const SHIFT_Y: f32 = -15.0;

/// An animated button with hover and click effects
struct AnimatedButton {
  label: &'static str,
  base_rect: Rect,
  scale: TweenFloat,
  glow_alpha: TweenFloat,
  click_scale: TweenFloat,
  color: Color,
  was_hovered: bool,
  was_clicked: bool,
}

impl AnimatedButton {
  fn new(label: &'static str, x: f32, y: f32, width: f32, height: f32, color: Color) -> Self {
    Self {
      label,
      base_rect: Rect::new(x, y, width, height),
      scale: TweenFloat::new(1.0, 1.0, 0.15).with_ease(Ease::OutQuad),
      glow_alpha: TweenFloat::new(0.0, 0.0, 0.2).with_ease(Ease::OutQuad),
      click_scale: TweenFloat::new(1.0, 1.0, 0.1).with_ease(Ease::OutQuad),
      color,
      was_hovered: false,
      was_clicked: false,
    }
  }

  fn update(&mut self, dt: f32, mouse_pos: (f32, f32), mouse_down: bool, mouse_pressed: bool) {
    self.scale.update(dt);
    self.glow_alpha.update(dt);
    self.click_scale.update(dt);

    let current_rect = self.current_rect();
    let is_hovered = current_rect.contains(mouse_pos.0, mouse_pos.1);

    // Handle hover state change
    if is_hovered && !self.was_hovered {
      // Mouse entered
      self.scale = TweenFloat::new(self.scale.value(), 1.08, 0.15).with_ease(Ease::OutBack);
      self.glow_alpha = TweenFloat::new(self.glow_alpha.value(), 0.4, 0.2).with_ease(Ease::OutQuad);
    } else if !is_hovered && self.was_hovered {
      // Mouse left
      self.scale = TweenFloat::new(self.scale.value(), 1.0, 0.2).with_ease(Ease::OutQuad);
      self.glow_alpha = TweenFloat::new(self.glow_alpha.value(), 0.0, 0.3).with_ease(Ease::OutQuad);
    }
    self.was_hovered = is_hovered;

    // Handle click animation
    if is_hovered && mouse_pressed && !self.was_clicked {
      // Click down - squish effect
      self.click_scale = TweenFloat::new(1.0, 0.92, 0.05).with_ease(Ease::OutQuad);
      self.was_clicked = true;
    } else if self.was_clicked && !mouse_down {
      // Release - bounce back
      self.click_scale =
        TweenFloat::new(self.click_scale.value(), 1.0, 0.15).with_ease(Ease::OutBack);
      self.was_clicked = false;
    }
  }

  fn current_rect(&self) -> Rect {
    let scale = self.scale.value() * self.click_scale.value();
    let w = self.base_rect.width * scale;
    let h = self.base_rect.height * scale;
    let cx = self.base_rect.x + self.base_rect.width / 2.0;
    let cy = self.base_rect.y + self.base_rect.height / 2.0;
    Rect::centered_at(cx, cy, w, h)
  }

  fn render(&self, ui: &mut UiContext) {
    let rect = self.current_rect();
    let glow_alpha = (self.glow_alpha.value() * 255.0) as u8;

    // Glow effect (larger background rect)
    if glow_alpha > 0 {
      let glow_rect = rect.expand(6.0);
      ui.draw_rect(glow_rect, self.color.with_alpha(glow_alpha));
    }

    // Button background
    ui.draw_panel(rect, self.color, Some(Color::WHITE.with_alpha(80)));

    // Label
    ui.draw_text_centered(self.label, rect, Color::WHITE);
  }

  fn is_hovered(&self) -> bool {
    self.was_hovered
  }
}

/// A notification that pops up and fades out
struct Notification {
  message: String,
  position: TweenVec2,
  alpha: TweenFloat,
  scale: TweenFloat,
  active: bool,
}

impl Notification {
  fn new() -> Self {
    Self {
      message: String::new(),
      position: TweenVec2::new((0.0, 0.0), (0.0, 0.0), 0.0),
      alpha: TweenFloat::new(0.0, 0.0, 0.0),
      scale: TweenFloat::new(0.0, 0.0, 0.0),
      active: false,
    }
  }

  fn show(&mut self, message: &str, x: f32, y: f32) {
    self.message = message.to_string();
    self.position = TweenVec2::new((x, y + 20.0), (x, y - 30.0), 0.5).with_ease(Ease::OutQuad);
    self.alpha = TweenFloat::new(1.0, 0.0, 1.5)
      .with_ease(Ease::InQuad)
      .with_delay(0.3);
    self.scale = TweenFloat::new(0.5, 1.0, 0.2).with_ease(Ease::OutBack);
    self.active = true;
  }

  fn update(&mut self, dt: f32) {
    if !self.active {
      return;
    }
    self.position.update(dt);
    self.alpha.update(dt);
    self.scale.update(dt);

    if self.alpha.is_finished() {
      self.active = false;
    }
  }

  fn render(&self, ui: &mut UiContext) {
    if !self.active {
      return;
    }

    let (x, y) = self.position.value();
    let alpha = (self.alpha.value() * 255.0) as u8;
    let scale = self.scale.value();

    let (text_w, text_h) = ui.text_size(&self.message);
    let w = text_w as f32 * scale + 20.0;
    let h = text_h as f32 * scale + 10.0;

    let rect = Rect::centered_at(x, y, w, h);
    ui.draw_panel(
      rect,
      Color::rgb(40, 80, 40).with_alpha(alpha),
      Some(Color::rgb(80, 160, 80).with_alpha(alpha)),
    );

    let (text_w, _) = ui.text_size(&self.message);
    ui.draw_text(
      &self.message,
      x - text_w as f32 / 2.0,
      y - 4.0,
      Color::WHITE.with_alpha(alpha),
    );
  }
}

pub struct InteractiveDemo {
  buttons: Vec<AnimatedButton>,
  notifications: Vec<Notification>,
  click_count: u32,
  hover_indicator: TweenFloat,
  last_hovered: Option<usize>,
}

impl InteractiveDemo {
  pub fn new() -> Self {
    let buttons = vec![
      AnimatedButton::new(
        "Button A",
        100.0,
        200.0 + SHIFT_Y,
        150.0,
        50.0,
        Color::rgb(80, 100, 160),
      ),
      AnimatedButton::new(
        "Button B",
        300.0,
        200.0 + SHIFT_Y,
        150.0,
        50.0,
        Color::rgb(160, 80, 100),
      ),
      AnimatedButton::new(
        "Button C",
        500.0,
        200.0 + SHIFT_Y,
        150.0,
        50.0,
        Color::rgb(80, 160, 100),
      ),
      AnimatedButton::new(
        "Large Button",
        200.0,
        320.0 + SHIFT_Y,
        250.0,
        70.0,
        Color::rgb(120, 100, 160),
      ),
      AnimatedButton::new(
        "Small",
        500.0,
        330.0 + SHIFT_Y,
        80.0,
        40.0,
        Color::rgb(160, 120, 80),
      ),
      AnimatedButton::new(
        "Tiny",
        620.0,
        330.0 + SHIFT_Y,
        60.0,
        40.0,
        Color::rgb(80, 140, 140),
      ),
    ];

    let notifications: Vec<Notification> = (0..5).map(|_| Notification::new()).collect();

    Self {
      buttons,
      notifications,
      click_count: 0,
      hover_indicator: TweenFloat::new(0.0, 0.0, 0.3).with_ease(Ease::OutQuad),
      last_hovered: None,
    }
  }
}

impl Default for InteractiveDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for InteractiveDemo {
  fn name(&self) -> &str {
    "Interactive"
  }

  fn description(&self) -> &str {
    "Buttons that scale on hover, squish on click and pop up notifications"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Hover and click the buttons")]
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    let mouse_pos = ctx.input.mouse_position();
    let mouse_down = ctx.input.mouse_down(MouseButton::Left);
    let mouse_pressed = ctx.input.mouse_pressed(MouseButton::Left);

    // Update buttons
    for (i, button) in self.buttons.iter_mut().enumerate() {
      let was_hovered = button.is_hovered();
      button.update(dt, mouse_pos, mouse_down, mouse_pressed);

      // Check for click
      if button.is_hovered() && ctx.input.mouse_released(MouseButton::Left) {
        self.click_count += 1;

        // Find an inactive notification slot, or recycle the oldest active one
        let idx = self
          .notifications
          .iter()
          .position(|n| !n.active)
          .unwrap_or_else(|| {
            // Pool full — recycle the one closest to finishing (lowest alpha)
            self
              .notifications
              .iter()
              .enumerate()
              .min_by(|(_, a), (_, b)| a.alpha.value().partial_cmp(&b.alpha.value()).unwrap())
              .map(|(i, _)| i)
              .unwrap_or(0)
          });
        let rect = button.current_rect();
        let (cx, cy) = rect.center();
        self.notifications[idx].show(&format!("Clicked! ({})", self.click_count), cx, cy);
      }

      // Track hover changes for indicator
      if button.is_hovered() && !was_hovered {
        self.last_hovered = Some(i);
      }
    }

    // Update hover indicator
    self.hover_indicator.update(dt);

    // Update notifications
    for notif in &mut self.notifications {
      notif.update(dt);
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;

    // Dark background
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w, screen_h),
      Color::rgb(20, 20, 30),
    );

    let text_accent = Color::rgb(130, 180, 255);
    let text_muted = Color::rgb(160, 160, 160);

    // Click counter
    let count_text = format!("Total clicks: {}", self.click_count);
    let area = content_area(ui);
    ui.draw_text(&count_text, screen_w - 150.0, area.y, Color::WHITE);

    // Section title
    ui.draw_text(
      "Animated Buttons with Hover & Click Effects:",
      100.0,
      160.0 + SHIFT_Y,
      Color::WHITE,
    );

    // Render buttons
    for button in &self.buttons {
      button.render(ui);
    }

    // Render notifications
    for notif in &self.notifications {
      notif.render(ui);
    }

    // Info panel
    ui.draw_panel(
      Rect::new(50.0, 420.0 + SHIFT_Y, 700.0, 130.0),
      Color::rgb(30, 30, 45),
      Some(Color::rgb(60, 60, 90)),
    );

    ui.draw_text(
      "Animation Techniques Used:",
      60.0,
      430.0 + SHIFT_Y,
      text_accent,
    );
    ui.draw_text(
      "Hover: Scale up with OutBack easing + glow fade in",
      60.0,
      455.0 + SHIFT_Y,
      Color::WHITE,
    );
    ui.draw_text(
      "Click: Quick scale down (squish) then bounce back",
      60.0,
      475.0 + SHIFT_Y,
      Color::WHITE,
    );
    ui.draw_text(
      "Notifications: Pop up, float upward, fade out",
      60.0,
      495.0 + SHIFT_Y,
      Color::WHITE,
    );
    ui.draw_text(
      "All animations use UiTween with retargeting for smooth interrupts",
      60.0,
      520.0 + SHIFT_Y,
      text_muted,
    );
  }
}
