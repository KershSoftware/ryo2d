//! Post-process effects on the latest image

use crate::core::render::context::image_texture;
use crate::core::render::{FrameImage, FrameInfo, RenderContext, RenderPass, pass_ids};
use crate::core::shaders::OutputBuffer;

/// Runs the active effect chain on the latest image (the lit scene when
/// lighting ran); the chain's output becomes the latest image.
pub(super) struct PostProcess;

impl RenderPass for PostProcess {
  fn id(&self) -> &str {
    pass_ids::POST_PROCESS
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.effects
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (device, queue) = (ctx.device(), ctx.queue());
    let (width, height) = ctx.game_size();
    let effects = ctx.scene.effects;
    let active: Vec<_> = effects
      .active_effects()
      .map(|effect| (effect, effects.time()))
      .collect();
    let RenderContext {
      encoder,
      targets,
      post_process,
      image,
      ..
    } = ctx;
    let Some(post_process) = post_process.as_deref_mut() else {
      return;
    };
    if active.is_empty() {
      return;
    }

    // The chain reads texture A; the image is already there if A holds it
    if *image != FrameImage::PostProcess(OutputBuffer::A) {
      let source = image_texture(targets, Some(&*post_process), *image);
      encoder.copy_texture_to_texture(
        wgpu::ImageCopyTexture {
          texture: source,
          mip_level: 0,
          origin: wgpu::Origin3d::ZERO,
          aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyTexture {
          texture: post_process.texture_a(),
          mip_level: 0,
          origin: wgpu::Origin3d::ZERO,
          aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    }

    let output = post_process.apply_effects(device, queue, encoder, &active);
    *image = FrameImage::PostProcess(output);
  }
}
