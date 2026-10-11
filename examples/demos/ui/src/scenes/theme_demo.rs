//! Theme demo page - demonstrates the hot-reloadable theme system

use demo_kit::prelude::*;

pub struct ThemeDemo {
  hot_theme: Option<HotReloadTheme>,
  theme_load_error: Option<String>,
  // Demo state
  slider_value: f32,
  checkbox_checked: bool,
  toggle_on: bool,
  progress_value: f32,
  progress_direction: f32,
  // Theme variant selection
  current_variant: usize,
  // Instructions visibility
  show_instructions: bool,
}

impl ThemeDemo {
  pub fn new() -> Self {
    Self {
      hot_theme: None,
      theme_load_error: None,
      slider_value: 0.5,
      checkbox_checked: false,
      toggle_on: true,
      progress_value: 0.3,
      progress_direction: 1.0,
      current_variant: 0,
      show_instructions: true,
    }
  }

  fn get_theme_path() -> std::path::PathBuf {
    std::path::PathBuf::from("assets/ui/theme.json")
  }

  fn initialize_theme(&mut self) {
    let path = Self::get_theme_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
      let _ = std::fs::create_dir_all(parent);
    }

    match HotReloadTheme::new(&path) {
      Ok(theme) => {
        // Sync current_variant with the loaded theme's name
        let loaded_theme = theme.get();
        self.current_variant = match loaded_theme.name.as_str() {
          "dark" => 0,
          "light" => 1,
          "neon" => 2,
          _ => 0,
        };
        self.hot_theme = Some(theme);
        self.theme_load_error = None;
      }
      Err(e) => {
        self.theme_load_error = Some(format!("Failed to load theme: {}", e));
      }
    }
  }

  fn switch_theme_variant(&mut self) {
    self.current_variant = (self.current_variant + 1) % 3;

    let new_theme = match self.current_variant {
      0 => Theme::default(),
      1 => Theme::light(),
      2 => create_neon_theme(),
      _ => Theme::default(),
    };

    if let Some(ref hot_theme) = self.hot_theme {
      let _ = hot_theme.set(new_theme);
    }
  }

  fn set_theme_variant(&mut self, variant: usize) {
    if variant != self.current_variant {
      self.current_variant = variant;
      let new_theme = match variant {
        0 => Theme::default(),
        1 => Theme::light(),
        2 => create_neon_theme(),
        _ => Theme::default(),
      };
      if let Some(ref hot_theme) = self.hot_theme {
        let _ = hot_theme.set(new_theme);
      }
    }
  }
}

/// Create a neon/cyberpunk themed UI variant
fn create_neon_theme() -> Theme {
  Theme {
    name: "neon".to_string(),
    button: ryo2d::core::ui::ButtonStyle {
      background: Color::rgb(30, 20, 50),
      background_hovered: Color::rgb(50, 30, 80),
      background_pressed: Color::rgb(80, 50, 120),
      background_disabled: Color::rgb(20, 15, 30),
      border: Color::rgb(150, 80, 255),
      border_hovered: Color::rgb(200, 120, 255),
      border_thickness: 2.0,
      text: Color::rgb(200, 180, 255),
      text_disabled: Color::rgb(80, 60, 100),
      padding: (8.0, 4.0),
    },
    panel: ryo2d::core::ui::PanelStyle {
      background: Color::new(20, 15, 35, 220),
      border: Some(Color::rgb(100, 60, 180)),
      border_thickness: 2.0,
      padding: 12.0,
    },
    progress_bar: ryo2d::core::ui::ProgressBarStyle {
      fill: Color::rgb(150, 80, 255),
      background: Color::rgb(30, 20, 50),
      border: Some(Color::rgb(100, 60, 180)),
      border_thickness: 1.0,
      height: 20.0,
    },
    slider: ryo2d::core::ui::SliderStyle {
      track_background: Color::rgb(40, 25, 60),
      track_fill: Color::rgb(150, 80, 255),
      handle: Color::rgb(200, 140, 255),
      handle_hovered: Color::rgb(220, 180, 255),
      track_height: 8.0,
      handle_width: 12.0,
      handle_height: 16.0,
    },
    checkbox: ryo2d::core::ui::CheckboxStyle {
      background: Color::rgb(30, 20, 50),
      background_checked: Color::rgb(100, 50, 180),
      background_hovered: Color::rgb(50, 35, 70),
      border: Color::rgb(150, 80, 255),
      checkmark: Color::rgb(220, 200, 255),
      size: 20.0,
      border_thickness: 2.0,
    },
    toggle: ryo2d::core::ui::ToggleStyle {
      track_off: Color::rgb(40, 25, 60),
      track_on: Color::rgb(100, 50, 180),
      track_hovered: Color::rgb(50, 35, 70),
      knob: Color::rgb(200, 140, 255),
      width: 44.0,
      height: 24.0,
      knob_size: 20.0,
    },
    label: ryo2d::core::ui::LabelStyle {
      text: Color::rgb(200, 180, 255),
      text_muted: Color::rgb(120, 100, 160),
      ..ryo2d::core::ui::LabelStyle::default()
    },
    ..Theme::default()
  }
}

impl Default for ThemeDemo {
  fn default() -> Self {
    Self::new()
  }
}

impl DemoPage for ThemeDemo {
  fn name(&self) -> &str {
    "Widgets & Themes"
  }

  fn description(&self) -> &str {
    "Buttons, sliders, toggles and a hot-reloaded theme (assets/ui/theme.json)."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::KeyT, "Next theme variant"),
      Control::key(KeyCode::KeyI, "Show or hide instructions"),
    ]
  }

  fn enter(&mut self, _ctx: &mut SceneContext) {
    self.initialize_theme();
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Poll for theme hot-reload
    if let Some(ref hot_theme) = self.hot_theme {
      hot_theme.poll_reload();
    }

    // Animate progress bar
    self.progress_value += dt * 0.2 * self.progress_direction;
    if self.progress_value >= 1.0 {
      self.progress_value = 1.0;
      self.progress_direction = -1.0;
    } else if self.progress_value <= 0.0 {
      self.progress_value = 0.0;
      self.progress_direction = 1.0;
    }

    // Handle input
    if ctx.input.key_pressed(KeyCode::KeyT) {
      self.switch_theme_variant();
    }

    if ctx.input.key_pressed(KeyCode::KeyI) {
      self.show_instructions = !self.show_instructions;
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let top = content_area(ui).y;

    // Get current theme for colors
    let theme = self.hot_theme.as_ref().map(|t| t.get());
    let text_color = theme.as_ref().map(|t| t.label.text).unwrap_or(Color::WHITE);
    let muted_color = theme
      .as_ref()
      .map(|t| t.label.text_muted)
      .unwrap_or(Color::GRAY);

    // Background
    let bg_color = theme
      .as_ref()
      .map(|t| {
        Color::rgb(
          (t.panel.background.r as u16 * 80 / 100) as u8,
          (t.panel.background.g as u16 * 80 / 100) as u8,
          (t.panel.background.b as u16 * 80 / 100) as u8,
        )
      })
      .unwrap_or(Color::rgb(20, 20, 30));
    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      bg_color,
    );

    let theme_name = theme
      .as_ref()
      .map(|t| t.name.as_str())
      .unwrap_or("(no theme)");
    ui.draw_text_anchored(
      &format!("Current theme: {}", theme_name),
      Anchor::TopCenter,
      0.0,
      top,
      muted_color,
    );

    // Show error if theme failed to load
    if let Some(ref error) = self.theme_load_error {
      ui.draw_text_anchored(error, Anchor::TopCenter, 0.0, top + 20.0, Color::RED);
    }

    // Main demo panel
    let panel_width = 500.0;
    let panel_height = 380.0;
    let panel_x = (screen_w as f32 - panel_width) / 2.0;
    let panel_y = top + 45.0;
    let panel_rect = Rect::new(panel_x, panel_y, panel_width, panel_height);

    // Draw themed panel
    if let Some(ref theme) = theme {
      ui.draw_rect(panel_rect, theme.panel.background);
      if let Some(border) = theme.panel.border {
        ui.draw_rect_outline(panel_rect, border, theme.panel.border_thickness);
      }
    } else {
      ui.draw_themed_panel(panel_rect);
    }

    // Content inside panel
    let content_x = panel_x + 20.0;
    let mut y = panel_y + 25.0;
    let label_spacing = 35.0;
    let widget_x = panel_x + 180.0;
    let widget_width = 240.0; // Reduced to leave room for value text
    let value_x = widget_x + widget_width + 10.0; // Where value text goes
    let text_height = 8.0; // Approximate font height for centering

    // Buttons section
    ui.draw_text("Buttons:", content_x, y, text_color);

    let button_rect = Rect::new(widget_x, y - 5.0, 120.0, 30.0);
    let _ = ui.button("Click Me", button_rect);

    let button2_rect = Rect::new(widget_x + 130.0, y - 5.0, 120.0, 30.0);
    let _ = ui.button("Another", button2_rect);

    y += label_spacing + 10.0;

    // Slider section
    ui.draw_text("Slider:", content_x, y, text_color);
    let slider_height = theme.as_ref().map(|t| t.slider.track_height).unwrap_or(8.0);
    let slider_rect = Rect::new(widget_x, y + 2.0, widget_width, slider_height);
    ui.slider(slider_rect, &mut self.slider_value, 0.0, 1.0);
    // Center-align value text with slider
    let slider_center_y = slider_rect.y + slider_height / 2.0 - text_height / 2.0;
    ui.draw_text(
      &format!("{:.2}", self.slider_value),
      value_x,
      slider_center_y,
      muted_color,
    );

    y += label_spacing;

    // Progress bar section
    ui.draw_text("Progress:", content_x, y, text_color);
    let progress_height = theme
      .as_ref()
      .map(|t| t.progress_bar.height)
      .unwrap_or(20.0);
    let progress_rect = Rect::new(widget_x, y - 2.0, widget_width, progress_height);
    if let Some(ref theme) = theme {
      ui.draw_progress_bar(
        progress_rect,
        self.progress_value,
        theme.progress_bar.fill,
        theme.progress_bar.background,
      );
      if let Some(border) = theme.progress_bar.border {
        ui.draw_rect_outline(progress_rect, border, theme.progress_bar.border_thickness);
      }
    } else {
      ui.draw_themed_progress_bar(progress_rect, self.progress_value);
    }
    // Center-align value text with progress bar
    let progress_center_y = progress_rect.y + progress_height / 2.0 - text_height / 2.0;
    ui.draw_text(
      &format!("{:.0}%", self.progress_value * 100.0),
      value_x,
      progress_center_y,
      muted_color,
    );

    y += label_spacing;

    // Checkbox section
    ui.draw_text("Checkbox:", content_x, y, text_color);
    let checkbox_size = theme.as_ref().map(|t| t.checkbox.size).unwrap_or(20.0);
    let checkbox_rect = Rect::new(widget_x, y - 2.0, checkbox_size, checkbox_size);
    ui.checkbox(
      checkbox_rect,
      &mut self.checkbox_checked,
      Some("Enable feature"),
    );

    y += label_spacing;

    // Toggle section
    ui.draw_text("Toggle:", content_x, y, text_color);
    let toggle_width = theme.as_ref().map(|t| t.toggle.width).unwrap_or(44.0);
    let toggle_height = theme.as_ref().map(|t| t.toggle.height).unwrap_or(24.0);
    let toggle_rect = Rect::new(widget_x, y - 2.0, toggle_width, toggle_height);
    ui.toggle(toggle_rect, &mut self.toggle_on);
    let toggle_label = if self.toggle_on { "ON" } else { "OFF" };
    // Center-align label with toggle
    let toggle_center_y = toggle_rect.y + toggle_height / 2.0 - text_height / 2.0;
    ui.draw_text(toggle_label, widget_x + 55.0, toggle_center_y, muted_color);

    y += label_spacing + 10.0;

    // Themed panel preview - use a lighter variant for contrast
    ui.draw_text("Nested Panel:", content_x, y, text_color);
    let nested_panel_rect = Rect::new(widget_x, y - 5.0, widget_width, 60.0);
    if let Some(ref theme) = theme {
      // Make nested panel slightly lighter/more opaque for visibility
      let nested_bg = Color::new(
        theme.panel.background.r.saturating_add(15),
        theme.panel.background.g.saturating_add(15),
        theme.panel.background.b.saturating_add(15),
        theme.panel.background.a.saturating_add(30),
      );
      ui.draw_rect(nested_panel_rect, nested_bg);
      if let Some(border) = theme.panel.border {
        // Make border slightly brighter too
        let nested_border = Color::rgb(
          border.r.saturating_add(20),
          border.g.saturating_add(20),
          border.b.saturating_add(20),
        );
        ui.draw_rect_outline(
          nested_panel_rect,
          nested_border,
          theme.panel.border_thickness,
        );
      }
    } else {
      ui.draw_themed_panel(nested_panel_rect);
    }
    ui.draw_text_centered("Content goes here", nested_panel_rect, text_color);

    y += 80.0;

    // Theme variant buttons
    ui.draw_text("Theme Variants:", content_x, y, text_color);

    let variants = ["Dark", "Light", "Neon"];
    for (i, name) in variants.iter().enumerate() {
      let btn_x = widget_x + (i as f32 * 95.0);
      let btn_rect = Rect::new(btn_x, y - 5.0, 85.0, 28.0);

      let is_selected = i == self.current_variant;
      if is_selected {
        // Highlight selected variant
        ui.draw_rect_outline(btn_rect.expand(2.0), Color::rgb(100, 180, 255), 2.0);
      }

      if ui.button(name, btn_rect) {
        self.set_theme_variant(i);
      }
    }

    // Instructions panel (collapsible)
    if self.show_instructions {
      let inst_panel = Rect::new(10.0, screen_h as f32 - 160.0, 280.0, 112.0);
      ui.draw_panel(
        inst_panel,
        Color::new(0, 0, 0, 180),
        Some(Color::rgb(60, 60, 80)),
      );

      let inst_x = 20.0;
      let mut inst_y = screen_h as f32 - 150.0;
      let inst_line_h = 18.0;

      ui.draw_text("Instructions:", inst_x, inst_y, Color::WHITE);
      inst_y += inst_line_h + 5.0;
      ui.draw_text(
        "T - Cycle theme variants",
        inst_x,
        inst_y,
        Color::LIGHT_GRAY,
      );
      inst_y += inst_line_h;
      ui.draw_text("I - Toggle instructions", inst_x, inst_y, Color::LIGHT_GRAY);
      inst_y += inst_line_h + 5.0;
      ui.draw_text(
        "Edit theme.json for hot-reload!",
        inst_x,
        inst_y,
        Color::rgb(150, 200, 255),
      );
    } else {
      ui.draw_text_anchored(
        "Press I for instructions",
        Anchor::BottomLeft,
        10.0,
        -34.0,
        muted_color,
      );
    }

    // Theme file path info
    let path_text = format!("Theme file: {}", Self::get_theme_path().display());
    ui.draw_text_anchored(&path_text, Anchor::BottomRight, -10.0, -34.0, muted_color);
  }
}
