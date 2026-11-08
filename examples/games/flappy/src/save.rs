//! High score persistence system

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

const SLOT: &str = "highscore";

/// High score save data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HighScoreData {
  pub high_score: u32,
}

/// The saved high score, or 0 when there is none or it does not load
pub fn load_high_score(ctx: &mut SceneContext) -> u32 {
  ctx
    .saves()
    .load::<HighScoreData>(SLOT)
    .map(|loaded| loaded.data.high_score)
    .unwrap_or(0)
}

/// Save the high score; the write finishes in the background
pub fn save_high_score(ctx: &mut SceneContext, score: u32) {
  let _ = ctx.saves().save(SLOT, HighScoreData { high_score: score });
}
