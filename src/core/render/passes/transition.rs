//! The scene transition fade

use wgpu::util::DeviceExt;

use crate::core::render::{FrameInfo, RenderContext, RenderPass, pass_ids};

/// The pipeline for one target format, and the overlay colour it reads.
struct FadePipeline {
  format: wgpu::TextureFormat,
  pipeline: wgpu::RenderPipeline,
  colour: wgpu::Buffer,
  bind_group: wgpu::BindGroup,
}

impl FadePipeline {
  fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Transition Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/core/transition.wgsl").into()),
    });
    let colour = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Transition Colour"),
      contents: bytemuck::cast_slice(&[0.0_f32; 4]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Transition Bind Group Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Transition Bind Group"),
      layout: &layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: colour.as_entire_binding(),
      }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Transition Pipeline Layout"),
      bind_group_layouts: &[&layout],
      push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Transition Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });
    Self {
      format,
      pipeline,
      colour,
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
    !frame.raycast && frame.transition
  }

  fn execute(&mut self, ctx: &mut RenderContext<'_>) {
    let ([r, g, b], alpha) = ctx.scene.overlay;
    let (device, queue, format) = (ctx.device(), ctx.queue(), ctx.format());
    let fade = match &mut self.fade {
      Some(fade) if fade.format == format => fade,
      slot => slot.insert(FadePipeline::new(device, format)),
    };
    let colour = [
      f32::from(r) / 255.0,
      f32::from(g) / 255.0,
      f32::from(b) / 255.0,
      alpha.clamp(0.0, 1.0),
    ];
    queue.write_buffer(&fade.colour, 0, bytemuck::cast_slice(&colour));

    let (encoder, view) = ctx.encoder_and_current_view();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Transition Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load,
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
    });
    pass.set_pipeline(&fade.pipeline);
    pass.set_bind_group(0, &fade.bind_group, &[]);
    pass.draw(0..3, 0..1);
  }
}
