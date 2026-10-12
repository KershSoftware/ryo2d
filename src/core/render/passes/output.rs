//! UI upload, the final composite and frame capture

use crate::core::graphics::Viewport;
use crate::core::render::{FrameInfo, RenderContext, RenderPass, pass_ids};

/// Uploads the UI buffer to the compositor's UI texture.
pub(super) struct Ui;

impl RenderPass for Ui {
  fn id(&self) -> &str {
    pass_ids::UI
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.ui
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (width, height) = ctx.ui_size();
    ctx
      .targets
      .compositor
      .upload_ui(ctx.queue(), ctx.scene.ui_buffer, width, height);
    ctx.ui_uploaded = true;
  }
}

/// Draws the latest image, then the UI (if the `ui` pass uploaded it this
/// frame), to the window or offscreen target with letterboxing.
pub(super) struct Composite;

impl RenderPass for Composite {
  fn id(&self) -> &str {
    pass_ids::COMPOSITE
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (device, compositor) = (ctx.device(), ctx.targets.compositor);
    let (output_view, include_ui) = (ctx.targets.output_view, ctx.ui_uploaded);
    let (encoder, source) = ctx.encoder_and_current_view();
    compositor.draw(
      device,
      encoder,
      output_view,
      source,
      include_ui,
      Viewport::Letterboxed,
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
    let (device, compositor, include_ui) = (ctx.device(), ctx.targets.compositor, ctx.ui_uploaded);
    let (encoder, source) = ctx.encoder_and_current_view();
    compositor.draw(
      device,
      encoder,
      &view,
      source,
      include_ui,
      Viewport::Identity,
    );
    ctx.captured = true;
  }
}
