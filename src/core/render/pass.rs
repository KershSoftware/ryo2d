//! The render pass trait

use super::{FrameInfo, RenderContext};

/// Where a [`RenderPass`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassScope {
  /// Once per frame, on the scene image (the default)
  #[default]
  Frame,
  /// Once per camera, on that camera's image, before the frame passes;
  /// [`RenderContext::camera`] is the camera's view
  Camera,
}

/// One stage of the frame, run in order by a [`RenderPipeline`](super::RenderPipeline).
///
/// A pass runs once per frame unless its [`scope`](Self::scope) is
/// [`PassScope::Camera`]: camera passes run first, once per camera in render
/// order, each on its camera's image; then the frame passes run once.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// struct Flash;
///
/// impl RenderPass for Flash {
///   fn id(&self) -> &str {
///     "flash"
///   }
///
///   fn execute(&mut self, ctx: &mut RenderContext) {
///     // Record GPU work with ctx.device(), ctx.queue() and ctx.encoder()
///   }
/// }
///
/// // Passes persist across scenes, so add it only once
/// let mut pipeline = ctx.render_pipeline();
/// if !pipeline.contains("flash") {
///   let _ = pipeline.insert_before(pass_ids::COMPOSITE, Flash);
/// }
/// ```
pub trait RenderPass: Send + Sync {
  /// Unique id, used to find, enable, disable and anchor passes.
  fn id(&self) -> &str;

  /// Whether this pass takes part in a frame. Only asked while the pass is
  /// enabled. Defaults to always.
  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  /// Whether this pass runs once per frame or once per camera. Defaults to
  /// once per frame.
  fn scope(&self) -> PassScope {
    PassScope::Frame
  }

  /// Record this pass's GPU work for the frame (or for the camera, in a
  /// camera pass).
  fn execute(&mut self, ctx: &mut RenderContext<'_>);
}

/// Lets a pass taken out with [`RenderPipeline::remove`](super::RenderPipeline::remove)
/// go back in.
impl RenderPass for Box<dyn RenderPass> {
  fn id(&self) -> &str {
    (**self).id()
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    (**self).should_run(frame)
  }

  fn scope(&self) -> PassScope {
    (**self).scope()
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    (**self).execute(ctx);
  }
}
