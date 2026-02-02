//! Graphics and rendering systems.
//!
//! This module provides:
//! - [`Sprite`] / [`AnimatedSprite`] - Sprite data and CPU blitting
//! - [`WgpuRenderer`] - GPU rendering pipeline with letterboxing
//! - [`SpriteGpuRenderer`] - GPU sprite batching for particles/effects
//! - [`Camera`] - 2D camera with world/screen transforms
//! - [`Animation`] / [`Animator`] / [`AnimationController`] - Sprite animation system
//! - Tiled rendering for repeating sprites and composite tiles
//! - [`ParallaxBackground`] - Layered scrolling backgrounds

pub mod animation;
pub mod camera;
pub mod parallax;
pub mod renderer;
pub mod sprite;
pub mod sprite_gpu;
pub mod tile;

pub use animation::{
  Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState,
};
pub use camera::{Camera, CameraBounds};
pub use parallax::{ParallaxBackground, ParallaxBackgroundBuilder, ParallaxLayer, TileMode};
pub use renderer::WgpuRenderer;
pub use sprite::{AnimatedSprite, Sprite};
pub use sprite_gpu::{
  BatchCollector, BatchKey, SpriteBatch, SpriteGlobals, SpriteGpuRenderer, SpriteInstance,
  get_sprite_effect,
};
pub use tile::{CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart};
