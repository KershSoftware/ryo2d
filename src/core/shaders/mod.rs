//! Shader system for post-processing effects
//!
//! This module provides the screen-wide post-processing pipeline. Scenes reach
//! its [`EffectsManager`] (a resource) through `ctx.effects()`.
//!
//! # Architecture
//!
//! The shader system is built around two core concepts:
//!
//! 1. **Effects** ([`PostProcessEffect`]) - Individual visual effects with parameters
//! 2. **Effect Stack** ([`EffectsManager`]) - Manages active effects and their order
//!
//! # Shader Organization
//!
//! ```text
//! shaders/
//! ├── core/           # Core rendering shaders
//! │   └── fullscreen_quad.wgsl
//! └── post-process/   # Post-processing effect shaders
//!     ├── fade.wgsl
//!     ├── color_tint.wgsl
//!     ├── vignette.wgsl
//!     ├── screen_flash.wgsl
//!     ├── crt.wgsl
//!     └── chromatic_aberration.wgsl
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!     let mut effects = ctx.effects();
//!
//!     // Scene transition fade
//!     effects.set_fade(self.fade_progress);
//!
//!     // Damage flash
//!     if player_hit {
//!         effects.flash_secs(Color::RED, 0.15);
//!     }
//!
//!     // Enable persistent effects
//!     effects.set_vignette(0.3);
//!     SceneAction::Continue
//! }
//! ```

// Consumed by the sprite and effect pipelines
pub(crate) mod custom;
mod effects;
mod manager;
mod pipeline;

pub use effects::{EffectParams, GodRaysParams, PostProcessEffect};
pub use manager::{EffectError, EffectsManager};
pub use pipeline::{OutputBuffer, PostProcessPipeline};

/// Embed core shaders at compile time
pub mod core_shaders {
  pub const FULLSCREEN_QUAD: &str = include_str!("core/fullscreen_quad.wgsl");
}

/// Embed post-process shaders at compile time
pub mod post_process_shaders {
  pub const FADE: &str = include_str!("post-process/fade.wgsl");
  pub const COLOR_TINT: &str = include_str!("post-process/color_tint.wgsl");
  pub const VIGNETTE: &str = include_str!("post-process/vignette.wgsl");
  pub const SCREEN_FLASH: &str = include_str!("post-process/screen_flash.wgsl");
  pub const CRT: &str = include_str!("post-process/crt.wgsl");
  pub const CHROMATIC_ABERRATION: &str = include_str!("post-process/chromatic_aberration.wgsl");
  pub const GOD_RAYS: &str = include_str!("post-process/god_rays.wgsl");
  pub const AMBIENT_OCCLUSION: &str = include_str!("post-process/ambient_occlusion.wgsl");
  pub const BLOOM_EXTRACT: &str = include_str!("post-process/bloom_extract.wgsl");
  pub const BLOOM_BLUR: &str = include_str!("post-process/bloom_blur.wgsl");
  pub const BLOOM_COMPOSITE: &str = include_str!("post-process/bloom_composite.wgsl");
}
