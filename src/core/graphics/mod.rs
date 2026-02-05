//! Graphics and rendering systems.
//!
//! This module provides:
//! - [`Sprite`] / [`AnimatedSprite`] - Sprite data and CPU blitting
//! - [`WgpuRenderer`] - GPU rendering pipeline with letterboxing
//! - [`SpriteGpuRenderer`] - GPU sprite batching for particles/effects
//! - [`Camera`] - 2D camera with world/screen transforms
//! - [`Animation`] / [`Animator`] / [`AnimationController`] / [`ValidatedAnimator`] - Sprite animation system
//! - Tiled rendering for repeating sprites and composite tiles
//! - [`ParallaxBackground`] - Layered scrolling backgrounds (CPU)
//! - [`GpuParallaxBackground`] - GPU-accelerated parallax (recommended for large images)

pub mod animation;
pub mod camera;
pub mod parallax;
pub mod parallax_gpu;
pub mod renderer;
pub mod sprite;
pub mod sprite_gpu;
pub mod tile;

pub use animation::{
  Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState, ValidatedAnimator,
};
pub use camera::{Camera, CameraBounds};
#[allow(deprecated)]
pub use parallax::{ParallaxBackground, ParallaxBackgroundBuilder, ParallaxLayer, TileMode};
pub use parallax_gpu::{GpuParallaxBackground, GpuParallaxBuilder, GpuParallaxLayer};
pub use renderer::WgpuRenderer;
pub use sprite::{AnimatedSprite, Sprite};
pub use sprite_gpu::{
  BatchCollector, BatchKey, SpriteBatch, SpriteGlobals, SpriteGpuRenderer, SpriteInstance,
  get_sprite_effect,
};
pub use tile::{CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart};
