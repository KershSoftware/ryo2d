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
  pub moving: bool,
  pub platform_index: u32,
  pub is_checkpoint: bool,
}

/// Procedural platform generator with seeded RNG
pub struct PlatformGenerator {
  seed: u64,
  screen_width: f32,
  generated_chunks: HashSet<i32>,
  last_platform_x: f32, // Track last platform X for horizontal continuity
  recent_platforms: Vec<PlatformData>, // Track recent platforms for cross-chunk overlap checks
  total_platforms_generated: u32,
  next_moving_at: u32,
}

impl PlatformGenerator {
  /// Create a new generator with the given seed and screen width
  pub fn new(seed: u64, screen_width: f32) -> Self {
    // Add spawn platform to recent platforms for overlap checking
    let spawn_x = screen_width / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    let spawn_platform = PlatformData {
      x: spawn_x,
      y: SPAWN_PLATFORM_Y,
      width: SPAWN_PLATFORM_WIDTH,
      moving: false,
      platform_index: 0,
      is_checkpoint: false,
    };
    // Compute first moving platform interval from seed
    let mut init_rng = SeededRng::new(seed.wrapping_add(0x4D4F5645));
    let next_moving_at =
      init_rng.gen_range(MOVING_PLATFORM_INTERVAL_MIN..=MOVING_PLATFORM_INTERVAL_MAX);
    Self {
      seed,
      screen_width,
      generated_chunks: HashSet::new(),
      last_platform_x: spawn_x,
      recent_platforms: vec![spawn_platform],
      total_platforms_generated: 0,
      next_moving_at,
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

  /// Generate a random platform width biased toward smaller sizes
  fn random_width(rng: &mut SeededRng) -> f32 {
    // Use a squared random to bias toward smaller platforms
    // r^2 where r is 0..1 gives more values near 0
    let r = rng.gen_range(0.0..1.0);
    let biased = r * r; // Square it for bias toward small
    MIN_PLATFORM_WIDTH + (MAX_PLATFORM_WIDTH - MIN_PLATFORM_WIDTH) * biased
  }

  /// Check if two platform rectangles overlap (with spacing buffer)
  fn platforms_overlap(x1: f32, y1: f32, w1: f32, x2: f32, y2: f32, w2: f32) -> bool {
    // Add spacing buffer to first platform
    let left1 = x1 - PLATFORM_SPACING;
    let right1 = x1 + w1 + PLATFORM_SPACING;
    let top1 = y1 - PLATFORM_SPACING;
    let bottom1 = y1 + PLATFORM_HEIGHT + PLATFORM_SPACING;

    let left2 = x2;
    let right2 = x2 + w2;
    let top2 = y2;
    let bottom2 = y2 + PLATFORM_HEIGHT;

    // Check 2D rectangle overlap
    let x_overlap = !(right1 <= left2 || right2 <= left1);
    let y_overlap = !(bottom1 <= top2 || bottom2 <= top1);
    x_overlap && y_overlap
  }

  /// Check if a candidate platform overlaps any existing platform
  fn overlaps_any(
    candidate_x: f32,
    candidate_y: f32,
    width: f32,
    existing: &[PlatformData],
  ) -> bool {
    existing
      .iter()
      .any(|p| Self::platforms_overlap(candidate_x, candidate_y, width, p.x, p.y, p.width))
  }

  /// Find a valid X position that doesn't overlap with existing platforms
  /// and is still jumpable from the previous platform
  fn find_valid_x(
    rng: &mut SeededRng,
    width: f32,
    target_y: f32,
    last_x: f32,
    last_width: f32,
    existing: &[PlatformData],
    screen_width: f32,
  ) -> Option<f32> {
    // Try multiple times to find a non-overlapping position
    for _ in 0..30 {
      // Generate candidate X position reachable from last platform
      let offset = rng.gen_range(-MAX_HORIZONTAL_OFFSET..MAX_HORIZONTAL_OFFSET);
      let candidate_x = (last_x + offset).clamp(0.0, screen_width - width);

      // Check edge-to-edge gap is jumpable
      let last_right = last_x + last_width;
      let candidate_right = candidate_x + width;
      let gap = if candidate_x > last_right {
        candidate_x - last_right // New platform is to the right
      } else if last_x > candidate_right {
        last_x - candidate_right // New platform is to the left
      } else {
        0.0 // Platforms overlap horizontally (fine for vertical jump)
      };

      if gap > MAX_JUMPABLE_GAP {
        continue; // Gap too large, try again
      }

      // Check no 2D overlap with any existing platform
      if !Self::overlaps_any(candidate_x, target_y, width, existing) {
        return Some(candidate_x);
      }
    }

    // Fallback: try positions along the screen width to find any valid spot
    let step = 40.0;
    let mut test_x = 0.0;
    while test_x <= screen_width - width {
      if !Self::overlaps_any(test_x, target_y, width, existing) {
        // Check it's jumpable from last platform
        let last_right = last_x + last_width;
        let test_right = test_x + width;
        let gap = if test_x > last_right {
          test_x - last_right
        } else if last_x > test_right {
          last_x - test_right
        } else {
          0.0
        };
        if gap <= MAX_JUMPABLE_GAP {
          return Some(test_x);
        }
      }
      test_x += step;
    }

    // No valid position found - skip this platform
    None
  }

  /// Convert a checkpoint height threshold (in height units) to a world Y for the platform top.
  fn checkpoint_threshold_to_y(threshold: f32) -> f32 {
    // Height display: height = (PLAYER_SPAWN_Y - player_y) / HEIGHT_SCALE
    // Player on platform: player_y = platform_y - PLAYER_HEIGHT
    // Substituting: height = (PLAYER_SPAWN_Y - platform_y + PLAYER_HEIGHT) / HEIGHT_SCALE
    //                      = (SPAWN_PLATFORM_Y - platform_y) / HEIGHT_SCALE
    // So: platform_y = SPAWN_PLATFORM_Y - threshold * HEIGHT_SCALE
    SPAWN_PLATFORM_Y - threshold * HEIGHT_SCALE
  }

  /// Collect all checkpoint platform Y positions that fall within [top_y, bottom_y].
  /// Note: top_y < bottom_y since Y decreases going up in world space.
  fn checkpoint_ys_in_range(top_y: f32, bottom_y: f32) -> Vec<f32> {
    let mut result = Vec::new();
    let mut threshold = FIRST_CHECKPOINT_HEIGHT;
    let mut interval = CHECKPOINT_INTERVAL;
    loop {
      let cp_y = Self::checkpoint_threshold_to_y(threshold);
      if cp_y < top_y {
        break; // Past the top of the range (higher in world)
      }
      if cp_y <= bottom_y && cp_y >= top_y {
        result.push(cp_y);
      }
      threshold += interval;
      interval += CHECKPOINT_INTERVAL_GROWTH;
    }
    result
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
    let num_platforms = rng.gen_range(MIN_PLATFORMS_PER_CHUNK..=MAX_PLATFORMS_PER_CHUNK) as usize;

    let mut platforms = Vec::with_capacity(num_platforms);
    let mut last_y = chunk_bottom;
    let mut last_x = self.last_platform_x;
    let mut last_width = SPAWN_PLATFORM_WIDTH; // Start with spawn platform width

    // Get recent platforms for cross-chunk overlap checking
    // Only check platforms within a reasonable Y range
    let check_y_min = chunk_top - CHUNK_HEIGHT;
    let check_y_max = chunk_bottom + CHUNK_HEIGHT;
    let nearby_platforms: Vec<PlatformData> = self
      .recent_platforms
      .iter()
      .filter(|p| p.y >= check_y_min && p.y <= check_y_max)
      .copied()
      .collect();

    for _ in 0..num_platforms {
      // Random vertical gap from last platform
      let gap = rng.gen_range(MIN_VERTICAL_GAP..MAX_VERTICAL_GAP);
      let y = last_y - gap;

      // Stop if we've gone past chunk top
      if y < chunk_top {
        break;
      }

      // Random width (biased toward smaller)
      let width = Self::random_width(&mut rng);

      // Combine nearby platforms and current chunk platforms for overlap check
      let mut all_to_check = nearby_platforms.clone();
      all_to_check.extend(&platforms);

      // Find X position that doesn't overlap and is jumpable
      let x = match Self::find_valid_x(
        &mut rng,
        width,
        y,
        last_x,
        last_width,
        &all_to_check,
        self.screen_width,
      ) {
        Some(x) => x,
        None => continue, // Skip this platform if we can't place it
      };

      self.total_platforms_generated += 1;
      let moving = self.total_platforms_generated == self.next_moving_at;
      if moving {
        self.next_moving_at = self.total_platforms_generated
          + rng.gen_range(MOVING_PLATFORM_INTERVAL_MIN..=MOVING_PLATFORM_INTERVAL_MAX);
      }
      platforms.push(PlatformData {
        x,
        y,
        width,
        moving,
        platform_index: self.total_platforms_generated,
        is_checkpoint: false,
      });
      last_y = y;
      last_x = x;
      last_width = width;
    }

    // Insert guaranteed checkpoint platforms at exact threshold positions.
    // These are wide, centered, non-moving platforms that serve as respawn points.
    let checkpoint_x = self.screen_width / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    for cp_y in Self::checkpoint_ys_in_range(chunk_top, chunk_bottom) {
      self.total_platforms_generated += 1;
      platforms.push(PlatformData {
        x: checkpoint_x,
        y: cp_y,
        width: SPAWN_PLATFORM_WIDTH,
        moving: false,
        platform_index: self.total_platforms_generated,
        is_checkpoint: true,
      });
    }

    // Remember last X for next chunk
    if let Some(last) = platforms.last() {
      self.last_platform_x = last.x;
    }

    // Add new platforms to recent_platforms for future cross-chunk checks
    self.recent_platforms.extend(&platforms);

    // Prune old platforms that are far below current generation
    // Keep platforms within 2 chunk heights of the highest generated platform
    if let Some(highest) = platforms
      .iter()
      .map(|p| p.y)
      .min_by(|a, b| a.partial_cmp(b).unwrap())
    {
      let cutoff_y = highest + CHUNK_HEIGHT * 3.0;
      self.recent_platforms.retain(|p| p.y < cutoff_y);
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
    let spawn_x = self.screen_width / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    self.last_platform_x = spawn_x;
    self.recent_platforms = vec![PlatformData {
      x: spawn_x,
      y: SPAWN_PLATFORM_Y,
      width: SPAWN_PLATFORM_WIDTH,
      moving: false,
      platform_index: 0,
      is_checkpoint: false,
    }];
    self.total_platforms_generated = 0;
    let mut init_rng = SeededRng::new(self.seed.wrapping_add(0x4D4F5645));
    self.next_moving_at =
      init_rng.gen_range(MOVING_PLATFORM_INTERVAL_MIN..=MOVING_PLATFORM_INTERVAL_MAX);
  }

  /// Reset with a new seed
  pub fn reset_with_seed(&mut self, seed: u64) {
    self.seed = seed;
    self.generated_chunks.clear();
    let spawn_x = self.screen_width / 2.0 - SPAWN_PLATFORM_WIDTH / 2.0;
    self.last_platform_x = spawn_x;
    self.recent_platforms = vec![PlatformData {
      x: spawn_x,
      y: SPAWN_PLATFORM_Y,
      width: SPAWN_PLATFORM_WIDTH,
      moving: false,
      platform_index: 0,
      is_checkpoint: false,
    }];
    self.total_platforms_generated = 0;
    let mut init_rng = SeededRng::new(seed.wrapping_add(0x4D4F5645));
    self.next_moving_at =
      init_rng.gen_range(MOVING_PLATFORM_INTERVAL_MIN..=MOVING_PLATFORM_INTERVAL_MAX);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_deterministic_generation() {
    let mut gen1 = PlatformGenerator::new(12345, 1280.0);
    let mut gen2 = PlatformGenerator::new(12345, 1280.0);

    let chunk1 = gen1.generate_chunk(1).unwrap();
    let chunk2 = gen2.generate_chunk(1).unwrap();

    assert_eq!(chunk1.len(), chunk2.len());
    for (p1, p2) in chunk1.iter().zip(chunk2.iter()) {
      assert_eq!(p1.x, p2.x);
      assert_eq!(p1.y, p2.y);
      assert_eq!(p1.width, p2.width);
      assert_eq!(p1.moving, p2.moving);
      assert_eq!(p1.platform_index, p2.platform_index);
      assert_eq!(p1.is_checkpoint, p2.is_checkpoint);
    }
  }

  #[test]
  fn test_no_duplicate_chunks() {
    let mut generator = PlatformGenerator::new(12345, 1280.0);

    let first = generator.generate_chunk(1);
    let second = generator.generate_chunk(1);

    assert!(first.is_some());
    assert!(second.is_none());
  }

  #[test]
  fn test_chunk_0_has_platforms() {
    let mut generator = PlatformGenerator::new(12345, 1280.0);
    let chunk0 = generator.generate_chunk(0).unwrap();
    // Chunk 0 should have platforms above the spawn area
    assert!(!chunk0.is_empty());
  }
}
