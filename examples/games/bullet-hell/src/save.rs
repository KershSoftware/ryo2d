//! Best time persistence and settings management

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

// Re-export for convenience in game scene
pub use ryo2d::prelude::DisplayMode;

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

/// Save display mode to system.json
pub fn save_display_mode(display_mode: DisplayMode) {
  let Ok(workspace) = Workspace::discover() else {
    return;
  };

  let Ok(save_manager) = workspace.create_save_manager() else {
    return;
  };

  // Load existing system state or create from defaults
  let mut system_state = save_manager
    .load_or_create_system(&workspace.config().system)
    .unwrap_or_else(|_| SystemState::from_defaults(&workspace.config().system));

  // Update display mode only (resolution is fixed by game design)
  system_state.display_mode = display_mode;

  // Save back to system.json
  let _ = save_manager.save_system(&system_state);
}
