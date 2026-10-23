//! Asset management for loading and caching game resources.
//!
//! This module provides:
//! - [`AssetManager`] - Central hub for loading sprites, audio, and other assets
//! - [`SpriteHandle`] - Lightweight reference to loaded sprite data
//! - [`HotReloadWatcher`] - Background filesystem watcher for dev-time iteration

pub mod assets;
pub mod handle;
pub mod hot_reload;
pub mod id;
pub mod manager;

pub use assets::Assets;
pub use handle::Handle;
pub use id::{AssetId, UntypedAssetId};

/// A type the registry stores: loaded from files by an `AssetLoader`, or
/// made in code and added with `Assets::add`
pub trait Asset: Send + Sync + 'static {}

pub use hot_reload::{HotReloadConfig, HotReloadEvent, HotReloadWatcher};
pub use manager::{
  AssetManager, AssetStats, AsyncSpriteHandle, LoadingProgress, LoadingState, SpriteHandle,
};
