//! Unified error types for the Ryo2D engine
//!
//! This module provides a hierarchy of error types:
//! - `EngineError` - Top-level engine errors
//! - `AssetError` - Asset loading and management errors
//! - `AudioError` - Audio system errors
//! - `SpriteError` - Sprite loading errors
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

  /// Save/load error
  #[error(transparent)]
  Save(#[from] SaveError),

  /// Workspace configuration error
  #[error("Workspace error: {0}")]
  WorkspaceError(String),

  /// The GPU could not be set up or was lost
  #[error(transparent)]
  Gpu(#[from] crate::core::graphics::GpuError),
}

/// Result type for engine operations
pub type EngineResult<T> = Result<T, EngineError>;

/// Errors that can occur during asset operations
#[derive(Error, Debug)]
pub enum AssetError {
  /// Asset file not found
  #[error("Asset not found: {0}")]
  NotFound(PathBuf),

  /// Failed to load asset data
  #[error("Failed to load {path}: {reason}")]
  LoadError { path: PathBuf, reason: String },

  /// Failed to set up file watcher for hot reloading
  #[error("Failed to set up file watcher: {0}")]
  WatchError(String),

  /// Sprite loading error (wraps SpriteError)
  #[error(transparent)]
  Sprite(#[from] SpriteError),
}

/// Result type for asset operations
pub type AssetResult<T> = Result<T, AssetError>;

/// Errors that can occur when loading sprites
#[derive(Error, Debug)]
pub enum SpriteError {
  /// Failed to decode image
  #[error("Failed to decode image {path}: {reason}")]
  DecodeError { path: PathBuf, reason: String },
}

/// Result type for sprite operations
pub type SpriteResult<T> = Result<T, SpriteError>;

/// Audio system errors
#[derive(Error, Debug)]
pub enum AudioError {
  /// Failed to initialize the audio backend
  #[error("Audio initialization failed: {0}")]
  InitError(String),

  /// Failed to load a sound file
  #[error("Failed to load audio: {0}")]
  LoadError(String),

  /// Failed to play a sound
  #[error("Failed to play audio: {0}")]
  PlayError(String),

  /// Sound not found in cache
  #[error("Sound not found: {0}")]
  NotFound(String),

  /// No audio output device, so nothing can play
  #[error("no audio device")]
  Unavailable,
}

/// Result type for audio operations
pub type AudioResult<T> = Result<T, AudioError>;

/// Hot reload system errors
#[derive(Error, Debug)]
pub enum HotReloadError {
  /// Failed to create the file watcher
  #[error("Failed to create file watcher: {0}")]
  WatcherCreation(String),
}

/// Result type for hot reload operations
pub type HotReloadResult<T> = Result<T, HotReloadError>;

/// Save/load system errors
#[derive(Error, Debug)]
pub enum SaveError {
  /// Failed to serialize data
  #[error("Serialization failed: {0}")]
  Serialization(String),

  /// Failed to deserialize data
  #[error("Deserialization failed: {0}")]
  Deserialization(String),

  /// Failed to compress data
  #[error("Compression failed: {0}")]
  Compression(String),

  /// Failed to decompress data
  #[error("Decompression failed: {0}")]
  Decompression(String),

  /// I/O error during save/load
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),

  /// Save data version mismatch
  #[error("Version mismatch: save is v{found}, expected v{expected}")]
  VersionMismatch { expected: u32, found: u32 },

  /// Save file not found
  #[error("Save not found: {0}")]
  NotFound(PathBuf),

  /// Invalid save slot
  #[error("Invalid save slot: {0}")]
  InvalidSlot(String),
}

/// Result type for save operations
pub type SaveResult<T> = Result<T, SaveError>;

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

  #[test]
  fn asset_error_display() {
    let err = AssetError::NotFound(PathBuf::from("missing.png"));
    assert_eq!(format!("{}", err), "Asset not found: missing.png");
  }

  #[test]
  fn sprite_error_chains_to_asset_error() {
    let sprite_err = SpriteError::DecodeError {
      path: PathBuf::from("test.png"),
      reason: "truncated".to_string(),
    };
    let asset_err: AssetError = sprite_err.into();
    assert!(matches!(asset_err, AssetError::Sprite(_)));
  }
}
