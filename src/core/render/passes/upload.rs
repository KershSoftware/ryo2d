//! Upload pass - uploads CPU pixel buffer to GPU texture

use crate::core::render::pass_ids;
use crate::core::render::{RenderContext, RenderPass, RenderPassId};

/// Render pass that uploads the CPU pixel buffer to the GPU game texture.
///
/// This pass bridges CPU and GPU rendering by copying the software-rendered
/// content to a GPU texture where GPU sprites, effects, and lighting can
/// be applied.
pub struct UploadPass {
  /// Whether this pass is enabled
  enabled: bool,
}

impl Default for UploadPass {
  fn default() -> Self {
    Self::new()
  }
}

impl UploadPass {
  /// Create a new upload pass
  pub fn new() -> Self {
    Self { enabled: true }
  }
}

impl RenderPass for UploadPass {
  fn id(&self) -> RenderPassId {
    pass_ids::UPLOAD
  }

  fn dependencies(&self) -> &[RenderPassId] {
    // Must run after CPU sprite rendering
    &[pass_ids::CPU_SPRITE]
  }

  fn execute(&mut self, ctx: &mut RenderContext) {
    // Upload pixel buffer to GPU texture
    ctx.queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: ctx.game_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      ctx.pixel_buffer,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * ctx.width),
        rows_per_image: Some(ctx.height),
      },
      wgpu::Extent3d {
        width: ctx.width,
        height: ctx.height,
        depth_or_array_layers: 1,
      },
    );
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
  fn test_pass_id() {
    let pass = UploadPass::new();
    assert_eq!(pass.id(), "upload");
  }

  #[test]
  fn test_dependencies() {
    let pass = UploadPass::new();
    assert_eq!(pass.dependencies(), &["cpu_sprite"]);
  }
}
