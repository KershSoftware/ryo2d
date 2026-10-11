//! Shared state between scenes

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Shared parallax time (stored as f32 bits in AtomicU32 for thread safety)
static PARALLAX_TIME: AtomicU32 = AtomicU32::new(0);

/// Shared flag for game reset (set by console command, checked by game scene)
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Get the current shared parallax time
pub fn get_parallax_time() -> f32 {
  f32::from_bits(PARALLAX_TIME.load(Ordering::Relaxed))
}

/// Set the shared parallax time
pub fn set_parallax_time(time: f32) {
  PARALLAX_TIME.store(time.to_bits(), Ordering::Relaxed);
}

/// Add delta time to the shared parallax time and return the new value
pub fn add_parallax_time(delta: f32) -> f32 {
  let current = get_parallax_time();
  let new_time = current + delta;
  set_parallax_time(new_time);
  new_time
}

/// Request a game reset (called by console command)
pub fn request_reset() {
  RESET_REQUESTED.store(true, Ordering::Relaxed);
}

/// Check if a reset was requested and clear the flag
pub fn take_reset_request() -> bool {
  RESET_REQUESTED.swap(false, Ordering::Relaxed)
}
