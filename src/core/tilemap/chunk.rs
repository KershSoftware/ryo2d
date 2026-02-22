/// Number of tiles along each axis of a chunk.
pub const CHUNK_SIZE: u32 = 16;

/// Identifies a specific chunk within a tilemap layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    /// Chunk column index.
    pub chunk_x: u32,
    /// Chunk row index.
    pub chunk_y: u32,
    /// Index of the layer this chunk belongs to.
    pub layer_index: usize,
}

/// Tracks the state of a single chunk for dirty-flag based updates.
#[derive(Debug, Clone)]
pub struct ChunkState {
    /// Which chunk this state belongs to.
    pub coord: ChunkCoord,
    /// Whether this chunk needs to be rebuilt (e.g. mesh regeneration).
    pub dirty: bool,
    /// Whether this chunk contains any non-empty tiles.
    pub has_tiles: bool,
}

impl ChunkState {
    /// Create a new chunk state. Starts dirty with no tiles.
    pub fn new(coord: ChunkCoord) -> Self {
        Self {
            coord,
            dirty: true,
            has_tiles: false,
        }
    }
}

/// Calculate how many chunks are needed along one axis.
///
/// Uses ceiling division: a 20-tile axis with `CHUNK_SIZE = 16` needs 2 chunks.
pub fn chunks_along_axis(map_tiles: u32) -> u32 {
    (map_tiles + CHUNK_SIZE - 1) / CHUNK_SIZE
}

/// Get the tile index range `[start, end)` for a chunk along one axis.
///
/// The range is clamped so that `end` never exceeds `map_tiles`.
pub fn chunk_tile_range(chunk_index: u32, map_tiles: u32) -> (u32, u32) {
    let start = chunk_index * CHUNK_SIZE;
    let end = (start + CHUNK_SIZE).min(map_tiles);
    (start, end)
}

/// Test whether a chunk's AABB overlaps a view rectangle.
///
/// Both the chunk and the view are specified in world (pixel) coordinates.
/// The chunk spans from `(chunk_x * CHUNK_SIZE * tile_width)` to
/// `((chunk_x + 1) * CHUNK_SIZE * tile_width)` (and similarly for y).
pub fn chunk_visible(
    chunk_x: u32,
    chunk_y: u32,
    tile_width: u32,
    tile_height: u32,
    view_min_x: f32,
    view_min_y: f32,
    view_max_x: f32,
    view_max_y: f32,
) -> bool {
    let chunk_min_x = (chunk_x * CHUNK_SIZE * tile_width) as f32;
    let chunk_min_y = (chunk_y * CHUNK_SIZE * tile_height) as f32;
    let chunk_max_x = chunk_min_x + (CHUNK_SIZE * tile_width) as f32;
    let chunk_max_y = chunk_min_y + (CHUNK_SIZE * tile_height) as f32;

    // AABB overlap: no overlap if one is entirely to the left/right/above/below
    chunk_max_x > view_min_x
        && chunk_min_x < view_max_x
        && chunk_max_y > view_min_y
        && chunk_min_y < view_max_y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_along_axis_exact() {
        // 16 tiles exactly => 1 chunk
        assert_eq!(chunks_along_axis(16), 1);
        // 32 tiles => 2 chunks
        assert_eq!(chunks_along_axis(32), 2);
    }

    #[test]
    fn chunks_along_axis_remainder() {
        // 17 tiles => 2 chunks (second chunk partially filled)
        assert_eq!(chunks_along_axis(17), 2);
        // 1 tile => 1 chunk
        assert_eq!(chunks_along_axis(1), 1);
    }

    #[test]
    fn chunk_tile_range_basic() {
        // First chunk of a 32-tile axis
        assert_eq!(chunk_tile_range(0, 32), (0, 16));
        // Second chunk
        assert_eq!(chunk_tile_range(1, 32), (16, 32));
    }

    #[test]
    fn chunk_tile_range_last_partial() {
        // 20-tile axis: chunk 1 spans tiles 16..20 (not 16..32)
        assert_eq!(chunk_tile_range(1, 20), (16, 20));
    }

    #[test]
    fn chunk_visible_fully_visible() {
        // Chunk (0,0) with 32px tiles: spans 0..512 x 0..512
        // View: 0..1000 x 0..1000 => fully inside
        assert!(chunk_visible(0, 0, 32, 32, 0.0, 0.0, 1000.0, 1000.0));
    }

    #[test]
    fn chunk_visible_off_screen_right() {
        // Chunk (0,0): 0..512 x 0..512
        // View starts at 600 => chunk is entirely to the left
        assert!(!chunk_visible(0, 0, 32, 32, 600.0, 0.0, 1000.0, 1000.0));
    }

    #[test]
    fn chunk_visible_partially_visible() {
        // Chunk (0,0): 0..512 x 0..512
        // View: 256..768 overlaps the right half of the chunk
        assert!(chunk_visible(0, 0, 32, 32, 256.0, 0.0, 768.0, 512.0));
    }

    #[test]
    fn chunk_visible_off_screen_left() {
        // Chunk (2,0) with 32px tiles: starts at 2*16*32 = 1024
        // View ends at 512 => chunk is entirely to the right of view
        assert!(!chunk_visible(2, 0, 32, 32, 0.0, 0.0, 512.0, 512.0));
    }
}
