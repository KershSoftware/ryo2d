//! Controller Demo - Live gamepad input visualization
//!
//! This demo provides a real-time visualization of controller inputs:
//! - Button press indicators
//! - Analog stick positions (with deadzone visualization)
//! - Trigger values
//! - Connection status
//!
//! Works gracefully when no controller is connected. Runs on the demo-kit
//! shell.

use demo_kit::prelude::*;

struct ControllerPage {
  button_history: Vec<String>,
  // Cached input state for ui
  gamepad_connected: bool,
  gamepad_name: String,
  left_stick: (f32, f32),
  left_stick_raw: (f32, f32),
  right_stick: (f32, f32),
  right_stick_raw: (f32, f32),
  left_trigger: f32,
  right_trigger: f32,
  buttons_down: Vec<GamepadButton>,
}

impl ControllerPage {
  fn new() -> Self {
    Self {
      button_history: Vec::new(),
      gamepad_connected: false,
      gamepad_name: String::new(),
      left_stick: (0.0, 0.0),
      left_stick_raw: (0.0, 0.0),
      right_stick: (0.0, 0.0),
      right_stick_raw: (0.0, 0.0),
      left_trigger: 0.0,
      right_trigger: 0.0,
      buttons_down: Vec::new(),
    }
  }

  fn record_button(&mut self, name: &str) {
    self.button_history.insert(0, name.to_string());
    if self.button_history.len() > 8 {
      self.button_history.pop();
    }
  }
}

impl DemoPage for ControllerPage {
  fn name(&self) -> &str {
    "Gamepad"
  }

  fn description(&self) -> &str {
    "Live gamepad visualization: buttons, sticks (raw and processed) and triggers"
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::keys("Gamepad", &[], "Buttons and sticks")]
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    // Cache input state for ui
    self.gamepad_connected = ctx.input.gamepad_connected();
    self.gamepad_name = ctx.input.gamepad_name().to_string();
    self.left_stick = ctx.input.left_stick();
    self.left_stick_raw = ctx.input.left_stick_raw();
    self.right_stick = ctx.input.right_stick();
    self.right_stick_raw = ctx.input.right_stick_raw();
    self.left_trigger = ctx.input.left_trigger();
    self.right_trigger = ctx.input.right_trigger();

    // Cache button states
    self.buttons_down.clear();
    let all_buttons = [
      GamepadButton::South,
      GamepadButton::East,
      GamepadButton::West,
      GamepadButton::North,
      GamepadButton::LeftBumper,
      GamepadButton::RightBumper,
      GamepadButton::LeftTrigger,
      GamepadButton::RightTrigger,
      GamepadButton::LeftStick,
      GamepadButton::RightStick,
      GamepadButton::Select,
      GamepadButton::Start,
      GamepadButton::DPadUp,
      GamepadButton::DPadDown,
      GamepadButton::DPadLeft,
      GamepadButton::DPadRight,
    ];
    for btn in all_buttons {
      if ctx.input.gamepad_button_down(btn) {
        self.buttons_down.push(btn);
      }
    }

    // Record button presses for history
    let buttons = [
      (GamepadButton::South, "South (A/X)"),
      (GamepadButton::East, "East (B/O)"),
      (GamepadButton::West, "West (X/Sq)"),
      (GamepadButton::North, "North (Y/Tr)"),
      (GamepadButton::LeftBumper, "L1/LB"),
      (GamepadButton::RightBumper, "R1/RB"),
      (GamepadButton::LeftTrigger, "L2/LT"),
      (GamepadButton::RightTrigger, "R2/RT"),
      (GamepadButton::LeftStick, "L3"),
      (GamepadButton::RightStick, "R3"),
      (GamepadButton::Select, "Select"),
      (GamepadButton::Start, "Start"),
      (GamepadButton::DPadUp, "D-Up"),
      (GamepadButton::DPadDown, "D-Down"),
      (GamepadButton::DPadLeft, "D-Left"),
      (GamepadButton::DPadRight, "D-Right"),
    ];

    for (btn, name) in buttons {
      if ctx.input.gamepad_button_pressed(btn) {
        self.record_button(name);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    // Two columns in the content area: the left one for status, sticks,
    // triggers and history, the right one for buttons and values
    let left = area.x + 16.0;
    let right = area.x + area.width * 0.56;
    let top = area.y + 6.0;

    // Connection status
    let status_y = top;
    if self.gamepad_connected {
      ui.draw_text(
        &format!("Connected: {}", self.gamepad_name),
        left,
        status_y,
        Color::hex(0x00FF00),
      );
    } else {
      ui.draw_text(
        "No controller connected",
        left,
        status_y,
        Color::hex(0xFF6666),
      );
      ui.draw_text(
        "Connect a gamepad to test inputs",
        left,
        status_y + 20.0,
        Color::hex(0xAAAAAA),
      );
    }

    // Left stick visualization
    let stick_section_y = top + 50.0;
    ui.draw_text("Left Stick", left + 60.0, stick_section_y, Color::WHITE);
    self.draw_stick(
      ui,
      left + 80.0,
      stick_section_y + 80.0,
      self.left_stick_raw,
      self.left_stick,
    );

    // Right stick visualization
    ui.draw_text("Right Stick", left + 240.0, stick_section_y, Color::WHITE);
    self.draw_stick(
      ui,
      left + 260.0,
      stick_section_y + 80.0,
      self.right_stick_raw,
      self.right_stick,
    );

    // Triggers
    let trigger_y = stick_section_y + 180.0;
    ui.draw_text("Triggers", left, trigger_y, Color::WHITE);

    // Left trigger bar
    ui.draw_text("LT:", left, trigger_y + 25.0, Color::hex(0xAAAAAA));
    let lt_rect = Rect::new(left + 40.0, trigger_y + 20.0, 150.0, 20.0);
    ui.draw_rect(lt_rect, Color::hex(0x333333));
    let lt_fill = Rect::new(
      left + 40.0,
      trigger_y + 20.0,
      150.0 * self.left_trigger,
      20.0,
    );
    ui.draw_rect(lt_fill, Color::hex(0x4488FF));
    ui.draw_text(
      &format!("{:.2}", self.left_trigger),
      left + 200.0,
      trigger_y + 25.0,
      Color::WHITE,
    );

    // Right trigger bar
    ui.draw_text("RT:", left, trigger_y + 50.0, Color::hex(0xAAAAAA));
    let rt_rect = Rect::new(left + 40.0, trigger_y + 45.0, 150.0, 20.0);
    ui.draw_rect(rt_rect, Color::hex(0x333333));
    let rt_fill = Rect::new(
      left + 40.0,
      trigger_y + 45.0,
      150.0 * self.right_trigger,
      20.0,
    );
    ui.draw_rect(rt_fill, Color::hex(0xFF8844));
    ui.draw_text(
      &format!("{:.2}", self.right_trigger),
      left + 200.0,
      trigger_y + 50.0,
      Color::WHITE,
    );

    // Buttons section
    let buttons_y = top + 50.0;
    let buttons_x = right;
    ui.draw_text("Buttons", buttons_x, buttons_y, Color::WHITE);

    // Face buttons (diamond layout)
    let face_center_x = buttons_x + 60.0;
    let face_center_y = buttons_y + 70.0;
    let face_spacing = 30.0;

    self.draw_button_indicator(
      ui,
      face_center_x,
      face_center_y - face_spacing,
      "Y",
      self.buttons_down.contains(&GamepadButton::North),
    );
    self.draw_button_indicator(
      ui,
      face_center_x,
      face_center_y + face_spacing,
      "A",
      self.buttons_down.contains(&GamepadButton::South),
    );
    self.draw_button_indicator(
      ui,
      face_center_x - face_spacing,
      face_center_y,
      "X",
      self.buttons_down.contains(&GamepadButton::West),
    );
    self.draw_button_indicator(
      ui,
      face_center_x + face_spacing,
      face_center_y,
      "B",
      self.buttons_down.contains(&GamepadButton::East),
    );

    // Shoulder buttons
    let shoulder_y = buttons_y + 130.0;
    self.draw_button_indicator(
      ui,
      buttons_x + 20.0,
      shoulder_y,
      "LB",
      self.buttons_down.contains(&GamepadButton::LeftBumper),
    );
    self.draw_button_indicator(
      ui,
      buttons_x + 100.0,
      shoulder_y,
      "RB",
      self.buttons_down.contains(&GamepadButton::RightBumper),
    );

    // D-pad
    let dpad_x = buttons_x + 180.0;
    let dpad_y = buttons_y + 70.0;
    ui.draw_text(
      "D-Pad",
      dpad_x - 10.0,
      buttons_y + 4.0,
      Color::hex(0xAAAAAA),
    );

    self.draw_button_indicator(
      ui,
      dpad_x,
      dpad_y - 20.0,
      "^",
      self.buttons_down.contains(&GamepadButton::DPadUp),
    );
    self.draw_button_indicator(
      ui,
      dpad_x,
      dpad_y + 20.0,
      "v",
      self.buttons_down.contains(&GamepadButton::DPadDown),
    );
    self.draw_button_indicator(
      ui,
      dpad_x - 20.0,
      dpad_y,
      "<",
      self.buttons_down.contains(&GamepadButton::DPadLeft),
    );
    self.draw_button_indicator(
      ui,
      dpad_x + 20.0,
      dpad_y,
      ">",
      self.buttons_down.contains(&GamepadButton::DPadRight),
    );

    // Center buttons
    let center_y = shoulder_y + 40.0;
    self.draw_button_indicator(
      ui,
      buttons_x + 40.0,
      center_y,
      "Sel",
      self.buttons_down.contains(&GamepadButton::Select),
    );
    self.draw_button_indicator(
      ui,
      buttons_x + 90.0,
      center_y,
      "Sta",
      self.buttons_down.contains(&GamepadButton::Start),
    );

    // Stick buttons
    let stick_btn_y = center_y + 40.0;
    self.draw_button_indicator(
      ui,
      buttons_x + 40.0,
      stick_btn_y,
      "L3",
      self.buttons_down.contains(&GamepadButton::LeftStick),
    );
    self.draw_button_indicator(
      ui,
      buttons_x + 90.0,
      stick_btn_y,
      "R3",
      self.buttons_down.contains(&GamepadButton::RightStick),
    );

    // Button history
    let history_y = area.y + area.height - 164.0;
    ui.draw_text("Button History", left, history_y, Color::WHITE);
    for (i, btn_name) in self.button_history.iter().enumerate() {
      let alpha = 255 - (i as u8 * 30).min(200);
      ui.draw_text(
        btn_name,
        left,
        history_y + 20.0 + (i as f32 * 18.0),
        Color::WHITE.with_alpha(alpha),
      );
    }

    // Raw vs processed values
    let values_x = right;
    let values_y = history_y + 40.0;
    ui.draw_text(
      "Stick Values (Raw / Processed)",
      values_x,
      values_y,
      Color::WHITE,
    );

    let (lx_raw, ly_raw) = self.left_stick_raw;
    let (lx, ly) = self.left_stick;
    ui.draw_text(
      &format!("L: ({:.2}, {:.2}) / ({:.2}, {:.2})", lx_raw, ly_raw, lx, ly),
      values_x,
      values_y + 25.0,
      Color::hex(0xAAAAAA),
    );

    let (rx_raw, ry_raw) = self.right_stick_raw;
    let (rx, ry) = self.right_stick;
    ui.draw_text(
      &format!("R: ({:.2}, {:.2}) / ({:.2}, {:.2})", rx_raw, ry_raw, rx, ry),
      values_x,
      values_y + 45.0,
      Color::hex(0xAAAAAA),
    );
  }
}

impl ControllerPage {
  fn draw_stick(
    &self,
    ui: &mut UiContext,
    center_x: f32,
    center_y: f32,
    raw: (f32, f32),
    processed: (f32, f32),
  ) {
    let radius = 50.0;

    // Draw outer circle (background)
    self.draw_circle_outline(ui, center_x, center_y, radius, Color::hex(0x444444));

    // Draw deadzone circle
    let deadzone_radius = radius * 0.15; // 15% deadzone
    self.draw_circle_outline(
      ui,
      center_x,
      center_y,
      deadzone_radius,
      Color::hex(0x664444),
    );

    // Draw raw position (red dot)
    let raw_x = center_x + raw.0 * radius;
    let raw_y = center_y + raw.1 * radius;
    self.draw_filled_circle(ui, raw_x, raw_y, 4.0, Color::hex(0xFF4444));

    // Draw processed position (green dot)
    let proc_x = center_x + processed.0 * radius;
    let proc_y = center_y + processed.1 * radius;
    self.draw_filled_circle(ui, proc_x, proc_y, 6.0, Color::hex(0x44FF44));

    // Draw center crosshair
    ui.draw_rect(
      Rect::new(center_x - 1.0, center_y - 8.0, 2.0, 16.0),
      Color::hex(0x666666),
    );
    ui.draw_rect(
      Rect::new(center_x - 8.0, center_y - 1.0, 16.0, 2.0),
      Color::hex(0x666666),
    );
  }

  fn draw_circle_outline(&self, ui: &mut UiContext, cx: f32, cy: f32, radius: f32, color: Color) {
    // Draw circle using line segments (approximation)
    let segments = 24;
    for i in 0..segments {
      let angle1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
      let angle2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;

      let x1 = cx + angle1.cos() * radius;
      let y1 = cy + angle1.sin() * radius;
      let x2 = cx + angle2.cos() * radius;
      let y2 = cy + angle2.sin() * radius;

      // Draw line segment as thin rectangle
      self.draw_line(ui, x1, y1, x2, y2, color);
    }
  }

  fn draw_filled_circle(&self, ui: &mut UiContext, cx: f32, cy: f32, radius: f32, color: Color) {
    // Simple filled circle approximation using rectangles
    let steps = (radius * 2.0) as i32;
    for dy in -steps..=steps {
      let y = cy + dy as f32 / 2.0;
      let half_width = (radius * radius - (dy as f32 / 2.0).powi(2)).sqrt();
      if half_width > 0.0 {
        ui.draw_rect(
          Rect::new(cx - half_width, y - 0.5, half_width * 2.0, 1.0),
          color,
        );
      }
    }
  }

  fn draw_line(&self, ui: &mut UiContext, x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 0.01 {
      return;
    }

    // Draw line as thin rectangle
    let angle = dy.atan2(dx);
    let thickness = 1.5;

    // Calculate perpendicular offset for thickness
    let perp_x = -angle.sin() * thickness / 2.0;
    let perp_y = angle.cos() * thickness / 2.0;

    // Draw as a series of small rectangles along the line
    let steps = (length / 2.0).ceil() as i32;
    for i in 0..steps {
      let t = i as f32 / steps as f32;
      let x = x1 + dx * t;
      let y = y1 + dy * t;
      ui.draw_rect(Rect::new(x + perp_x, y + perp_y, 2.0, 2.0), color);
    }
  }

  fn draw_button_indicator(&self, ui: &mut UiContext, x: f32, y: f32, label: &str, pressed: bool) {
    let size = 24.0;
    let rect = Rect::new(x - size / 2.0, y - size / 2.0, size, size);

    // Background
    let bg_color = if pressed {
      Color::hex(0x44FF44)
    } else {
      Color::hex(0x333333)
    };
    ui.draw_rect(rect, bg_color);

    // Border
    let border_color = if pressed {
      Color::hex(0x88FF88)
    } else {
      Color::hex(0x555555)
    };
    // Top
    ui.draw_rect(Rect::new(rect.x, rect.y, rect.width, 1.0), border_color);
    // Bottom
    ui.draw_rect(
      Rect::new(rect.x, rect.y + rect.height - 1.0, rect.width, 1.0),
      border_color,
    );
    // Left
    ui.draw_rect(Rect::new(rect.x, rect.y, 1.0, rect.height), border_color);
    // Right
    ui.draw_rect(
      Rect::new(rect.x + rect.width - 1.0, rect.y, 1.0, rect.height),
      border_color,
    );

    // Label
    let text_color = if pressed {
      Color::hex(0x000000)
    } else {
      Color::hex(0xAAAAAA)
    };
    let label_x = x - (label.len() as f32 * 4.0); // Approximate centering
    ui.draw_text(label, label_x, y - 4.0, text_color);
  }
}

fn main() {
  Demo::new("Controller")
    .resolution(800, 600)
    .page(ControllerPage::new)
    .run();
}
