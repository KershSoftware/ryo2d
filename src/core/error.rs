//! Unified error types for the Ryo2D engine
//!
//! This module provides a hierarchy of error types:
//! - `EngineError` - Top-level engine errors
//! - `ImageError` - Image decoding errors
//!
//! Assets that do not load, audio among them, give the registry's
//! `asset::AssetError`.
//!
//! All error types implement `std::error::Error` and use `thiserror` for
//! ergonomic error handling.

use std::path::PathBuf;
use thiserror::Error;

/// Top-level engine error type
///
/// This encompasses all errors that can occur within the engine.
/// Use this type for functions that may fail in multiple subsystems.
#[derive(Error, Debug)]
pub enum EngineError {
  /// Window creation or management failed
  #[error("Window error: {0}")]
  Window(String),

  /// Rendering error
  #[error("Render error: {0}")]
  Render(String),

  /// Workspace configuration error
  #[error("Workspace error: {0}")]
  WorkspaceError(String),

  /// The GPU could not be set up or was lost
  #[error(transparent)]
  Gpu(#[from] crate::core::graphics::GpuError),
}

/// Result type for engine operations
pub type EngineResult<T> = Result<T, EngineError>;

/// Errors that can occur when decoding images
#[derive(Error, Debug)]
pub enum ImageError {
  /// The bytes are not an image the `image` crate can decode (`Image::decode`)
  #[error(transparent)]
  Decode(#[from] image::ImageError),
}

/// Errors that can occur while capturing a frame
#[derive(Error, Debug)]
pub enum CaptureError {
  /// The texture's format can't be converted to RGBA8
  #[error("Texture format {0:?} is not supported for capture")]
  UnsupportedFormat(wgpu::TextureFormat),

  /// Copying or mapping the texture on the GPU failed
  #[error("GPU readback failed: {0}")]
  Readback(String),

  /// The output directory could not be created
  #[error("Failed to create capture directory {path}: {source}")]
  CreateDir {
    path: PathBuf,
    source: std::io::Error,
  },

  /// Encoding or writing the PNG failed
  #[error("Failed to write {path}: {source}")]
  Write {
    path: PathBuf,
    source: image::ImageError,
  },
}

/// Result type for capture operations
pub type CaptureResult<T> = Result<T, CaptureError>;

/// Errors from running the engine headless (`ryo2d::testing`)
#[derive(Error, Debug)]
pub enum HeadlessError {
  /// No GPU adapter is available on this machine
  #[error("no GPU adapter available")]
  NoGpu,

  /// Rendering a frame failed
  #[error("render failed: {0}")]
  Render(String),

  /// Reading a frame back failed
  #[error(transparent)]
  Capture(#[from] CaptureError),
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn capture_error_display() {
    let err = CaptureError::UnsupportedFormat(wgpu::TextureFormat::R8Unorm);
    assert_eq!(
      format!("{}", err),
      "Texture format R8Unorm is not supported for capture"
    );
  }
}
