//! Anchor Positioning Demo - Ryo2D
//!
//! Shows the Anchor system for sprite positioning: the 9-point anchors
//! (TopLeft, Center, BottomRight, ...), how they change where a sprite appears
//! relative to its position, and offset adjustments for fine-tuning. Each cell
//! has a red crosshair at the target position and a sprite anchored to it.
//! Runs on the demo-kit shell.

use demo_kit::prelude::*;

/// Marker component for the demo sprites
#[derive(Debug)]
struct DemoSprite;

/// Marker for crosshair entities
#[derive(Debug)]
struct Crosshair;

/// Grid margin around the cells
const MARGIN: f32 = 40.0;
/// Space above the grid for the shell's top bar and description
const TOP: f32 = 72.0;
/// Space below the grid for the shell's legend and the offset readout
const BOTTOM: f32 = 70.0;
/// Share of the screen width the grid may use; the settings panel covers the rest
const GRID_WIDTH_FRACTION: f32 = 0.58;

/// Anchors with their labels and grid positions (column, row)
const ANCHORS: [(Anchor, &str, u32, u32); 9] = [
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

/// Size of one grid cell for a screen size
fn cell_size(screen_w: f32, screen_h: f32) -> (f32, f32) {
  (
    (screen_w * GRID_WIDTH_FRACTION - MARGIN) / 3.0,
    (screen_h - TOP - BOTTOM) / 3.0,
  )
}

/// The anchors page
#[derive(Default)]
struct AnchorPage {
  sprites: Vec<Entity>,
  offset_x: f32,
  offset_y: f32,
  show_offset_info: bool,
}

impl AnchorPage {
  fn new() -> Self {
    Self {
      show_offset_info: true,
      ..Self::default()
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
        transform.offset_x = self.offset_x;
        transform.offset_y = self.offset_y;
      }
    }
  }
}

impl DemoPage for AnchorPage {
  fn name(&self) -> &str {
    "Anchors"
  }

  fn description(&self) -> &str {
    "Red crosshair = target position | Blue rect = sprite with anchor\n\
     The anchor picks which part of the sprite sits on the crosshair (Transform x, y).\n\
     Offset is extra pixels applied after the anchor: Transform::with_offset(x, y)."
  }

  fn settings(&mut self, s: &mut Settings) {
    s.float("Offset X", &mut self.offset_x, -200.0..=200.0, 2.0);
    s.float("Offset Y", &mut self.offset_y, -200.0..=200.0, 2.0);
    s.toggle("Info panel", &mut self.show_offset_info);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let (screen_w, screen_h) = ctx.screen_size();

    // Create demo sprite (a colored rectangle with visible bounds)
    let sprite_w = 60u32;
    let sprite_h = 40u32;
    let demo_sprite = Sprite::solid_rect(sprite_w, sprite_h, 80, 140, 220, 200);
    let sprite_handle = SpriteHandle::from_sprite(demo_sprite, "demo_rect");

    // Create crosshair for marking target positions
    let crosshair_handle = Self::create_crosshair();

    let (cell_w, cell_h) = cell_size(screen_w, screen_h);

    // Spawn sprites for each anchor type
    for (anchor, _label, col, row) in ANCHORS {
      // Calculate target position (center of each cell)
      let target_x = MARGIN + cell_w * (col as f32 + 0.5);
      let target_y = TOP + cell_h * (row as f32 + 0.5);

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

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.update_offsets(ctx.world);
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    let (screen_w, screen_h) = ui.screen_size();
    let (cell_w, cell_h) = cell_size(screen_w as f32, screen_h as f32);

    for (_, label, col, row) in ANCHORS {
      let label_x = MARGIN + cell_w * col as f32 + 5.0;
      let label_y = TOP + cell_h * row as f32 + 5.0;
      ui.draw_text(label, label_x, label_y, Color::YELLOW);
    }

    if self.show_offset_info {
      let area = content_area(ui);
      let offset_text = format!(
        "Current Offset: ({:.0}, {:.0})",
        self.offset_x, self.offset_y
      );
      ui.draw_text(
        &offset_text,
        area.x,
        area.y + area.height - 20.0,
        Color::GREEN,
      );
    }
  }
}

fn main() {
  Demo::new("Anchor Positioning")
    .resolution(800, 600)
    .configure(|b| {
      b.window_mode(WindowMode::ResizableIntegerScale)
        .target_fps(60)
        .physics_hz(60)
    })
    .page(AnchorPage::new)
    .run();
}
