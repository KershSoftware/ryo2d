//! Graphics and rendering systems.
//!
//! This module provides:
//! - [`Sprite`] / [`AnimatedSprite`] - Sprite data and CPU blitting
//! - [`WgpuRenderer`] - GPU rendering pipeline with letterboxing
//! - [`SpriteGpuRenderer`] - GPU sprite batching for particles/effects
//! - [`Camera`] - 2D camera with world/screen transforms
//! - [`Animation`] / [`Animator`] / [`AnimationController`] / [`ValidatedAnimator`] - Sprite animation system
//! - Tiled rendering for repeating sprites and composite tiles
//! - [`GpuParallaxBackground`] - GPU-accelerated parallax for layered scrolling backgrounds

pub mod animation;
mod atlas;
pub mod camera;
mod compositor;
mod gpu_context;
pub mod parallax;
pub mod parallax_gpu;
pub mod renderer;
pub mod sprite;
pub mod sprite_gpu;
pub mod tile;

pub use animation::{
  Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState, ValidatedAnimator,
};
pub(crate) use atlas::{AtlasKey, AtlasSlot, SlotPage, TextureAtlas};
pub use camera::{Camera, CameraBounds};
pub(crate) use compositor::{Compositor, Viewport};
pub use gpu_context::GpuRenderContext;
pub use parallax::TileMode;
pub use parallax_gpu::{GpuParallaxBackground, GpuParallaxBuilder, GpuParallaxLayer};
pub use renderer::WgpuRenderer;
pub use sprite::{AnimatedSprite, Sprite};
pub(crate) use sprite_gpu::effect_padding;
pub use sprite_gpu::{
  BatchCollector, SpriteGlobals, SpriteGpuRenderer, SpriteInstance, get_sprite_effect,
};
pub use tile::{CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart};
