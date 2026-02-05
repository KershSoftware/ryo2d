//! Procedural platform generation

use std::collections::HashSet;

use ryo2d::prelude::SeededRng;

use crate::constants::*;

/// Data for a single platform to be spawned
#[derive(Debug, Clone, Copy)]
pub struct PlatformData {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

/// Procedural platform generator with seeded RNG
pub struct PlatformGenerator {
    seed: u64,
    generated_chunks: HashSet<i32>,
    last_platform_x: f32, // Track last platform X for horizontal continuity
}

impl PlatformGenerator {
    /// Create a new generator with the given seed
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            generated_chunks: HashSet::new(),
            last_platform_x: SCREEN_WIDTH as f32 / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0,
        }
    }

    /// Get the seed used by this generator
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Check if a chunk has already been generated
    pub fn is_chunk_generated(&self, chunk_index: i32) -> bool {
        self.generated_chunks.contains(&chunk_index)
    }

    /// Generate platforms for a specific chunk
    /// Returns None if chunk was already generated
    pub fn generate_chunk(&mut self, chunk_index: i32) -> Option<Vec<PlatformData>> {
        // Don't regenerate chunks
        if self.generated_chunks.contains(&chunk_index) {
            return None;
        }

        self.generated_chunks.insert(chunk_index);

        // Create deterministic RNG for this chunk
        let chunk_seed = self.seed.wrapping_add(chunk_index as u64 * 31337);
        let mut rng = SeededRng::new(chunk_seed);

        // Calculate chunk Y bounds
        // Chunk 0 starts just above spawn platform and goes up
        // Chunk 1, 2, etc continue upward
        let (chunk_bottom, chunk_top) = if chunk_index == 0 {
            // Chunk 0: from spawn platform up to Y=0
            (SPAWN_PLATFORM_Y - MIN_VERTICAL_GAP, 0.0)
        } else {
            // Subsequent chunks continue upward (negative Y is up)
            let top = -((chunk_index - 1) as f32) * CHUNK_HEIGHT;
            let bottom = top + CHUNK_HEIGHT;
            (bottom.min(0.0), top - CHUNK_HEIGHT)
        };

        // Determine number of platforms for this chunk
        let num_platforms =
            rng.gen_range(MIN_PLATFORMS_PER_CHUNK..=MAX_PLATFORMS_PER_CHUNK) as usize;

        let mut platforms = Vec::with_capacity(num_platforms);
        let mut last_y = chunk_bottom;
        let mut last_x = self.last_platform_x;

        for _ in 0..num_platforms {
            // Random vertical gap from last platform
            let gap = rng.gen_range(MIN_VERTICAL_GAP..MAX_VERTICAL_GAP);
            let y = last_y - gap;

            // Stop if we've gone past chunk top
            if y < chunk_top {
                break;
            }

            // Random width
            let width = rng.gen_range(MIN_PLATFORM_WIDTH..MAX_PLATFORM_WIDTH);

            // X position: constrained to be reachable from last platform
            // Allow some horizontal offset but not too much
            let offset = rng.gen_range(-MAX_HORIZONTAL_OFFSET..MAX_HORIZONTAL_OFFSET);
            let x = (last_x + offset).clamp(0.0, SCREEN_WIDTH as f32 - width);

            platforms.push(PlatformData { x, y, width });
            last_y = y;
            last_x = x;
        }

        // Remember last X for next chunk
        if let Some(last) = platforms.last() {
            self.last_platform_x = last.x;
        }

        Some(platforms)
    }

    /// Get the highest chunk index that should be generated
    /// based on the highest player Y position
    pub fn required_chunk_for_y(y: f32) -> i32 {
        // Generate 2 chunks ahead
        // Account for chunk 0 starting at spawn platform
        if y > 0.0 {
            2 // At least generate chunks 0 and 1
        } else {
            let current_chunk = ((-y) / CHUNK_HEIGHT).ceil() as i32 + 1;
            current_chunk + 2
        }
    }

    /// Clear all generated chunk tracking (for reset)
    pub fn reset(&mut self) {
        self.generated_chunks.clear();
        self.last_platform_x = SCREEN_WIDTH as f32 / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    }

    /// Reset with a new seed
    pub fn reset_with_seed(&mut self, seed: u64) {
        self.seed = seed;
        self.generated_chunks.clear();
        self.last_platform_x = SCREEN_WIDTH as f32 / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_generation() {
        let mut gen1 = PlatformGenerator::new(12345);
        let mut gen2 = PlatformGenerator::new(12345);

        let chunk1 = gen1.generate_chunk(1).unwrap();
        let chunk2 = gen2.generate_chunk(1).unwrap();

        assert_eq!(chunk1.len(), chunk2.len());
        for (p1, p2) in chunk1.iter().zip(chunk2.iter()) {
            assert_eq!(p1.x, p2.x);
            assert_eq!(p1.y, p2.y);
            assert_eq!(p1.width, p2.width);
        }
    }

    #[test]
    fn test_no_duplicate_chunks() {
        let mut generator = PlatformGenerator::new(12345);

        let first = generator.generate_chunk(1);
        let second = generator.generate_chunk(1);

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn test_chunk_0_has_platforms() {
        let mut generator = PlatformGenerator::new(12345);
        let chunk0 = generator.generate_chunk(0).unwrap();
        // Chunk 0 should have platforms above the spawn area
        assert!(!chunk0.is_empty());
    }
}
