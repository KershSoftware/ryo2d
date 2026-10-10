//! Render Pipeline System (experimental, not used by the engine)
//!
//! **Status:** the engine never builds or executes a `RenderPipeline` - frame
//! rendering is hard-coded in `engine.rs` and `graphics/renderer.rs`, so passes
//! configured here don't change what a game draws. Only `ClearPass`,
//! `ParallaxPass`, `UploadPass` and `LightingPass` exist. The module is kept for a
//! future pass-based renderer and isn't part of the prelude.
//!
//! The design: each rendering phase is encapsulated in a `RenderPass`; passes
//! can be added, removed, or reordered at runtime.
//!
//! # Architecture
//!
//! ```text
//! Game code describes intent
//!     │
//!     ▼
//! Engine extracts intent into render data
//!     │
//!     ▼
//! RenderPipeline plans passes
//!     │
//!     ├── ClearPass: Clear buffers
//!     ├── ParallaxPass: GPU parallax layers
//!     ├── CpuSpritePass: Software sprite rendering
//!     ├── UploadPass: CPU buffer → GPU texture
//!     ├── GpuSpritePass: Batched GPU sprites
//!     ├── TrailPass: Motion trails
//!     ├── LightingPass: 2D lighting
//!     ├── UiPass: User interface
//!     ├── EffectsPass: Post-processing
//!     └── PresentPass: Final output
//!     │
//!     ▼
//! Swapchain shows the result
//! ```
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::core::render::{RenderPipeline, passes::ClearPass};
//!
//! let mut pipeline = RenderPipeline::new();
//! pipeline.add_pass(ClearPass::new());
//! pipeline.add_pass(MyCustomPass::new());
//!
//! // Disable a pass without removing it
//! pipeline.set_enabled("clear", false);
//!
//! // Execute pipeline
//! pipeline.execute(&mut render_context);
//! ```

mod context;
mod pass;
pub mod passes;
mod pipeline;

pub use context::{RenderContext, TextureSlot};
pub use pass::{RenderPass, RenderPassId};
pub use pipeline::RenderPipeline;

/// Pass IDs for the planned passes. Only `CLEAR`, `PARALLAX`, `UPLOAD` and
/// `LIGHTING` have implementations (see [`passes`]).
pub mod pass_ids {
  pub const CLEAR: &str = "clear";
  pub const PARALLAX: &str = "parallax";
  pub const CPU_SPRITE: &str = "cpu_sprite";
  pub const UPLOAD: &str = "upload";
  pub const GPU_SPRITE: &str = "gpu_sprite";
  pub const TRAIL: &str = "trail";
  pub const LIGHTING: &str = "lighting";
  pub const UI: &str = "ui";
  pub const EFFECTS: &str = "effects";
  pub const PRESENT: &str = "present";
}
