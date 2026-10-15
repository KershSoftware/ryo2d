//! Graphics and rendering systems.
//!
//! This module provides:
//! - [`Sprite`] / [`AnimatedSprite`] - Sprite data and CPU blitting
//! - [`WgpuRenderer`] - the GPU renderer, presenting through [`ScreenMapping`]
//! - [`SpriteGpuRenderer`] - GPU sprite batching for particles/effects
//! - [`Camera2d`] / [`CameraView`] / [`CameraMut`] - cameras as entities, with
//!   world/screen transforms; [`RenderTargets`] for render-to-texture
//! - [`Animation`] / [`Animator`] / [`AnimationController`] / [`ValidatedAnimator`] - Sprite animation system
//! - Tiled rendering for repeating sprites and composite tiles
//! - [`ParallaxLayer`] - parallax layers laid out for every camera

pub mod animation;
mod atlas;
pub(crate) mod bind_groups;
pub mod camera;
pub mod camera2d;
pub(crate) mod camera_gpu;
pub(crate) mod camera_images;
pub mod camera_mut;
mod compositor;
mod glyph_atlas;
mod gpu_context;
pub mod gpu_error;
pub mod parallax;
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
pub(crate) use glyph_atlas::GlyphPage;
pub use gpu_context::GpuRenderContext;
pub use gpu_error::GpuError;
pub use parallax::{ParallaxLayer, TileMode};
pub use render_layers::RenderLayers;
pub use render_targets::{MAX_TARGET_SIDE, RenderTargets};
pub use renderer::WgpuRenderer;
pub use screen::{Presentation, ScreenMapping};
pub use sprite::{AnimatedSprite, Sprite};
pub(crate) use sprite_gpu::place_in_slot;
pub use sprite_gpu::{
  BatchCollector, SpriteDraw, SpriteGpuRenderer, SpriteInstance, get_sprite_effect,
};
pub(crate) use texture_registry::{TextureId, TextureRegistry, TextureSource};
pub use tile::{CompositeTile, CompositeTileBuilder, TileDirection, TilePart};

/// The GPU device and queue, shared by the renderers and the texture manager.
#[derive(crate::core::ecs::Resource, Clone)]
pub struct Gpu {
  pub device: std::sync::Arc<wgpu::Device>,
  pub queue: std::sync::Arc<wgpu::Queue>,
}
