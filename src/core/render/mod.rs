//! Render Pipeline System
//!
//! This module provides a configurable render pipeline where each rendering
//! phase is encapsulated in a `RenderPass`. Passes can be added, removed,
//! or reordered at runtime.
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
//! // Create default 2D pipeline
//! let mut pipeline = RenderPipeline::default_2d();
//!
//! // Disable lighting pass
//! pipeline.set_enabled("lighting", false);
//!
//! // Add custom pass after sprites
//! pipeline.add_pass(MyCustomPass::new());
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

/// Default pass IDs used by the engine
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
