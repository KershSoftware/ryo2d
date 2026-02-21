//! Anchor Positioning Demo - Ryo2D Example
//!
//! This example demonstrates the Anchor system for sprite positioning:
//! - 9-point anchor system (TopLeft, Center, BottomRight, etc.)
//! - How anchors affect where sprites appear relative to their position
//! - Offset adjustments for fine-tuning
//!
//! ## Visual Guide
//!
//! Each cell shows a red crosshair at the "target position" and a sprite
//! anchored to that position. The anchor determines which part of the
//! sprite aligns with the crosshair.
//!
//! ## Controls
//!
//! - Arrow keys: Adjust global offset
//! - R: Reset offset to zero
//! - Space: Toggle offset display
//! - Escape: Exit

use ryo2d::prelude::*;

/// Marker component for the demo sprites
#[derive(Debug)]
struct DemoSprite;

/// Marker for crosshair entities
#[derive(Debug)]
struct Crosshair;

/// Main demo scene
struct AnchorDemoScene {
  sprites: Vec<Entity>,
  offset: (f32, f32),
  show_offset_info: bool,
}

impl AnchorDemoScene {
  fn new() -> Self {
    Self {
      sprites: Vec::new(),
      offset: (0.0, 0.0),
      show_offset_info: true,
    }
  }

  /// Create a crosshair marker sprite
  fn create_crosshair() -> SpriteHandle {
    // 11x11 crosshair with red center
    let size = 11u32;
    let center = size / 2;
    let mut data = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
      for x in 0..size {
        let idx = ((y * size + x) * 4) as usize;
        // Draw cross pattern
        if x == center || y == center {
          // Red color for crosshair
          data[idx] = 255; // R
          data[idx + 1] = 50; // G
          data[idx + 2] = 50; // B
          data[idx + 3] = 255; // A
        }
      }
    }

    let sprite = Sprite::from_rgba(data, size, size);
    SpriteHandle::from_sprite(sprite, "crosshair")
  }

  /// Update sprite offsets
  fn update_offsets(&self, world: &mut World) {
    for &entity in &self.sprites {
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.offset_x = self.offset.0;
        transform.offset_y = self.offset.1;
      }
    }
  }
}

impl Scene for AnchorDemoScene {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    println!("Anchor Positioning Demo initialized!");
    println!("  Arrow keys: Adjust offset");
    println!("  R: Reset offset");
    println!("  Space: Toggle offset info");
    println!("  ESC: Exit");

    let (screen_w, screen_h) = ctx.screen_size();

    // Create demo sprite (a colored rectangle with visible bounds)
    let sprite_w = 60u32;
    let sprite_h = 40u32;
    let demo_sprite = Sprite::solid_rect(sprite_w, sprite_h, 80, 140, 220, 200);
    let sprite_handle = SpriteHandle::from_sprite(demo_sprite, "demo_rect");

    // Create crosshair for marking target positions
    let crosshair_handle = Self::create_crosshair();

    // Define the 9 anchors with their labels and grid positions
    let anchors = [
      (Anchor::TopLeft, "TopLeft", 0, 0),
      (Anchor::TopCenter, "TopCenter", 1, 0),
      (Anchor::TopRight, "TopRight", 2, 0),
      (Anchor::CenterLeft, "CenterLeft", 0, 1),
      (Anchor::Center, "Center", 1, 1),
      (Anchor::CenterRight, "CenterRight", 2, 1),
      (Anchor::BottomLeft, "BottomLeft", 0, 2),
      (Anchor::BottomCenter, "BottomCenter", 1, 2),
      (Anchor::BottomRight, "BottomRight", 2, 2),
    ];

    // Calculate grid cell size
    let margin = 40.0;
    let cell_w = (screen_w - margin * 2.0) / 3.0;
    let cell_h = (screen_h - margin * 2.0 - 60.0) / 3.0; // Extra space for title

    // Spawn sprites for each anchor type
    for (anchor, _label, col, row) in anchors {
      // Calculate target position (center of each cell)
      let target_x = margin + cell_w * (col as f32 + 0.5);
      let target_y = margin + 60.0 + cell_h * (row as f32 + 0.5);

      // Spawn crosshair at target position (always centered)
      ctx.world.spawn((
        Crosshair,
        Transform::new(target_x, target_y).with_anchor(Anchor::Center),
        Renderable::sprite(crosshair_handle.clone()).with_z_order(10),
      ));

      // Spawn demo sprite at target position with the specified anchor
      let entity = ctx.world.spawn((
        DemoSprite,
        Transform::new(target_x, target_y).with_anchor(anchor),
        Renderable::sprite(sprite_handle.clone()).with_z_order(5),
      ));

      self.sprites.push(entity);
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    // Exit on escape
    if ctx.input.key_pressed(KeyCode::Escape) {
      return SceneAction::Quit;
    }

    // Toggle offset info
    if ctx.input.key_pressed(KeyCode::Space) {
      self.show_offset_info = !self.show_offset_info;
    }

    // Reset offset
    if ctx.input.key_pressed(KeyCode::KeyR) {
      self.offset = (0.0, 0.0);
      self.update_offsets(ctx.world);
    }

    // Adjust offset with arrow keys (key_down for continuous movement)
    let offset_step = 2.0;
    let mut changed = false;

    if ctx.input.key_down(KeyCode::ArrowLeft) {
      self.offset.0 -= offset_step;
      changed = true;
    }
    if ctx.input.key_down(KeyCode::ArrowRight) {
      self.offset.0 += offset_step;
      changed = true;
    }
    if ctx.input.key_down(KeyCode::ArrowUp) {
      self.offset.1 -= offset_step;
      changed = true;
    }
    if ctx.input.key_down(KeyCode::ArrowDown) {
      self.offset.1 += offset_step;
      changed = true;
    }

    if changed {
      self.update_offsets(ctx.world);
    }

    SceneAction::Continue
  }

  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {
    // Sprites are rendered automatically by the engine
  }

  fn render_ui(&mut self, ui: &mut UiContext) -> SceneAction {
    let (screen_w, screen_h) = ui.screen_size();
    let screen_w = screen_w as f32;
    let screen_h = screen_h as f32;

    // Title
    ui.draw_text(
      "Anchor Positioning Demo",
      screen_w / 2.0 - 100.0,
      10.0,
      Color::WHITE,
    );

    // Subtitle explaining the visual
    ui.draw_text(
      "Red crosshair = target position | Blue rect = sprite with anchor",
      screen_w / 2.0 - 230.0,
      30.0,
      Color::new(180, 180, 180, 255),
    );

    // Draw labels for each cell
    let margin = 40.0;
    let cell_w = (screen_w - margin * 2.0) / 3.0;
    let cell_h = (screen_h - margin * 2.0 - 60.0) / 3.0;

    let labels = [
      ("TopLeft", 0, 0),
      ("TopCenter", 1, 0),
      ("TopRight", 2, 0),
      ("CenterLeft", 0, 1),
      ("Center", 1, 1),
      ("CenterRight", 2, 1),
      ("BottomLeft", 0, 2),
      ("BottomCenter", 1, 2),
      ("BottomRight", 2, 2),
    ];

    for (label, col, row) in labels {
      let label_x = margin + cell_w * col as f32 + 5.0;
      let label_y = margin + 60.0 + cell_h * row as f32 + 5.0;
      ui.draw_text(label, label_x, label_y, Color::YELLOW);
    }

    // Controls and offset info at bottom
    let info_y = screen_h - 60.0;
    ui.draw_text(
      "Controls: Arrows=Offset  R=Reset  Space=Toggle Info  ESC=Exit",
      10.0,
      info_y,
      Color::WHITE,
    );

    if self.show_offset_info {
      let offset_text = format!(
        "Current Offset: ({:.0}, {:.0})",
        self.offset.0, self.offset.1
      );
      ui.draw_text(&offset_text, 10.0, info_y + 20.0, Color::GREEN);
    }
    SceneAction::Continue
  }
}

fn main() {
  let engine = EngineBuilder::new()
    .title("Anchor Positioning Demo - Ryo2D")
    .resolution(800, 600)
    .window_mode(WindowMode::ResizableIntegerScale)
    .target_fps(60)
    .physics_hz(60)
    .start_scene(AnchorDemoScene::new())
    .build();

  if let Err(e) = engine.run() {
    eprintln!("Engine error: {}", e);
  }
}
