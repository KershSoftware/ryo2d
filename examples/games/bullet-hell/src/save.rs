//! Best time persistence

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

// Re-export for convenience in game scene
pub use ryo2d::prelude::DisplayMode;

const SLOT: &str = "best_time";

#[derive(Serialize, Deserialize, Default)]
struct BestTimeData {
  time: f32,
}

/// The saved best time, or 0 when there is none or it does not load
pub fn load_best_time(ctx: &mut SceneContext) -> f32 {
  ctx
    .saves()
    .load::<BestTimeData>(SLOT)
    .map(|loaded| loaded.data.time)
    .unwrap_or(0.0)
}

/// Save the best time; the write finishes in the background
pub fn save_best_time(ctx: &mut SceneContext, time: f32) {
  let _ = ctx.saves().save(SLOT, BestTimeData { time });
}
