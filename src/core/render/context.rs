//! What a render pass can use while it records its work

use super::FrameInfo;

/// What a [`RenderPass`](super::RenderPass) can use while it records its work.
pub struct RenderContext<'a> {
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  encoder: &'a mut wgpu::CommandEncoder,
  frame: FrameInfo,
}

impl<'a> RenderContext<'a> {
  /// The GPU device.
  pub fn device(&self) -> &'a wgpu::Device {
    self.device
  }

  /// The GPU queue (buffer and texture writes land before the frame's commands).
  pub fn queue(&self) -> &'a wgpu::Queue {
    self.queue
  }

  /// The frame's command encoder.
  pub fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
    self.encoder
  }

  /// What this frame contains.
  pub fn frame(&self) -> &FrameInfo {
    &self.frame
  }

  /// Game resolution in pixels (width, height).
  pub fn game_size(&self) -> (u32, u32) {
    (self.frame.game_width, self.frame.game_height)
  }
}
