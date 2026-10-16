//! The UI, the final composite and frame capture

use crate::core::graphics::CompositeViewport;
use crate::core::render::{FrameInfo, RenderContext, RenderPass, pass_ids};

/// Draws the frame's UI (the UI camera's draws, laid out by the batch) into
/// the compositor's UI image, cleared first.
pub(super) struct Ui;

impl RenderPass for Ui {
  fn id(&self) -> &str {
    pass_ids::UI
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.ui
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (Some(camera), Some(batch)) = (ctx.scene.ui_camera, ctx.scene.batch) else {
      return;
    };
    let sprites = ctx.scene.sprites;
    let image = ctx.scene.ui_size;
    let draws = batch.draws(camera.entity, false);
    let compositor = ctx.targets.compositor;
    let view = compositor.ui_view();
    let mut pass = ctx
      .encoder()
      .begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("UI Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
    for draw in draws {
      batch.draw(&mut pass, draw, camera.entity, sprites, image);
    }
    drop(pass);
    ctx.ui_drawn = true;
  }
}

/// Draws the latest image, then the UI (if the `ui` pass drew it this
/// frame), to the window or offscreen target with letterboxing.
pub(super) struct Composite;

impl RenderPass for Composite {
  fn id(&self) -> &str {
    pass_ids::COMPOSITE
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (device, compositor) = (ctx.device(), ctx.targets.compositor);
    let (output_view, include_ui) = (ctx.targets.output_view, ctx.ui_drawn);
    let (encoder, source) = ctx.encoder_and_current_view();
    compositor.draw(
      device,
      encoder,
      output_view,
      source,
      include_ui,
      CompositeViewport::Letterboxed,
    );
  }
}

/// Draws the same composite into the capture texture, without letterboxing.
pub(super) struct Capture;

impl RenderPass for Capture {
  fn id(&self) -> &str {
    pass_ids::CAPTURE
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.capture
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(texture) = ctx.targets.capture else {
      return;
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let (device, compositor, include_ui) = (ctx.device(), ctx.targets.compositor, ctx.ui_drawn);
    let (encoder, source) = ctx.encoder_and_current_view();
    compositor.draw(
      device,
      encoder,
      &view,
      source,
      include_ui,
      CompositeViewport::Identity,
    );
    ctx.captured = true;
  }
}
