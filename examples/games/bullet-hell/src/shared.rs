//! Shared state between scenes

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ryo2d::prelude::*;

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

/// Spawn the desert backdrop: a fixed sky and five layers scrolling at
/// their own speeds, as far along as the shared parallax time (so the menu
/// and the game continue one another).
pub fn spawn_background(ctx: &mut SceneContext) {
  // Fits the 1000 px images to the 1080 px screen height
  let scale = 1080.0 / 1000.0;
  let elapsed = get_parallax_time();
  // (image, z, auto-scroll in pixels per second; 0 is the fixed sky)
  let layers = [
    ("desert_sky", -100, 0.0),
    ("desert_moon", -90, 10.0),
    ("desert_cloud", -80, 30.0),
    ("desert_mountain", -60, 60.0),
    ("desert_dunemid", -40, 100.0),
    ("desert_dunefrontt", -20, 160.0),
  ];
  for (name, z, speed) in layers {
    let Ok(sprite) = Sprite::load(format!("assets/{name}.png")) else {
      ryo2d::game_warn!("assets/{name}.png could not be loaded");
      continue;
    };
    let auto_scroll = Vec2::new(speed, 0.0);
    let mut layer = ParallaxLayer::new(&SpriteHandle::from_sprite(sprite, name))
      .with_scale(scale)
      .with_z_order(z)
      .with_auto_scroll(auto_scroll);
    if speed > 0.0 {
      layer = layer.with_tile(TileMode::InfiniteScroll);
    }
    layer.scrolled = auto_scroll * elapsed;
    ctx.world.spawn(layer);
  }
}
