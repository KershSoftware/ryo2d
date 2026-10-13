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
pub mod camera2d;
pub(crate) mod camera_gpu;
pub(crate) mod camera_images;
pub mod camera_mut;
mod compositor;
mod gpu_context;
pub mod gpu_error;
pub mod parallax;
pub mod parallax_gpu;
pub mod render_layers;
pub mod render_targets;
pub mod renderer;
pub mod screen;
pub mod sprite;
pub mod sprite_gpu;
mod texture_registry;
pub mod tile;

pub use animation::{
  Animation, AnimationController, AnimationId, Animator, LoopMode, PlaybackState, ValidatedAnimator,
};
pub(crate) use atlas::{AtlasSlot, SlotPage, TextureAtlas};
pub use camera::CameraShake;
pub use camera_mut::CameraMut;
pub use camera2d::{
  Camera2d, CameraBounds, CameraClear, CameraTarget, CameraView, MainCamera, Projection,
  RenderTarget, Viewport,
};
pub(crate) use compositor::{CompositeViewport, Compositor};
pub use gpu_context::GpuRenderContext;
pub use gpu_error::GpuError;
pub use parallax::TileMode;
pub use parallax_gpu::{GpuParallaxBackground, GpuParallaxBuilder, GpuParallaxLayer};
pub use render_layers::RenderLayers;
pub use render_targets::{MAX_TARGET_SIDE, RenderTargets};
pub use renderer::WgpuRenderer;
pub use screen::{Presentation, ScreenMapping};
pub use sprite::{AnimatedSprite, Sprite};
pub use sprite_gpu::{BatchCollector, SpriteGpuRenderer, SpriteInstance, get_sprite_effect};
pub(crate) use texture_registry::{TextureId, TextureRegistry, TextureSource};
pub use tile::{CompositeTile, CompositeTileBuilder, GroupedCollider, TileDirection, TilePart};

/// The GPU device and queue, shared by the renderers and the texture manager.
#[derive(crate::core::ecs::Resource, Clone)]
pub struct Gpu {
  pub device: std::sync::Arc<wgpu::Device>,
  pub queue: std::sync::Arc<wgpu::Queue>,
}
