//! The look: the scene image tonemapped into the display image

use crate::core::graphics::bind_groups::{BindGroupCache, Bound};
use crate::core::render::context::image_view;
use crate::core::render::look::LookUniforms;
use crate::core::render::{FrameImage, RenderContext, RenderPass, pass_ids};
use crate::core::shaders::OutputBuffer;

/// The tonemap pipeline for one display format and render size, and the
/// bind groups of the scene images it has read.
struct TonemapGpu {
  format: wgpu::TextureFormat,
  size: (u32, u32),
  layout: wgpu::BindGroupLayout,
  uniforms: wgpu::Buffer,
  pipeline: wgpu::RenderPipeline,
  bind_groups: BindGroupCache,
}

impl TonemapGpu {
  fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Tonemap Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Tonemap Shader"),
      source: wgpu::ShaderSource::Wgsl(crate::core::shaders::compose::TONEMAP.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Tonemap Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Tonemap Pipeline"),
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
          format,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview_mask: None,
      cache: None,
    });
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Tonemap Uniforms"),
      size: std::mem::size_of::<LookUniforms>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    Self {
      format,
      size,
      layout,
      uniforms,
      pipeline,
      bind_groups: BindGroupCache::default(),
    }
  }
}

/// Writes the display image (the display stage's buffer `A`) from the
/// latest scene image; the transition, display effects, UI and composite
/// work on it.
#[derive(Default)]
pub(super) struct Tonemap {
  gpu: Option<TonemapGpu>,
}

impl RenderPass for Tonemap {
  fn id(&self) -> &str {
    pass_ids::TONEMAP
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    // Already the display image: nothing to map (and a pass can't read the
    // image it writes)
    if matches!(ctx.image, FrameImage::Display(_)) {
      return;
    }
    let device = ctx.device();
    let size = ctx.render_size();
    let format = ctx.stages.display.texture_a().format();
    // A resize recreates the images the cached bind groups point at
    let gpu = match &mut self.gpu {
      Some(gpu) if gpu.format == format && gpu.size == size => gpu,
      slot => slot.insert(TonemapGpu::new(device, format, size)),
    };
    let queue = ctx.queue();
    queue.write_buffer(
      &gpu.uniforms,
      0,
      bytemuck::bytes_of(&ctx.scene.look.uniforms()),
    );
    let RenderContext {
      encoder,
      targets,
      stages,
      image,
      ..
    } = ctx;
    let source = image_view(targets, stages, *image).clone();
    let group = gpu.bind_groups.get(
      device,
      "Tonemap Bind Group",
      &gpu.layout,
      &[Bound::View(source), Bound::Buffer(gpu.uniforms.clone())],
    );
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Tonemap Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: stages.display.get_output_view(OutputBuffer::A),
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&gpu.pipeline);
      pass.set_bind_group(0, &group, &[]);
      pass.draw(0..6, 0..1);
    }
    *image = FrameImage::Display(OutputBuffer::A);
  }
}
