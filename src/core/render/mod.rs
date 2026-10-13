//! Render Pipeline: an ordered list of render passes
//!
//! A [`RenderPipeline`] holds [`RenderPass`]es in order. For each frame, every
//! enabled pass whose [`RenderPass::should_run`] holds for the [`FrameInfo`]
//! records its work, in list order, on one command encoder: first the camera
//! passes ([`PassScope::Camera`]: clear, world, lighting, unlit world), once
//! per camera on its image, then the frame passes once (the cameras' images
//! placed in their viewports, transition, post-process, UI, composite,
//! capture).
//!
//! Passes are found by id: add one at the end with `push`, or place it with
//! `insert_before` / `insert_after` a named pass. Duplicate or unknown ids are
//! reported as [`RenderPipelineError`], never panics. The ids of the built-in
//! passes live in [`pass_ids`].
//!
//! The engine draws every frame through this pipeline
//! ([`RenderPipeline::default_2d`]), on one command encoder with one submit.
//!
//! # Using it from a game
//!
//! The engine's pipeline is a resource, reachable as `ctx.render_pipeline()`
//! in scene callbacks. Changes persist across scene transitions. A pass that
//! draws on the frame uses [`RenderContext::encoder_and_current_view`]:
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Draw the frame without post-process effects
//! let _ = ctx.render_pipeline().set_enabled(pass_ids::POST_PROCESS, false);
//! // Run a game pass before the frame reaches the screen. Passes persist
//! // across scenes, so add it once (or `remove("my_pass")` in `on_exit`).
//! let mut pipeline = ctx.render_pipeline();
//! if !pipeline.contains("my_pass") {
//!   let _ = pipeline.insert_before(pass_ids::COMPOSITE, MyPass);
//! }
//! ```

pub(crate) mod batch;
mod context;
mod draw_list;
mod entities;
mod frame;
mod pass;
mod passes;
mod pipeline;

pub(crate) use context::FrameTargets;
pub use context::RenderContext;
pub(crate) use draw_list::{DrawItem, DrawKind, DrawList, DrawPayload, SpriteLook, SpriteShading};
pub(crate) use entities::EntityDrawer;
pub use frame::FrameInfo;
pub(crate) use frame::{
  CameraDraw, FrameImage, FrameScene, LightingFrame, PixelRect, TRANSITION_VISIBLE, plan_cameras,
};
pub use pass::{PassScope, RenderPass};
pub use pipeline::{RenderPipeline, RenderPipelineError};

/// Ids of the built-in passes, in the order `RenderPipeline::default_2d` runs them.
pub mod pass_ids {
  /// A camera's image clear (per camera)
  pub const CLEAR: &str = "clear";
  /// Entities, sprites and lit trails and lines, drawn together in z order
  /// (per camera)
  pub const WORLD: &str = "world";
  /// 2D lighting (per camera)
  pub const LIGHTING: &str = "lighting";
  /// Trails and lines that skip lighting, drawn in z order (per camera)
  pub const UNLIT_WORLD: &str = "unlit_world";
  /// The window cameras' images placed in their viewports
  pub const CAMERAS: &str = "cameras";
  /// Scene transition fade over the whole frame
  pub const TRANSITION: &str = "transition";
  /// Post-process effects
  pub const POST_PROCESS: &str = "post_process";
  /// UI upload
  pub const UI: &str = "ui";
  /// Final composite to the window or offscreen target
  pub const COMPOSITE: &str = "composite";
  /// Frame capture
  pub const CAPTURE: &str = "capture";
}
