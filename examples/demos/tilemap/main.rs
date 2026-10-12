//! Tilemap System Demo - Ryo2D Example
//!
//! Demonstrates the tilemap data API: programmatic creation, tile queries,
//! coordinate conversion, collision rect generation, and the TilemapManager.
//!
//! Since GPU tile rendering is not yet wired up, tiles are visualised as
//! colored Renderable entities and collision rectangles are drawn as
//! semi-transparent UI overlays.
//!
//! Runs on the demo-kit shell; hover, click and WASD are listed in its legend.

use std::collections::HashMap;

use demo_kit::prelude::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TILE_SIZE: u32 = 32;
const MAP_WIDTH: u32 = 20;
const MAP_HEIGHT: u32 = 15;
const CAMERA_SPEED: f32 = 200.0;

// Tile IDs
const TILE_GROUND: TileId = 0;
const TILE_PLATFORM: TileId = 1;

// ---------------------------------------------------------------------------
// Marker component for tile visualisation entities
// ---------------------------------------------------------------------------

/// Marker so we can despawn all tile sprites when rebuilding.
#[derive(Component, Debug)]
struct TileVis;

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

struct TilemapPage {
  map_entity: Option<Entity>,
  collision_rects: Vec<CollisionRect>,
  hover_tile: Option<(u32, u32)>,
  hover_world: (f32, f32),
}

impl TilemapPage {
  fn new() -> Self {
    Self {
      map_entity: None,
      collision_rects: Vec::new(),
      hover_tile: None,
      hover_world: (0.0, 0.0),
    }
  }

  /// Build the initial tilemap data (tilesets, layers, tiles).
  fn build_tilemap() -> Tilemap {
    // Create tileset with two tile types: ground (0) and platform (1).
    // Both are marked solid so collision generation picks them up.
    let mut props = HashMap::new();
    props.insert(
      TILE_GROUND,
      TileProperties {
        solid: true,
        ..Default::default()
      },
    );
    props.insert(
      TILE_PLATFORM,
      TileProperties {
        solid: true,
        ..Default::default()
      },
    );

    let tileset = Tileset {
      name: "demo".to_string(),
      image_path: String::new(), // no image — we use sprite proxies
      tile_width: TILE_SIZE,
      tile_height: TILE_SIZE,
      columns: 2,
      tile_count: 2,
      spacing: 0,
      margin: 0,
      image_width: TILE_SIZE * 2,
      image_height: TILE_SIZE,
      properties: props,
    };

    let mut tilemap = Tilemap::new(TILE_SIZE, TILE_SIZE, MAP_WIDTH, MAP_HEIGHT);
    tilemap.add_tileset(tileset);

    // Single layer
    let mut layer = TilemapLayer::new("terrain", MAP_WIDTH, MAP_HEIGHT);

    // Fill bottom 2 rows with ground
    for y in (MAP_HEIGHT - 2)..MAP_HEIGHT {
      for x in 0..MAP_WIDTH {
        layer.set_tile(x, y, TILE_GROUND);
      }
    }

    // Floating platforms
    for x in 3..7 {
      layer.set_tile(x, 10, TILE_PLATFORM);
    }
    for x in 10..15 {
      layer.set_tile(x, 8, TILE_PLATFORM);
    }
    for x in 6..9 {
      layer.set_tile(x, 5, TILE_PLATFORM);
    }

    tilemap.add_layer(layer);
    tilemap
  }

  /// Create a solid-color sprite for a tile type.
  fn tile_sprite(tile_id: TileId) -> SpriteHandle {
    let (r, g, b) = match tile_id {
      TILE_GROUND => (90, 65, 45),    // brown earth
      TILE_PLATFORM => (70, 130, 70), // mossy green
      _ => (180, 180, 180),           // grey fallback
    };
    let sprite = Sprite::solid_rect(TILE_SIZE, TILE_SIZE, r, g, b, 255);
    SpriteHandle::from_sprite(sprite, format!("tile_{}", tile_id))
  }

  /// Spawn Renderable entities for every non-empty tile so we can see them.
  fn spawn_tile_visuals(&self, ctx: &mut SceneContext) {
    let Some(entity) = self.map_entity else {
      return;
    };

    // Read the tiles first: the manager is a world resource
    let tiles: Vec<(f32, f32, TileId)> = {
      let tilemaps = ctx.tilemaps();
      let Some(tilemap) = tilemaps.get_tilemap(entity) else {
        return;
      };
      let mut tiles = Vec::new();
      for layer in &tilemap.layers {
        for y in 0..layer.height {
          for x in 0..layer.width {
            if let Some(tile_id) = layer.get_tile(x, y) {
              let (wx, wy) = tilemap.tile_to_world(x, y);
              tiles.push((wx, wy, tile_id));
            }
          }
        }
      }
      tiles
    };

    // Pre-build sprites per tile ID
    let ground_sprite = Self::tile_sprite(TILE_GROUND);
    let platform_sprite = Self::tile_sprite(TILE_PLATFORM);

    for (wx, wy, tile_id) in tiles {
      let sprite = match tile_id {
        TILE_GROUND => ground_sprite.clone(),
        TILE_PLATFORM => platform_sprite.clone(),
        _ => Self::tile_sprite(tile_id),
      };
      // Tiles are placed by their top-left corner, like the tile grid
      ctx.world.spawn((
        TileVis,
        Transform::from_xy(wx, wy),
        Renderable::sprite(sprite)
          .with_anchor(Anchor::TopLeft)
          .with_z_order(0),
      ));
    }
  }

  /// Despawn all tile visualisation entities and respawn from current data.
  fn rebuild_visuals(&self, ctx: &mut SceneContext) {
    let to_despawn: Vec<Entity> = ctx
      .world
      .query_filtered::<Entity, With<TileVis>>()
      .iter(ctx.world)
      .collect();
    for e in to_despawn {
      ctx.world.despawn(e);
    }
    self.spawn_tile_visuals(ctx);
  }

  /// Regenerate the collision rects from the map's current tiles.
  fn refresh_collision_rects(&mut self, ctx: &mut SceneContext, entity: Entity) {
    if let Some(tilemap) = ctx.tilemaps().get_tilemap(entity) {
      self.collision_rects = generate_collision_rects(tilemap);
    }
  }
}

impl DemoPage for TilemapPage {
  fn name(&self) -> &str {
    "Tilemap"
  }

  fn description(&self) -> &str {
    "Tilemap data API: tile queries, coordinate conversion and collision rects"
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::keys(
        "WASD",
        &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD],
        "Pan",
      ),
      Control::mouse("Hover for tile info, left click places, right click clears"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    // Position camera at the center of the map
    let map_pixel_w = (MAP_WIDTH * TILE_SIZE) as f32;
    let map_pixel_h = (MAP_HEIGHT * TILE_SIZE) as f32;
    ctx
      .camera
      .set_position(map_pixel_w / 2.0, map_pixel_h / 2.0);

    // Build and register tilemap
    let tilemap = Self::build_tilemap();
    self.collision_rects = generate_collision_rects(&tilemap);

    let entity = ctx.world.spawn_empty().id();
    ctx.tilemaps().register(entity, tilemap);
    self.map_entity = Some(entity);

    // Visualise tiles as sprites
    self.spawn_tile_visuals(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    // Camera panning
    let mut dx = 0.0f32;
    let mut dy = 0.0f32;
    if ctx.input.key_down(KeyCode::KeyA) {
      dx -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyD) {
      dx += 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyW) {
      dy -= 1.0;
    }
    if ctx.input.key_down(KeyCode::KeyS) {
      dy += 1.0;
    }
    if dx != 0.0 || dy != 0.0 {
      let len = (dx * dx + dy * dy).sqrt();
      dx /= len;
      dy /= len;
      let (cx, cy) = (ctx.camera.x, ctx.camera.y);
      ctx
        .camera
        .set_position(cx + dx * CAMERA_SPEED * dt, cy + dy * CAMERA_SPEED * dt);
    }

    // Track mouse in world space
    let (mx, my) = ctx.input.mouse_position();
    let (wx, wy) = ctx.camera.screen_to_world(mx, my);
    self.hover_world = (wx, wy);

    if let Some(entity) = self.map_entity {
      self.hover_tile = ctx.tilemaps().world_to_tile(entity, wx, wy);

      // Place solid ground tile with left click
      if ctx.input.mouse_pressed(MouseButton::Left)
        && let Some((tx, ty)) = self.hover_tile
      {
        ctx
          .tilemaps()
          .set_tile(entity, 0, tx, ty, Some(TILE_GROUND));
        self.rebuild_visuals(ctx);
        self.refresh_collision_rects(ctx, entity);
      }

      // Clear tile with right click
      if ctx.input.mouse_pressed(MouseButton::Right)
        && let Some((tx, ty)) = self.hover_tile
      {
        ctx.tilemaps().set_tile(entity, 0, tx, ty, None);
        self.rebuild_visuals(ctx);
        self.refresh_collision_rects(ctx, entity);
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let left = area.x + 10.0;
    let top = area.y + 4.0;
    let white = Color::rgb(220, 220, 220);
    let accent = Color::rgb(130, 180, 255);
    let muted = Color::rgb(160, 160, 160);
    let green = Color::rgb(100, 220, 100);
    let red_tint = Color::new(255, 80, 80, 100);

    // Map info
    let info = format!(
      "Map: {}x{} tiles ({}x{}px) | Tile: {}px",
      MAP_WIDTH,
      MAP_HEIGHT,
      MAP_WIDTH * TILE_SIZE,
      MAP_HEIGHT * TILE_SIZE,
      TILE_SIZE,
    );
    ui.draw_text(&info, left, top, accent);

    // Hover info
    let hover_text = if let Some((tx, ty)) = self.hover_tile {
      format!(
        "Tile: ({}, {}) | World: ({:.0}, {:.0})",
        tx, ty, self.hover_world.0, self.hover_world.1,
      )
    } else {
      format!(
        "Outside map | World: ({:.0}, {:.0})",
        self.hover_world.0, self.hover_world.1,
      )
    };
    ui.draw_text(&hover_text, left, top + 20.0, green);

    // Collision rect count
    let col_text = format!("Collision rects: {}", self.collision_rects.len());
    ui.draw_text(&col_text, left, top + 40.0, accent);

    // Draw collision rects as UI overlays (screen-space approximation).
    // We convert world rects to screen positions using the camera transform
    // embedded in the hover_world calculation. Since UiContext doesn't have
    // direct camera access, we draw them as outlines relative to the screen.
    // For a proper implementation this would use the render() callback with
    // world-space drawing, but for this data-API demo we show them as
    // rect outlines in the bottom-right info panel instead.
    let panel_x = area.x + area.width - 260.0;
    ui.draw_text("Collision Rects:", panel_x, top, white);
    for (i, rect) in self.collision_rects.iter().enumerate() {
      let (wx, wy, ww, wh) = rect.to_world(TILE_SIZE, TILE_SIZE);
      let line = format!(
        "  [{}] pos({:.0},{:.0}) size({:.0}x{:.0})",
        i, wx, wy, ww, wh,
      );
      let y_pos = top + 20.0 + i as f32 * 16.0;
      if y_pos > area.y + area.height - 20.0 {
        ui.draw_text("  ...", panel_x, y_pos, muted);
        break;
      }
      ui.draw_text(&line, panel_x, y_pos, muted);

      // Also draw small colored indicators for each rect
      let indicator_size = 8.0;
      let ix = panel_x - 12.0;
      let iy = y_pos + 2.0;
      ui.draw_rect(Rect::new(ix, iy, indicator_size, indicator_size), red_tint);
    }
  }
}

fn main() {
  Demo::new("Tilemap")
    .resolution(800, 600)
    .configure(|b| b.target_fps(60).fixed_hz(60))
    .page(TilemapPage::new)
    .run();
}
