//! Post-process effects on the display image

use crate::core::render::{FrameImage, FrameInfo, RenderContext, RenderPass, pass_ids};
use crate::core::shaders::OutputBuffer;

/// Runs the active display effects on the display image the tonemap pass
/// wrote; the chain's output becomes the latest image.
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
    let (width, height) = ctx.render_size();
    let effects = ctx.scene.effects;
    let active: Vec<_> = effects
      .active_effects()
      .map(|effect| (effect, effects.time()))
      .collect();
    let custom: Vec<_> = effects.enabled_custom().collect();
    let time = effects.time();
    let RenderContext {
      encoder,
      stages,
      image,
      ..
    } = ctx;
    // Display effects work on the display image the tonemap pass wrote
    let FrameImage::Display(buffer) = *image else {
      return;
    };
    if active.is_empty() && custom.is_empty() {
      return;
    }
    let display = &mut *stages.display;
    // The chain reads texture A
    if buffer == OutputBuffer::B {
      encoder.copy_texture_to_texture(
        display.texture_b().as_image_copy(),
        display.texture_a().as_image_copy(),
        wgpu::Extent3d {
          width,
          height,
          depth_or_array_layers: 1,
        },
      );
    }
    let output = display.apply_chain(device, queue, encoder, &active, &custom, time);
    *image = FrameImage::Display(output);
  }
}
