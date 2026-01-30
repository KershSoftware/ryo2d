//! Save Data
//!
//! Persistent game data like high scores and settings.

use std::path::Path;

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

/// Path to the save data directory
const SAVE_PATH: &str = "examples/breakout/data";

/// High score save data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HighScoreData {
  pub high_score: u32,
}

/// Load the high score from the save file
///
/// Returns 0 if no save file exists or if loading fails
pub fn load_high_score() -> u32 {
  if let Ok(data) =
    SaveManager::new(Path::new(SAVE_PATH)).and_then(|sm| sm.load_game::<HighScoreData>("highscore"))
  {
    return data.data.high_score;
  }
  0
}

/// Save the high score to disk
pub fn save_high_score(score: u32) {
  if let Ok(save_manager) = SaveManager::new(Path::new(SAVE_PATH)) {
    let data = HighScoreData { high_score: score };
    let _ = save_manager.save_game("highscore", &data);
  }
}
