//! Final composite: draws the scene, with the UI image blended over it, into
//! a target view.
//!
//! The compositor owns the resources every composite needs, so render passes can call
//! [`Compositor::draw`] with only a shared borrow of it. It also owns the UI
//! image the `ui` pass draws into: `UI_FORMAT`, gamma-encoded colour
//! premultiplied by alpha, which the composite shader blends over the frame
//! in sRGB space.

use std::sync::atomic::{AtomicU64, Ordering};

use wgpu::util::DeviceExt;

use super::bind_groups::{BindGroupCache, Bound};
use crate::core::render::UI_FORMAT;

/// Viewport uniform data sent to the composite shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
  // x: scale_x, y: scale_y, z: offset_x, w: offset_y
  transform: [f32; 4],
  // x: 1.0 when the target is not sRGB and the shader must encode
  // y: 1.0 when the UI image is blended over the frame
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
  // The bind groups (source, UI image or the blank, uniform), made once per
  // set; cleared when the views they bind are recreated
  bind_groups: BindGroupCache,
  // 1.0 when the target is not sRGB, so the composite encodes to sRGB itself
  encode_srgb: f32,
  // REPLACE blend: the frame, the UI blended in by the shader
  render_pipeline: wgpu::RenderPipeline,
  bind_group_layout: wgpu::BindGroupLayout,
  sampler: wgpu::Sampler,
  // The UI image the `ui` pass draws into
  ui_texture: wgpu::Texture,
  ui_texture_view: wgpu::TextureView,
  /// Changes whenever the UI image is created anew
  ui_epoch: u64,
  // A transparent 1x1 image bound when no UI is blended
  blank_view: wgpu::TextureView,
  // One uniform per (viewport, UI blended): the letterbox transform is
  // written when the window or mode changes; the identity ones never change
  letterboxed: [wgpu::Buffer; 2],
  identity: [wgpu::Buffer; 2],
}

/// An epoch no UI image has had in this process: a recreated compositor's
/// image never matches an old one's
fn next_ui_epoch() -> u64 {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

/// An image the `ui` pass draws into and the composite samples.
fn create_ui_texture(
  device: &wgpu::Device,
  width: u32,
  height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
  let texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("UI Image"),
    size: wgpu::Extent3d {
      width: width.max(1),
      height: height.max(1),
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: UI_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  });
  let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
  (texture, view)
}

/// A viewport uniform buffer holding `transform`, blending the UI if `ui`.
fn viewport_buffer(
  device: &wgpu::Device,
  label: &str,
  transform: [f32; 4],
  encode_srgb: f32,
  ui: bool,
) -> wgpu::Buffer {
  let uniform = ViewportUniform {
    transform,
    flags: [encode_srgb, if ui { 1.0 } else { 0.0 }, 0.0, 0.0],
  };
  device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some(label),
    contents: bytemuck::cast_slice(&[uniform]),
    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
  })
}

impl Compositor {
  /// Create the composite resources for a `target_format` target and a UI
  /// image of `width` x `height`.
  pub(crate) fn new(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
  ) -> Self {
    let (ui_texture, ui_texture_view) = create_ui_texture(device, width, height);
    // New textures start zeroed: transparent
    let (_, blank_view) = create_ui_texture(device, 1, 1);

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

    let encode_srgb = if target_format.is_srgb() { 0.0 } else { 1.0 };
    let full = [1.0, 1.0, 0.0, 0.0];
    let letterboxed = [false, true]
      .map(|ui| viewport_buffer(device, "Viewport Uniform Buffer", full, encode_srgb, ui));
    let identity = [false, true]
      .map(|ui| viewport_buffer(device, "Identity Uniform Buffer", full, encode_srgb, ui));

    let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
      binding,
      visibility: wgpu::ShaderStages::FRAGMENT,
      ty: wgpu::BindingType::Texture {
        multisampled: false,
        view_dimension: wgpu::TextureViewDimension::D2,
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
      },
      count: None,
    };
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Composite Bind Group Layout"),
      entries: &[
        texture_entry(0),
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
        // The UI image
        texture_entry(3),
      ],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Fullscreen Quad Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::COMPOSITE.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Render Pipeline Layout"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    // REPLACE: the shader blends the UI itself, in sRGB space
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

    Self {
      bind_groups: BindGroupCache::default(),
      encode_srgb,
      render_pipeline,
      bind_group_layout,
      sampler,
      ui_texture,
      ui_texture_view,
      ui_epoch: next_ui_epoch(),
      blank_view,
      letterboxed,
      identity,
    }
  }

  /// The UI image's size in pixels.
  pub(crate) fn ui_size(&self) -> (u32, u32) {
    (self.ui_texture.width(), self.ui_texture.height())
  }

  /// The UI image, for the `ui` pass to draw into.
  pub(crate) fn ui_view(&self) -> &wgpu::TextureView {
    &self.ui_texture_view
  }

  /// Changes whenever the UI image is created anew (it is then blank)
  pub(crate) fn ui_epoch(&self) -> u64 {
    self.ui_epoch
  }

  /// Recreate the UI image at `width` x `height`.
  pub(crate) fn resize_ui(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    (self.ui_texture, self.ui_texture_view) = create_ui_texture(device, width, height);
    self.ui_epoch = next_ui_epoch();
    self.bind_groups.clear();
  }

  /// The scene images were recreated: forget the bind groups sampling them.
  pub(crate) fn forget_sources(&self) {
    self.bind_groups.clear();
  }

  /// Set the letterbox transform: `[scale_x, scale_y, offset_x, offset_y]`.
  pub(crate) fn set_viewport_transform(&self, queue: &wgpu::Queue, transform: [f32; 4]) {
    for (buffer, ui) in self.letterboxed.iter().zip([0.0, 1.0]) {
      let uniform = ViewportUniform {
        transform,
        flags: [self.encode_srgb, ui, 0.0, 0.0],
      };
      queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[uniform]));
    }
  }

  /// Draw the scene `source`, with the UI image blended over it when
  /// `include_ui`, into `target`.
  pub(crate) fn draw(
    &self,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    source: &wgpu::TextureView,
    include_ui: bool,
    viewport: CompositeViewport,
  ) {
    let buffers = match viewport {
      CompositeViewport::Letterboxed => &self.letterboxed,
      CompositeViewport::Identity => &self.identity,
    };
    let uniform = &buffers[usize::from(include_ui)];
    let ui = if include_ui {
      &self.ui_texture_view
    } else {
      &self.blank_view
    };
    let bind_group = self.bind_group(device, source, ui, uniform);

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
    render_pass.set_bind_group(0, &bind_group, &[]);
    render_pass.draw(0..6, 0..1);
  }

  /// Bind group sampling `source` and the `ui` image with a viewport uniform.
  fn bind_group(
    &self,
    device: &wgpu::Device,
    source: &wgpu::TextureView,
    ui: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
  ) -> wgpu::BindGroup {
    self.bind_groups.get(
      device,
      "Composite Bind Group",
      &self.bind_group_layout,
      &[
        Bound::View(source.clone()),
        Bound::Sampler(self.sampler.clone()),
        Bound::Buffer(uniform.clone()),
        Bound::View(ui.clone()),
      ],
    )
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

  /// The UI is drawn on the GPU: nothing writes a whole UI texture per frame
  #[test]
  fn the_compositor_uploads_no_pixels() {
    let source = include_str!("compositor.rs");
    let code = source.split("#[cfg(test)]").next().unwrap_or(source);
    // The whole non-test code is scanned (no earlier test-only item ends it)
    assert!(
      code.contains("fn bind_group("),
      "scanned only part of the code"
    );
    assert!(!code.contains("write_texture"));
  }
}
