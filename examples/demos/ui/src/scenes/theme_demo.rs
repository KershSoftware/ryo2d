//! Theme demo page - the UI theme: built-in themes, variants, skins and a
//! hot-reloaded theme file

use demo_kit::prelude::*;

/// The theme file the "File" theme loads, and reloads when it is saved
const THEME_FILE: &str = "assets/ui/theme.ron";

/// The themes the T key cycles through
const THEME_NAMES: [&str; 5] = ["Dark", "Light", "Pixel", "Neon", "File"];

pub struct ThemeDemo {
  theme_load_error: Option<String>,
  // Demo state
  slider_value: f32,
  checkbox_checked: bool,
  toggle_on: bool,
  progress_value: f32,
  progress_direction: f32,
  /// Which of `THEME_NAMES` is in use
  current_theme: usize,
  // Instructions visibility
  show_instructions: bool,
}

impl ThemeDemo {
  pub fn new() -> Self {
    Self {
      theme_load_error: None,
      slider_value: 0.5,
      checkbox_checked: false,
      toggle_on: true,
      progress_value: 0.3,
      progress_direction: 1.0,
      current_theme: 0,
      show_instructions: true,
    }
  }

  /// Use the theme `index` of `THEME_NAMES` from the next frame
  fn use_theme(&mut self, index: usize, ctx: &mut SceneContext) {
    self.current_theme = index % THEME_NAMES.len();
    self.theme_load_error = None;
    let theme = match self.current_theme {
      0 => Theme::dark(),
      1 => Theme::light(),
      2 => Theme::pixel(),
      3 => neon_theme(),
      _ => match Theme::load(THEME_FILE) {
        Ok(theme) => theme,
        Err(error) => {
          self.theme_load_error = Some(error.to_string());
          Theme::dark()
        }
      },
    };
    ctx.ui_theme().set(theme);
  }
}

/// A neon theme: the dark theme with a purple palette, looks for buttons,
/// the panel and the progress bar, and a variant of its own
fn neon_theme() -> Theme {
  let purple = Color::rgb(150, 80, 255);
  let mut theme = Theme::dark();
  theme.name = "neon".to_string();
  theme.palette.background = Color::rgb(16, 10, 28);
  theme.palette.text = Color::rgb(200, 180, 255);
  theme.palette.muted = Color::rgb(120, 100, 160);
  theme.palette.accent = purple;

  let button = &mut theme.widgets.button;
  button.normal.fill = Color::rgb(30, 20, 50);
  button.normal.border = purple;
  button.normal.border_width = 2.0;
  button.normal.text = Color::rgb(200, 180, 255);
  button.hovered = LookPatch::default()
    .fill(Color::rgb(50, 30, 80))
    .border(Color::rgb(200, 120, 255));
  button.pressed = LookPatch::default().fill(Color::rgb(80, 50, 120));
  button.focused = LookPatch::default()
    .fill(Color::rgb(50, 30, 80))
    .border(Color::rgb(220, 180, 255));
  button.disabled = LookPatch::default()
    .fill(Color::new(30, 20, 50, 120))
    .text(Color::new(200, 180, 255, 120));

  let panel = &mut theme.widgets.panel;
  panel.normal.fill = Color::new(20, 15, 35, 220);
  panel.normal.border = Color::rgb(100, 60, 180);
  panel.normal.border_width = 2.0;
  theme.widgets.progress.normal.fill = Color::rgb(30, 20, 50);
  theme.widgets.progress.normal.border = purple;

  theme.metrics.focus_ring.color = Color::rgb(220, 180, 255);
  theme.variant(
    "glow",
    Variant::of(WidgetKind::Button)
      .normal(
        LookPatch::default()
          .fill(Color::rgb(90, 40, 160))
          .border(Color::rgb(230, 150, 255)),
      )
      .hovered(LookPatch::default().fill(Color::rgb(120, 60, 200))),
  )
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
    "Buttons and panels in the dark, light, pixel, neon and file themes (assets/ui/theme.ron reloads when saved)."
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::KeyT, "Next theme"),
      Control::key(KeyCode::KeyI, "Show or hide instructions"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.use_theme(0, ctx);
  }

  /// The theme is global: the other pages draw in the dark theme again
  fn exit(&mut self, ctx: &mut SceneContext) {
    ctx.ui_theme().set(Theme::dark());
  }

  /// The whole page is UI: its keys and the progress bar's animation run in
  /// the UI pass, on real time
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    // Animate progress bar
    self.progress_value += ui.dt() * 0.2 * self.progress_direction;
    if self.progress_value >= 1.0 {
      self.progress_value = 1.0;
      self.progress_direction = -1.0;
    } else if self.progress_value <= 0.0 {
      self.progress_value = 0.0;
      self.progress_direction = 1.0;
    }

    // Handle input
    if ui.input().key_pressed(KeyCode::KeyT) {
      ui.consume_key(KeyCode::KeyT);
      self.use_theme(self.current_theme + 1, ctx);
    }

    if ui.input().key_pressed(KeyCode::KeyI) {
      ui.consume_key(KeyCode::KeyI);
      self.show_instructions = !self.show_instructions;
    }

    let (screen_w, screen_h) = ui.screen_size();
    let top = content_area(ui).y;

    // This frame's theme (a change applies from the next frame)
    let palette = ui.theme().palette.clone();
    let sizes = ui.theme().metrics.sizes.clone();
    let theme_name = ui.theme().name.clone();
    let (text_color, muted_color) = (palette.text, palette.muted);

    ui.draw_rect(
      Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32),
      palette.background,
    );

    ui.draw_text_anchored(
      &format!("Current theme: {theme_name}"),
      Anchor::TopCenter,
      0.0,
      top,
      muted_color,
    );

    // Show error if the theme file failed to load
    if let Some(ref error) = self.theme_load_error {
      ui.draw_text_anchored(error, Anchor::TopCenter, 0.0, top + 20.0, Color::RED);
    }

    // Main demo panel
    let panel_width = 500.0;
    let panel_height = 380.0;
    let panel_x = (screen_w as f32 - panel_width) / 2.0;
    let panel_y = top + 45.0;
    ui.panel()
      .at(Rect::new(panel_x, panel_y, panel_width, panel_height))
      .show(|_| {});

    // Content inside panel
    let content_x = panel_x + 20.0;
    let mut y = panel_y + 25.0;
    let label_spacing = 35.0;
    let widget_x = panel_x + 180.0;
    let widget_width = 240.0; // Reduced to leave room for value text
    let value_x = widget_x + widget_width + 10.0; // Where value text goes
    let text_height = 8.0; // Approximate font height for centering

    // Buttons section: the plain look, then three variants
    ui.draw_text("Buttons:", content_x, y, text_color);
    let variants = [None, Some("accent"), Some("danger"), Some("tab")];
    let labels = ["Plain", "Accent", "Danger", "Tab"];
    for (i, (variant, label)) in variants.into_iter().zip(labels).enumerate() {
      let rect = Rect::new(widget_x + i as f32 * 80.0, y - 5.0, 74.0, 30.0);
      let button = ui.button(label).at(rect);
      let _ = match variant {
        Some(name) => button.variant(name).clicked(),
        None => button.clicked(),
      };
    }

    y += label_spacing + 10.0;

    // Slider section
    ui.draw_text("Slider:", content_x, y, text_color);
    let slider_height = sizes.slider[1];
    let slider_rect = Rect::new(widget_x, y + 2.0, widget_width, slider_height);
    // The handle is taller than the track: the widget is as tall as the handle
    let handle_height = sizes.slider_handle[1].max(slider_height);
    ui.slider(&mut self.slider_value, 0.0..=1.0)
      .at(Rect::new(
        slider_rect.x,
        slider_rect.y + slider_height / 2.0 - handle_height / 2.0,
        widget_width,
        handle_height,
      ))
      .show();
    // Center-align value text with slider
    let slider_center_y = slider_rect.y + slider_height / 2.0 - text_height / 2.0;
    ui.draw_text(
      &format!("{:.2}", self.slider_value),
      value_x,
      slider_center_y,
      muted_color,
    );

    y += label_spacing;

    // Progress bar section: the track is the look's fill, the filled part its border
    ui.draw_text("Progress:", content_x, y, text_color);
    let progress_height = 20.0;
    let progress_rect = Rect::new(widget_x, y - 2.0, widget_width, progress_height);
    ui.progress(self.progress_value).at(progress_rect).show();
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
    let checkbox_rect = Rect::new(widget_x, y - 2.0, sizes.checkbox, sizes.checkbox);
    ui.checkbox(&mut self.checkbox_checked, "Enable feature")
      .at(Rect::new(
        checkbox_rect.x,
        checkbox_rect.y,
        widget_width,
        checkbox_rect.height,
      ))
      .show();

    y += label_spacing;

    // Toggle section
    ui.draw_text("Toggle:", content_x, y, text_color);
    let toggle_rect = Rect::new(widget_x, y - 2.0, sizes.toggle[0], sizes.toggle[1]);
    let toggle_label = if self.toggle_on { "ON" } else { "OFF" };
    ui.toggle(&mut self.toggle_on, toggle_label)
      .at(Rect::new(
        toggle_rect.x,
        toggle_rect.y,
        widget_width,
        toggle_rect.height,
      ))
      .show();

    y += label_spacing + 10.0;

    // A second panel, to see the panel look (and its skin) at another size
    ui.draw_text("Nested Panel:", content_x, y, text_color);
    let nested_panel_rect = Rect::new(widget_x, y - 5.0, widget_width, 60.0);
    ui.panel().at(nested_panel_rect).show(|_| {});
    ui.draw_text_centered("Content goes here", nested_panel_rect, text_color);

    y += 80.0;

    // Theme buttons: the one in use is an accent button
    ui.draw_text("Themes:", content_x, y, text_color);
    for (i, name) in THEME_NAMES.iter().enumerate() {
      let btn_rect = Rect::new(widget_x - 60.0 + i as f32 * 80.0, y - 5.0, 74.0, 28.0);
      let variant = if i == self.current_theme {
        "accent"
      } else {
        "tab"
      };
      if ui.button(*name).at(btn_rect).variant(variant).clicked() {
        self.use_theme(i, ctx);
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
      ui.draw_text("T - Cycle themes", inst_x, inst_y, Color::LIGHT_GRAY);
      inst_y += inst_line_h;
      ui.draw_text("I - Toggle instructions", inst_x, inst_y, Color::LIGHT_GRAY);
      inst_y += inst_line_h + 5.0;
      ui.draw_text(
        "Edit theme.ron for hot-reload!",
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
    let path_text = format!("Theme file: {THEME_FILE}");
    ui.draw_text_anchored(&path_text, Anchor::BottomRight, -10.0, -34.0, muted_color);
  }
}
