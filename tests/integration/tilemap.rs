//! Integration tests for the tilemap system.
//!
//! These tests verify the public tilemap API works correctly, including:
//! - Tilemap creation and tile queries
//! - Coordinate conversion (world <-> tile)
//! - Collision rectangle generation and merging
//! - Solid tile property checks
//! - Multi-layer tile access

use ryo2d::prelude::*;
use std::collections::HashMap;

// ============================================================================
// HELPERS
// ============================================================================

/// Build a simple test tileset with 16x16 tiles in a 4-column, 16-tile sheet.
fn make_test_tileset() -> Tileset {
    Tileset {
        name: "test".into(),
        image_path: "test.png".into(),
        tile_width: 16,
        tile_height: 16,
        columns: 4,
        tile_count: 16,
        spacing: 0,
        margin: 0,
        image_width: 64,
        image_height: 64,
        properties: HashMap::new(),
    }
}

/// Build a test tileset where tile 1 is solid and tile 2 is non-solid.
fn make_solid_tileset() -> Tileset {
    let mut props = HashMap::new();
    props.insert(
        1,
        TileProperties {
            solid: true,
            ..Default::default()
        },
    );
    Tileset {
        name: "solid_test".into(),
        image_path: "test.png".into(),
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

// ============================================================================
// TILEMAP CREATE AND QUERY
// ============================================================================

/// Create a Tilemap with a tileset and ground layer, verify pixel dimensions
/// and tile_at_world for filled and empty cells.
#[test]
fn tilemap_create_and_query() {
    let mut map = Tilemap::new(16, 16, 10, 8);
    map.add_tileset(make_test_tileset());

    // Verify pixel dimensions.
    assert_eq!(map.pixel_width(), 160.0, "10 tiles * 16px = 160px wide");
    assert_eq!(map.pixel_height(), 128.0, "8 tiles * 16px = 128px tall");

    // Add a ground layer and place a tile at (2, 3).
    let mut ground = TilemapLayer::new("ground", 10, 8);
    ground.set_tile(2, 3, 5);
    map.add_layer(ground);

    // Query the filled cell — world position inside tile (2,3).
    // Tile (2,3) spans x: 32..48, y: 48..64.
    assert_eq!(
        map.tile_at_world(0, 33.0, 49.0),
        Some(5),
        "Should find tile 5 at world (33, 49)"
    );

    // Query an empty cell.
    assert_eq!(
        map.tile_at_world(0, 0.0, 0.0),
        None,
        "Empty cell should return None"
    );

    // Query outside the map.
    assert_eq!(
        map.tile_at_world(0, 999.0, 999.0),
        None,
        "Out-of-bounds should return None"
    );
}

// ============================================================================
// COORDINATE CONVERSION
// ============================================================================

/// Test world_to_tile and tile_to_world roundtrips, including out-of-bounds.
#[test]
fn tilemap_coordinate_conversion() {
    let map = Tilemap::new(16, 16, 10, 8);

    // Basic conversion: tile (0,0) top-left corner.
    assert_eq!(map.world_to_tile(0.0, 0.0), Some((0, 0)));

    // Mid-tile position should still resolve to the same tile.
    assert_eq!(map.world_to_tile(8.0, 8.0), Some((0, 0)));

    // Just past the tile boundary.
    assert_eq!(map.world_to_tile(16.0, 0.0), Some((1, 0)));

    // Last valid tile (9, 7).
    assert_eq!(
        map.world_to_tile(159.0, 127.0),
        Some((9, 7)),
        "Last valid tile at bottom-right"
    );

    // Out of bounds: exactly at the pixel boundary is out of range.
    assert_eq!(map.world_to_tile(160.0, 0.0), None);
    assert_eq!(map.world_to_tile(0.0, 128.0), None);

    // Negative coordinates are out of bounds.
    assert_eq!(map.world_to_tile(-1.0, 0.0), None);
    assert_eq!(map.world_to_tile(0.0, -1.0), None);

    // Roundtrip: tile_to_world gives top-left, adding a small offset lands in the same tile.
    let (wx, wy) = map.tile_to_world(5, 3);
    assert_eq!(wx, 80.0);
    assert_eq!(wy, 48.0);
    let roundtrip = map.world_to_tile(wx + 1.0, wy + 1.0);
    assert_eq!(roundtrip, Some((5, 3)), "Roundtrip should return same tile");

    // tile_to_world_center should be at the center of the tile.
    let (cx, cy) = map.tile_to_world_center(5, 3);
    assert_eq!(cx, 88.0, "Center x of tile (5,3) = 80 + 8");
    assert_eq!(cy, 56.0, "Center y of tile (5,3) = 48 + 8");
    let from_center = map.world_to_tile(cx, cy);
    assert_eq!(
        from_center,
        Some((5, 3)),
        "Center of tile should resolve to same tile"
    );
}

// ============================================================================
// COLLISION RECTS — GROUND PLANE
// ============================================================================

/// Create a tilemap with solid tiles filling the entire bottom row. The greedy
/// merge should produce exactly 1 merged rectangle spanning the full width.
#[test]
fn collision_rects_ground_plane() {
    let mut map = Tilemap::new(16, 16, 8, 6);
    map.add_tileset(make_solid_tileset());

    let mut ground = TilemapLayer::new("ground", 8, 6);
    // Fill the bottom row (y = 5) with solid tile 1.
    for x in 0..8 {
        ground.set_tile(x, 5, 1);
    }
    map.add_layer(ground);

    let rects = generate_collision_rects(&map);
    assert_eq!(rects.len(), 1, "Bottom row should merge into 1 rectangle");
    assert_eq!(
        rects[0],
        CollisionRect {
            x: 0,
            y: 5,
            w: 8,
            h: 1
        },
        "Rectangle should span full width at bottom row"
    );

    // Verify world-space conversion.
    let (wx, wy, ww, wh) = rects[0].to_world(16, 16);
    assert_eq!(wx, 0.0);
    assert_eq!(wy, 80.0);
    assert_eq!(ww, 128.0);
    assert_eq!(wh, 16.0);
}

// ============================================================================
// COLLISION RECTS — SEPARATE PLATFORMS
// ============================================================================

/// Create a tilemap with two separate horizontal platform segments.
/// They should produce 2 distinct collision rectangles.
#[test]
fn collision_rects_platforms() {
    let mut map = Tilemap::new(16, 16, 10, 6);
    map.add_tileset(make_solid_tileset());

    let mut layer = TilemapLayer::new("platforms", 10, 6);
    // Platform 1: tiles (1,2), (2,2), (3,2)
    for x in 1..4 {
        layer.set_tile(x, 2, 1);
    }
    // Platform 2: tiles (6,2), (7,2)
    for x in 6..8 {
        layer.set_tile(x, 2, 1);
    }
    map.add_layer(layer);

    let rects = generate_collision_rects(&map);
    assert_eq!(
        rects.len(),
        2,
        "Two separate platform segments should produce 2 rects"
    );
    assert_eq!(
        rects[0],
        CollisionRect {
            x: 1,
            y: 2,
            w: 3,
            h: 1
        }
    );
    assert_eq!(
        rects[1],
        CollisionRect {
            x: 6,
            y: 2,
            w: 2,
            h: 1
        }
    );
}

// ============================================================================
// IS_SOLID_AT CHECKS PROPERTIES
// ============================================================================

/// Create a tilemap with solid and non-solid tiles, verify is_solid_at returns
/// the correct result based on tile properties.
#[test]
fn is_solid_at_checks_properties() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(make_solid_tileset());

    let mut layer = TilemapLayer::new("ground", 4, 4);
    // Tile 1 is solid, tile 2 is non-solid (no properties).
    layer.set_tile(0, 0, 1); // solid
    layer.set_tile(1, 0, 2); // non-solid
    // (2, 0) is empty — no tile set.
    map.add_layer(layer);

    // Solid tile at (0,0): world position (1, 1).
    assert!(
        map.is_solid_at(1.0, 1.0),
        "Tile 1 should be solid"
    );

    // Non-solid tile at (1,0): world position (17, 1).
    assert!(
        !map.is_solid_at(17.0, 1.0),
        "Tile 2 should not be solid"
    );

    // Empty cell at (2,0): world position (33, 1).
    assert!(
        !map.is_solid_at(33.0, 1.0),
        "Empty cell should not be solid"
    );

    // Out of bounds should not be solid.
    assert!(
        !map.is_solid_at(-5.0, -5.0),
        "Negative coords should not be solid"
    );
    assert!(
        !map.is_solid_at(999.0, 999.0),
        "Far out-of-bounds should not be solid"
    );
}

// ============================================================================
// LAYER VISIBILITY / MULTI-LAYER ACCESS
// ============================================================================

/// Create a tilemap with multiple layers and verify get_tile (via tile_at_world)
/// works on each layer independently.
#[test]
fn layer_visibility() {
    let mut map = Tilemap::new(16, 16, 4, 4);
    map.add_tileset(make_test_tileset());

    // Layer 0: "background" with tile 3 at (0, 0).
    let mut bg = TilemapLayer::new("background", 4, 4);
    bg.set_tile(0, 0, 3);
    let bg_idx = map.add_layer(bg);

    // Layer 1: "foreground" with tile 7 at (0, 0) and tile 9 at (1, 1).
    let mut fg = TilemapLayer::new("foreground", 4, 4);
    fg.set_tile(0, 0, 7);
    fg.set_tile(1, 1, 9);
    let fg_idx = map.add_layer(fg);

    // Query layer 0 at (0,0) — should return tile 3.
    assert_eq!(
        map.tile_at_world(bg_idx, 1.0, 1.0),
        Some(3),
        "Background layer should have tile 3 at (0,0)"
    );

    // Query layer 1 at (0,0) — should return tile 7.
    assert_eq!(
        map.tile_at_world(fg_idx, 1.0, 1.0),
        Some(7),
        "Foreground layer should have tile 7 at (0,0)"
    );

    // Query layer 1 at (1,1) — should return tile 9.
    assert_eq!(
        map.tile_at_world(fg_idx, 17.0, 17.0),
        Some(9),
        "Foreground layer should have tile 9 at (1,1)"
    );

    // Query layer 0 at (1,1) — should be empty.
    assert_eq!(
        map.tile_at_world(bg_idx, 17.0, 17.0),
        None,
        "Background layer should be empty at (1,1)"
    );

    // Query a non-existent layer index.
    assert_eq!(
        map.tile_at_world(99, 1.0, 1.0),
        None,
        "Invalid layer index should return None"
    );
}
