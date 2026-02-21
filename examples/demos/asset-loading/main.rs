//! Asset Loading Demo
//!
//! Demonstrates the AssetManager's async loading with progress tracking.
//! Shows how to build a loading screen with progress bar and current asset display.

use ryo2d::prelude::*;

const SCREEN_WIDTH: u32 = 600;
const SCREEN_HEIGHT: u32 = 400;

fn main() {
  let engine = EngineBuilder::new()
    .title("Asset Loading Demo")
    .resolution(SCREEN_WIDTH, SCREEN_HEIGHT)
    .show_fps(true)
    .start_scene(LoadingDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}

/// Demo states
#[derive(Debug, Clone, Copy, PartialEq)]
enum DemoState {
  /// Waiting for user to click load button
  Ready,
  /// Assets are being loaded
  Loading,
  /// All assets loaded, displaying them
  Complete,
}

struct LoadingDemoScene {
  state: DemoState,
  load_clicked: bool,
  reset_clicked: bool,
  /// Loaded sprite handles (populated after loading completes)
  loaded_sprites: Vec<SpriteHandle>,
  /// Animation timer for displaying sprites
  display_timer: f32,
  /// How many completed assets to show in UI (revealed incrementally for demo effect)
  ui_shown_count: usize,
  /// Timer for incrementally revealing loaded assets in UI
  reveal_timer: f32,
  /// Actual loading progress from AssetManager
  actual_progress: LoadingProgress,
}

impl LoadingDemoScene {
  fn new() -> Self {
    Self {
      state: DemoState::Ready,
      load_clicked: false,
      reset_clicked: false,
      loaded_sprites: Vec::new(),
      display_timer: 0.0,
      ui_shown_count: 0,
      reveal_timer: 0.0,
      actual_progress: LoadingProgress::default(),
    }
  }

  /// Get the list of assets to load for the demo
  fn assets_to_load() -> Vec<&'static str> {
    vec![
      // Forest battle background layers (various sizes for realistic loading demo)
      "assets/back_decor.png",   // 457KB - largest
      "assets/bg.png",           // 138KB
      "assets/front_decor.png",  // 132KB
      "assets/battleground.png", // 104KB
      "assets/back_land.png",    // 87KB
      "assets/ground_decor.png", // 76KB
      "assets/bg-ground.png",    // 39KB
      "assets/ground.png",       // 1KB
      "assets/pipe_cap.png",     // 1KB
      "assets/pipe_body.png",    // 1KB
    ]
  }
}

impl Scene for LoadingDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.world.clear();
    ctx
      .camera
      .set_position(SCREEN_WIDTH as f32 / 2.0, SCREEN_HEIGHT as f32 / 2.0);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    match self.state {
      DemoState::Ready => {
        if self.load_clicked {
          self.load_clicked = false;

          // Reset for new loading sequence
          self.loaded_sprites.clear();
          self.ui_shown_count = 0;
          self.reveal_timer = 0.0;
          self.display_timer = 0.0;

          // Start async loading of ALL assets at once (real API usage)
          let assets = Self::assets_to_load();
          ctx.assets.start_loading(assets.clone());

          self.state = DemoState::Loading;
        }
      }

      DemoState::Loading => {
        // Process async loads every frame (normal real-world usage)
        ctx.assets.process_async_loads();

        // Get actual progress from AssetManager
        self.actual_progress = ctx.assets.loading_progress();
        self.display_timer += dt;

        // Incrementally reveal completed assets in UI for demo visibility
        // (UI catches up to actual progress over time)
        const REVEAL_DELAY: f32 = 0.25; // seconds between revealing each asset in UI
        self.reveal_timer += dt;
        if self.reveal_timer >= REVEAL_DELAY && self.ui_shown_count < self.actual_progress.completed
        {
          self.reveal_timer = 0.0;
          self.ui_shown_count += 1;
        }

        // Complete when UI has caught up to actual progress and all assets are loaded
        if ctx.assets.is_loading_complete() && self.ui_shown_count >= self.actual_progress.completed
        {
          // Collect the loaded sprites
          self.loaded_sprites.clear();
          for path in Self::assets_to_load() {
            if let Ok(handle) = ctx.assets.load_sprite(path) {
              self.loaded_sprites.push(handle);
            }
          }

          self.state = DemoState::Complete;
          self.display_timer = 0.0;
        }
      }

      DemoState::Complete => {
        // Animate the display
        self.display_timer += dt;

        if self.reset_clicked {
          self.reset_clicked = false;

          // Clear loaded sprites and reset
          self.loaded_sprites.clear();
          ctx.assets.reset_loading_progress();

          self.state = DemoState::Ready;
          self.display_timer = 0.0;
        }
      }
    }

    SceneAction::Continue
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();
    let center_x = screen_w as f32 / 2.0;
    let screen_h = screen_h as f32;

    // Title
    ui.draw_text_anchored(
      "Asset Loading Demo",
      Anchor::TopCenter,
      0.0,
      20.0,
      Color::WHITE,
    );

    match self.state {
      DemoState::Ready => {
        self.render_ready_state(ui, center_x, screen_h);
      }
      DemoState::Loading => {
        self.render_loading_state(ui, center_x, screen_h);
      }
      DemoState::Complete => {
        self.render_complete_state(ui, center_x, screen_h);
      }
    }
    SceneAction::Continue
  }
}

impl LoadingDemoScene {
  fn render_ready_state(&mut self, ui: &mut UiContext, center_x: f32, screen_h: f32) {
    // Instructions
    ui.draw_text_anchored(
      "Click the button to start loading assets",
      Anchor::TopCenter,
      0.0,
      100.0,
      Color::LIGHT_GRAY,
    );

    // Asset list preview
    let assets = Self::assets_to_load();
    let list_y = 140.0;
    ui.draw_text("Assets to load:", 50.0, list_y, Color::GRAY);

    for (i, path) in assets.iter().enumerate() {
      let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
      let y = list_y + 25.0 + (i as f32 * 20.0);
      ui.draw_text(&format!("  - {}", filename), 50.0, y, Color::DARK_GRAY);
    }

    // Load button
    let button_w = 200.0;
    let button_h = 50.0;
    let button_x = center_x - button_w / 2.0;
    let button_y = screen_h - 100.0;

    if ui.button(
      "Load Assets",
      Rect::new(button_x, button_y, button_w, button_h),
    ) {
      self.load_clicked = true;
    }
  }

  fn render_loading_state(&mut self, ui: &mut UiContext, center_x: f32, _screen_h: f32) {
    // Use ui_shown_count for visual progress (gradually catches up to actual progress)
    let total = Self::assets_to_load().len().max(1);
    let completed = self.ui_shown_count;
    let percentage = completed as f32 / total as f32;

    // Loading title with animated dots
    let dots = match ((self.display_timer * 3.0) as usize) % 4 {
      0 => "",
      1 => ".",
      2 => "..",
      _ => "...",
    };
    let loading_text = format!("Loading{}", dots);
    ui.draw_text_anchored(&loading_text, Anchor::TopCenter, 0.0, 120.0, Color::WHITE);

    // Progress bar
    let bar_w = 400.0;
    let bar_h = 30.0;
    let bar_x = center_x - bar_w / 2.0;
    let bar_y = 160.0;

    // Background
    ui.draw_rect(Rect::new(bar_x, bar_y, bar_w, bar_h), Color::DARK_GRAY);

    // Progress fill
    let fill_w = bar_w * percentage;
    if fill_w > 0.0 {
      ui.draw_rect(Rect::new(bar_x, bar_y, fill_w, bar_h), Color::GREEN);
    }

    // Border
    ui.draw_rect_outline(Rect::new(bar_x, bar_y, bar_w, bar_h), Color::WHITE, 2.0);

    // Progress text
    let progress_text = format!("{}/{} ({:.0}%)", completed, total, percentage * 100.0);
    ui.draw_text_anchored(
      &progress_text,
      Anchor::TopCenter,
      0.0,
      bar_y + bar_h + 10.0,
      Color::LIGHT_GRAY,
    );

    // Current asset being loaded (show next one to be revealed)
    let assets = Self::assets_to_load();
    if completed < total {
      let current_path = assets[completed];
      let filename = current_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(current_path);
      let current_text = format!("Loading: {}", filename);
      ui.draw_text_anchored(
        &current_text,
        Anchor::TopCenter,
        0.0,
        bar_y + bar_h + 35.0,
        Color::GRAY,
      );
    }
  }

  fn render_complete_state(&mut self, ui: &mut UiContext, center_x: f32, screen_h: f32) {
    // Success message
    let success_text = format!("Loaded {} assets!", self.loaded_sprites.len());
    ui.draw_text_anchored(&success_text, Anchor::TopCenter, 0.0, 60.0, Color::GREEN);

    // Display loaded sprites in a grid
    let start_x = 50.0;
    let start_y = 100.0;
    let max_preview_size = 80.0;
    let spacing = 110.0;

    for (i, sprite) in self.loaded_sprites.iter().enumerate() {
      let col = i % 5;
      let row = i / 5;

      let x = start_x + (col as f32 * spacing);
      let y = start_y + (row as f32 * spacing);

      // Scale sprite to fit preview size
      let scale_x = max_preview_size / sprite.width() as f32;
      let scale_y = max_preview_size / sprite.height() as f32;
      let scale = scale_x.min(scale_y).min(1.0);

      let draw_h = sprite.height() as f32 * scale;

      // Draw sprite preview
      ui.draw_sprite_handle_scaled(sprite, x, y, scale);

      // Draw size info below
      let size_text = format!("{}x{}", sprite.width(), sprite.height());
      let (text_w, _) = ui.text_size(&size_text);
      ui.draw_text(
        &size_text,
        x + max_preview_size / 2.0 - text_w as f32 / 2.0,
        y + draw_h + 5.0,
        Color::GRAY,
      );
    }

    // Reset button
    let button_w = 150.0;
    let button_h = 40.0;
    let button_x = center_x - button_w / 2.0;
    let button_y = screen_h - 60.0;

    if ui.button("Reset", Rect::new(button_x, button_y, button_w, button_h)) {
      self.reset_clicked = true;
    }
  }
}
