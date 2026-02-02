//! GPU-based sprite rendering with per-sprite shader effects
//!
//! This module provides a GPU rendering path for sprites that have `Material`
//! components or inline shaders. Unlike the CPU software renderer, this path:
//!
//! - Renders sprites as textured quads on the GPU
//! - Applies per-sprite shader effects (outline, glow, dissolve, etc.)
//! - Supports custom user shaders via `ShaderHandle`
//! - Can batch sprites with the same shader for performance
//!
//! # Architecture
//!
//! ```text
//! Engine render loop
//!     │
//!     ├── CPU path: sprites without shaders
//!     │   └── draw to pixel buffer → upload texture → fullscreen quad
//!     │
//!     └── GPU path: sprites with Material/shader
//!         └── SpriteGpuRenderer::render()
//!             ├── Upload sprite textures (cached)
//!             ├── Build instance buffer (transforms, UVs, effect params)
//!             └── Draw calls per shader type
//! ```
//!
//! # Usage
//!
//! The GPU renderer is automatically used for entities that have:
//! - A `Renderable` with `.shader` set, OR
//! - A `Material` component
//!
//! CPU rendering is used as fallback for sprites without shaders.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::core::ecs::{Material, SpriteEffect, SpriteShader};
use crate::core::ui::Color;

/// Per-instance data for GPU sprite rendering
///
/// This struct is passed to the shader for each sprite instance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
  /// Transform matrix (2D affine: scale, rotation, translation)
  /// Row-major 3x3 packed into 3 vec4s (last component unused)
  pub transform: [[f32; 4]; 3],
  /// UV rect: x=u_min, y=v_min, z=u_max, w=v_max
  pub uv_rect: [f32; 4],
  /// Tint color (RGBA, pre-multiplied alpha)
  pub tint: [f32; 4],
  /// Effect parameters (meaning depends on shader)
  /// x=effect_type, y,z,w=effect-specific params
  pub effect_params: [f32; 4],
  /// Additional effect parameters
  /// Used for effects that need more than 4 floats
  pub effect_extra: [f32; 4],
}

impl SpriteInstance {
  /// Create a basic instance with identity transform and full UV
  pub fn new() -> Self {
    Self {
      transform: [
        [1.0, 0.0, 0.0, 0.0], // row 0: scale_x, shear_x, translate_x
        [0.0, 1.0, 0.0, 0.0], // row 1: shear_y, scale_y, translate_y
        [0.0, 0.0, 1.0, 0.0], // row 2: (unused in 2D)
      ],
      uv_rect: [0.0, 0.0, 1.0, 1.0],
      tint: [1.0, 1.0, 1.0, 1.0],
      effect_params: [0.0; 4],
      effect_extra: [0.0; 4],
    }
  }

  /// Set transform from position, rotation (radians), and scale
  pub fn with_transform(
    mut self,
    x: f32,
    y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
  ) -> Self {
    let cos = rotation.cos();
    let sin = rotation.sin();

    // 2D affine transform matrix (column-major for WGSL mat3x3)
    // WGSL mat3x3 constructor takes column vectors:
    // Column 0: X-axis transform [scale_x * cos, scale_x * sin, 0]
    // Column 1: Y-axis transform [-scale_y * sin, scale_y * cos, 0]
    // Column 2: Translation [x, y, 1]
    self.transform[0] = [scale_x * cos, scale_x * sin, 0.0, 0.0];
    self.transform[1] = [-scale_y * sin, scale_y * cos, 0.0, 0.0];
    self.transform[2] = [x, y, 1.0, 0.0];

    self
  }

  /// Set UV rectangle (for sprite sheets/atlases)
  pub fn with_uv(mut self, u_min: f32, v_min: f32, u_max: f32, v_max: f32) -> Self {
    self.uv_rect = [u_min, v_min, u_max, v_max];
    self
  }

  /// Set tint color with alpha
  pub fn with_tint(mut self, color: Color, alpha: f32) -> Self {
    let a = (color.a as f32 / 255.0) * alpha;
    self.tint = [
      color.r as f32 / 255.0,
      color.g as f32 / 255.0,
      color.b as f32 / 255.0,
      a,
    ];
    self
  }

  /// Set effect parameters from a SpriteEffect
  pub fn with_effect(mut self, effect: &SpriteEffect) -> Self {
    match effect {
      SpriteEffect::Outline { color, thickness } => {
        self.effect_params = [1.0, *thickness as f32, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Glow {
        color,
        radius,
        intensity,
      } => {
        self.effect_params = [2.0, *radius, *intensity, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Flash { color, intensity } => {
        self.effect_params = [3.0, *intensity, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::ColorShift { color, amount } => {
        self.effect_params = [4.0, *amount, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Silhouette { color } => {
        self.effect_params = [5.0, 0.0, 0.0, 0.0];
        self.effect_extra = [
          color.r as f32 / 255.0,
          color.g as f32 / 255.0,
          color.b as f32 / 255.0,
          color.a as f32 / 255.0,
        ];
      }
      SpriteEffect::Pixelate { block_size } => {
        self.effect_params = [6.0, *block_size as f32, 0.0, 0.0];
      }
      SpriteEffect::Wave {
        amplitude,
        frequency,
        speed,
        time,
      } => {
        self.effect_params = [7.0, *amplitude, *frequency, *time];
        self.effect_extra = [*speed, 0.0, 0.0, 0.0];
      }
      SpriteEffect::Dissolve {
        progress,
        edge_color,
      } => {
        self.effect_params = [8.0, *progress, 0.0, 0.0];
        if let Some(color) = edge_color {
          self.effect_extra = [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
          ];
        }
      }
      SpriteEffect::Invert => {
        self.effect_params = [9.0, 0.0, 0.0, 0.0];
      }
      SpriteEffect::Grayscale { intensity } => {
        self.effect_params = [10.0, *intensity, 0.0, 0.0];
      }
    }
    self
  }
}

impl Default for SpriteInstance {
  fn default() -> Self {
    Self::new()
  }
}

/// Global uniforms for sprite rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteGlobals {
  /// View-projection matrix (orthographic 2D)
  pub view_proj: [[f32; 4]; 4],
  /// Time and resolution: x=time, y=width, z=height, w=unused
  pub time_resolution: [f32; 4],
}

impl SpriteGlobals {
  /// Create globals for orthographic 2D rendering
  pub fn new(width: f32, height: f32, time: f32) -> Self {
    // Orthographic projection: maps (0,0)-(width,height) to clip space (-1,-1)-(1,1)
    // Y is flipped so 0 is at top
    Self {
      view_proj: [
        [2.0 / width, 0.0, 0.0, 0.0],
        [0.0, -2.0 / height, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
      ],
      time_resolution: [time, width, height, 0.0],
    }
  }
}

/// Texture cache entry
#[allow(dead_code)] // Fields used for cache management
struct CachedTexture {
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  bind_group: wgpu::BindGroup,
  width: u32,
  height: u32,
}

/// GPU-based sprite renderer with per-sprite shader effects
#[allow(dead_code)] // Some fields reserved for future use
pub struct SpriteGpuRenderer {
  // Shader pipelines (one per effect type + base)
  base_pipeline: wgpu::RenderPipeline,
  effect_pipeline: wgpu::RenderPipeline,

  // Bind group layouts
  globals_layout: wgpu::BindGroupLayout,
  texture_layout: wgpu::BindGroupLayout,

  // Global uniforms
  globals_buffer: wgpu::Buffer,
  globals_bind_group: wgpu::BindGroup,

  // Instance buffer (rebuilt each frame)
  instance_buffer: wgpu::Buffer,
  instance_capacity: usize,

  // Vertex buffer (quad)
  vertex_buffer: wgpu::Buffer,

  // Texture cache
  texture_cache: HashMap<String, CachedTexture>,

  // Timing
  time: f32,
}

impl SpriteGpuRenderer {
  /// Maximum sprites per draw call
  const MAX_INSTANCES: usize = 10000;

  /// Create a new GPU sprite renderer
  pub fn new(
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
    surface_format: wgpu::TextureFormat,
  ) -> Self {
    // Create bind group layouts
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Sprite Globals Layout"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: false,
          min_binding_size: None,
        },
        count: None,
      }],
    });

    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Sprite Texture Layout"),
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
      ],
    });

    // Create globals buffer
    let globals = SpriteGlobals::new(320.0, 180.0, 0.0);
    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Sprite Globals Buffer"),
      contents: bytemuck::cast_slice(&[globals]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Sprite Globals Bind Group"),
      layout: &globals_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: globals_buffer.as_entire_binding(),
      }],
    });

    // Create instance buffer
    let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Sprite Instance Buffer"),
      size: (Self::MAX_INSTANCES * std::mem::size_of::<SpriteInstance>()) as u64,
      usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    // Create quad vertex buffer
    // Each vertex: position (x, y), uv (u, v)
    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Vertex {
      position: [f32; 2],
      uv: [f32; 2],
    }

    let vertices = [
      Vertex {
        position: [0.0, 0.0],
        uv: [0.0, 0.0],
      }, // top-left
      Vertex {
        position: [1.0, 0.0],
        uv: [1.0, 0.0],
      }, // top-right
      Vertex {
        position: [0.0, 1.0],
        uv: [0.0, 1.0],
      }, // bottom-left
      Vertex {
        position: [1.0, 0.0],
        uv: [1.0, 0.0],
      }, // top-right
      Vertex {
        position: [1.0, 1.0],
        uv: [1.0, 1.0],
      }, // bottom-right
      Vertex {
        position: [0.0, 1.0],
        uv: [0.0, 1.0],
      }, // bottom-left
    ];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Sprite Vertex Buffer"),
      contents: bytemuck::cast_slice(&vertices),
      usage: wgpu::BufferUsages::VERTEX,
    });

    // Create shader module
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Sprite Shader"),
      source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Sprite Pipeline Layout"),
      bind_group_layouts: &[&globals_layout, &texture_layout],
      push_constant_ranges: &[],
    });

    // Vertex buffer layout
    let vertex_layout = wgpu::VertexBufferLayout {
      array_stride: 16, // 4 floats * 4 bytes
      step_mode: wgpu::VertexStepMode::Vertex,
      attributes: &[
        wgpu::VertexAttribute {
          offset: 0,
          shader_location: 0, // position
          format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
          offset: 8,
          shader_location: 1, // uv
          format: wgpu::VertexFormat::Float32x2,
        },
      ],
    };

    // Instance buffer layout
    let instance_layout = wgpu::VertexBufferLayout {
      array_stride: std::mem::size_of::<SpriteInstance>() as u64,
      step_mode: wgpu::VertexStepMode::Instance,
      attributes: &[
        // transform row 0
        wgpu::VertexAttribute {
          offset: 0,
          shader_location: 2,
          format: wgpu::VertexFormat::Float32x4,
        },
        // transform row 1
        wgpu::VertexAttribute {
          offset: 16,
          shader_location: 3,
          format: wgpu::VertexFormat::Float32x4,
        },
        // transform row 2
        wgpu::VertexAttribute {
          offset: 32,
          shader_location: 4,
          format: wgpu::VertexFormat::Float32x4,
        },
        // uv_rect
        wgpu::VertexAttribute {
          offset: 48,
          shader_location: 5,
          format: wgpu::VertexFormat::Float32x4,
        },
        // tint
        wgpu::VertexAttribute {
          offset: 64,
          shader_location: 6,
          format: wgpu::VertexFormat::Float32x4,
        },
        // effect_params
        wgpu::VertexAttribute {
          offset: 80,
          shader_location: 7,
          format: wgpu::VertexFormat::Float32x4,
        },
        // effect_extra
        wgpu::VertexAttribute {
          offset: 96,
          shader_location: 8,
          format: wgpu::VertexFormat::Float32x4,
        },
      ],
    };

    // Base pipeline (no effects, just textured sprites)
    let base_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Sprite Base Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[vertex_layout.clone(), instance_layout.clone()],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_base",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });

    // Effect pipeline (applies per-sprite effects)
    let effect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Sprite Effect Pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: "vs_main",
        buffers: &[vertex_layout, instance_layout],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: "fs_effect",
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
          blend: Some(wgpu::BlendState::ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: Default::default(),
      }),
      primitive: wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        ..Default::default()
      },
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      multiview: None,
      cache: None,
    });

    Self {
      base_pipeline,
      effect_pipeline,
      globals_layout,
      texture_layout,
      globals_buffer,
      globals_bind_group,
      instance_buffer,
      instance_capacity: Self::MAX_INSTANCES,
      vertex_buffer,
      texture_cache: HashMap::new(),
      time: 0.0,
    }
  }

  /// Update time (called each frame)
  pub fn update(&mut self, dt: f32) {
    self.time += dt;
  }

  /// Update globals for current frame
  pub fn set_viewport(&mut self, queue: &wgpu::Queue, width: f32, height: f32) {
    let globals = SpriteGlobals::new(width, height, self.time);
    queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&[globals]));
  }

  /// Upload a sprite texture to GPU (cached by id)
  pub fn upload_texture(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    id: &str,
    width: u32,
    height: u32,
    data: &[u8],
  ) {
    if self.texture_cache.contains_key(id) {
      return; // Already cached
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some(id),
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

    queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      data,
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

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Sprite Sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Sprite Texture Bind Group"),
      layout: &self.texture_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
      ],
    });

    self.texture_cache.insert(
      id.to_string(),
      CachedTexture {
        texture,
        view,
        bind_group,
        width,
        height,
      },
    );
  }

  /// Get a cached texture's bind group
  pub fn get_texture_bind_group(&self, id: &str) -> Option<&wgpu::BindGroup> {
    self.texture_cache.get(id).map(|t| &t.bind_group)
  }

  /// Render sprites to a render pass
  ///
  /// # Arguments
  /// - `queue`: GPU queue for uploading instance data
  /// - `encoder`: Command encoder for recording GPU commands
  /// - `view`: Target texture view to render to
  /// - `sprites`: Iterator of (texture_id, instances, has_effects)
  pub fn render<'a>(
    &'a self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    sprites: impl Iterator<Item = (&'a str, &'a [SpriteInstance], bool)>,
  ) {
    // Collect sprites into a Vec so we can iterate twice
    let sprites: Vec<_> = sprites.collect();

    // Pre-upload ALL instance data BEFORE starting the render pass
    let instance_size = std::mem::size_of::<SpriteInstance>() as u64;
    let mut batch_info: Vec<(u64, u32)> = Vec::with_capacity(sprites.len());
    let mut current_offset: u64 = 0;

    for (texture_id, instances, _has_effects) in &sprites {
      if instances.is_empty() {
        batch_info.push((0, 0));
        continue;
      }

      if !self.texture_cache.contains_key(*texture_id) {
        batch_info.push((0, 0));
        continue;
      }

      let instance_count = instances.len() as u32;
      let instance_data = bytemuck::cast_slice(instances);

      queue.write_buffer(&self.instance_buffer, current_offset, instance_data);

      batch_info.push((current_offset, instance_count));
      current_offset += instance_count as u64 * instance_size;
    }

    // Now start the render pass - all buffer writes are complete
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Sprite Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load, // Preserve existing content
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
    });

    render_pass.set_bind_group(0, &self.globals_bind_group, &[]);
    render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

    for (i, (texture_id, _instances, has_effects)) in sprites.iter().enumerate() {
      let (offset, instance_count) = batch_info[i];
      if instance_count == 0 {
        continue;
      }

      let Some(cached) = self.texture_cache.get(*texture_id) else {
        continue;
      };

      // Choose pipeline based on whether we need effects
      if *has_effects {
        render_pass.set_pipeline(&self.effect_pipeline);
      } else {
        render_pass.set_pipeline(&self.base_pipeline);
      }

      render_pass.set_bind_group(1, &cached.bind_group, &[]);

      // Use buffer slice starting at this batch's offset
      let end_offset = offset + instance_count as u64 * instance_size;
      render_pass.set_vertex_buffer(1, self.instance_buffer.slice(offset..end_offset));
      render_pass.draw(0..6, 0..instance_count);
    }
  }

  /// Render sprites using the BatchCollector for automatic batching
  ///
  /// This is the preferred method for high-performance sprite rendering.
  /// Sprites are grouped by texture and effect, minimizing draw calls.
  ///
  /// # Arguments
  /// - `queue`: GPU queue for uploading instance data
  /// - `encoder`: Command encoder for recording GPU commands
  /// - `view`: Target texture view to render to
  /// - `collector`: BatchCollector containing grouped sprite instances
  ///
  /// # Example
  ///
  /// ```ignore
  /// let mut collector = BatchCollector::new();
  ///
  /// for (entity, renderable, transform) in entities {
  ///     let instance = SpriteInstance::new()
  ///         .with_transform(transform.x, transform.y, ...)
  ///         .with_effect(&effect);
  ///     collector.add(renderable.sprite.id(), has_effect, instance, z_order);
  /// }
  ///
  /// renderer.render_batches(queue, encoder, view, &collector);
  /// ```
  pub fn render_batches(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    collector: &BatchCollector,
  ) {
    if collector.sprite_count() == 0 {
      return;
    }

    // Get sorted batches for consistent ordering
    let sorted_batches = collector.sorted_batches();

    // Pre-upload ALL instance data BEFORE starting the render pass
    // This ensures buffer writes complete before GPU reads
    let instance_size = std::mem::size_of::<SpriteInstance>() as u64;
    let mut batch_offsets: Vec<(u64, u32)> = Vec::with_capacity(sorted_batches.len());
    let mut current_offset: u64 = 0;

    for (key, batch) in &sorted_batches {
      if batch.is_empty() {
        batch_offsets.push((0, 0));
        continue;
      }

      // Skip batches without cached textures
      if !self.texture_cache.contains_key(&key.texture_id) {
        batch_offsets.push((0, 0));
        continue;
      }

      let instance_count = batch.len() as u32;
      let instance_data = bytemuck::cast_slice(batch.instances());

      // Write this batch's data at the current offset
      queue.write_buffer(&self.instance_buffer, current_offset, instance_data);

      batch_offsets.push((current_offset, instance_count));
      current_offset += instance_count as u64 * instance_size;
    }

    // Now start the render pass - all buffer writes are complete
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Sprite Batch Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load, // Preserve existing content
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      timestamp_writes: None,
      occlusion_query_set: None,
    });

    render_pass.set_bind_group(0, &self.globals_bind_group, &[]);
    render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

    // Render batches in z-order using pre-uploaded data
    for (i, (key, _batch)) in sorted_batches.iter().enumerate() {
      let (offset, instance_count) = batch_offsets[i];
      if instance_count == 0 {
        continue;
      }

      let Some(cached) = self.texture_cache.get(&key.texture_id) else {
        continue;
      };

      // Choose pipeline based on whether batch has effects
      if key.has_effects {
        render_pass.set_pipeline(&self.effect_pipeline);
      } else {
        render_pass.set_pipeline(&self.base_pipeline);
      }

      render_pass.set_bind_group(1, &cached.bind_group, &[]);

      // Use buffer slice starting at this batch's offset
      let end_offset = offset + instance_count as u64 * instance_size;
      render_pass.set_vertex_buffer(1, self.instance_buffer.slice(offset..end_offset));
      render_pass.draw(0..6, 0..instance_count);
    }
  }

  /// Upload a sprite from a SpriteHandle to the GPU texture cache
  ///
  /// Convenience method that extracts data from SpriteHandle.
  /// Uses the sprite's ID as the cache key.
  pub fn upload_sprite(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sprite: &crate::core::asset::SpriteHandle,
  ) {
    let id = sprite.id();
    if self.texture_cache.contains_key(&id) {
      return; // Already cached
    }

    sprite.with_sprite(|s| {
      self.upload_texture(device, queue, &id, s.width(), s.height(), s.data());
    });
  }

  /// Check if a texture is already cached
  pub fn has_texture(&self, id: &str) -> bool {
    self.texture_cache.contains_key(id)
  }

  /// Clear the texture cache
  pub fn clear_cache(&mut self) {
    self.texture_cache.clear();
  }
}

/// Get the shader effect from a Material or inline shader
pub fn get_sprite_effect<'a>(
  material: Option<&'a Material>,
  inline_shader: Option<&'a SpriteShader>,
) -> Option<&'a SpriteEffect> {
  // Material takes precedence
  if let Some(mat) = material {
    return mat.shader.as_builtin();
  }

  if let Some(shader) = inline_shader {
    return shader.as_builtin();
  }

  None
}

/// The WGSL shader for sprite rendering (loaded from external file)
pub const SPRITE_SHADER: &str = include_str!("../shaders/core/sprite.wgsl");

// =============================================================================
// Sprite Batching System
// =============================================================================

/// Key for grouping sprites into batches
///
/// Sprites with the same BatchKey can be rendered in a single draw call.
/// This includes texture ID and whether effects are applied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchKey {
  /// Texture identifier (from SpriteHandle)
  pub texture_id: String,
  /// Whether this batch uses effects (determines pipeline)
  pub has_effects: bool,
}

impl BatchKey {
  /// Create a new batch key
  pub fn new(texture_id: impl Into<String>, has_effects: bool) -> Self {
    Self {
      texture_id: texture_id.into(),
      has_effects,
    }
  }
}

/// A batch of sprites to be rendered in a single draw call
///
/// Sprites in a batch share the same texture and effect pipeline.
/// They are rendered with GPU instancing for maximum performance.
#[derive(Debug)]
pub struct SpriteBatch {
  /// Per-sprite instance data
  instances: Vec<SpriteInstance>,
  /// Z-order for sorting batches (uses minimum z from all sprites)
  z_order: i32,
}

impl Default for SpriteBatch {
  fn default() -> Self {
    Self::new()
  }
}

impl SpriteBatch {
  /// Create a new empty batch
  pub fn new() -> Self {
    Self {
      instances: Vec::new(),
      z_order: i32::MAX,
    }
  }

  /// Create a batch with pre-allocated capacity
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      instances: Vec::with_capacity(capacity),
      z_order: i32::MAX,
    }
  }

  /// Add a sprite instance to this batch
  pub fn add(&mut self, instance: SpriteInstance, z_order: i32) {
    self.instances.push(instance);
    self.z_order = self.z_order.min(z_order);
  }

  /// Get the number of sprites in this batch
  pub fn len(&self) -> usize {
    self.instances.len()
  }

  /// Check if batch is empty
  pub fn is_empty(&self) -> bool {
    self.instances.is_empty()
  }

  /// Get the instance data slice
  pub fn instances(&self) -> &[SpriteInstance] {
    &self.instances
  }

  /// Clear the batch for reuse
  pub fn clear(&mut self) {
    self.instances.clear();
    self.z_order = i32::MAX;
  }
}

/// Collector for batching sprites by texture and effect type
///
/// Use this to efficiently group sprites for GPU rendering:
///
/// ```ignore
/// let mut collector = BatchCollector::new();
///
/// // Add sprites during render phase
/// for entity in entities {
///     let texture_id = renderable.sprite.id();
///     let has_effect = renderable.shader.is_some();
///     let instance = SpriteInstance::new()
///         .with_transform(x, y, rotation, sx, sy)
///         .with_effect(&effect);
///     collector.add(texture_id, has_effect, instance, z_order);
/// }
///
/// // Render all batches
/// gpu_renderer.render_batches(queue, encoder, view, &collector);
/// ```
#[derive(Debug, Default)]
pub struct BatchCollector {
  batches: HashMap<BatchKey, SpriteBatch>,
  /// Total sprites collected
  sprite_count: usize,
}

impl BatchCollector {
  /// Create a new batch collector
  pub fn new() -> Self {
    Self {
      batches: HashMap::new(),
      sprite_count: 0,
    }
  }

  /// Create a collector with expected batch capacity
  pub fn with_capacity(batch_count: usize) -> Self {
    Self {
      batches: HashMap::with_capacity(batch_count),
      sprite_count: 0,
    }
  }

  /// Add a sprite to the appropriate batch
  ///
  /// Sprites are automatically grouped by texture_id and effect status.
  pub fn add(
    &mut self,
    texture_id: impl Into<String>,
    has_effects: bool,
    instance: SpriteInstance,
    z_order: i32,
  ) {
    let key = BatchKey::new(texture_id, has_effects);
    self.batches.entry(key).or_default().add(instance, z_order);
    self.sprite_count += 1;
  }

  /// Get the total number of sprites collected
  pub fn sprite_count(&self) -> usize {
    self.sprite_count
  }

  /// Get the number of batches (draw calls)
  pub fn batch_count(&self) -> usize {
    self.batches.len()
  }

  /// Get batches sorted by z-order for correct draw ordering
  pub fn sorted_batches(&self) -> Vec<(&BatchKey, &SpriteBatch)> {
    let mut batches: Vec<_> = self.batches.iter().collect();
    batches.sort_by_key(|(_, batch)| batch.z_order);
    batches
  }

  /// Clear all batches for reuse next frame
  pub fn clear(&mut self) {
    self.batches.clear();
    self.sprite_count = 0;
  }

  /// Iterate over all batches (unordered)
  pub fn iter(&self) -> impl Iterator<Item = (&BatchKey, &SpriteBatch)> {
    self.batches.iter()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sprite_instance_size() {
    // Verify instance size matches expected (for GPU buffer layout)
    assert_eq!(std::mem::size_of::<SpriteInstance>(), 112);
  }

  #[test]
  fn sprite_globals_size() {
    assert_eq!(std::mem::size_of::<SpriteGlobals>(), 80);
  }

  #[test]
  fn sprite_instance_default() {
    let instance = SpriteInstance::new();
    assert_eq!(instance.transform[0][0], 1.0);
    assert_eq!(instance.transform[1][1], 1.0);
    assert_eq!(instance.uv_rect, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(instance.tint, [1.0, 1.0, 1.0, 1.0]);
  }

  #[test]
  fn sprite_instance_with_transform() {
    let instance = SpriteInstance::new().with_transform(100.0, 200.0, 0.0, 2.0, 3.0);
    // Column-major layout: translation is in column 2
    assert_eq!(instance.transform[2][0], 100.0); // x translation
    assert_eq!(instance.transform[2][1], 200.0); // y translation
    assert_eq!(instance.transform[0][0], 2.0); // scale_x (column 0, row 0)
    assert_eq!(instance.transform[1][1], 3.0); // scale_y (column 1, row 1)
  }

  #[test]
  fn sprite_instance_with_effect_flash() {
    let effect = SpriteEffect::flash(Color::RED, 0.8);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(instance.effect_params[0], 3.0); // Flash effect type
    assert_eq!(instance.effect_params[1], 0.8); // intensity
  }

  // =========================================================================
  // Batching tests
  // =========================================================================

  #[test]
  fn sprite_batch_add() {
    let mut batch = SpriteBatch::new();
    assert!(batch.is_empty());

    batch.add(SpriteInstance::new(), 5);
    assert_eq!(batch.len(), 1);
    assert_eq!(batch.z_order, 5);

    batch.add(SpriteInstance::new(), 2);
    assert_eq!(batch.len(), 2);
    assert_eq!(batch.z_order, 2); // Min z-order

    batch.add(SpriteInstance::new(), 10);
    assert_eq!(batch.len(), 3);
    assert_eq!(batch.z_order, 2); // Still min
  }

  #[test]
  fn batch_collector_grouping() {
    let mut collector = BatchCollector::new();

    // Add sprites with same texture, different effects
    collector.add("texture_a", false, SpriteInstance::new(), 0);
    collector.add("texture_a", false, SpriteInstance::new(), 1);
    collector.add("texture_a", true, SpriteInstance::new(), 2);

    // Add sprites with different texture
    collector.add("texture_b", false, SpriteInstance::new(), 0);

    assert_eq!(collector.sprite_count(), 4);
    assert_eq!(collector.batch_count(), 3); // 3 unique (texture, effect) combos
  }

  #[test]
  fn batch_collector_sorted() {
    let mut collector = BatchCollector::new();

    // Add sprites in non-z-order sequence
    collector.add("tex_c", false, SpriteInstance::new(), 100);
    collector.add("tex_a", false, SpriteInstance::new(), 0);
    collector.add("tex_b", false, SpriteInstance::new(), 50);

    let sorted = collector.sorted_batches();
    assert_eq!(sorted.len(), 3);

    // Verify z-order sorting
    assert_eq!(sorted[0].0.texture_id, "tex_a");
    assert_eq!(sorted[1].0.texture_id, "tex_b");
    assert_eq!(sorted[2].0.texture_id, "tex_c");
  }

  #[test]
  fn batch_key_equality() {
    let key1 = BatchKey::new("test", false);
    let key2 = BatchKey::new("test", false);
    let key3 = BatchKey::new("test", true);
    let key4 = BatchKey::new("other", false);

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
    assert_ne!(key1, key4);
  }
}
