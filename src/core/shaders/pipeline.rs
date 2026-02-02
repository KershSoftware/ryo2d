//! Post-processing GPU pipeline
//!
//! Manages GPU resources for rendering post-processing effects.
//! Uses ping-pong buffers for effect chaining.

use super::effects::{EffectParams, PostProcessEffect};
use super::post_process_shaders;
use wgpu::util::DeviceExt;

/// Which buffer contains the effect output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputBuffer {
  A,
  B,
}

/// GPU pipeline for post-processing effects
pub struct PostProcessPipeline {
  // Ping-pong textures for effect chaining
  texture_a: wgpu::Texture,
  texture_b: wgpu::Texture,
  view_a: wgpu::TextureView,
  view_b: wgpu::TextureView,

  // Sampler for texture sampling
  sampler: wgpu::Sampler,

  // Effect pipelines (created on first use)
  fade_pipeline: Option<EffectPipeline>,
  color_tint_pipeline: Option<EffectPipeline>,
  vignette_pipeline: Option<EffectPipeline>,
  screen_flash_pipeline: Option<EffectPipeline>,
  crt_pipeline: Option<EffectPipeline>,
  chromatic_aberration_pipeline: Option<EffectPipeline>,

  // Uniform buffer for effect parameters
  uniform_buffer: wgpu::Buffer,

  // Bind group layout shared by all effects
  bind_group_layout: wgpu::BindGroupLayout,

  // Dimensions
  width: u32,
  height: u32,
  format: wgpu::TextureFormat,
}

/// A single effect's GPU resources
struct EffectPipeline {
  pipeline: wgpu::RenderPipeline,
}

impl PostProcessPipeline {
  /// Create a new post-process pipeline
  pub fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
    // Create ping-pong textures
    let texture_desc = wgpu::TextureDescriptor {
      label: Some("Post-Process Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC, // Allow copying between ping-pong buffers
      view_formats: &[],
    };

    let texture_a = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture A"),
      ..texture_desc
    });
    let texture_b = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture B"),
      ..texture_desc
    });

    let view_a = texture_a.create_view(&wgpu::TextureViewDescriptor::default());
    let view_b = texture_b.create_view(&wgpu::TextureViewDescriptor::default());

    // Create sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Post-Process Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create uniform buffer
    let uniform = EffectParams {
      color: [0.0, 0.0, 0.0, 1.0],
      params: [0.0; 4],
      time_resolution: [0.0, width as f32, height as f32, 0.0],
      _padding: [0.0; 4],
    };
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Post-Process Uniform Buffer"),
      contents: bytemuck::cast_slice(&[uniform]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create bind group layout (shared by all effects)
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Post-Process Bind Group Layout"),
      entries: &[
        // Input texture
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
        // Sampler
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        // Uniform buffer
        wgpu::BindGroupLayoutEntry {
          binding: 2,
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

    Self {
      texture_a,
      texture_b,
      view_a,
      view_b,
      sampler,
      fade_pipeline: None,
      color_tint_pipeline: None,
      vignette_pipeline: None,
      screen_flash_pipeline: None,
      crt_pipeline: None,
      chromatic_aberration_pipeline: None,
      uniform_buffer,
      bind_group_layout,
      width,
      height,
      format,
    }
  }

  /// Resize the ping-pong textures
  pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    if width == self.width && height == self.height {
      return;
    }

    self.width = width;
    self.height = height;

    let texture_desc = wgpu::TextureDescriptor {
      label: Some("Post-Process Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: self.format,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST,
      view_formats: &[],
    };

    self.texture_a = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture A"),
      ..texture_desc
    });
    self.texture_b = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Post-Process Texture B"),
      ..texture_desc
    });

    self.view_a = self
      .texture_a
      .create_view(&wgpu::TextureViewDescriptor::default());
    self.view_b = self
      .texture_b
      .create_view(&wgpu::TextureViewDescriptor::default());
  }

  /// Apply a chain of effects to the source in texture A.
  ///
  /// The source must already be copied to texture A before calling this.
  /// Returns which buffer contains the final result.
  pub fn apply_effects(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    effects: &[(&PostProcessEffect, f32)], // (effect, time)
  ) -> OutputBuffer {
    if effects.is_empty() {
      return OutputBuffer::A;
    }

    // Pre-create all needed pipelines to avoid borrow conflicts
    for (effect, _) in effects {
      self.ensure_pipeline_exists(device, effect);
    }

    let mut use_a_as_input = true;

    for (effect, time) in effects {
      // Update uniform buffer
      let params = EffectParams::from_effect(effect, *time, self.width, self.height);
      queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[params]));

      // Get the pipeline (guaranteed to exist after ensure_pipeline_exists)
      let pipeline = self.get_pipeline(effect);

      // Create bind group for this pass
      let (input_view, output_view) = if use_a_as_input {
        (&self.view_a, &self.view_b)
      } else {
        (&self.view_b, &self.view_a)
      };

      let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Post-Process Bind Group"),
        layout: &self.bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(input_view),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
          wgpu::BindGroupEntry {
            binding: 2,
            resource: self.uniform_buffer.as_entire_binding(),
          },
        ],
      });

      // Render pass
      {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
          label: Some("Post-Process Pass"),
          color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output_view,
            resolve_target: None,
            ops: wgpu::Operations {
              load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
              store: wgpu::StoreOp::Store,
            },
          })],
          depth_stencil_attachment: None,
          occlusion_query_set: None,
          timestamp_writes: None,
        });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..6, 0..1);
      }

      // Swap for next iteration
      use_a_as_input = !use_a_as_input;
    }

    // After all effects, use_a_as_input indicates what the NEXT input would be
    // If use_a_as_input is FALSE, last write was to B (we just flipped from true)
    // If use_a_as_input is TRUE, last write was to A (we just flipped from false)
    // So the output is the OPPOSITE of use_a_as_input
    if use_a_as_input {
      OutputBuffer::A
    } else {
      OutputBuffer::B
    }
  }

  /// Get the view for a specific output buffer
  pub fn get_output_view(&self, buffer: OutputBuffer) -> &wgpu::TextureView {
    match buffer {
      OutputBuffer::A => &self.view_a,
      OutputBuffer::B => &self.view_b,
    }
  }

  /// Ensure the pipeline for an effect exists (creates it if needed)
  fn ensure_pipeline_exists(&mut self, device: &wgpu::Device, effect: &PostProcessEffect) {
    match effect {
      PostProcessEffect::Fade { .. } => {
        if self.fade_pipeline.is_none() {
          self.fade_pipeline =
            Some(self.create_effect_pipeline(device, "Fade", post_process_shaders::FADE));
        }
      }
      PostProcessEffect::ColorTint { .. } => {
        if self.color_tint_pipeline.is_none() {
          self.color_tint_pipeline = Some(self.create_effect_pipeline(
            device,
            "Color Tint",
            post_process_shaders::COLOR_TINT,
          ));
        }
      }
      PostProcessEffect::Vignette { .. } => {
        if self.vignette_pipeline.is_none() {
          self.vignette_pipeline =
            Some(self.create_effect_pipeline(device, "Vignette", post_process_shaders::VIGNETTE));
        }
      }
      PostProcessEffect::ScreenFlash { .. } => {
        if self.screen_flash_pipeline.is_none() {
          self.screen_flash_pipeline = Some(self.create_effect_pipeline(
            device,
            "Screen Flash",
            post_process_shaders::SCREEN_FLASH,
          ));
        }
      }
      PostProcessEffect::Crt { .. } => {
        if self.crt_pipeline.is_none() {
          self.crt_pipeline =
            Some(self.create_effect_pipeline(device, "CRT", post_process_shaders::CRT));
        }
      }
      PostProcessEffect::ChromaticAberration { .. } => {
        if self.chromatic_aberration_pipeline.is_none() {
          self.chromatic_aberration_pipeline = Some(self.create_effect_pipeline(
            device,
            "Chromatic Aberration",
            post_process_shaders::CHROMATIC_ABERRATION,
          ));
        }
      }
    }
  }

  /// Get the pipeline for an effect (must already exist)
  fn get_pipeline(&self, effect: &PostProcessEffect) -> &wgpu::RenderPipeline {
    match effect {
      PostProcessEffect::Fade { .. } => &self.fade_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ColorTint { .. } => &self.color_tint_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::Vignette { .. } => &self.vignette_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ScreenFlash { .. } => {
        &self.screen_flash_pipeline.as_ref().unwrap().pipeline
      }
      PostProcessEffect::Crt { .. } => &self.crt_pipeline.as_ref().unwrap().pipeline,
      PostProcessEffect::ChromaticAberration { .. } => {
        &self
          .chromatic_aberration_pipeline
          .as_ref()
          .unwrap()
          .pipeline
      }
    }
  }

  /// Create a render pipeline for an effect shader
  fn create_effect_pipeline(
    &self,
    device: &wgpu::Device,
    name: &str,
    shader_source: &str,
  ) -> EffectPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some(&format!("{} Shader", name)),
      source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some(&format!("{} Pipeline Layout", name)),
      bind_group_layouts: &[&self.bind_group_layout],
      push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some(&format!("{} Pipeline", name)),
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
          format: self.format,
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
      multiview: None,
      cache: None,
    });

    EffectPipeline { pipeline }
  }

  /// Get texture A view (for copying source to)
  pub fn texture_a_view(&self) -> &wgpu::TextureView {
    &self.view_a
  }

  /// Get texture A (for copying source to)
  pub fn texture_a(&self) -> &wgpu::Texture {
    &self.texture_a
  }

  /// Get texture B view (for lighting output)
  pub fn texture_b_view(&self) -> &wgpu::TextureView {
    &self.view_b
  }

  /// Get texture B (for lighting output)
  pub fn texture_b(&self) -> &wgpu::Texture {
    &self.texture_b
  }
}
