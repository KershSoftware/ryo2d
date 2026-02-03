//! Shared state between scenes

use std::sync::atomic::{AtomicU32, Ordering};

/// Shared parallax time (stored as f32 bits in AtomicU32 for thread safety)
static PARALLAX_TIME: AtomicU32 = AtomicU32::new(0);

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
