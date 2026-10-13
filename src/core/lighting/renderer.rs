//! Lighting renderer that manages the light accumulation buffer
//!
//! This renderer collects all lights in the scene and renders them
//! to an accumulation buffer, which is then used to light the scene.

use super::shadow::{MAX_SHADOW_VERTICES, ShadowGeometry, ShadowVertex};
use super::{AmbientLight, Atmosphere, Light2D, LightOccluder2D};
use crate::core::ecs::{Entity, GlobalTransform, Vec2};
use crate::core::graphics::camera_gpu::{CameraGlobals, CameraUniforms};
use crate::core::graphics::{Camera2d, CameraView};
use crate::core::shaders::EffectsManager;
use wgpu::util::DeviceExt;

/// Maximum number of lights that can be rendered per frame
pub const MAX_LIGHTS: usize = 256;

/// Maximum number of occluder edges for ray-marched shadows
pub const MAX_RAYMARCH_EDGES: usize = 64;

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
  /// Volumetric: intensity (x), density (y), scattering (z), god_rays (w: 0 or 1)
  volumetric: [f32; 4],
}

/// Uniforms for the lighting composition pass
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingParams {
  /// Ambient color (RGB) + unused (A)
  pub color: [f32; 4],
  /// Ambient intensity (x), atmosphere density (y), atmosphere scattering (z), unused (w)
  pub params: [f32; 4],
  /// Time (x), width (y), height (z), unused (w)
  pub time_resolution: [f32; 4],
  /// Atmosphere fog color (RGB) + unused (A)
  pub atmosphere_color: [f32; 4],
}

/// Parameters for ray-marched shadow rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RaymarchParams {
  /// Light position (x, y), radius (z), samples (w)
  light_params: [f32; 4],
  /// Shadow color (RGB) and intensity (A)
  shadow_color: [f32; 4],
  /// Number of edges (x), softness (y), unused (z, w)
  edge_params: [f32; 4],
  /// Padding for alignment
  _padding: [f32; 4],
}

/// An occluder edge for ray-marched shadows
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RaymarchEdge {
  /// Start point (x, y), end point (z, w)
  points: [f32; 4],
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

  /// Uniform buffer for composition pass
  compose_uniform_buffer: wgpu::Buffer,

  /// Bind group layout for light rendering
  light_bind_group_layout: wgpu::BindGroupLayout,

  /// Bind group layout for composition
  compose_bind_group_layout: wgpu::BindGroupLayout,

  /// Current ambient light settings
  ambient: AmbientLight,

  /// Current atmosphere settings
  atmosphere: Atmosphere,

  /// Render size of the light and scene textures, in pixels
  width: u32,
  height: u32,

  /// Whether lighting is enabled
  enabled: bool,

  /// Whether UI should be affected by lighting (default: false for always-visible UI)
  ui_affected_by_lighting: bool,

  /// Copy of scene texture for compose shader (workaround for texture sampling issue)
  scene_copy_texture: wgpu::Texture,
  scene_copy_view: wgpu::TextureView,

  // Shadow rendering resources
  /// Pipeline for rendering shadow geometry
  shadow_pipeline: wgpu::RenderPipeline,
  /// Vertex buffer for shadow geometry
  shadow_vertex_buffer: wgpu::Buffer,
  /// Bind group layout for shadow rendering
  shadow_bind_group_layout: wgpu::BindGroupLayout,

  // Ray-marched shadow resources
  /// Pipeline for ray-marched shadows
  raymarch_pipeline: wgpu::RenderPipeline,
  /// Uniform buffer for raymarch parameters
  raymarch_params_buffer: wgpu::Buffer,
  /// Storage buffer for occluder edges
  raymarch_edge_buffer: wgpu::Buffer,
  /// Bind group layout for raymarch shadows
  raymarch_bind_group_layout: wgpu::BindGroupLayout,
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
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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

    // Create compose uniform buffer
    let compose_uniform = LightingParams {
      color: [0.1, 0.1, 0.12, 1.0],
      params: [0.15, 0.0, 0.0, 0.0], // ambient intensity, atmosphere density, scattering, unused
      time_resolution: [0.0, width as f32, height as f32, 0.0],
      atmosphere_color: [0.78, 0.78, 0.82, 1.0], // Default fog color (light gray-blue)
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
      bind_group_layouts: &[Some(&light_bind_group_layout)],
      immediate_size: 0,
    });

    let light_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Light Pipeline"),
      layout: Some(&light_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &light_shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &light_shader,
        entry_point: Some("fs_main"),
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
      multiview_mask: None,
      cache: None,
    });

    // Create composition pipeline
    let compose_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Lighting Compose Shader"),
      source: wgpu::ShaderSource::Wgsl(
        include_str!("../shaders/post-process/lighting.wgsl").into(),
      ),
    });

    let compose_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Lighting Compose Pipeline Layout"),
      bind_group_layouts: &[Some(&compose_bind_group_layout)],
      immediate_size: 0,
    });

    let compose_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Lighting Compose Pipeline"),
      layout: Some(&compose_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &compose_shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &compose_shader,
        entry_point: Some("fs_main"),
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
      multiview_mask: None,
      cache: None,
    });

    // Create scene copy texture (workaround for texture sampling issue in compose shader)
    let scene_copy_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Scene Copy Texture"),
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
    let scene_copy_view = scene_copy_texture.create_view(&wgpu::TextureViewDescriptor::default());

    // === Shadow Pipeline Setup ===

    // Shadow bind group layout (same globals as lights)
    let shadow_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Shadow Bind Group Layout"),
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

    // Shadow vertex buffer
    let shadow_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Shadow Vertex Buffer"),
      size: (std::mem::size_of::<ShadowVertex>() * MAX_SHADOW_VERTICES) as u64,
      usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    // Shadow shader
    let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Shadow Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/shadow.wgsl").into()),
    });

    let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Shadow Pipeline Layout"),
      bind_group_layouts: &[Some(&shadow_bind_group_layout)],
      immediate_size: 0,
    });

    // Shadow pipeline with reverse-subtract blending
    let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Shadow Pipeline"),
      layout: Some(&shadow_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shadow_shader,
        entry_point: Some("vs_main"),
        buffers: &[Some(wgpu::VertexBufferLayout {
          array_stride: std::mem::size_of::<ShadowVertex>() as u64,
          step_mode: wgpu::VertexStepMode::Vertex,
          attributes: &[
            // position: vec2<f32>
            wgpu::VertexAttribute {
              offset: 0,
              shader_location: 0,
              format: wgpu::VertexFormat::Float32x2,
            },
            // light_pos: vec2<f32>
            wgpu::VertexAttribute {
              offset: 8,
              shader_location: 1,
              format: wgpu::VertexFormat::Float32x2,
            },
            // light_radius: f32
            wgpu::VertexAttribute {
              offset: 16,
              shader_location: 2,
              format: wgpu::VertexFormat::Float32,
            },
            // softness: f32
            wgpu::VertexAttribute {
              offset: 20,
              shader_location: 3,
              format: wgpu::VertexFormat::Float32,
            },
            // shadow_color: u32 (packed RGBA)
            wgpu::VertexAttribute {
              offset: 24,
              shader_location: 4,
              format: wgpu::VertexFormat::Uint32,
            },
          ],
        })],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &shadow_shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba16Float,
          // Reverse subtract: dst = dst - src (subtract shadow from light)
          blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::One,
              dst_factor: wgpu::BlendFactor::One,
              operation: wgpu::BlendOperation::ReverseSubtract,
            },
            alpha: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::Zero,
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
      multiview_mask: None,
      cache: None,
    });

    // === Ray-march Shadow Pipeline Setup ===

    // Ray-march bind group layout
    let raymarch_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Raymarch Shadow Bind Group Layout"),
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
          // Raymarch params uniform
          wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Uniform,
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
          // Edge storage buffer
          wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
              ty: wgpu::BufferBindingType::Storage { read_only: true },
              has_dynamic_offset: false,
              min_binding_size: None,
            },
            count: None,
          },
        ],
      });

    // Raymarch params buffer
    let raymarch_params = RaymarchParams {
      light_params: [0.0; 4],
      shadow_color: [0.0, 0.0, 0.0, 1.0],
      edge_params: [0.0; 4],
      _padding: [0.0; 4],
    };
    let raymarch_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Raymarch Params Buffer"),
      contents: bytemuck::cast_slice(&[raymarch_params]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Raymarch edge buffer
    let raymarch_edge_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Raymarch Edge Buffer"),
      size: (std::mem::size_of::<RaymarchEdge>() * MAX_RAYMARCH_EDGES) as u64,
      usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    // Raymarch shader
    let raymarch_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Raymarch Shadow Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/raymarch_shadow.wgsl").into()),
    });

    let raymarch_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Raymarch Shadow Pipeline Layout"),
      bind_group_layouts: &[Some(&raymarch_bind_group_layout)],
      immediate_size: 0,
    });

    // Raymarch pipeline with reverse-subtract blending (same as shadow pipeline)
    let raymarch_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("Raymarch Shadow Pipeline"),
      layout: Some(&raymarch_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &raymarch_shader,
        entry_point: Some("vs_main"),
        buffers: &[],
        compilation_options: Default::default(),
      },
      fragment: Some(wgpu::FragmentState {
        module: &raymarch_shader,
        entry_point: Some("fs_main"),
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba16Float,
          // Reverse subtract: dst = dst - src (subtract shadow from light)
          blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::One,
              dst_factor: wgpu::BlendFactor::One,
              operation: wgpu::BlendOperation::ReverseSubtract,
            },
            alpha: wgpu::BlendComponent {
              src_factor: wgpu::BlendFactor::Zero,
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
      multiview_mask: None,
      cache: None,
    });

    Self {
      light_texture,
      light_texture_view,
      sampler,
      light_pipeline,
      compose_pipeline,
      light_buffer,
      compose_uniform_buffer,
      light_bind_group_layout,
      compose_bind_group_layout,
      ambient: AmbientLight::default(),
      atmosphere: Atmosphere::default(),
      width,
      height,
      enabled: false, // Disabled by default, enabled when Light2D entities exist
      ui_affected_by_lighting: false, // By default, UI is always visible
      scene_copy_texture,
      scene_copy_view,
      shadow_pipeline,
      shadow_vertex_buffer,
      shadow_bind_group_layout,
      raymarch_pipeline,
      raymarch_params_buffer,
      raymarch_edge_buffer,
      raymarch_bind_group_layout,
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

  /// Set the atmosphere settings.
  ///
  /// Atmosphere controls fog density, color tinting, and light scattering.
  /// Use presets like `Atmosphere::dusty()` or `Atmosphere::foggy()`.
  pub fn set_atmosphere(&mut self, atmosphere: Atmosphere) {
    self.atmosphere = atmosphere;
  }

  /// Get the current atmosphere settings.
  pub fn atmosphere(&self) -> &Atmosphere {
    &self.atmosphere
  }

  /// Get a mutable reference to the atmosphere settings.
  pub fn atmosphere_mut(&mut self) -> &mut Atmosphere {
    &mut self.atmosphere
  }

  /// Enable or disable the lighting system.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Check if lighting is enabled.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Set whether UI should be affected by lighting.
  ///
  /// - `true`: UI is lit like the rest of the scene (fades in darkness)
  /// - `false`: UI is always fully visible (rendered after lighting)
  pub fn set_ui_affected_by_lighting(&mut self, affected: bool) {
    self.ui_affected_by_lighting = affected;
  }

  /// Check if UI is affected by lighting.
  pub fn is_ui_affected_by_lighting(&self) -> bool {
    self.ui_affected_by_lighting
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

    // Recreate scene copy texture
    self.scene_copy_texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("Scene Copy Texture"),
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
    self.scene_copy_view = self
      .scene_copy_texture
      .create_view(&wgpu::TextureViewDescriptor::default());
  }

  /// Render lights to the accumulation buffer.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `lights` - Iterator of (position, light) pairs
  /// * `camera_pos`, `camera_zoom` - The view's centre and zoom over a target
  ///   of the renderer's size (for using the renderer on its own; the engine
  ///   draws each camera through its uniforms)
  pub fn render_lights<'a>(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    lights: impl Iterator<Item = (&'a Vec2, &'a Light2D)>,
    camera_pos: Vec2,
    camera_zoom: f32,
  ) {
    let globals = self.standalone_globals(device, camera_pos, camera_zoom);
    self.draw_lights(device, queue, encoder, lights, Some(&globals));
  }

  /// Render lights through `camera`, whose uniforms `uniforms` holds.
  pub(crate) fn render_lights_for_camera<'a>(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    lights: impl Iterator<Item = (&'a Vec2, &'a Light2D)>,
    camera: Entity,
    uniforms: &CameraUniforms,
  ) {
    self.draw_lights(device, queue, encoder, lights, uniforms.buffer(camera));
  }

  /// The camera uniforms of a view centred on `center` at `zoom`, one world
  /// unit per pixel of the renderer's size.
  fn standalone_globals(&self, device: &wgpu::Device, center: Vec2, zoom: f32) -> wgpu::Buffer {
    let camera = Camera2d {
      zoom,
      ..Camera2d::default()
    };
    let size = Vec2::new(self.width as f32, self.height as f32);
    let view = CameraView::compute(center, 0.0, &camera, Vec2::ZERO, size);
    let globals = CameraGlobals::new(&view, (self.width, self.height), 0.0, 0.0, false);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Standalone Light Globals"),
      contents: bytemuck::bytes_of(&globals),
      usage: wgpu::BufferUsages::UNIFORM,
    })
  }

  /// Clear the light buffer and draw `lights` through `globals` (nothing is
  /// drawn without them).
  fn draw_lights<'a>(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    lights: impl Iterator<Item = (&'a Vec2, &'a Light2D)>,
    globals: Option<&wgpu::Buffer>,
  ) {
    // Note: We still render the light accumulation pass even if disabled to ensure
    // the light_texture is cleared. The compose pass handles ambient lighting.

    // Collect light instances
    let mut instances: Vec<LightInstance> = Vec::with_capacity(MAX_LIGHTS);

    for (position, light) in lights {
      if instances.len() >= MAX_LIGHTS {
        break;
      }

      let pos_x = position.x;
      let pos_y = position.y;

      let instance = match light {
        Light2D::Point {
          color,
          radius,
          intensity,
          falloff,
          volumetric,
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
          volumetric: volumetric
            .as_ref()
            .map(|v| v.to_gpu_vec4())
            .unwrap_or([0.0; 4]),
        },
        Light2D::Spot {
          color,
          radius,
          intensity,
          falloff,
          angle,
          direction,
          softness,
          volumetric,
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
          volumetric: volumetric
            .as_ref()
            .map(|v| v.to_gpu_vec4())
            .unwrap_or([0.0; 4]),
        },
      };

      instances.push(instance);
    }

    // Upload light data
    if !instances.is_empty() {
      queue.write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&instances));
    }

    // The camera's uniforms and the lights, for this camera
    let light_bind_group = globals.map(|globals| {
      device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Light Bind Group"),
        layout: &self.light_bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: globals.as_entire_binding(),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: self.light_buffer.as_entire_binding(),
          },
        ],
      })
    });

    // Clear and render lights to accumulation buffer
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Light Accumulation Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.light_texture_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        multiview_mask: None,
        timestamp_writes: None,
      });

      if let Some(light_bind_group) = &light_bind_group
        && !instances.is_empty()
      {
        render_pass.set_pipeline(&self.light_pipeline);
        render_pass.set_bind_group(0, light_bind_group, &[]);
        // 6 vertices per light quad, N instances
        render_pass.draw(0..6, 0..instances.len() as u32);
      }
    }
  }

  /// Render shadows to subtract from the light accumulation buffer.
  ///
  /// Call this after `render_lights()` to apply shadow occlusion.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `shadow_geometry` - Pre-generated shadow geometry from `generate_shadow_geometry()`
  pub(crate) fn render_shadows(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    shadow_geometry: &ShadowGeometry,
    camera: Entity,
    uniforms: &CameraUniforms,
  ) {
    if let Some(globals) = uniforms.buffer(camera) {
      self.draw_shadows(device, queue, encoder, shadow_geometry, globals);
    }
  }

  /// Subtract `shadow_geometry` from the light buffer, through `globals`.
  fn draw_shadows(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    shadow_geometry: &ShadowGeometry,
    globals: &wgpu::Buffer,
  ) {
    if shadow_geometry.is_empty() {
      return;
    }

    // Upload shadow vertices
    queue.write_buffer(
      &self.shadow_vertex_buffer,
      0,
      bytemuck::cast_slice(&shadow_geometry.vertices),
    );

    // The camera's uniforms
    let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Shadow Bind Group"),
      layout: &self.shadow_bind_group_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: globals.as_entire_binding(),
      }],
    });

    // Render shadows with reverse-subtract blending to cut from light buffer
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Shadow Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.light_texture_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load, // Keep existing light accumulation
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        multiview_mask: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.shadow_pipeline);
      render_pass.set_bind_group(0, &shadow_bind_group, &[]);
      render_pass.set_vertex_buffer(0, self.shadow_vertex_buffer.slice(..));
      render_pass.draw(0..shadow_geometry.vertex_count(), 0..1);
    }
  }

  /// Render ray-marched shadows for a single light.
  ///
  /// This is an alternative to geometry-based shadows that uses per-pixel
  /// ray casting for more accurate shadow rendering in complex scenes.
  #[allow(clippy::too_many_arguments)]
  fn render_raymarched_shadow(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    globals: &wgpu::Buffer,
    light_pos: Vec2,
    light_radius: f32,
    samples: u32,
    shadow_softness: f32,
    shadow_color: crate::core::ui::Color,
    occluders: &[(&GlobalTransform, &LightOccluder2D)],
  ) {
    // Collect edges from occluders within light radius
    let mut edges: Vec<RaymarchEdge> = Vec::with_capacity(MAX_RAYMARCH_EDGES);

    for (transform, occluder) in occluders {
      if !occluder.is_enabled() {
        continue;
      }

      // Quick distance check
      let position = transform.translation();
      let dist = position.distance(light_pos);
      if dist > light_radius * 2.0 {
        continue;
      }

      // Transform vertices to world space
      let world_vertices =
        occluder.transform_vertices(position.x, position.y, transform.rotation());

      // Add edges
      for i in 0..world_vertices.len() {
        if edges.len() >= MAX_RAYMARCH_EDGES {
          break;
        }

        let v0 = world_vertices[i];
        let v1 = world_vertices[(i + 1) % world_vertices.len()];

        edges.push(RaymarchEdge {
          points: [v0.x, v0.y, v1.x, v1.y],
        });
      }

      if edges.len() >= MAX_RAYMARCH_EDGES {
        break;
      }
    }

    if edges.is_empty() {
      return; // No edges to cast shadows from
    }

    // Update raymarch params
    let params = RaymarchParams {
      light_params: [light_pos.x, light_pos.y, light_radius, samples as f32],
      shadow_color: [
        shadow_color.r as f32 / 255.0,
        shadow_color.g as f32 / 255.0,
        shadow_color.b as f32 / 255.0,
        1.0,
      ],
      edge_params: [edges.len() as f32, shadow_softness, 0.0, 0.0],
      _padding: [0.0; 4],
    };
    queue.write_buffer(
      &self.raymarch_params_buffer,
      0,
      bytemuck::cast_slice(&[params]),
    );

    // Upload edges
    queue.write_buffer(&self.raymarch_edge_buffer, 0, bytemuck::cast_slice(&edges));

    // Create bind group
    let raymarch_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Raymarch Shadow Bind Group"),
      layout: &self.raymarch_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: globals.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: self.raymarch_params_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
          binding: 2,
          resource: self.raymarch_edge_buffer.as_entire_binding(),
        },
      ],
    });

    // Render raymarch shadows
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Raymarch Shadow Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &self.light_texture_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load, // Keep existing light accumulation
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        multiview_mask: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.raymarch_pipeline);
      render_pass.set_bind_group(0, &raymarch_bind_group, &[]);
      render_pass.draw(0..6, 0..1); // Single quad covering the light area
    }
  }

  /// Convenience method to render lights and shadows together.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `lights` - Iterator of (transform, light) pairs
  /// * `occluders` - Slice of (transform, occluder) pairs for shadow casting
  /// * `camera_pos`, `camera_zoom` - The view, as for [`render_lights`](Self::render_lights)
  #[allow(clippy::too_many_arguments)]
  pub fn render_lights_with_shadows<'a>(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    lights: impl Iterator<Item = (&'a Vec2, &'a Light2D)> + Clone,
    occluders: &[(&'a GlobalTransform, &'a LightOccluder2D)],
    camera_pos: Vec2,
    camera_zoom: f32,
  ) {
    let globals = self.standalone_globals(device, camera_pos, camera_zoom);
    // First render all lights
    self.draw_lights(device, queue, encoder, lights.clone(), Some(&globals));

    // Separate lights into regular shadows and raymarched shadows
    let mut raymarch_lights: Vec<(&Vec2, &Light2D)> = Vec::new();
    let regular_lights = lights.clone().filter(|(t, l)| {
      if l.uses_raymarched_shadows() {
        raymarch_lights.push((*t, *l));
        false
      } else {
        true
      }
    });

    // Generate and render regular shadow volumes
    let shadow_geometry = super::shadow::generate_shadow_geometry(regular_lights, occluders);
    self.draw_shadows(device, queue, encoder, &shadow_geometry, &globals);

    // Render ray-marched shadows for lights that use them
    for (position, light) in raymarch_lights {
      if let Some(samples) = light.raymarch_samples() {
        self.render_raymarched_shadow(
          device,
          queue,
          encoder,
          &globals,
          *position,
          light.radius(),
          samples,
          light.shadow_softness(),
          light.shadow_color(),
          occluders,
        );
      }
    }
  }

  /// Compose the lit scene by combining the scene texture with the light buffer.
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `pixel_data` - The scene pixel data (RGBA8)
  /// * `output_view` - The output texture view
  /// * `time` - Current time for effects
  pub fn compose(
    &self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pixel_data: &[u8],
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    // Note: We don't early-return here even if disabled, because we need to
    // initialize output_view with valid content. The render_lights() call
    // handles the enabled check for light accumulation.

    // Write pixel data to our scene copy texture
    queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture: &self.scene_copy_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      pixel_data,
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(4 * self.width),
        rows_per_image: Some(self.height),
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );

    // Update compose uniforms with atmosphere
    let compose_uniform = LightingParams {
      color: [
        self.ambient.color.r as f32 / 255.0,
        self.ambient.color.g as f32 / 255.0,
        self.ambient.color.b as f32 / 255.0,
        1.0,
      ],
      params: [
        self.ambient.intensity,
        self.atmosphere.density,
        self.atmosphere.scattering,
        0.0,
      ],
      time_resolution: [time, self.width as f32, self.height as f32, 0.0],
      atmosphere_color: [
        self.atmosphere.color.r as f32 / 255.0,
        self.atmosphere.color.g as f32 / 255.0,
        self.atmosphere.color.b as f32 / 255.0,
        1.0,
      ],
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
          resource: wgpu::BindingResource::TextureView(&self.scene_copy_view),
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
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        multiview_mask: None,
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

  /// Update the god rays post-process effect with positions from lights that contribute.
  ///
  /// This method filters lights with `contributes_to_god_rays()` enabled, converts their
  /// world positions to screen UV coordinates, and updates the EffectsManager.
  ///
  /// # Arguments
  /// * `effects` - The effects manager to update
  /// * `lights` - Slice of (transform, light) pairs
  /// * `camera_pos` - Camera position in world space
  /// * `camera_zoom` - Camera zoom level
  pub fn update_god_rays(
    &self,
    effects: &mut EffectsManager,
    lights: &[(Vec2, Light2D)],
    view: &CameraView,
  ) {
    // Filter lights that contribute to god rays and convert to screen UV
    let positions: Vec<(f32, f32)> = lights
      .iter()
      .filter(|(_, light)| light.contributes_to_god_rays())
      .take(4) // Shader supports max 4 lights
      .map(|(position, _)| {
        // Where the camera shows the light, as a UV of its rectangle
        let uv = (view.world_to_screen(*position) - view.rect_min) / view.rect_size;
        (uv.x, uv.y)
      })
      .collect();

    if !positions.is_empty() {
      if !effects.has_god_rays() {
        effects.set_god_rays(0.8, positions);
      } else {
        effects.update_god_rays_lights(positions);
      }
    } else if effects.has_god_rays() {
      effects.disable_god_rays();
    }
  }

  /// Compose the lit scene from a texture view (instead of pixel data).
  ///
  /// This is the preferred method when the scene is already on a GPU texture
  /// (e.g., after the world has been rendered to the game texture).
  ///
  /// # Arguments
  /// * `device` - The wgpu device
  /// * `queue` - The wgpu queue
  /// * `encoder` - The command encoder
  /// * `scene_texture` - The source scene texture (will be copied to internal buffer)
  /// * `output_view` - The output texture view
  /// * `time` - Current time for effects
  pub fn compose_from_texture(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    scene_texture: &wgpu::Texture,
    output_view: &wgpu::TextureView,
    time: f32,
  ) {
    // Auto-resize if scene texture dimensions don't match our internal textures
    let scene_size = scene_texture.size();
    if scene_size.width != self.width || scene_size.height != self.height {
      self.resize(device, scene_size.width, scene_size.height);
    }

    // Copy scene texture to our internal scene_copy_texture
    encoder.copy_texture_to_texture(
      wgpu::TexelCopyTextureInfo {
        texture: scene_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::TexelCopyTextureInfo {
        texture: &self.scene_copy_texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );

    // Update compose uniforms with atmosphere
    let compose_uniform = LightingParams {
      color: [
        self.ambient.color.r as f32 / 255.0,
        self.ambient.color.g as f32 / 255.0,
        self.ambient.color.b as f32 / 255.0,
        1.0,
      ],
      params: [
        self.ambient.intensity,
        self.atmosphere.density,
        self.atmosphere.scattering,
        0.0,
      ],
      time_resolution: [time, self.width as f32, self.height as f32, 0.0],
      atmosphere_color: [
        self.atmosphere.color.r as f32 / 255.0,
        self.atmosphere.color.g as f32 / 255.0,
        self.atmosphere.color.b as f32 / 255.0,
        1.0,
      ],
    };
    queue.write_buffer(
      &self.compose_uniform_buffer,
      0,
      bytemuck::cast_slice(&[compose_uniform]),
    );

    // Create compose bind group
    let compose_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Lighting Compose Bind Group (from texture)"),
      layout: &self.compose_bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&self.scene_copy_view),
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
        label: Some("Lighting Compose Pass (from texture)"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: output_view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        multiview_mask: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(&self.compose_pipeline);
      render_pass.set_bind_group(0, &compose_bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }
  }
}
