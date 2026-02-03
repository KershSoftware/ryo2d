//! Clear pass - clears the pixel buffer to background color

use crate::core::render::pass_ids;
use crate::core::render::{RenderContext, RenderPass, RenderPassId};

/// Default background color for the framebuffer (#191923).
/// This color is visible when nothing is rendered, making it easy to verify
/// the render pipeline is working.
pub const DEFAULT_CLEAR_COLOR: [u8; 4] = [0x19, 0x19, 0x23, 0xff];

/// Render pass that clears the CPU pixel buffer to a background color.
///
/// This pass runs first in the pipeline to ensure a clean slate for rendering.
/// The default color is #191923 - a dark gray that indicates the renderer is working.
pub struct ClearPass {
  /// Background color (RGBA)
  pub color: [u8; 4],
  /// Whether this pass is enabled
  enabled: bool,
}

impl Default for ClearPass {
  fn default() -> Self {
    Self::new()
  }
}

impl ClearPass {
  /// Create a new clear pass with the default background color (#191923)
  pub fn new() -> Self {
    Self {
      color: DEFAULT_CLEAR_COLOR,
      enabled: true,
    }
  }

  /// Create a clear pass with a custom color
  pub fn with_color(r: u8, g: u8, b: u8, a: u8) -> Self {
    Self {
      color: [r, g, b, a],
      enabled: true,
    }
  }

  /// Set the background color
  pub fn set_color(&mut self, r: u8, g: u8, b: u8, a: u8) {
    self.color = [r, g, b, a];
  }
}

impl RenderPass for ClearPass {
  fn id(&self) -> RenderPassId {
    pass_ids::CLEAR
  }

  fn dependencies(&self) -> &[RenderPassId] {
    &[] // Clear runs first, no dependencies
  }

  fn execute(&mut self, ctx: &mut RenderContext) {
    // Fill pixel buffer with background color
    for pixel in ctx.pixel_buffer.chunks_exact_mut(4) {
      pixel[0] = self.color[0];
      pixel[1] = self.color[1];
      pixel[2] = self.color[2];
      pixel[3] = self.color[3];
    }
  }

  fn is_enabled(&self) -> bool {
    self.enabled
  }

  fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_default_color() {
    let pass = ClearPass::new();
    assert_eq!(pass.color, DEFAULT_CLEAR_COLOR);
    assert_eq!(pass.color, [0x19, 0x19, 0x23, 0xff]);
  }

  #[test]
  fn test_custom_color() {
    let pass = ClearPass::with_color(255, 0, 0, 255);
    assert_eq!(pass.color, [255, 0, 0, 255]);
  }

  #[test]
  fn test_pass_id() {
    let pass = ClearPass::new();
    assert_eq!(pass.id(), "clear");
  }

  #[test]
  fn test_no_dependencies() {
    let pass = ClearPass::new();
    assert!(pass.dependencies().is_empty());
  }
}
