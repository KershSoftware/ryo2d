use std::collections::HashMap;

/// Unique identifier for a tile within a tileset.
pub type TileId = u32;

/// A tileset defines the image and layout used to render tiles.
///
/// Tiles are arranged in a grid within the tileset image. Each tile can be
/// referenced by its [`TileId`] (0-based index). The tileset handles mapping
/// from tile IDs to UV coordinates for rendering.
#[derive(Debug, Clone)]
pub struct Tileset {
    /// Human-readable name for this tileset.
    pub name: String,
    /// Path to the tileset image asset.
    pub image_path: String,
    /// Width of each tile in pixels.
    pub tile_width: u32,
    /// Height of each tile in pixels.
    pub tile_height: u32,
    /// Number of tile columns in the tileset image.
    pub columns: u32,
    /// Total number of tiles in the tileset.
    pub tile_count: u32,
    /// Spacing between tiles in pixels.
    pub spacing: u32,
    /// Margin around the edge of the tileset image in pixels.
    pub margin: u32,
    /// Width of the tileset image in pixels.
    pub image_width: u32,
    /// Height of the tileset image in pixels.
    pub image_height: u32,
    /// Per-tile properties indexed by tile ID.
    pub properties: HashMap<TileId, TileProperties>,
}

/// Properties associated with a single tile.
///
/// Tiles default to non-solid and non-platform with no animation.
#[derive(Debug, Clone, Default)]
pub struct TileProperties {
    /// Whether this tile blocks movement (full collision).
    pub solid: bool,
    /// Whether this tile acts as a one-way platform (passable from below).
    pub platform: bool,
    /// Optional animation data for animated tiles.
    pub animation: Option<TileAnimation>,
    /// Arbitrary custom properties (e.g. from Tiled editor).
    pub custom: HashMap<String, String>,
}

/// Animation data for an animated tile.
///
/// Each frame is a `(TileId, duration_ms)` pair describing which tile to
/// display and for how long.
#[derive(Debug, Clone)]
pub struct TileAnimation {
    /// Sequence of `(tile_id, duration_ms)` frames.
    pub frames: Vec<(TileId, u32)>,
}

impl Tileset {
    /// Calculate the UV rectangle for a tile by its ID.
    ///
    /// Returns `Some((u_min, v_min, u_max, v_max))` with values normalized
    /// to the `0.0..1.0` range, or `None` if the tile ID is out of range.
    ///
    /// The calculation accounts for [`spacing`](Tileset::spacing) between tiles
    /// and [`margin`](Tileset::margin) around the tileset image edges.
    pub fn tile_uv(&self, tile_id: TileId) -> Option<(f32, f32, f32, f32)> {
        if tile_id >= self.tile_count {
            return None;
        }

        let col = tile_id % self.columns;
        let row = tile_id / self.columns;

        let x = self.margin + col * (self.tile_width + self.spacing);
        let y = self.margin + row * (self.tile_height + self.spacing);

        let u_min = x as f32 / self.image_width as f32;
        let v_min = y as f32 / self.image_height as f32;
        let u_max = (x + self.tile_width) as f32 / self.image_width as f32;
        let v_max = (y + self.tile_height) as f32 / self.image_height as f32;

        Some((u_min, v_min, u_max, v_max))
    }

    /// Get the properties for a tile, returning defaults if none are set.
    pub fn get_properties(&self, tile_id: TileId) -> TileProperties {
        self.properties
            .get(&tile_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a simple tileset with no spacing or margin.
    fn simple_tileset() -> Tileset {
        Tileset {
            name: "test".to_string(),
            image_path: "test.png".to_string(),
            tile_width: 32,
            tile_height: 32,
            columns: 4,
            tile_count: 8,
            spacing: 0,
            margin: 0,
            image_width: 128,
            image_height: 64,
            properties: HashMap::new(),
        }
    }

    /// Helper to create a tileset with spacing and margin.
    fn spaced_tileset() -> Tileset {
        // 2px margin, 1px spacing, 16x16 tiles, 4 columns
        // image_width = 2 + 4*16 + 3*1 + 2 = 69  (margin on both sides? No — margin is edge only on left/top)
        // Actually: tiles start at margin, then tile+spacing repeats.
        // x positions: 2, 2+16+1=19, 19+16+1=36, 36+16+1=53
        // last tile right edge: 53+16=69
        // So image_width needs to be at least 69. Use 69.
        // y positions: 2, 2+16+1=19
        // last tile bottom: 19+16=35. Use 35.
        Tileset {
            name: "spaced".to_string(),
            image_path: "spaced.png".to_string(),
            tile_width: 16,
            tile_height: 16,
            columns: 4,
            tile_count: 8,
            spacing: 1,
            margin: 2,
            image_width: 69,
            image_height: 35,
            properties: HashMap::new(),
        }
    }

    #[test]
    fn tile_uv_first_tile() {
        let ts = simple_tileset();
        let uv = ts.tile_uv(0).unwrap();
        assert_eq!(uv, (0.0, 0.0, 0.25, 0.5));
    }

    #[test]
    fn tile_uv_second_tile() {
        let ts = simple_tileset();
        let uv = ts.tile_uv(1).unwrap();
        assert_eq!(uv, (0.25, 0.0, 0.5, 0.5));
    }

    #[test]
    fn tile_uv_second_row() {
        let ts = simple_tileset();
        // Tile 4 is the first tile on the second row (columns = 4).
        let uv = ts.tile_uv(4).unwrap();
        assert_eq!(uv, (0.0, 0.5, 0.25, 1.0));
    }

    #[test]
    fn tile_uv_out_of_range() {
        let ts = simple_tileset();
        assert!(ts.tile_uv(8).is_none());
        assert!(ts.tile_uv(100).is_none());
    }

    #[test]
    fn tile_uv_with_spacing_and_margin_first() {
        let ts = spaced_tileset();
        let uv = ts.tile_uv(0).unwrap();
        // x = 2, y = 2, width = 16, height = 16
        // image: 69 x 35
        let expected = (2.0 / 69.0, 2.0 / 35.0, 18.0 / 69.0, 18.0 / 35.0);
        assert!((uv.0 - expected.0).abs() < 1e-6);
        assert!((uv.1 - expected.1).abs() < 1e-6);
        assert!((uv.2 - expected.2).abs() < 1e-6);
        assert!((uv.3 - expected.3).abs() < 1e-6);
    }

    #[test]
    fn tile_uv_with_spacing_and_margin_third() {
        let ts = spaced_tileset();
        let uv = ts.tile_uv(2).unwrap();
        // col=2, row=0. x = 2 + 2*(16+1) = 36, y = 2
        let expected = (36.0 / 69.0, 2.0 / 35.0, 52.0 / 69.0, 18.0 / 35.0);
        assert!((uv.0 - expected.0).abs() < 1e-6);
        assert!((uv.1 - expected.1).abs() < 1e-6);
        assert!((uv.2 - expected.2).abs() < 1e-6);
        assert!((uv.3 - expected.3).abs() < 1e-6);
    }

    #[test]
    fn tile_uv_with_spacing_and_margin_second_row() {
        let ts = spaced_tileset();
        let uv = ts.tile_uv(5).unwrap();
        // col=1, row=1. x = 2 + 1*(16+1) = 19, y = 2 + 1*(16+1) = 19
        let expected = (19.0 / 69.0, 19.0 / 35.0, 35.0 / 69.0, 35.0 / 35.0);
        assert!((uv.0 - expected.0).abs() < 1e-6);
        assert!((uv.1 - expected.1).abs() < 1e-6);
        assert!((uv.2 - expected.2).abs() < 1e-6);
        assert!((uv.3 - expected.3).abs() < 1e-6);
    }

    #[test]
    fn get_properties_returns_default_when_missing() {
        let ts = simple_tileset();
        let props = ts.get_properties(0);
        assert!(!props.solid);
        assert!(!props.platform);
        assert!(props.animation.is_none());
        assert!(props.custom.is_empty());
    }

    #[test]
    fn get_properties_returns_stored_value() {
        let mut ts = simple_tileset();
        ts.properties.insert(
            3,
            TileProperties {
                solid: true,
                platform: false,
                animation: Some(TileAnimation {
                    frames: vec![(3, 100), (4, 100)],
                }),
                custom: HashMap::new(),
            },
        );
        let props = ts.get_properties(3);
        assert!(props.solid);
        assert!(props.animation.is_some());
        assert_eq!(props.animation.unwrap().frames.len(), 2);
    }
}
