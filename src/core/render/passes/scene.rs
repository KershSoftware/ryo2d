//! The 2D scene: CPU upload, clear, GPU sprites and the CPU blend

use crate::core::graphics::renderer::DEFAULT_BACKGROUND_COLOR;
use crate::core::render::{FrameImage, FrameInfo, RenderContext, RenderPass, pass_ids};

/// Uploads the CPU pixel buffer to the CPU staging texture.
pub(super) struct Upload;

impl RenderPass for Upload {
  fn id(&self) -> &str {
    pass_ids::UPLOAD
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let (width, height) = ctx.game_size();
    ctx.queue().write_texture(
      wgpu::ImageCopyTexture {
        texture: ctx.targets.cpu_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      ctx.targets.pixel_buffer,
      wgpu::ImageDataLayout {
        offset: 0,
        bytes_per_row: Some(4 * width),
        rows_per_image: Some(height),
      },
      wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
    );
  }
}

/// Clears the game texture to the background colour.
pub(super) struct Clear;

impl RenderPass for Clear {
  fn id(&self) -> &str {
    pass_ids::CLEAR
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let game_view = ctx.targets.game_view;
    clear(ctx.encoder(), game_view, DEFAULT_BACKGROUND_COLOR);
    ctx.image = FrameImage::Game;
  }
}

/// Draws the GPU sprite batches (`render_gpu`, parallax) onto the game texture.
pub(super) struct GpuSprites;

impl RenderPass for GpuSprites {
  fn id(&self) -> &str {
    pass_ids::GPU_SPRITES
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast && frame.gpu_sprites
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let sprites = ctx.scene.sprites;
    let (queue, game_view) = (ctx.queue(), ctx.targets.game_view);
    sprites.render_batches(queue, ctx.encoder(), game_view);
  }
}

/// Alpha-blends the CPU staging texture onto the game texture.
pub(super) struct CpuBlend;

impl RenderPass for CpuBlend {
  fn id(&self) -> &str {
    pass_ids::CPU_BLEND
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    !frame.raycast
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let targets = &ctx.targets;
    let mut blend_pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("CPU Content Blend Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: targets.game_view,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load, // Keep the GPU sprites
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
    });
    blend_pass.set_pipeline(targets.cpu_blend_pipeline);
    blend_pass.set_bind_group(0, targets.cpu_bind_group, &[]);
    blend_pass.draw(0..6, 0..1);
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
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(color),
        store: wgpu::StoreOp::Store,
      },
    })],
    depth_stencil_attachment: None,
    timestamp_writes: None,
    occlusion_query_set: None,
  });
}
