use super::TileId;

/// A single layer within a tilemap.
///
/// Each layer stores a 2D grid of optional tile IDs. Tiles are stored in
/// row-major order and indexed as `tiles[y * width + x]`.
#[derive(Debug, Clone)]
pub struct TilemapLayer {
  /// Human-readable name for this layer.
  pub name: String,
  /// Tile data in row-major order. `None` means empty (no tile).
  pub tiles: Vec<Option<TileId>>,
  /// Width of this layer in tiles.
  pub width: u32,
  /// Height of this layer in tiles.
  pub height: u32,
  /// Index into the parent tilemap's tileset list.
  pub tileset_index: usize,
  /// Whether this layer is rendered.
  pub visible: bool,
  /// Rendering opacity (0.0 = transparent, 1.0 = opaque).
  pub opacity: f32,
  /// Draw order; higher values render on top.
  pub z_order: i32,
  /// Horizontal parallax scrolling factor.
  pub parallax_x: f32,
  /// Vertical parallax scrolling factor.
  pub parallax_y: f32,
}

impl TilemapLayer {
  /// Create a new empty layer with the given name and dimensions.
  ///
  /// Defaults: `visible = true`, `opacity = 1.0`, `parallax = (1.0, 1.0)`,
  /// `z_order = 0`, `tileset_index = 0`.
  pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
    let count = (width as usize) * (height as usize);
    Self {
      name: name.into(),
      tiles: vec![None; count],
      width,
      height,
      tileset_index: 0,
      visible: true,
      opacity: 1.0,
      z_order: 0,
      parallax_x: 1.0,
      parallax_y: 1.0,
    }
  }

  /// Check whether `(x, y)` is within this layer's bounds.
  pub fn in_bounds(&self, x: u32, y: u32) -> bool {
    x < self.width && y < self.height
  }

  /// Get the tile at `(x, y)`, or `None` if the position is out of bounds
  /// or the cell is empty.
  pub fn get_tile(&self, x: u32, y: u32) -> Option<TileId> {
    if !self.in_bounds(x, y) {
      return None;
    }
    self.tiles[(y as usize) * (self.width as usize) + (x as usize)]
  }

  /// Set the tile at `(x, y)`.
  ///
  /// Returns `None` if the position is out of bounds.
  /// Returns `Some(previous)` on success, where `previous` is the old tile
  /// value (which may itself be `None` if the cell was empty).
  pub fn set_tile(&mut self, x: u32, y: u32, tile: TileId) -> Option<Option<TileId>> {
    if !self.in_bounds(x, y) {
      return None;
    }
    let index = (y as usize) * (self.width as usize) + (x as usize);
    let previous = self.tiles[index];
    self.tiles[index] = Some(tile);
    Some(previous)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_layer_is_empty() {
    let layer = TilemapLayer::new("ground", 10, 8);
    assert_eq!(layer.name, "ground");
    assert_eq!(layer.width, 10);
    assert_eq!(layer.height, 8);
    assert!(layer.visible);
    assert_eq!(layer.opacity, 1.0);
    assert_eq!(layer.parallax_x, 1.0);
    assert_eq!(layer.parallax_y, 1.0);
    assert_eq!(layer.tiles.len(), 80);
    assert!(layer.tiles.iter().all(|t| t.is_none()));
  }

  #[test]
  fn set_and_get_tile() {
    let mut layer = TilemapLayer::new("test", 4, 4);
    layer.set_tile(2, 3, 42);
    assert_eq!(layer.get_tile(2, 3), Some(42));
  }

  #[test]
  fn get_tile_empty() {
    let layer = TilemapLayer::new("test", 4, 4);
    assert_eq!(layer.get_tile(0, 0), None);
  }

  #[test]
  fn get_tile_out_of_bounds() {
    let layer = TilemapLayer::new("test", 4, 4);
    assert_eq!(layer.get_tile(4, 0), None);
    assert_eq!(layer.get_tile(0, 4), None);
    assert_eq!(layer.get_tile(100, 100), None);
  }

  #[test]
  fn set_tile_out_of_bounds() {
    let mut layer = TilemapLayer::new("test", 4, 4);
    assert!(layer.set_tile(4, 0, 1).is_none());
    assert!(layer.set_tile(0, 4, 1).is_none());
  }

  #[test]
  fn set_tile_returns_previous() {
    let mut layer = TilemapLayer::new("test", 4, 4);
    // First set returns None (was empty).
    assert_eq!(layer.set_tile(1, 1, 10), Some(None));
    // Second set returns the previous value.
    assert_eq!(layer.set_tile(1, 1, 20), Some(Some(10)));
  }
}
