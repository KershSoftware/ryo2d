//! The scene transition fade

use wgpu::util::DeviceExt;

use crate::core::render::context::image_texture;
use crate::core::render::{FrameInfo, RenderContext, RenderPass, pass_ids};

/// The pipeline for one target format and size, the overlay it reads, and the
/// copy of the frame it mixes over.
struct FadePipeline {
  format: wgpu::TextureFormat,
  size: (u32, u32),
  pipeline: wgpu::RenderPipeline,
  overlay: wgpu::Buffer,
  copy: wgpu::Texture,
  bind_group: wgpu::BindGroup,
}

impl FadePipeline {
  fn new(device: &wgpu::Device, format: wgpu::TextureFormat, (width, height): (u32, u32)) -> Self {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Transition Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/core/transition.wgsl").into()),
    });
    let overlay = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Transition Overlay"),
      contents: bytemuck::cast_slice(&[0.0_f32; 8]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let copy = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Transition Frame Copy"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Transition Bind Group Layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
      ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Transition Bind Group"),
      layout: &layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: overlay.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::TextureView(
            &copy.create_view(&wgpu::TextureViewDescriptor::default()),
          ),
        },
      ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Transition Pipeline Layout"),
      bind_group_layouts: &[Some(&layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Transition Pipeline"),
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
        // The shader mixes with the copy itself, so it replaces the frame
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
    Self {
      format,
      size: (width, height),
      pipeline,
      overlay,
      copy,
      bind_group,
    }
  }
}

/// Fades the whole frame (sprites, trails and lines, lighting) toward the
/// scene transition's colour. The UI is composited afterwards, above it.
#[derive(Default)]
pub(super) struct TransitionPass {
  fade: Option<FadePipeline>,
}

impl RenderPass for TransitionPass {
  fn id(&self) -> &str {
    pass_ids::TRANSITION
  }

  fn should_run(&self, frame: &FrameInfo) -> bool {
    frame.transition
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let ([r, g, b], alpha) = ctx.scene.overlay;
    let (device, queue, format) = (ctx.device(), ctx.queue(), ctx.format());
    let (width, height) = ctx.render_size();
    let fade = match &mut self.fade {
      Some(fade) if fade.format == format && fade.size == (width, height) => fade,
      slot => slot.insert(FadePipeline::new(device, format, (width, height))),
    };
    let overlay = [
      f32::from(r) / 255.0,
      f32::from(g) / 255.0,
      f32::from(b) / 255.0,
      alpha.clamp(0.0, 1.0),
      if format.is_srgb() { 1.0 } else { 0.0 },
      0.0,
      0.0,
      0.0,
    ];
    queue.write_buffer(&fade.overlay, 0, bytemuck::cast_slice(&overlay));

    let RenderContext {
      encoder,
      targets,
      post_process,
      image,
      ..
    } = ctx;
    let source = image_texture(targets, post_process.as_deref(), *image);
    encoder.copy_texture_to_texture(
      source.as_image_copy(),
      fade.copy.as_image_copy(),
      wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
    );

    let (encoder, view) = ctx.encoder_and_current_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Transition Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load,
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
      multiview_mask: None,
    });
    pass.set_pipeline(&fade.pipeline);
    pass.set_bind_group(0, &fade.bind_group, &[]);
    pass.draw(0..3, 0..1);
  }
}
