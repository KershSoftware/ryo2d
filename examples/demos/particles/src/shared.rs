//! What the pages share: the view, the particle image, placing an emitter,
//! the live count, and the looks of the demo's own effects

use demo_kit::prelude::*;

/// The screen's size, with the camera centred on it so that world positions
/// are screen positions (0, 0 at the top left)
pub fn screen_view(ctx: &mut SceneContext) -> Vec2 {
  let (width, height) = ctx.screen_size();
  ctx.camera().set_position(width / 2.0, height / 2.0);
  Vec2::new(width, height)
}

/// A white disc: the image every particle draws, tinted by its effect
pub fn particle_image(ctx: &mut SceneContext) -> Handle<Image> {
  ctx
    .assets()
    .add(Image::solid_circle(16, 255, 255, 255, 255))
}

/// An emitter of `effect` at `(x, y)`
pub fn emit(ctx: &mut SceneContext, x: f32, y: f32, effect: &Handle<ParticleEffect>) -> Entity {
  ctx
    .world
    .spawn((
      Transform::from_xy(x, y),
      ParticleEmitter::new(effect.clone()),
    ))
    .id()
}

/// `effect` with each particle fading from `from` to `to` and shrinking to
/// nothing over its life
pub fn fading(effect: ParticleEffect, from: Color, to: Color) -> ParticleEffect {
  effect
    .with_color_over_life(Gradient::new([(0.0, from), (1.0, to)]))
    .with_size_over_life(Curve::new([(0.0, 1.0), (1.0, 0.0)]))
}

/// `effect` as a burst of `count` particles every `interval` seconds, the
/// first at once, for as long as it plays
pub fn every(mut effect: ParticleEffect, count: u32, interval: f32) -> ParticleEffect {
  effect.bursts = vec![Burst::at(0.0, count as f32)];
  effect
    .with_rate(0.0)
    .with_duration(interval)
    .with_looping(true)
}

/// The pointer in world space
pub fn pointer(ctx: &mut SceneContext) -> Vec2 {
  let (x, y) = ctx.input.mouse_position();
  ctx.camera().screen_to_world(Vec2::new(x, y))
}

/// The live particles against the engine's budget, at the top right of the
/// content area
pub fn count_label(ui: &mut UiContext, ctx: &mut SceneContext) {
  count_label_below(ui, ctx, 0);
}

/// `count_label` under the settings panel of a page with `rows` settings
pub fn count_label_below(ui: &mut UiContext, ctx: &mut SceneContext, rows: usize) {
  let particles = ctx.particles();
  let count = format!("Particles: {} / {}", particles.count(), particles.budget());
  let area = content_area(ui);
  let (text_w, _) = ui.text_size(&count);
  ui.draw_text(
    &count,
    area.x + area.width - text_w as f32,
    area.y + settings_panel_height(ui, rows) + if rows > 0 { 6.0 } else { 0.0 },
    Color::GREEN,
  );
}

/// `text` in `color` at `(x, y)` on a dim backing, so it reads over particles
pub fn label(ui: &mut UiContext, x: f32, y: f32, text: &str, color: Color) {
  let (text_w, text_h) = ui.text_size(text);
  let (text_w, text_h) = (text_w as f32, text_h as f32);
  ui.draw_rect(
    Rect::new(x - 3.0, y - 2.0, text_w + 6.0, text_h + 4.0),
    Color::new(12, 12, 20, 190),
  );
  ui.draw_text(text, x, y, color);
}

/// `label`, centred on `x`
pub fn label_centered(ui: &mut UiContext, x: f32, y: f32, text: &str, color: Color) {
  let (text_w, _) = ui.text_size(text);
  label(ui, x - text_w as f32 / 2.0, y, text, color);
}

/// A tile's side in the pages' small maps
pub const TILE: f32 = 40.0;

/// A run of solid tiles: `cols` by `rows`, from tile (`col`, `row`)
#[derive(Clone, Copy)]
pub struct Block {
  pub col: i32,
  pub row: i32,
  pub cols: i32,
  pub rows: i32,
}

impl Block {
  pub const fn new(col: i32, row: i32, cols: i32, rows: i32) -> Self {
    Self {
      col,
      row,
      cols,
      rows,
    }
  }
}

/// A tile map of `blocks` built in code: its `TileColliders`, which the
/// particles meet, and a sprite for each block, which draws it. Tile (0, 0)
/// is the screen's top left corner
pub fn solid_map(ctx: &mut SceneContext, blocks: &[Block]) {
  let (width, height) = ctx.screen_size();
  let mut tiles = TileColliders::new(
    (width / TILE).ceil() as u32,
    (height / TILE).ceil() as u32,
    Vec2::splat(TILE),
  );
  let stone = ctx
    .assets()
    .add(Image::solid_rect(TILE as u32, TILE as u32, 58, 66, 92, 255));
  for block in blocks {
    for col in block.col..block.col + block.cols {
      for row in block.row..block.row + block.rows {
        tiles.set(IVec2::new(col, row), TileCell::Solid);
      }
    }
    ctx.world.spawn((
      Transform::from_xy(block.col as f32 * TILE, block.row as f32 * TILE)
        .with_scale(Vec2::new(block.cols as f32, block.rows as f32)),
      Renderable::image(stone.clone())
        .with_anchor(Anchor::TopLeft)
        .with_z_order(-10),
    ));
  }
  ctx.world.spawn((Transform::from_xy(0.0, 0.0), tiles));
}
