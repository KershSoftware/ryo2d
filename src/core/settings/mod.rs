//! The player's settings.
//!
//! [`Settings`] is the live resource: the volumes, window size, display mode,
//! vsync and FPS counter, plus the game's own typed sections. The settings
//! file, `system.json`, keeps only the kept settings that differ from the
//! game's defaults, so a default changed later reaches every setting the
//! player never touched. Files from older engines convert as they load.

#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the engine reads and writes the settings file through it"
  )
)]
pub(crate) mod file;
mod resource;
#[cfg_attr(
  not(test),
  expect(
    dead_code,
    reason = "the engine registers, loads and stores sections through it"
  )
)]
pub(crate) mod sections;

pub use resource::Settings;

use crate::core::audio::VolumeKind;
use crate::core::config::DisplayMode;
use crate::core::config::resolve::DEFAULT_RESOLUTION;

/// The engine's settings, as [`Settings`] holds them and its defaults
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EngineValues {
  /// In [`VolumeKind::ALL`] order
  pub(crate) volumes: [f32; 6],
  /// The window's size in logical pixels
  pub(crate) window_size: (u32, u32),
  pub(crate) display_mode: DisplayMode,
  pub(crate) vsync: bool,
  pub(crate) show_fps: bool,
}

impl Default for EngineValues {
  fn default() -> Self {
    EngineValues {
      volumes: VolumeKind::ALL.map(VolumeKind::default_value),
      window_size: DEFAULT_RESOLUTION,
      display_mode: DisplayMode::Windowed,
      vsync: true,
      show_fps: false,
    }
  }
}
