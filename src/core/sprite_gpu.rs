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

use super::ecs::{Material, SpriteEffect, SpriteShader};
use super::ui::Color;

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

    // 2D affine transform matrix (row-major)
    // [ scale_x * cos, -scale_y * sin, x ]
    // [ scale_x * sin,  scale_y * cos, y ]
    self.transform[0][0] = scale_x * cos;
    self.transform[0][1] = -scale_y * sin;
    self.transform[0][2] = x;

    self.transform[1][0] = scale_x * sin;
    self.transform[1][1] = scale_y * cos;
    self.transform[1][2] = y;

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

    for (texture_id, instances, has_effects) in sprites {
      if instances.is_empty() {
        continue;
      }

      let Some(cached) = self.texture_cache.get(texture_id) else {
        continue;
      };

      // Choose pipeline based on whether we need effects
      if has_effects {
        render_pass.set_pipeline(&self.effect_pipeline);
      } else {
        render_pass.set_pipeline(&self.base_pipeline);
      }

      render_pass.set_bind_group(1, &cached.bind_group, &[]);

      // Upload instance data
      // Note: In a real implementation, we'd batch these writes
      // For now, we update the buffer for each draw call
      let instance_data = bytemuck::cast_slice(instances);
      queue.write_buffer(&self.instance_buffer, 0, instance_data);

      render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
      render_pass.draw(0..6, 0..instances.len() as u32);
    }
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
pub const SPRITE_SHADER: &str = include_str!("shaders/core/sprite.wgsl");

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
    assert_eq!(instance.transform[0][2], 100.0); // x translation
    assert_eq!(instance.transform[1][2], 200.0); // y translation
    assert_eq!(instance.transform[0][0], 2.0); // scale_x
    assert_eq!(instance.transform[1][1], 3.0); // scale_y
  }

  #[test]
  fn sprite_instance_with_effect_flash() {
    let effect = SpriteEffect::flash(Color::RED, 0.8);
    let instance = SpriteInstance::new().with_effect(&effect);
    assert_eq!(instance.effect_params[0], 3.0); // Flash effect type
    assert_eq!(instance.effect_params[1], 0.8); // intensity
  }
}
