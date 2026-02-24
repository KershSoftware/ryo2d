use super::map::Tilemap;

/// An axis-aligned rectangle of solid tiles, measured in tile coordinates.
///
/// Produced by [`generate_collision_rects`] after greedy-merging adjacent
/// solid tiles into the fewest rectangles possible.
#[derive(Debug, Clone, PartialEq)]
pub struct CollisionRect {
  /// Tile X coordinate of the rectangle's left edge.
  pub x: u32,
  /// Tile Y coordinate of the rectangle's top edge.
  pub y: u32,
  /// Width of the rectangle in tiles.
  pub w: u32,
  /// Height of the rectangle in tiles.
  pub h: u32,
}

impl CollisionRect {
  /// Convert tile-space rectangle to world (pixel) coordinates.
  ///
  /// Returns `(world_x, world_y, world_width, world_height)`.
  pub fn to_world(&self, tile_width: u32, tile_height: u32) -> (f32, f32, f32, f32) {
    (
      (self.x * tile_width) as f32,
      (self.y * tile_height) as f32,
      (self.w * tile_width) as f32,
      (self.h * tile_height) as f32,
    )
  }
}

/// Generate merged collision rectangles for the entire tilemap.
///
/// Builds a solid bitmap by checking every tile position across all layers
/// against the tileset properties, then greedy-merges adjacent solid cells
/// into the fewest rectangles possible.
pub fn generate_collision_rects(tilemap: &Tilemap) -> Vec<CollisionRect> {
  let w = tilemap.map_width;
  let h = tilemap.map_height;
  let solid = build_solid_bitmap(tilemap, 0, 0, w, h);
  greedy_merge(&solid, w, h)
}

/// Generate merged collision rectangles for a sub-region of the tilemap.
///
/// Coordinates are in tile space. The returned rectangles have their positions
/// offset back to map coordinates (not relative to the region).
///
/// The region is clamped to the map bounds.
pub fn generate_collision_rects_region(
  tilemap: &Tilemap,
  start_x: u32,
  start_y: u32,
  end_x: u32,
  end_y: u32,
) -> Vec<CollisionRect> {
  let clamped_end_x = end_x.min(tilemap.map_width);
  let clamped_end_y = end_y.min(tilemap.map_height);
  if start_x >= clamped_end_x || start_y >= clamped_end_y {
    return Vec::new();
  }
  let region_w = clamped_end_x - start_x;
  let region_h = clamped_end_y - start_y;
  let solid = build_solid_bitmap(tilemap, start_x, start_y, clamped_end_x, clamped_end_y);
  let mut rects = greedy_merge(&solid, region_w, region_h);
  // Offset back to map coordinates.
  for rect in &mut rects {
    rect.x += start_x;
    rect.y += start_y;
  }
  rects
}

/// Build a flat boolean bitmap indicating which tiles are solid.
///
/// The bitmap covers tiles from `(start_x, start_y)` to `(end_x, end_y)`,
/// stored in row-major order with width `end_x - start_x`.
fn build_solid_bitmap(
  tilemap: &Tilemap,
  start_x: u32,
  start_y: u32,
  end_x: u32,
  end_y: u32,
) -> Vec<bool> {
  let w = (end_x - start_x) as usize;
  let h = (end_y - start_y) as usize;
  let mut solid = vec![false; w * h];

  for layer in &tilemap.layers {
    if let Some(tileset) = tilemap.tilesets.get(layer.tileset_index) {
      for ty in start_y..end_y {
        for tx in start_x..end_x {
          if let Some(tile_id) = layer.get_tile(tx, ty) {
            let props = tileset.get_properties(tile_id);
            if props.solid {
              let local_x = (tx - start_x) as usize;
              let local_y = (ty - start_y) as usize;
              solid[local_y * w + local_x] = true;
            }
          }
        }
      }
    }
  }

  solid
}

/// Greedy-merge a boolean grid into the fewest axis-aligned rectangles.
///
/// Iterates row by row, expanding each unvisited solid cell rightward, then
/// downward, producing maximal rectangles.
fn greedy_merge(solid: &[bool], w: u32, h: u32) -> Vec<CollisionRect> {
  let w_usize = w as usize;
  let h_usize = h as usize;
  let mut visited = vec![false; w_usize * h_usize];
  let mut rects = Vec::new();

  for y in 0..h_usize {
    for x in 0..w_usize {
      let idx = y * w_usize + x;
      if !solid[idx] || visited[idx] {
        continue;
      }

      // Expand right.
      let mut rect_w = 1;
      while x + rect_w < w_usize {
        let next = y * w_usize + x + rect_w;
        if solid[next] && !visited[next] {
          rect_w += 1;
        } else {
          break;
        }
      }

      // Expand down.
      let mut rect_h = 1;
      'down: while y + rect_h < h_usize {
        for dx in 0..rect_w {
          let check = (y + rect_h) * w_usize + x + dx;
          if !solid[check] || visited[check] {
            break 'down;
          }
        }
        rect_h += 1;
      }

      // Mark as visited.
      for dy in 0..rect_h {
        for dx in 0..rect_w {
          visited[(y + dy) * w_usize + x + dx] = true;
        }
      }

      rects.push(CollisionRect {
        x: x as u32,
        y: y as u32,
        w: rect_w as u32,
        h: rect_h as u32,
      });
    }
  }

  rects
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::tilemap::layer::TilemapLayer;
  use crate::core::tilemap::tileset::{TileProperties, Tileset};
  use std::collections::HashMap;

  // -- greedy_merge unit tests --

  #[test]
  fn greedy_merge_empty() {
    let solid = vec![false; 16];
    let rects = greedy_merge(&solid, 4, 4);
    assert!(rects.is_empty());
  }

  #[test]
  fn greedy_merge_single_tile() {
    // 3x3 grid with one solid cell at (1, 1)
    let mut solid = vec![false; 9];
    solid[1 * 3 + 1] = true;
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 1,
        y: 1,
        w: 1,
        h: 1
      }
    );
  }

  #[test]
  fn greedy_merge_horizontal_run() {
    // 4x1 grid, all solid
    let solid = vec![true; 4];
    let rects = greedy_merge(&solid, 4, 1);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 4,
        h: 1
      }
    );
  }

  #[test]
  fn greedy_merge_vertical_run() {
    // 1x4 grid, all solid
    let solid = vec![true; 4];
    let rects = greedy_merge(&solid, 1, 4);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 1,
        h: 4
      }
    );
  }

  #[test]
  fn greedy_merge_full_block() {
    // 3x3 grid, all solid
    let solid = vec![true; 9];
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 3,
        h: 3
      }
    );
  }

  #[test]
  fn greedy_merge_two_separate_tiles() {
    // 3x3 grid with solid at (0,0) and (2,2)
    let mut solid = vec![false; 9];
    solid[0] = true;
    solid[2 * 3 + 2] = true;
    let rects = greedy_merge(&solid, 3, 3);
    assert_eq!(rects.len(), 2);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 1,
        h: 1
      }
    );
    assert_eq!(
      rects[1],
      CollisionRect {
        x: 2,
        y: 2,
        w: 1,
        h: 1
      }
    );
  }

  #[test]
  fn greedy_merge_l_shape() {
    // 3x3 L-shape:
    // X . .
    // X . .
    // X X X
    let mut solid = vec![false; 9];
    solid[0] = true; // (0,0)
    solid[1 * 3 + 0] = true; // (0,1)
    solid[2 * 3 + 0] = true; // (0,2)
    solid[2 * 3 + 1] = true; // (1,2)
    solid[2 * 3 + 2] = true; // (2,2)
    let rects = greedy_merge(&solid, 3, 3);
    // The vertical column (0,0) w=1 h=3, then horizontal (1,2) w=2 h=1
    // OR the greedy algorithm starting top-left:
    // First: (0,0) expands right -> only 1 wide, expands down -> 3 tall => (0,0,1,3)
    // Then: (1,2) expands right -> 2 wide, can't go down => (1,2,2,1)
    assert_eq!(rects.len(), 2);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 1,
        h: 3
      }
    );
    assert_eq!(
      rects[1],
      CollisionRect {
        x: 1,
        y: 2,
        w: 2,
        h: 1
      }
    );
  }

  #[test]
  fn greedy_merge_ground_plane() {
    // 5x3 grid, bottom row solid (ground)
    // . . . . .
    // . . . . .
    // X X X X X
    let mut solid = vec![false; 15];
    for x in 0..5 {
      solid[2 * 5 + x] = true;
    }
    let rects = greedy_merge(&solid, 5, 3);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 2,
        w: 5,
        h: 1
      }
    );
  }

  #[test]
  fn collision_rect_to_world() {
    let rect = CollisionRect {
      x: 2,
      y: 3,
      w: 4,
      h: 2,
    };
    let (wx, wy, ww, wh) = rect.to_world(32, 16);
    assert_eq!(wx, 64.0);
    assert_eq!(wy, 48.0);
    assert_eq!(ww, 128.0);
    assert_eq!(wh, 32.0);
  }

  // -- Integration tests with Tilemap --

  fn solid_tileset() -> Tileset {
    let mut props = HashMap::new();
    props.insert(
      1,
      TileProperties {
        solid: true,
        ..Default::default()
      },
    );
    Tileset {
      name: "test".to_string(),
      image_path: "test.png".to_string(),
      tile_width: 16,
      tile_height: 16,
      columns: 4,
      tile_count: 16,
      spacing: 0,
      margin: 0,
      image_width: 64,
      image_height: 64,
      properties: props,
    }
  }

  #[test]
  fn generate_rects_empty_map() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(solid_tileset());
    map.add_layer(TilemapLayer::new("ground", 4, 4));
    let rects = generate_collision_rects(&map);
    assert!(rects.is_empty());
  }

  #[test]
  fn generate_rects_single_solid() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(solid_tileset());
    let mut layer = TilemapLayer::new("ground", 4, 4);
    layer.set_tile(1, 2, 1); // tile 1 is solid
    map.add_layer(layer);
    let rects = generate_collision_rects(&map);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 1,
        y: 2,
        w: 1,
        h: 1
      }
    );
  }

  #[test]
  fn generate_rects_ground_row() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(solid_tileset());
    let mut layer = TilemapLayer::new("ground", 4, 4);
    for x in 0..4 {
      layer.set_tile(x, 3, 1);
    }
    map.add_layer(layer);
    let rects = generate_collision_rects(&map);
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 3,
        w: 4,
        h: 1
      }
    );
  }

  #[test]
  fn generate_rects_region_offsets_correctly() {
    let mut map = Tilemap::new(16, 16, 8, 8);
    map.add_tileset(solid_tileset());
    let mut layer = TilemapLayer::new("ground", 8, 8);
    // Place solid tiles at (4,4) through (5,5)
    layer.set_tile(4, 4, 1);
    layer.set_tile(5, 4, 1);
    layer.set_tile(4, 5, 1);
    layer.set_tile(5, 5, 1);
    map.add_layer(layer);
    let rects = generate_collision_rects_region(&map, 3, 3, 7, 7);
    assert_eq!(rects.len(), 1);
    // Result should be in map coordinates, not region-relative
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 4,
        y: 4,
        w: 2,
        h: 2
      }
    );
  }

  #[test]
  fn generate_rects_multi_layer() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(solid_tileset());
    let mut layer1 = TilemapLayer::new("ground", 4, 4);
    layer1.set_tile(0, 0, 1); // solid
    map.add_layer(layer1);
    let mut layer2 = TilemapLayer::new("walls", 4, 4);
    layer2.set_tile(1, 0, 1); // solid on different layer
    map.add_layer(layer2);
    let rects = generate_collision_rects(&map);
    // Both tiles are adjacent, should merge into one rect
    assert_eq!(rects.len(), 1);
    assert_eq!(
      rects[0],
      CollisionRect {
        x: 0,
        y: 0,
        w: 2,
        h: 1
      }
    );
  }
}
