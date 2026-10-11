//! What a frame contains, decided before any pass runs

/// What this frame contains, decided before any render pass runs.
///
/// Passes read it in [`RenderPass::should_run`](super::RenderPass::should_run)
/// to decide whether they take part in the frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInfo {
  /// Game resolution width in pixels
  pub game_width: u32,
  /// Game resolution height in pixels
  pub game_height: u32,
  /// The GPU raycaster draws this frame instead of the 2D scene
  pub raycast: bool,
  /// GPU sprite batches were submitted this frame
  pub gpu_sprites: bool,
  /// Trails or lines have geometry this frame
  pub trails: bool,
  /// 2D lighting is on
  pub lighting: bool,
  /// Post-process effects are active
  pub effects: bool,
  /// The UI buffer has something to draw
  pub ui: bool,
  /// A frame capture was requested for this frame
  pub capture: bool,
}
