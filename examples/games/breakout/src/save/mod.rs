//! Save Data
//!
//! Persistent game data like high scores and settings.
//! Uses the workspace configuration from ryo.toml for save paths.

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

/// High score save data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HighScoreData {
  pub high_score: u32,
}

/// Load the high score from the save file
///
/// Returns 0 if no save file exists or if loading fails
pub fn load_high_score() -> u32 {
  // Use workspace configuration for save paths
  let Ok(workspace) = Workspace::discover() else {
    return 0;
  };

  let Ok(save_manager) = workspace.create_save_manager() else {
    return 0;
  };

  if let Ok(data) = save_manager.load_game::<HighScoreData>("highscore") {
    return data.data.high_score;
  }
  0
}

/// Save the high score to disk
pub fn save_high_score(score: u32) {
  // Use workspace configuration for save paths
  let Ok(workspace) = Workspace::discover() else {
    return;
  };

  let Ok(save_manager) = workspace.create_save_manager() else {
    return;
  };

  let data = HighScoreData { high_score: score };
  let _ = save_manager.save_game("highscore", &data);
}
