//! Lighting renderer that manages the light accumulation buffer
//!
//! This renderer collects all lights in the scene and renders them
//! to an accumulation buffer, which is then used to light the scene.

use super::{AmbientLight, Light2D};
use crate::core::ecs::{Transform, Vec2};
use wgpu::util::DeviceExt;

/// Maximum number of lights that can be rendered per frame
pub const MAX_LIGHTS: usize = 256;

/// Per-light GPU data (must match the shader struct)
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct LightInstance {
  /// Position (x, y), radius (z), type (w: 0=point, 1=spot)
  position_radius_type: [f32; 4],
  /// Color (RGB) and intensity (A)
  color_intensity: [f32; 4],
  /// Falloff (x), angle (y), direction (z), softness (w)
  params: [f32; 4],
}

/// Global uniforms for light rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct LightGlobals {
  /// View-projection matrix
  view_proj: [[f32; 4]; 4],
  /// Resolution (x, y) and time (z), padding (w)
  resolution_time: [f32; 4],
}

/// Uniforms for the lighting composition pass
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingParams {
  /// Ambient color (RGB) + unused (A)
  pub color: [f32; 4],
  /// Ambient intensity (x), unused (y, z, w)
  pub params: [f32; 4],
  /// Time (x), width (y), height (z), unused (w)
  pub time_resolution: [f32; 4],
  /// Padding for alignment
  pub _padding: [f32; 4],
}

/// The lighting renderer manages GPU resources for 2D lighting.
pub struct LightingRenderer {
  /// Light accumulation texture
  light_texture: wgpu::Texture,
  light_texture_view: wgpu::TextureView,

  /// Sampler for textures
  sampler: wgpu::Sampler,

  /// Pipeline for rendering lights to accumulation buffer
  light_pipeline: wgpu::RenderPipeline,

  /// Pipeline for compositing lights with scene
  compose_pipeline: wgpu::RenderPipeline,

  /// Storage buffer for light instances
  light_buffer: wgpu::Buffer,

  /// Uniform buffer for light globals
  globals_buffer: wgpu::Buffer,

  /// Uniform buffer for composition pass
  compose_uniform_buffer: wgpu::Buffer,

  /// Bind group layout for light rendering
  light_bind_group_layout: wgpu::BindGroupLayout,

  /// Bind group layout for composition
  compose_bind_group_layout: wgpu::BindGroupLayout,

  /// Current ambient light settings
  ambient: AmbientLight,

  /// Dimensions
  width: u32,
  height: u32,

  /// Whether lighting is enabled
  enabled: bool,
}

impl LightingRenderer {
  /// Create a new lighting renderer.
  pub fn new(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    output_format: wgpu::TextureFormat,
  ) -> Self {
    // Create light accumulation texture (additive blending target)
    let light_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Light Accumulation Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba16Float, // HDR-capable
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    let light_texture_view = light_texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Lighting Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create light storage buffer
    let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Light Instance Buffer"),
      size: (std::mem::size_of::<LightInstance>() * MAX_LIGHTS) as u64,
      usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    // Create globals uniform buffer
    let globals = LightGlobals {
      view_proj: [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
      ],
      resolution_time: [width as f32, height as f32, 0.0, 0.0],
    };
    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Light Globals Buffer"),
      contents: bytemuck::cast_slice(&[globals]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create compose uniform buffer
    let compose_uniform = LightingParams {
      color: [0.1, 0.1, 0.12, 1.0],
      params: [0.15, 0.0, 0.0, 0.0],
      time_resolution: [0.0, width as f32, height as f32, 0.0],
      _padding: [0.0; 4],
    };
    let compose_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Lighting Compose Uniform Buffer"),
      contents: bytemuck::cast_slice(&[compose_uniform]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create bind group layouts
    let light_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Light Bind Group Layout"),
        entries: &[
          // Globals uniform
          wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          // Light instances storage
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Storage { read_only: true },
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
        ],
      });

    let compose_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Lighting Compose Bind Group Layout"),
        entries: &[
          // Scene texture
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
          // Light texture
          wgpu::BindGroupLayoutEntry {
            binding: 1,
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
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
          },
          // Uniforms
          wgpu::BindGroupLayoutEntry {
            binding: 3,
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

    // Create light rendering pipeline
    let light_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Light Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/light.wgsl").into()),
    });

    let light_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Light Pipeline Layout"),
      bind_group_layouts: &[&light_bind_group_layout],
      push_constant_ranges: &[],
    });

    let light_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Light Pipeline"),
      layout: Some(&light_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &light_shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &light_shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba16Float,
          // Additive blending for light accumulation
          blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::One,
              dst_factor: wgpu::BlendFactor::One,
              operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::One,
              dst_factor: wgpu::BlendFactor::One,
              operation: wgpu::BlendOperation::Add,
            },
          }),
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

    // Create composition pipeline
    let compose_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Lighting Compose Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/post-process/lighting.wgsl").into()),
    });

    let compose_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Lighting Compose Pipeline Layout"),
      bind_group_layouts: &[&compose_bind_group_layout],
      push_constant_ranges: &[],
    });

    let compose_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Lighting Compose Pipeline"),
      layout: Some(&compose_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &compose_shader,
        entry_point: "vs_main",
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &compose_shader,
        entry_point: "fs_main",
        targets: &[Some(wgpu::ColorTargetState {
          format: output_format,
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

    Self {
      light_texture,
      light_texture_view,
      sampler,
      light_pipeline,
      compose_pipeline,
      light_buffer,
      globals_buffer,
      compose_uniform_buffer,
      light_bind_group_layout,
      compose_bind_group_layout,
      ambient: AmbientLight::default(),
      width,
      height,
      enabled: true,
    }
  }

  /// Set the ambient light.
  pub fn set_ambient(&mut self, ambient: AmbientLight) {
    self.ambient = ambient;
  }

  /// Get the current ambient light.
  pub fn ambient(&self) -> &AmbientLight {
    &self.ambient
  }

  /// Get a mutable reference to the ambient light.
  pub fn ambient_mut(&mut self) -> &mut AmbientLight {
    &mut self.ambient
  }

  /// Enable or disable the lighting system.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Check if lighting is enabled.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Resize the light accumulation texture.
  pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
    if width == self.width && height == self.height {
      return;
    }

    self.width = width;
    self.height = height;

    self.light_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Light Accumulation Texture"),
      size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba16Float,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats: &[],
    });
    self.light_texture_view = self
      .light_texture
      .create_view(&wgpu::TextureViewDescriptor::default());
  }

  /// Render lights to the accumulation buffer.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `lights` - Iterator of (position, light) pairs
  /// * `camera_pos` - Camera position for view matrix
  /// * `camera_zoom` - Camera zoom level
  pub fn render_lights<'a>(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    lights: impl Iterator<Item = (&'a Transform, &'a Light2D)>,
    camera_pos: Vec2,
    camera_zoom: f32,
  ) {
    if !self.enabled {
      return;
    }

    // Collect light instances
    let mut instances: Vec<LightInstance> = Vec::with_capacity(MAX_LIGHTS);

    for (transform, light) in lights {
      if instances.len() >= MAX_LIGHTS {
        break;
      }

      let pos_x = transform.x;
      let pos_y = transform.y;

      let instance = match light {
        Light2D::Point {
          color,
          radius,
          intensity,
          falloff,
          ..
        } => LightInstance {
          position_radius_type: [pos_x, pos_y, *radius, 0.0],
          color_intensity: [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            *intensity,
          ],
          params: [*falloff, 0.0, 0.0, 0.0],
        },
        Light2D::Spot {
          color,
          radius,
          intensity,
          falloff,
          angle,
          direction,
          softness,
          ..
        } => LightInstance {
          position_radius_type: [pos_x, pos_y, *radius, 1.0],
          color_intensity: [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            *intensity,
          ],
          params: [*falloff, *angle, *direction, *softness],
        },
      };

      instances.push(instance);
    }

    // Upload light data
    if !instances.is_empty() {
      queue.write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&instances));
    }

    // Update globals with camera transform
    let half_w = self.width as f32 / 2.0;
    let half_h = self.height as f32 / 2.0;

    // Create orthographic projection with camera offset
    // This transforms world coordinates to clip space (-1 to 1)
    let scale_x = camera_zoom / half_w;
    let scale_y = -camera_zoom / half_h; // Flip Y
    let offset_x = -camera_pos.x * scale_x;
    let offset_y = -camera_pos.y * scale_y;

    let view_proj = [
      [scale_x, 0.0, 0.0, 0.0],
      [0.0, scale_y, 0.0, 0.0],
      [0.0, 0.0, 1.0, 0.0],
      [offset_x, offset_y, 0.0, 1.0],
    ];

    let globals = LightGlobals {
      view_proj,
      resolution_time: [self.width as f32, self.height as f32, 0.0, 0.0],
    };
    queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&[globals]));

    // Create bind group for light rendering
    let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Light Bind Group"),
      layout: &self.light_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: self.globals_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: self.light_buffer.as_entire_binding(),
        },
      ],
    });

    // Clear and render lights to accumulation buffer
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Light Accumulation Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.light_texture_view,
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

      if !instances.is_empty() {
        render_pass.set_pipeline(&self.light_pipeline);
        render_pass.set_bind_group(0, &light_bind_group, &[]);
        // 6 vertices per light quad, N instances
        render_pass.draw(0..6, 0..instances.len() as u32);
      }
    }
  }

  /// Compose the lit scene by combining the scene texture with the light buffer.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `scene_view` - The scene texture view (input)
  /// * `output_view` - The output texture view
  /// * `time` - Current time for effects
  pub fn compose(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    scene_view: &wgpu::TextureView,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    if !self.enabled {
      return;
    }

    // Update compose uniforms
    let compose_uniform = LightingParams {
      color: [
        self.ambient.color.r as f32 / 255.0,
        self.ambient.color.g as f32 / 255.0,
        self.ambient.color.b as f32 / 255.0,
        1.0,
      ],
      params: [self.ambient.intensity, 0.0, 0.0, 0.0],
      time_resolution: [time, self.width as f32, self.height as f32, 0.0],
      _padding: [0.0; 4],
    };
    queue.write_buffer(
      &self.compose_uniform_buffer,
      0,
      bytemuck::cast_slice(&[compose_uniform]),
    );

    // Create compose bind group
    let compose_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Lighting Compose Bind Group"),
      layout: &self.compose_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(scene_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::TextureView(&self.light_texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: wgpu::BindingResource::Sampler(&self.sampler),
        },
        wgpu::BindGroupEntry {
          binding: 3,
          resource: self.compose_uniform_buffer.as_entire_binding(),
        },
      ],
    });

    // Render composition
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Lighting Compose Pass"),
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

      render_pass.set_pipeline(&self.compose_pipeline);
      render_pass.set_bind_group(0, &compose_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }
  }

  /// Get the light texture view (for debugging or custom effects).
  pub fn light_texture_view(&self) -> &wgpu::TextureView {
    &self.light_texture_view
  }
}
