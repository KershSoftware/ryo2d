//! What a render pass can use while it records its work

use super::FrameInfo;
use super::frame::{FrameImage, FrameScene};
use crate::core::graphics::Compositor;
use crate::core::shaders::{OutputBuffer, PostProcessPipeline};

/// The renderer's textures and pipelines the built-in passes draw with.
pub(crate) struct FrameTargets<'a> {
  pub(crate) game: &'a wgpu::Texture,
  pub(crate) game_view: &'a wgpu::TextureView,
  pub(crate) lit: &'a wgpu::Texture,
  pub(crate) lit_view: &'a wgpu::TextureView,
  pub(crate) compositor: &'a Compositor,
  /// The window surface or offscreen texture this frame is presented to
  pub(crate) output_view: &'a wgpu::TextureView,
  /// The capture texture, only while a capture is pending this frame
  pub(crate) capture: Option<&'a wgpu::Texture>,
}

/// What a [`RenderPass`](super::RenderPass) can use while it records its work.
pub struct RenderContext<'a> {
  device: &'a wgpu::Device,
  queue: &'a wgpu::Queue,
  // Built-in passes borrow it next to the crate-internal fields
  pub(super) encoder: &'a mut wgpu::CommandEncoder,
  frame: FrameInfo,
  pub(crate) targets: FrameTargets<'a>,
  pub(crate) scene: FrameScene<'a>,
  pub(crate) post_process: Option<&'a mut PostProcessPipeline>,
  /// Which texture holds the latest image
  pub(crate) image: FrameImage,
  /// Whether the `ui` pass uploaded this frame's UI (the composite only blends it then)
  pub(crate) ui_uploaded: bool,
  /// Whether the `capture` pass drew into the capture texture this frame
  pub(crate) captured: bool,
}

impl<'a> RenderContext<'a> {
  /// A context for one frame; the latest image starts as the game texture.
  pub(crate) fn new(
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,
    frame: FrameInfo,
    targets: FrameTargets<'a>,
    scene: FrameScene<'a>,
    post_process: Option<&'a mut PostProcessPipeline>,
  ) -> Self {
    Self {
      device,
      queue,
      encoder,
      frame,
      targets,
      scene,
      post_process,
      image: FrameImage::Game,
      ui_uploaded: false,
      captured: false,
    }
  }

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

  /// The format of [`current_view`](Self::current_view), for the render
  /// pipelines a pass builds. Every game-size image (game, lit and
  /// post-process output) shares it.
  pub fn format(&self) -> wgpu::TextureFormat {
    image_texture(&self.targets, self.post_process.as_deref(), self.image).format()
  }

  /// The texture holding the frame's latest image (the game texture, the lit
  /// scene or the post-process output), at game resolution.
  ///
  /// Only passes before `composite` reach the window: it copies this image to
  /// the screen, so drawing on it from a pass placed after `composite` is not
  /// shown. Insert game passes before `composite` (or after `post_process`).
  pub fn current_view(&self) -> &wgpu::TextureView {
    image_view(&self.targets, self.post_process.as_deref(), self.image)
  }

  /// The encoder and the latest image together, to draw onto the image.
  ///
  /// Only passes before `composite` reach the window: a pass placed after
  /// `composite` draws on an image that is no longer presented.
  pub fn encoder_and_current_view(&mut self) -> (&mut wgpu::CommandEncoder, &wgpu::TextureView) {
    let view = image_view(&self.targets, self.post_process.as_deref(), self.image);
    (&mut *self.encoder, view)
  }
}

/// The view of the texture that holds `image`.
pub(super) fn image_view<'t>(
  targets: &'t FrameTargets<'_>,
  post_process: Option<&'t PostProcessPipeline>,
  image: FrameImage,
) -> &'t wgpu::TextureView {
  match (image, post_process) {
    (FrameImage::Lit, _) => targets.lit_view,
    (FrameImage::PostProcess(buffer), Some(post_process)) => post_process.get_output_view(buffer),
    // Post-process output without a post-process pipeline can't happen; fall back to the scene
    (FrameImage::Game, _) | (FrameImage::PostProcess(_), None) => targets.game_view,
  }
}

/// The texture that holds `image`.
pub(super) fn image_texture<'t>(
  targets: &'t FrameTargets<'_>,
  post_process: Option<&'t PostProcessPipeline>,
  image: FrameImage,
) -> &'t wgpu::Texture {
  match (image, post_process) {
    (FrameImage::Lit, _) => targets.lit,
    (FrameImage::PostProcess(OutputBuffer::A), Some(post_process)) => post_process.texture_a(),
    (FrameImage::PostProcess(OutputBuffer::B), Some(post_process)) => post_process.texture_b(),
    (FrameImage::Game, _) | (FrameImage::PostProcess(_), None) => targets.game,
  }
}
