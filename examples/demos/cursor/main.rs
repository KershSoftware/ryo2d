//! Cursor Demo - Ryo2D Example
//!
//! Demonstrates the cursor customization system on the demo-kit shell:
//! - Different system cursor styles
//! - Custom sprite cursors
//! - Cursor visibility control
//! - Cursor confinement to window
//! - Interactive click area with spark effects

use demo_kit::prelude::*;

fn main() {
  Demo::new("Cursor")
    .resolution(800, 600)
    .page(CursorDemoScene::new)
    .run();
}

struct CursorButton {
  label: &'static str,
  style: CursorStyle,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
}

impl CursorButton {
  fn contains(&self, mx: f32, my: f32) -> bool {
    mx >= self.x && mx <= self.x + self.width && my >= self.y && my <= self.y + self.height
  }
}

/// A click marker that fades out over time
struct ClickMarker {
  x: f32,
  y: f32,
  age: f32,
  max_age: f32,
}

/// A spark particle
struct Spark {
  x: f32,
  y: f32,
  vx: f32,
  vy: f32,
  age: f32,
  max_age: f32,
}

struct CursorDemoScene {
  buttons: Vec<CursorButton>,
  current_style: CursorStyle,
  custom_cursor_enabled: bool,
  custom_cursor_sprite: Option<SpriteHandle>,
  mouse_x: f32,
  mouse_y: f32,
  // Interactive area
  click_area: Rect,
  click_markers: Vec<ClickMarker>,
  sparks: Vec<Spark>,
  // Custom cursor toggle button
  custom_btn_rect: Rect,
  status_rect: Rect,
  // Settings
  cursor_visible: bool,
  cursor_confined: bool,
  // Last values handed to the engine
  applied_visible: bool,
  applied_confined: bool,
}

impl CursorDemoScene {
  fn new() -> Self {
    Self {
      buttons: Vec::new(),
      current_style: CursorStyle::Default,
      custom_cursor_enabled: false,
      custom_cursor_sprite: None,
      mouse_x: 0.0,
      mouse_y: 0.0,
      click_area: Rect::new(480.0, 100.0, 280.0, 200.0),
      click_markers: Vec::new(),
      sparks: Vec::new(),
      custom_btn_rect: Rect::new(480.0, 320.0, 280.0, 40.0),
      status_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
      cursor_visible: true,
      cursor_confined: false,
      applied_visible: true,
      applied_confined: false,
    }
  }

  fn create_buttons(&mut self) {
    let styles = [
      ("Default", CursorStyle::Default),
      ("Text", CursorStyle::Text),
      ("Crosshair", CursorStyle::Crosshair),
      ("Pointer", CursorStyle::Pointer),
      ("Move", CursorStyle::Move),
      ("Not Allowed", CursorStyle::NotAllowed),
      ("Grab", CursorStyle::Grab),
      ("Grabbing", CursorStyle::Grabbing),
      ("Wait", CursorStyle::Wait),
      ("Progress", CursorStyle::Progress),
      ("Help", CursorStyle::Help),
      ("Cell", CursorStyle::Cell),
      ("Zoom In", CursorStyle::ZoomIn),
      ("Zoom Out", CursorStyle::ZoomOut),
      ("Resize H", CursorStyle::ResizeHorizontal),
      ("Resize V", CursorStyle::ResizeVertical),
    ];

    for (label, style) in styles {
      self.buttons.push(CursorButton {
        label,
        style,
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 30.0,
      });
    }
  }

  /// Place the widgets inside the shell's content area
  fn layout(&mut self, area: Rect) {
    let padding = 8.0;
    let columns = 4;
    let start_x = area.x + 20.0;
    let start_y = area.y + 8.0;
    for (i, button) in self.buttons.iter_mut().enumerate() {
      button.x = start_x + (button.width + padding) * (i % columns) as f32;
      button.y = start_y + (button.height + padding) * (i / columns) as f32;
    }
    let rows = self.buttons.len().div_ceil(columns) as f32;
    let below = start_y + rows * 38.0 + 6.0;
    self.click_area = Rect::new(start_x, below, 424.0, 100.0);
    self.custom_btn_rect = Rect::new(start_x, below + 108.0, 424.0, 34.0);
    self.status_rect = Rect::new(start_x, below + 150.0, 424.0, 100.0);
  }

  /// Create a custom crosshair cursor sprite
  fn create_custom_cursor_sprite(&self, assets: &mut AssetManager) -> SpriteHandle {
    let size = 32u32;
    let mut data = vec![0u8; (size * size * 4) as usize];

    // Draw a cyan crosshair with a hole in the middle
    let center = size / 2;
    let line_width = 2;
    let gap = 4; // Gap in the center

    for y in 0..size {
      for x in 0..size {
        let idx = ((y * size + x) * 4) as usize;
        let dx = (x as i32 - center as i32).abs();
        let dy = (y as i32 - center as i32).abs();

        // Horizontal line (but not in center gap)
        let on_h_line = dy < line_width && dx > gap;
        // Vertical line (but not in center gap)
        let on_v_line = dx < line_width && dy > gap;
        // Center dot
        let on_center = dx < 2 && dy < 2;

        if on_h_line || on_v_line || on_center {
          data[idx] = 0; // R
          data[idx + 1] = 255; // G (cyan)
          data[idx + 2] = 255; // B (cyan)
          data[idx + 3] = 255; // A
        }
      }
    }

    let sprite = Sprite::from_rgba(data, size, size);
    assets.create_sprite(sprite, "custom_crosshair")
  }

  fn spawn_sparks(&mut self, x: f32, y: f32) {
    // Spawn 8-12 sparks in random directions
    let count = 8 + (x as usize % 5);
    for i in 0..count {
      let angle = (i as f32 / count as f32) * std::f32::consts::TAU + (y * 0.1);
      let speed = 80.0 + (x * 0.3) % 60.0;
      self.sparks.push(Spark {
        x,
        y,
        vx: angle.cos() * speed,
        vy: angle.sin() * speed,
        age: 0.0,
        max_age: 0.4 + (i as f32 * 0.02),
      });
    }
  }

  fn update_particles(&mut self, dt: f32) {
    // Update click markers
    for marker in &mut self.click_markers {
      marker.age += dt;
    }
    self.click_markers.retain(|m| m.age < m.max_age);

    // Update sparks
    for spark in &mut self.sparks {
      spark.x += spark.vx * dt;
      spark.y += spark.vy * dt;
      spark.vy += 200.0 * dt; // Gravity
      spark.age += dt;
    }
    self.sparks.retain(|s| s.age < s.max_age);
  }
}

impl DemoPage for CursorDemoScene {
  fn name(&self) -> &str {
    "Cursor"
  }

  fn description(&self) -> &str {
    "Hover the buttons to change the cursor style; click the area for sparks"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Visible", &mut self.cursor_visible);
    s.toggle("Confined", &mut self.cursor_confined);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse(
      "Hover the buttons to change the cursor, click the area for sparks",
    )]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.create_buttons();

    // Create custom cursor sprite
    self.custom_cursor_sprite = Some(self.create_custom_cursor_sprite(ctx.assets));

    // Set initial cursor; visibility and confinement are engine state that
    // scene cleanup does not reset, so a rebuilt page restores the defaults here
    ctx.set_cursor(CursorStyle::Default);
    ctx.set_cursor_visible(self.cursor_visible);
    ctx.set_cursor_confined(self.cursor_confined);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Store mouse position for ui
    let (mx, my) = ctx.input.mouse_position();
    self.mouse_x = mx;
    self.mouse_y = my;

    // Update particles
    self.update_particles(dt);

    // Cursor visibility and confinement are engine state, so keep them in step
    if self.cursor_visible != self.applied_visible {
      self.applied_visible = self.cursor_visible;
      ctx.set_cursor_visible(self.cursor_visible);
    }
    if self.cursor_confined != self.applied_confined {
      self.applied_confined = self.cursor_confined;
      ctx.set_cursor_confined(self.cursor_confined);
    }

    // Check click on custom cursor toggle button
    if ctx.input.mouse_pressed(MouseButton::Left) {
      if self.custom_btn_rect.contains(mx, my) {
        self.custom_cursor_enabled = !self.custom_cursor_enabled;
        if self.custom_cursor_enabled {
          if let Some(sprite) = &self.custom_cursor_sprite {
            ctx.set_custom_cursor_centered(sprite);
          }
        } else {
          ctx.set_cursor(CursorStyle::Default);
          self.current_style = CursorStyle::Default;
        }
      }

      // Check click in interactive area
      if self.click_area.contains(mx, my) {
        // Add click marker
        self.click_markers.push(ClickMarker {
          x: mx,
          y: my,
          age: 0.0,
          max_age: 1.5,
        });
        // Spawn sparks
        self.spawn_sparks(mx, my);
      }
    }

    // Check mouse hover over buttons (only if custom cursor not enabled)
    if !self.custom_cursor_enabled {
      let mut hovered_style = None;

      for button in &self.buttons {
        if button.contains(mx, my) {
          hovered_style = Some(button.style);
          break;
        }
      }

      // Update cursor if hovering a button
      if let Some(style) = hovered_style {
        if self.current_style != style {
          self.current_style = style;
          ctx.set_cursor(style);
        }
      } else if self.current_style != CursorStyle::Default {
        self.current_style = CursorStyle::Default;
        ctx.set_cursor(CursorStyle::Default);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    self.layout(content_area(ui));
    let mx = self.mouse_x;
    let my = self.mouse_y;

    // Draw style buttons
    for button in &self.buttons {
      let is_hovered = button.contains(mx, my);
      let is_active = !self.custom_cursor_enabled && self.current_style == button.style;
      let btn_rect = Rect::new(button.x, button.y, button.width, button.height);

      let bg_color = if is_active {
        Color::rgb(80, 140, 200)
      } else if is_hovered {
        Color::rgb(70, 75, 85)
      } else {
        Color::rgb(55, 60, 70)
      };
      ui.draw_rect(btn_rect, bg_color);

      let border_color = if is_active {
        Color::rgb(100, 180, 255)
      } else {
        Color::rgb(80, 85, 95)
      };
      ui.draw_rect_outline(btn_rect, border_color, 1.0);

      let text_color = if is_active {
        Color::WHITE
      } else {
        Color::rgb(200, 200, 200)
      };
      ui.draw_text(button.label, button.x + 8.0, button.y + 8.0, text_color);
    }

    // Interactive click area
    ui.draw_rect(self.click_area, Color::rgb(25, 30, 40));
    ui.draw_rect_outline(self.click_area, Color::rgb(80, 100, 120), 2.0);
    ui.draw_text(
      "Click Area",
      self.click_area.x + 10.0,
      self.click_area.y + 10.0,
      Color::rgb(100, 130, 160),
    );
    ui.draw_text(
      "Click to spawn sparks!",
      self.click_area.x + 10.0,
      self.click_area.y + 30.0,
      Color::rgb(80, 100, 120),
    );

    // Draw sparks
    for spark in &self.sparks {
      let alpha = 1.0 - (spark.age / spark.max_age);
      let size = 4.0 * alpha;
      let color = Color::new(255, 200, 50, (alpha * 255.0) as u8);
      ui.draw_rect(
        Rect::new(spark.x - size / 2.0, spark.y - size / 2.0, size, size),
        color,
      );
    }

    // Draw click markers (X marks)
    for marker in &self.click_markers {
      let alpha = 1.0 - (marker.age / marker.max_age);
      let color = Color::new(255, 100, 100, (alpha * 255.0) as u8);
      let size = 8.0;

      // Draw X by placing small squares along diagonals
      for i in 0..9 {
        let t = (i as f32 / 8.0) * 2.0 - 1.0; // -1 to 1
        let offset = t * size;

        // Top-left to bottom-right diagonal
        ui.draw_rect(
          Rect::new(marker.x + offset - 1.5, marker.y + offset - 1.5, 3.0, 3.0),
          color,
        );

        // Top-right to bottom-left diagonal
        ui.draw_rect(
          Rect::new(marker.x - offset - 1.5, marker.y + offset - 1.5, 3.0, 3.0),
          color,
        );
      }
    }

    // Custom cursor toggle button
    let custom_hovered = self.custom_btn_rect.contains(mx, my);
    let custom_bg = if self.custom_cursor_enabled {
      Color::rgb(60, 140, 80)
    } else if custom_hovered {
      Color::rgb(70, 75, 85)
    } else {
      Color::rgb(55, 60, 70)
    };
    ui.draw_rect(self.custom_btn_rect, custom_bg);
    ui.draw_rect_outline(
      self.custom_btn_rect,
      if self.custom_cursor_enabled {
        Color::rgb(100, 200, 120)
      } else {
        Color::rgb(80, 85, 95)
      },
      2.0,
    );
    let custom_label = if self.custom_cursor_enabled {
      "Custom Cursor: ON"
    } else {
      "Custom Cursor: OFF"
    };
    ui.draw_text(
      custom_label,
      self.custom_btn_rect.x + 10.0,
      self.custom_btn_rect.y + 12.0,
      Color::WHITE,
    );

    // Status panel
    let panel = self.status_rect;
    let panel_y = panel.y;
    let panel_x = panel.x + 10.0;
    ui.draw_rect(panel, Color::rgb(30, 34, 42));
    ui.draw_rect_outline(panel, Color::rgb(60, 65, 75), 1.0);

    ui.draw_text("Status", panel_x, panel_y + 8.0, Color::rgb(150, 200, 255));

    let style_text = if self.custom_cursor_enabled {
      "Current: Custom Sprite Cursor".to_string()
    } else {
      format!("Current Style: {:?}", self.current_style)
    };
    ui.draw_text(&style_text, panel_x, panel_y + 26.0, Color::WHITE);

    let visible_text = format!(
      "Visible: {}",
      if self.cursor_visible { "On" } else { "Off" }
    );
    let visible_color = if self.cursor_visible {
      Color::rgb(100, 255, 100)
    } else {
      Color::rgb(255, 100, 100)
    };
    ui.draw_text(&visible_text, panel_x, panel_y + 44.0, visible_color);

    let confined_text = format!(
      "Confined: {}",
      if self.cursor_confined { "On" } else { "Off" }
    );
    let confined_color = if self.cursor_confined {
      Color::rgb(255, 200, 100)
    } else {
      Color::rgb(150, 150, 150)
    };
    ui.draw_text(&confined_text, panel_x, panel_y + 62.0, confined_color);

    let coords_text = format!("Mouse: ({:.0}, {:.0})", mx, my);
    ui.draw_text(
      &coords_text,
      panel_x,
      panel_y + 80.0,
      Color::rgb(200, 200, 200),
    );
  }
}
