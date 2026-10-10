//! Frame capture: save the final composed frame (scene + UI) as a PNG.
//!
//! Captures are taken at game resolution (no letterboxing) and triggered by
//! `RYO_CAPTURE_*` environment variables, the F12 dev key, or the
//! `debug:capture` console command.

mod readback;

use std::path::PathBuf;

use image::RgbaImage;

pub use readback::read_texture_rgba;

/// A frame read back from the GPU, ready to be written.
#[derive(Debug)]
pub struct CapturedFrame {
  /// Pixels at game resolution
  pub image: RgbaImage,
  /// Destination requested for this frame
  pub path: PathBuf,
}

/// Where a manual capture should be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
  /// `<capture dir>/frame-NNNNNN.png`
  Default,
  /// An explicit path; relative paths resolve against the working directory
  Path(PathBuf),
}
