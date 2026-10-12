use std::collections::HashMap;

use crate::core::ecs::{Entity, Resource};

use super::TileId;
use super::chunk::*;
use super::map::Tilemap;

/// Internal record for a spawned tilemap.
struct TilemapRecord {
  tilemap: Tilemap,
  chunks: HashMap<ChunkCoord, ChunkState>,
}

/// Drop the records of tilemaps whose entity no longer exists (runs in
/// `Last` and after a scene cleanup despawns the world).
pub(crate) fn forget_despawned_tilemaps(world: &mut crate::core::ecs::World) {
  let Some(manager) = world.get_resource::<TilemapManager>() else {
    return;
  };
  let gone: Vec<Entity> = manager
    .tilemaps
    .keys()
    .copied()
    .filter(|&entity| world.get_entity(entity).is_err())
    .collect();
  if gone.is_empty() {
    return;
  }
  let mut manager = world.resource_mut::<TilemapManager>();
  for entity in gone {
    manager.unregister(entity);
  }
}

/// Manages all active tilemaps.
///
/// Each tilemap is associated with an [`Entity`] and tracked with
/// per-chunk dirty state. The manager provides tile queries, coordinate
/// conversion, and viewport culling without depending on rendering types.
///
/// Full engine integration (rendering, asset loading) is added in a later
/// stage; this stub contains only the data-management surface.
#[derive(Resource)]
pub struct TilemapManager {
  tilemaps: HashMap<Entity, TilemapRecord>,
}

impl TilemapManager {
  /// Create a new empty manager.
  pub fn new() -> Self {
    Self {
      tilemaps: HashMap::new(),
    }
  }

  /// Register a tilemap with a pre-existing entity.
  ///
  /// Builds the chunk map and scans each chunk for non-empty tiles.
  pub fn register(&mut self, entity: Entity, tilemap: Tilemap) {
    let chunks_x = chunks_along_axis(tilemap.map_width);
    let chunks_y = chunks_along_axis(tilemap.map_height);
    let mut chunks = HashMap::new();
    for layer_index in 0..tilemap.layers.len() {
      for cy in 0..chunks_y {
        for cx in 0..chunks_x {
          let coord = ChunkCoord {
            chunk_x: cx,
            chunk_y: cy,
            layer_index,
          };
          let mut state = ChunkState::new(coord);
          state.has_tiles = chunk_has_tiles(&tilemap, coord);
          chunks.insert(coord, state);
        }
      }
    }
    self
      .tilemaps
      .insert(entity, TilemapRecord { tilemap, chunks });
  }

  /// Remove a tilemap.
  pub fn unregister(&mut self, entity: Entity) {
    self.tilemaps.remove(&entity);
  }

  /// Get a tile from a spawned tilemap.
  pub fn get_tile(&self, entity: Entity, layer: usize, x: u32, y: u32) -> Option<TileId> {
    let record = self.tilemaps.get(&entity)?;
    record.tilemap.layers.get(layer)?.get_tile(x, y)
  }

  /// Set a tile. Marks the affected chunk dirty.
  ///
  /// Pass `Some(id)` to place a tile or `None` to clear it.
  pub fn set_tile(&mut self, entity: Entity, layer: usize, x: u32, y: u32, tile: Option<TileId>) {
    let Some(record) = self.tilemaps.get_mut(&entity) else {
      return;
    };
    let Some(layer_data) = record.tilemap.layers.get_mut(layer) else {
      return;
    };
    if !layer_data.in_bounds(x, y) {
      return;
    }
    match tile {
      Some(id) => {
        layer_data.set_tile(x, y, id);
      }
      None => {
        let index = (y as usize) * (layer_data.width as usize) + (x as usize);
        layer_data.tiles[index] = None;
      }
    }
    let coord = ChunkCoord {
      chunk_x: x / CHUNK_SIZE,
      chunk_y: y / CHUNK_SIZE,
      layer_index: layer,
    };
    if let Some(chunk) = record.chunks.get_mut(&coord) {
      chunk.dirty = true;
      if tile.is_some() {
        chunk.has_tiles = true;
      }
    }
  }

  /// Convert world position to tile coordinates.
  pub fn world_to_tile(&self, entity: Entity, world_x: f32, world_y: f32) -> Option<(u32, u32)> {
    self
      .tilemaps
      .get(&entity)?
      .tilemap
      .world_to_tile(world_x, world_y)
  }

  /// Convert tile to world position (top-left corner).
  pub fn tile_to_world(&self, entity: Entity, tile_x: u32, tile_y: u32) -> Option<(f32, f32)> {
    let record = self.tilemaps.get(&entity)?;
    Some(record.tilemap.tile_to_world(tile_x, tile_y))
  }

  /// Check if a world position has a solid tile.
  pub fn is_solid(&self, entity: Entity, world_x: f32, world_y: f32) -> bool {
    self
      .tilemaps
      .get(&entity)
      .map(|r| r.tilemap.is_solid_at(world_x, world_y))
      .unwrap_or(false)
  }

  /// Get the tilemap data for an entity (read-only).
  pub fn get_tilemap(&self, entity: Entity) -> Option<&Tilemap> {
    self.tilemaps.get(&entity).map(|r| &r.tilemap)
  }

  /// Get visible chunk coords for a viewport.
  ///
  /// Returns chunks that have tiles, belong to a visible layer, and overlap
  /// the given view rectangle (in world coordinates).
  pub fn visible_chunks(
    &self,
    entity: Entity,
    view_min_x: f32,
    view_min_y: f32,
    view_max_x: f32,
    view_max_y: f32,
  ) -> Vec<ChunkCoord> {
    let Some(record) = self.tilemaps.get(&entity) else {
      return Vec::new();
    };
    let tm = &record.tilemap;
    record
      .chunks
      .values()
      .filter(|cs| {
        cs.has_tiles
          && tm
            .layers
            .get(cs.coord.layer_index)
            .map(|l| l.visible)
            .unwrap_or(false)
      })
      .filter(|cs| {
        chunk_visible(
          cs.coord.chunk_x,
          cs.coord.chunk_y,
          tm.tile_width,
          tm.tile_height,
          view_min_x,
          view_min_y,
          view_max_x,
          view_max_y,
        )
      })
      .map(|cs| cs.coord)
      .collect()
  }
}

impl Default for TilemapManager {
  fn default() -> Self {
    Self::new()
  }
}

/// Check whether any tile is present in the given chunk.
fn chunk_has_tiles(tilemap: &Tilemap, coord: ChunkCoord) -> bool {
  let Some(layer) = tilemap.layers.get(coord.layer_index) else {
    return false;
  };
  let (start_x, end_x) = chunk_tile_range(coord.chunk_x, tilemap.map_width);
  let (start_y, end_y) = chunk_tile_range(coord.chunk_y, tilemap.map_height);
  for ty in start_y..end_y {
    for tx in start_x..end_x {
      if layer.get_tile(tx, ty).is_some() {
        return true;
      }
    }
  }
  false
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::tilemap::layer::TilemapLayer;
  use crate::core::tilemap::tileset::{TileProperties, Tileset};
  use std::collections::HashMap;

  #[test]
  fn a_despawned_tilemap_entity_is_forgotten() {
    let mut world = crate::core::ecs::World::new();
    world.insert_resource(TilemapManager::new());
    let kept = world.spawn_empty().id();
    let gone = world.spawn_empty().id();
    for entity in [kept, gone] {
      world
        .resource_mut::<TilemapManager>()
        .register(entity, Tilemap::new(32, 32, 4, 4));
    }
    world.despawn(gone);
    forget_despawned_tilemaps(&mut world);
    let manager = world.resource::<TilemapManager>();
    assert!(manager.get_tilemap(kept).is_some());
    assert!(manager.get_tilemap(gone).is_none());
  }

  fn test_tileset() -> Tileset {
    Tileset {
      name: "test".to_string(),
      image_path: "test.png".to_string(),
      tile_width: 32,
      tile_height: 32,
      columns: 4,
      tile_count: 16,
      spacing: 0,
      margin: 0,
      image_width: 128,
      image_height: 128,
      properties: HashMap::new(),
    }
  }

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
      tile_width: 32,
      tile_height: 32,
      columns: 4,
      tile_count: 16,
      spacing: 0,
      margin: 0,
      image_width: 128,
      image_height: 128,
      properties: props,
    }
  }

  fn make_entity(index: u32) -> Entity {
    Entity::from_raw_u32(index).expect("a small index is a valid entity")
  }

  fn small_map_with_tiles() -> Tilemap {
    let mut map = Tilemap::new(32, 32, 4, 4);
    map.add_tileset(test_tileset());
    let mut layer = TilemapLayer::new("ground", 4, 4);
    layer.set_tile(1, 2, 5);
    layer.set_tile(3, 0, 10);
    map.add_layer(layer);
    map
  }

  #[test]
  fn register_and_get_tile() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, small_map_with_tiles());
    assert_eq!(mgr.get_tile(entity, 0, 1, 2), Some(5));
    assert_eq!(mgr.get_tile(entity, 0, 3, 0), Some(10));
    assert_eq!(mgr.get_tile(entity, 0, 0, 0), None);
  }

  #[test]
  fn get_tile_unknown_entity() {
    let mgr = TilemapManager::new();
    let entity = make_entity(99);
    assert_eq!(mgr.get_tile(entity, 0, 0, 0), None);
  }

  #[test]
  fn set_tile_marks_chunk_dirty() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, Tilemap::new(32, 32, 4, 4));
    let record = mgr.tilemaps.get(&entity).unwrap();

    // After register all chunks start dirty. We need a layer first.
    let _ = record;

    // Re-register with a layer.
    let mut map = Tilemap::new(32, 32, 4, 4);
    map.add_tileset(test_tileset());
    map.add_layer(TilemapLayer::new("ground", 4, 4));
    mgr.register(entity, map);

    // Clear dirty flag manually for testing.
    let record = mgr.tilemaps.get_mut(&entity).unwrap();
    for chunk in record.chunks.values_mut() {
      chunk.dirty = false;
    }

    // Set a tile and check chunk is now dirty.
    mgr.set_tile(entity, 0, 2, 3, Some(7));
    assert_eq!(mgr.get_tile(entity, 0, 2, 3), Some(7));

    let record = mgr.tilemaps.get(&entity).unwrap();
    let coord = ChunkCoord {
      chunk_x: 0,
      chunk_y: 0,
      layer_index: 0,
    };
    let chunk = record.chunks.get(&coord).unwrap();
    assert!(chunk.dirty);
    assert!(chunk.has_tiles);
  }

  #[test]
  fn set_tile_clear() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, small_map_with_tiles());
    assert_eq!(mgr.get_tile(entity, 0, 1, 2), Some(5));

    mgr.set_tile(entity, 0, 1, 2, None);
    assert_eq!(mgr.get_tile(entity, 0, 1, 2), None);
  }

  #[test]
  fn world_to_tile_delegation() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, Tilemap::new(32, 32, 10, 8));
    assert_eq!(mgr.world_to_tile(entity, 33.0, 65.0), Some((1, 2)));
    assert_eq!(mgr.world_to_tile(entity, -1.0, 0.0), None);
  }

  #[test]
  fn tile_to_world_delegation() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, Tilemap::new(32, 32, 10, 8));
    assert_eq!(mgr.tile_to_world(entity, 3, 5), Some((96.0, 160.0)));
  }

  #[test]
  fn is_solid_delegation() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);

    let mut map = Tilemap::new(32, 32, 4, 4);
    map.add_tileset(solid_tileset());
    let mut layer = TilemapLayer::new("ground", 4, 4);
    layer.set_tile(0, 0, 1); // tile 1 is solid
    layer.set_tile(1, 0, 0); // tile 0 is not solid
    map.add_layer(layer);

    mgr.register(entity, map);
    assert!(mgr.is_solid(entity, 1.0, 1.0));
    assert!(!mgr.is_solid(entity, 33.0, 1.0));
  }

  #[test]
  fn is_solid_unknown_entity() {
    let mgr = TilemapManager::new();
    assert!(!mgr.is_solid(make_entity(99), 0.0, 0.0));
  }

  #[test]
  fn visible_chunks_returns_correct_chunks() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);

    let mut map = Tilemap::new(32, 32, 20, 20);
    map.add_tileset(test_tileset());
    let mut layer = TilemapLayer::new("ground", 20, 20);
    // Place a tile in chunk (0,0) layer 0
    layer.set_tile(0, 0, 1);
    // Place a tile in chunk (1,1) layer 0 (tile 17,17 => chunk 1,1)
    layer.set_tile(17, 17, 1);
    map.add_layer(layer);

    mgr.register(entity, map);

    // View covers chunk (0,0) only: 0..512 x 0..512
    // Chunk (0,0) with 32px tiles spans 0..512 x 0..512
    let chunks = mgr.visible_chunks(entity, 0.0, 0.0, 100.0, 100.0);
    // Only chunk (0,0) layer 0 should be visible (has tiles and in view)
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].chunk_x, 0);
    assert_eq!(chunks[0].chunk_y, 0);
  }

  #[test]
  fn visible_chunks_excludes_hidden_layers() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);

    let mut map = Tilemap::new(32, 32, 4, 4);
    map.add_tileset(test_tileset());
    let mut layer = TilemapLayer::new("hidden", 4, 4);
    layer.set_tile(0, 0, 1);
    layer.visible = false;
    map.add_layer(layer);

    mgr.register(entity, map);

    let chunks = mgr.visible_chunks(entity, 0.0, 0.0, 1000.0, 1000.0);
    assert!(chunks.is_empty());
  }

  #[test]
  fn unregister_removes_tilemap() {
    let mut mgr = TilemapManager::new();
    let entity = make_entity(1);
    mgr.register(entity, small_map_with_tiles());
    assert!(mgr.get_tilemap(entity).is_some());
    mgr.unregister(entity);
    assert!(mgr.get_tilemap(entity).is_none());
  }
}
