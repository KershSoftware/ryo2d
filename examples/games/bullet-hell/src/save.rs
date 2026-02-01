//! Best time persistence

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct BestTimeData {
  time: f32,
}

pub fn load_best_time() -> f32 {
  let Ok(workspace) = Workspace::discover() else {
    return 0.0;
  };

  let Ok(save_manager) = workspace.create_save_manager() else {
    return 0.0;
  };

  save_manager
    .load_game::<BestTimeData>("best_time")
    .ok()
    .map(|data| data.data.time)
    .unwrap_or(0.0)
}

pub fn save_best_time(time: f32) {
  let Ok(workspace) = Workspace::discover() else {
    return;
  };

  let Ok(save_manager) = workspace.create_save_manager() else {
    return;
  };

  let data = BestTimeData { time };
  let _ = save_manager.save_game("best_time", &data);
}
