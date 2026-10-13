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
pub(crate) mod compose;
pub(crate) mod custom;
mod effects;
mod manager;
mod pipeline;
pub(crate) mod scope;

pub use effects::{EffectParams, EffectStage, GodRaysParams, PostProcessEffect};
pub use manager::{EffectError, EffectsManager};
pub use pipeline::{OutputBuffer, PostProcessPipeline};

/// Embed core shaders at compile time
pub mod core_shaders {
  pub const FULLSCREEN_QUAD: &str = super::compose::COMPOSITE;
}

/// Embed post-process shaders at compile time
pub mod post_process_shaders {
  pub const FADE: &str = super::compose::FADE;
  pub const COLOR_TINT: &str = super::compose::COLOR_TINT;
  pub const VIGNETTE: &str = super::compose::VIGNETTE;
  pub const SCREEN_FLASH: &str = super::compose::SCREEN_FLASH;
  pub const CRT: &str = super::compose::CRT;
  pub const CHROMATIC_ABERRATION: &str = super::compose::CHROMATIC_ABERRATION;
  pub const GOD_RAYS: &str = super::compose::GOD_RAYS;
  pub const AMBIENT_OCCLUSION: &str = super::compose::AMBIENT_OCCLUSION;
  pub const BLOOM_EXTRACT: &str = super::compose::BLOOM_EXTRACT;
  pub const BLOOM_BLUR: &str = super::compose::BLOOM_BLUR;
  pub const BLOOM_COMPOSITE: &str = super::compose::BLOOM_COMPOSITE;
}
