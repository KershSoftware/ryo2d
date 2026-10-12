//! Run the engine without a window, for tests.
//!
//! [`HeadlessEngine`] drives a real scene through the full update and render
//! pipeline at a fixed timestep and reads frames back as pixels:
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//! use ryo2d::testing::HeadlessEngine;
//!
//! #[test]
//! fn enemy_turns_red_when_hit() {
//!   // None (with a "skipping: no GPU" line) on machines without a GPU
//!   let Some(mut engine) = HeadlessEngine::new_or_skip(
//!     EngineBuilder::new().resolution(320, 180).start_scene(Arena::default()),
//!   ) else { return };
//!
//!   engine.step_frames(10).unwrap();
//!   engine.tap_key(KeyCode::Space);
//!   engine.step_frames(30).unwrap();
//!
//!   let frame = engine.frame().unwrap();
//!   assert_eq!(frame.get_pixel(160, 90).0, [255, 0, 0, 255]);
//! }
//! ```
//!
//! Headless runs have no audio device (`ctx.audio` works, but its `Result`
//! methods return `AudioError::Unavailable`), no gamepads, no hot reload and
//! no dev windows; they never read or write `system.json` user preferences,
//! ignore `RYO_CAPTURE_*` and `RYO_INPUT`, and keep the FPS, frame-timing and
//! stats overlays off. `Ryo.toml` is honoured as in a normal build. Input
//! injected with `press_key`/`tap_key`/`press_mouse`/`mouse_move` is queued
//! like window events and seen from the next step; the engine's own debug
//! keys (F1-F5, F9, F10, F12) are inert headless. Seed any randomness your
//! scene uses if you compare frames exactly: `ParticleSystem::new()` seeds
//! from entropy, so use `ParticleSystem::with_seed` for frames containing
//! particles.

#[cfg(test)]
mod atlas_bleeding;
#[cfg(test)]
mod capture_matches_frame;
mod gpu;
#[cfg(test)]
mod gpu_world;
mod headless;

pub use headless::{HeadlessEngine, StepOutcome};

pub use crate::core::error::HeadlessError;
pub use gpu::shared_gpu;
