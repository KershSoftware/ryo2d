//! A camera's image clear

use crate::core::graphics::CameraClear;
use crate::core::graphics::renderer::DEFAULT_BACKGROUND_COLOR;
use crate::core::render::{FrameInfo, PassScope, RenderContext, RenderPass, pass_ids};
use crate::core::ui::Color;

/// Clears each camera's image as its [`CameraClear`] says: to the
/// background colour, to a colour, or (for a camera that does not clear) to
/// transparent, so the cameras below show through.
pub(super) struct Clear;

impl RenderPass for Clear {
  fn id(&self) -> &str {
    pass_ids::CLEAR
  }

  fn scope(&self) -> PassScope {
    PassScope::Camera
  }

  fn should_run(&self, _frame: &FrameInfo) -> bool {
    true
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let Some(camera) = ctx.current_camera() else {
      return;
    };
    let color = match camera.clear {
      CameraClear::Background => DEFAULT_BACKGROUND_COLOR,
      CameraClear::Color(color) => premultiplied_linear(color),
      CameraClear::None => wgpu::Color::TRANSPARENT,
    };
    let (encoder, view) = ctx.encoder_and_current_view();
    clear(encoder, view, color);
  }
}

/// `color` (sRGB) as the linear, premultiplied colour an sRGB image holds.
fn premultiplied_linear(color: Color) -> wgpu::Color {
  let linear = |channel: u8| {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
      c / 12.92
    } else {
      ((c + 0.055) / 1.055).powf(2.4)
    }
  };
  let a = f64::from(color.a) / 255.0;
  wgpu::Color {
    r: linear(color.r) * a,
    g: linear(color.g) * a,
    b: linear(color.b) * a,
    a,
  }
}

/// Record an empty render pass that clears `view` to `color`.
pub(super) fn clear(
  encoder: &mut wgpu::CommandEncoder,
  view: &wgpu::TextureView,
  color: wgpu::Color,
) {
  let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("Clear Game Texture Pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(color),
        store: wgpu::StoreOp::Store,
      },
    })],
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
    multiview_mask: None,
  });
}
