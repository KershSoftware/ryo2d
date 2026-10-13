//! Final composite: draws the scene (plus the UI overlay) into a target view.
//!
//! The compositor owns the resources every composite needs, so render passes can call
//! [`Compositor::draw`] with only a shared borrow of it.

use wgpu::util::DeviceExt;

/// Viewport uniform data sent to the composite shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
  // x: scale_x, y: scale_y, z: offset_x, w: offset_y
  transform: [f32; 4],
  // x: 1.0 when the target is not sRGB and the shader must encode
  flags: [f32; 4],
}

/// Which viewport transform a composite uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeViewport {
  /// Letterboxed/scaled to the window (the presented surface).
  Letterboxed,
  /// 1:1 at game resolution (captures).
  Identity,
}

/// Resources and drawing for the final composite.
pub(crate) struct Compositor {
  // 1.0 when the target is not sRGB, so the composite encodes to sRGB itself
  encode_srgb: f32,
  // REPLACE blend: draws the scene source
  render_pipeline: wgpu::RenderPipeline,
  // ALPHA_BLENDING: draws the UI overlay on top
  blend_pipeline: wgpu::RenderPipeline,
  pub(crate) bind_group_layout: wgpu::BindGroupLayout,
  pub(crate) sampler: wgpu::Sampler,
  // Dedicated UI texture (its own texture, so uploading it never races with the game texture)
  ui_texture: wgpu::Texture,
  ui_texture_view: wgpu::TextureView,
  // Letterbox transform, updated when the window or mode changes
  pub(crate) uniform_buffer: wgpu::Buffer,
  // 1:1 transform for game-resolution passes
  pub(crate) identity_uniform_buffer: wgpu::Buffer,
}

/// Dedicated UI texture (its own texture, so uploading it never races with the game texture).
fn create_ui_texture(
  device: &wgpu::Device,
  width: u32,
  height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("UI Texture"),
    size: wgpu::Extent3d {
      width,
      height,
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Rgba8UnormSrgb,
    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    view_formats: &[],
  });
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  (texture, view)
}

impl Compositor {
  /// Create the composite resources for a `target_format` target and a game-sized UI texture.
  pub(crate) fn new(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
  ) -> Self {
    let (ui_texture, ui_texture_view) = create_ui_texture(device, width, height);

    // Create sampler (nearest neighbor for pixel art crispness)
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Game Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      ..Default::default()
    });

    // Create uniform buffer for viewport transform (changes based on window size/mode)
    let encode_srgb = if target_format.is_srgb() { 0.0 } else { 1.0 };
    let uniform = ViewportUniform {
      transform: [1.0, 1.0, 0.0, 0.0],
      flags: [encode_srgb, 0.0, 0.0, 0.0],
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Viewport Uniform Buffer"),
      contents: bytemuck::cast_slice(&[uniform]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create identity uniform buffer for game texture blending (always 1:1 scale)
    let identity_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Identity Uniform Buffer"),
      contents: bytemuck::cast_slice(&[uniform]),
      usage: wgpu::BufferUsages::UNIFORM, // No COPY_DST needed - never changes
    });

    // Create bind group layout
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Texture Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 2,
          visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });

    // Create shader module
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Fullscreen Quad Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::COMPOSITE.into()),
    });

    // Create pipeline layout
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Render Pipeline Layout"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    // Create render pipeline (REPLACE blend - for drawing scene)
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Render Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format: target_format,
          blend: Some(wgpu::BlendState::REPLACE),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
      },
      multiview_mask: None,
      cache: None,
    });

    // Create blend pipeline (ALPHA_BLENDING - for UI overlay onto surface)
    let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Blend Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format: target_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
      },
      multiview_mask: None,
      cache: None,
    });

    Self {
      encode_srgb,
      render_pipeline,
      blend_pipeline,
      bind_group_layout,
      sampler,
      ui_texture,
      ui_texture_view,
      uniform_buffer,
      identity_uniform_buffer,
    }
  }

  /// The UI overlay texture.
  #[cfg(test)]
  pub(crate) fn ui_texture(&self) -> &wgpu::Texture {
    &self.ui_texture
  }

  /// Recreate the UI texture at a new game resolution.
  pub(crate) fn resize_ui(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    (self.ui_texture, self.ui_texture_view) = create_ui_texture(device, width, height);
  }

  /// Set the letterbox transform: `[scale_x, scale_y, offset_x, offset_y]`.
  pub(crate) fn set_viewport_transform(&self, queue: &wgpu::Queue, transform: [f32; 4]) {
    let uniform = ViewportUniform {
      transform,
      flags: [self.encode_srgb, 0.0, 0.0, 0.0],
    };
    queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));
  }

  /// Upload the UI overlay pixels (RGBA, `width` x `height`) for the next [`draw`](Self::draw).
  pub(crate) fn upload_ui(&self, queue: &wgpu::Queue, data: &[u8], width: u32, height: u32) {
    queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture: &self.ui_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      data,
      wgpu::TexelCopyBufferLayout {
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

  /// Draw the scene `source` (plus the UI overlay when `include_ui`) into `target`.
  pub(crate) fn draw(
    &self,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    source: &wgpu::TextureView,
    include_ui: bool,
    viewport: CompositeViewport,
  ) {
    let uniform = match viewport {
      CompositeViewport::Letterboxed => &self.uniform_buffer,
      CompositeViewport::Identity => &self.identity_uniform_buffer,
    };
    let source_bind_group = self.bind_group(device, "Final Source Bind Group", source, uniform);
    let ui_bind_group = include_ui.then(|| {
      self.bind_group(
        device,
        "UI Overlay Bind Group",
        &self.ui_texture_view,
        uniform,
      )
    });

    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Final Composite Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: target,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          // Clear to black (letterbox color)
          load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      occlusion_query_set: None,
      multiview_mask: None,
      timestamp_writes: None,
    });

    render_pass.set_pipeline(&self.render_pipeline);
    render_pass.set_bind_group(0, &source_bind_group, &[]);
    render_pass.draw(0..6, 0..1);

    if let Some(ui_bind_group) = &ui_bind_group {
      render_pass.set_pipeline(&self.blend_pipeline);
      render_pass.set_bind_group(0, ui_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }
  }

  /// Bind group sampling `view` with the given viewport uniform.
  fn bind_group(
    &self,
    device: &wgpu::Device,
    label: &str,
    view: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
  ) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some(label),
      layout: &self.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: uniform.as_entire_binding(),
        },
      ],
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn viewport_uniform_is_pod() {
    // ViewportUniform must be Pod for bytemuck::cast_slice
    let uniform = ViewportUniform {
      transform: [1.0, 1.0, 0.0, 0.0],
      flags: [0.0; 4],
    };
    let bytes = bytemuck::bytes_of(&uniform);
    assert_eq!(bytes.len(), 32); // 8 floats * 4 bytes
  }
}
