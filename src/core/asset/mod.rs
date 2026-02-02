//! Asset management for loading and caching game resources.
//!
//! This module provides:
//! - [`AssetManager`] - Central hub for loading sprites, audio, and other assets
//! - [`SpriteHandle`] - Lightweight reference to loaded sprite data
//! - [`HotReloadWatcher`] - Background filesystem watcher for dev-time iteration

pub mod hot_reload;
pub mod manager;

pub use hot_reload::{HotReloadConfig, HotReloadEvent, HotReloadWatcher};
pub use manager::{
    AssetManager, AssetStats, AsyncSpriteHandle, LoadingProgress, LoadingState, SpriteHandle,
};
