use super::layer::TilemapLayer;
use super::tileset::Tileset;
use super::TileId;

/// Orientation of the tilemap grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TilemapOrientation {
    /// Standard rectangular grid (default).
    #[default]
    Orthogonal,
    /// Diamond-shaped isometric grid.
    Isometric,
    /// Hexagonal grid with flat-topped hexagons.
    HexagonalFlat,
    /// Hexagonal grid with pointy-topped hexagons.
    HexagonalPointy,
}

/// A complete tilemap composed of tilesets and layers.
///
/// The tilemap defines the grid dimensions (tile size and map size in tiles)
/// and owns all tilesets and layers. It provides coordinate conversion between
/// world (pixel) space and tile (grid) space.
#[derive(Debug, Clone)]
pub struct Tilemap {
    /// Tilesets used by this map's layers.
    pub tilesets: Vec<Tileset>,
    /// Ordered list of layers (bottom to top).
    pub layers: Vec<TilemapLayer>,
    /// Width of a single tile in pixels.
    pub tile_width: u32,
    /// Height of a single tile in pixels.
    pub tile_height: u32,
    /// Width of the map in tiles.
    pub map_width: u32,
    /// Height of the map in tiles.
    pub map_height: u32,
    /// Grid orientation.
    pub orientation: TilemapOrientation,
}

impl Tilemap {
    /// Create a new empty tilemap with the given tile and map dimensions.
    ///
    /// Orientation defaults to [`TilemapOrientation::Orthogonal`].
    pub fn new(tile_width: u32, tile_height: u32, map_width: u32, map_height: u32) -> Self {
        Self {
            tilesets: Vec::new(),
            layers: Vec::new(),
            tile_width,
            tile_height,
            map_width,
            map_height,
            orientation: TilemapOrientation::default(),
        }
    }

    /// Total width of the map in pixels.
    pub fn pixel_width(&self) -> f32 {
        (self.map_width * self.tile_width) as f32
    }

    /// Total height of the map in pixels.
    pub fn pixel_height(&self) -> f32 {
        (self.map_height * self.tile_height) as f32
    }

    /// Convert world (pixel) coordinates to tile coordinates.
    ///
    /// Returns `None` if the position is outside the map bounds (including
    /// negative coordinates).
    pub fn world_to_tile(&self, world_x: f32, world_y: f32) -> Option<(u32, u32)> {
        if world_x < 0.0 || world_y < 0.0 {
            return None;
        }
        let tx = (world_x / self.tile_width as f32) as u32;
        let ty = (world_y / self.tile_height as f32) as u32;
        if tx >= self.map_width || ty >= self.map_height {
            return None;
        }
        Some((tx, ty))
    }

    /// Convert tile coordinates to the world-space position of the tile's
    /// top-left corner.
    pub fn tile_to_world(&self, tile_x: u32, tile_y: u32) -> (f32, f32) {
        (
            (tile_x * self.tile_width) as f32,
            (tile_y * self.tile_height) as f32,
        )
    }

    /// Convert tile coordinates to the world-space position of the tile's
    /// center.
    pub fn tile_to_world_center(&self, tile_x: u32, tile_y: u32) -> (f32, f32) {
        let (wx, wy) = self.tile_to_world(tile_x, tile_y);
        (
            wx + self.tile_width as f32 / 2.0,
            wy + self.tile_height as f32 / 2.0,
        )
    }

    /// Get the tile ID at a world position on a specific layer.
    ///
    /// Returns `None` if the layer index is invalid, the position is outside
    /// the map, or the cell is empty.
    pub fn tile_at_world(&self, layer_index: usize, world_x: f32, world_y: f32) -> Option<TileId> {
        let (tx, ty) = self.world_to_tile(world_x, world_y)?;
        let layer = self.layers.get(layer_index)?;
        layer.get_tile(tx, ty)
    }

    /// Check whether any tile at the given world position is solid.
    ///
    /// Iterates over all layers and checks the tileset properties for each
    /// tile found at the position.
    pub fn is_solid_at(&self, world_x: f32, world_y: f32) -> bool {
        let (tx, ty) = match self.world_to_tile(world_x, world_y) {
            Some(coords) => coords,
            None => return false,
        };

        for layer in &self.layers {
            if let Some(tile_id) = layer.get_tile(tx, ty) {
                if let Some(tileset) = self.tilesets.get(layer.tileset_index) {
                    let props = tileset.get_properties(tile_id);
                    if props.solid {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Add a layer to this tilemap. Returns the index of the new layer.
    pub fn add_layer(&mut self, layer: TilemapLayer) -> usize {
        let index = self.layers.len();
        self.layers.push(layer);
        index
    }

    /// Add a tileset to this tilemap. Returns the index of the new tileset.
    pub fn add_tileset(&mut self, tileset: Tileset) -> usize {
        let index = self.tilesets.len();
        self.tilesets.push(tileset);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::core::tilemap::tileset::TileProperties;

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

    #[test]
    fn pixel_dimensions() {
        let map = Tilemap::new(32, 32, 10, 8);
        assert_eq!(map.pixel_width(), 320.0);
        assert_eq!(map.pixel_height(), 256.0);
    }

    #[test]
    fn world_to_tile_basic() {
        let map = Tilemap::new(32, 32, 10, 8);
        assert_eq!(map.world_to_tile(0.0, 0.0), Some((0, 0)));
        assert_eq!(map.world_to_tile(33.0, 65.0), Some((1, 2)));
    }

    #[test]
    fn world_to_tile_out_of_bounds() {
        let map = Tilemap::new(32, 32, 10, 8);
        // Negative
        assert_eq!(map.world_to_tile(-1.0, 0.0), None);
        assert_eq!(map.world_to_tile(0.0, -1.0), None);
        // Beyond map
        assert_eq!(map.world_to_tile(320.0, 0.0), None);
        assert_eq!(map.world_to_tile(0.0, 256.0), None);
    }

    #[test]
    fn tile_to_world_roundtrip() {
        let map = Tilemap::new(32, 32, 10, 8);
        let (wx, wy) = map.tile_to_world(3, 5);
        let (tx, ty) = map.world_to_tile(wx + 1.0, wy + 1.0).unwrap();
        assert_eq!((tx, ty), (3, 5));
    }

    #[test]
    fn tile_to_world_center() {
        let map = Tilemap::new(32, 32, 10, 8);
        let (cx, cy) = map.tile_to_world_center(0, 0);
        assert_eq!(cx, 16.0);
        assert_eq!(cy, 16.0);
    }

    #[test]
    fn tile_at_world_returns_tile() {
        let mut map = Tilemap::new(32, 32, 4, 4);
        map.add_tileset(test_tileset());
        let mut layer = TilemapLayer::new("ground", 4, 4);
        layer.set_tile(1, 2, 7);
        map.add_layer(layer);

        // World position inside tile (1, 2)
        assert_eq!(map.tile_at_world(0, 33.0, 65.0), Some(7));
    }

    #[test]
    fn tile_at_world_empty() {
        let mut map = Tilemap::new(32, 32, 4, 4);
        map.add_tileset(test_tileset());
        map.add_layer(TilemapLayer::new("ground", 4, 4));

        assert_eq!(map.tile_at_world(0, 0.0, 0.0), None);
    }

    #[test]
    fn is_solid_at_with_solid_tile() {
        let mut ts = test_tileset();
        ts.properties.insert(
            5,
            TileProperties {
                solid: true,
                ..Default::default()
            },
        );
        let mut map = Tilemap::new(32, 32, 4, 4);
        map.add_tileset(ts);
        let mut layer = TilemapLayer::new("ground", 4, 4);
        layer.set_tile(0, 0, 5);
        map.add_layer(layer);

        assert!(map.is_solid_at(1.0, 1.0));
    }

    #[test]
    fn is_solid_at_without_solid_tile() {
        let mut map = Tilemap::new(32, 32, 4, 4);
        map.add_tileset(test_tileset());
        let mut layer = TilemapLayer::new("ground", 4, 4);
        layer.set_tile(0, 0, 3); // tile 3 has no properties => not solid
        map.add_layer(layer);

        assert!(!map.is_solid_at(1.0, 1.0));
    }

    #[test]
    fn is_solid_at_empty_position() {
        let map = Tilemap::new(32, 32, 4, 4);
        assert!(!map.is_solid_at(0.0, 0.0));
    }
}
