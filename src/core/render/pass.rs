//! The render pass trait

use super::{FrameInfo, RenderContext};

/// One stage of the frame, run in order by a [`RenderPipeline`](super::RenderPipeline).
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
/// ctx.render_pipeline.insert_after(pass_ids::POST_PROCESS, Flash).expect("known, unique pass id");
/// ```
pub trait RenderPass: Send + Sync {
  /// Unique id, used to find, enable, disable and anchor passes.
  fn id(&self) -> &str;

  /// Whether this pass takes part in a frame. Only asked while the pass is
  /// enabled. Defaults to always.
  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  /// Record this pass's GPU work for the frame.
  fn execute(&mut self, ctx: &mut RenderContext<'_>);
}
