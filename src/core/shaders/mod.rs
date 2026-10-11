//! Shader system for post-processing effects
//!
//! This module provides a flexible post-processing pipeline that can be applied
//! to both the global game context and individual cameras.
//!
//! # Architecture
//!
//! The shader system is built around three core concepts:
//!
//! 1. **Effects** ([`PostProcessEffect`]) - Individual visual effects with parameters
//! 2. **Effect Stack** ([`EffectsManager`]) - Manages active effects and their order
//! 3. **Effect Target** ([`EffectTarget`]) - Trait for types that can have effects applied
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
//! // In Game::update()
//! fn update(&mut self, ctx: &mut GameContext, dt: f32) {
//!     // Scene transition fade
//!     ctx.effects.set_fade(self.fade_progress);
//!
//!     // Damage flash
//!     if player_hit {
//!         ctx.effects.flash(Color::RED, 0.15);
//!     }
//!
//!     // Enable persistent effects
//!     ctx.effects.set_vignette(0.3);
//! }
//! ```

// Consumed by the sprite and effect pipelines
#[allow(dead_code)]
pub(crate) mod custom;
mod effects;
mod manager;
mod pipeline;

pub use effects::{EffectParams, GodRaysParams, PostProcessEffect};
pub use manager::{EffectTarget, EffectsManager};
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
