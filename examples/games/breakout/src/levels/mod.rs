//! Level Loading
//!
//! Utilities for loading level data from text files.

use std::fs;
use std::path::Path;

use crate::components::BlockColor;
use crate::constants::{
  BLOCK_HEIGHT, BLOCK_LEFT_OFFSET, BLOCK_PADDING, BLOCK_TOP_OFFSET, BLOCK_WIDTH,
};

/// Loaded level data containing block positions and properties
pub struct LevelData {
  /// Blocks: (x, y, color, hits, points)
  pub blocks: Vec<(f32, f32, BlockColor, u32, u32)>,
}

impl LevelData {
  /// Get the number of blocks in this level
  pub fn block_count(&self) -> usize {
    self.blocks.len()
  }
}

/// Load a level from a text file
///
/// Level format:
/// - `#` starts a comment line
/// - `.` is an empty space
/// - Letters represent blocks: R=Red, O=Orange, Y=Yellow, G=Green, B=Blue, P=Purple, W=White, S=Silver
///
/// # Example
/// ```text
/// # Level 1
/// WWWWWWWWWWWW
/// OOOOOOOOOOOO
/// GGGGGGGGGGGG
/// ```
pub fn load_level(path: &Path) -> Result<LevelData, String> {
  let content = fs::read_to_string(path).map_err(|e| format!("Failed to read level: {}", e))?;

  let mut blocks = Vec::new();
  let mut current_row = 0usize;

  for line in content.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }

    for (col, c) in line.chars().enumerate() {
      if c == '.' {
        continue;
      }

      if let Some((color, hits, points)) = BlockColor::from_char(c) {
        let x = BLOCK_LEFT_OFFSET + col as f32 * (BLOCK_WIDTH + BLOCK_PADDING);
        let y = BLOCK_TOP_OFFSET + current_row as f32 * (BLOCK_HEIGHT + BLOCK_PADDING);
        blocks.push((x, y, color, hits, points));
      }
    }
    current_row += 1;
  }

  Ok(LevelData { blocks })
}

/// Get the path for a numbered level file
pub fn level_path(level: u32) -> String {
  format!("levels/level{}.txt", level)
}
