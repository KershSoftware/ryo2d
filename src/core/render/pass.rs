//! Render Pass trait definition

/// Unique identifier for a render pass
pub type RenderPassId = &'static str;

use super::RenderContext;

/// A render pass that performs one phase of the rendering pipeline.
///
/// Each pass has a unique ID and can declare dependencies on other passes.
/// The pipeline executor ensures passes run in dependency order.
///
/// # Implementing a Custom Pass
///
/// ```ignore
/// struct MyPass {
///     enabled: bool,
/// }
///
/// impl RenderPass for MyPass {
///     fn id(&self) -> &'static str { "my_pass" }
///
///     fn dependencies(&self) -> &[&'static str] {
///         &["gpu_sprite"]  // Run after GPU sprites
///     }
///
///     fn execute(&mut self, ctx: &mut RenderContext) {
///         // Your rendering code here
///     }
/// }
/// ```
pub trait RenderPass: Send + Sync {
  /// Unique identifier for this pass.
  ///
  /// Used for dependency resolution and enabling/disabling passes.
  fn id(&self) -> RenderPassId;

  /// Passes that must execute before this one.
  ///
  /// The pipeline will ensure all dependencies have run before executing
  /// this pass. Circular dependencies will cause a panic at pipeline creation.
  fn dependencies(&self) -> &[RenderPassId] {
    &[]
  }

  /// Execute the render pass.
  ///
  /// Called once per frame if the pass is enabled and `should_execute` returns true.
  /// The `RenderContext` provides access to all rendering resources.
  fn execute(&mut self, ctx: &mut RenderContext);

  /// Whether this pass should execute this frame.
  ///
  /// Called after dependency checking. Override to skip execution based on
  /// runtime conditions (e.g., no lights in scene, no UI visible).
  fn should_execute(&self, _ctx: &RenderContext) -> bool {
    true
  }

  /// Whether this pass is enabled.
  ///
  /// Disabled passes are skipped entirely, including dependency checking.
  fn is_enabled(&self) -> bool {
    true
  }

  /// Enable or disable this pass.
  ///
  /// Default implementation does nothing - override to support runtime toggling.
  fn set_enabled(&mut self, _enabled: bool) {}
}
