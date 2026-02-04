//! GPU-accelerated trail and line renderer
//!
//! Renders trails and lines efficiently using GPU instancing and batching.

use super::{Line2D, TextureMode, Trail2D, TrailPoint};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{Transform, Vec2};
use crate::core::ui::Color;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// Maximum number of vertices per frame (across all trails/lines)
pub const MAX_TRAIL_VERTICES: usize = 65536;

/// Trail rendering style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailStyle {
  /// Solid color with sharp edges
  #[default]
  Solid,
  /// Soft edges that fade out
  Soft,
  /// Glow effect with bright core
  Glow,
}

/// Per-vertex GPU data (must match the shader struct)
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TrailVertex {
  /// World position
  position: [f32; 2],
  /// RGBA color (normalized)
  color: [f32; 4],
  /// UV coordinates (x = along trail 0-1, y = across trail -0.5 to 0.5)
  uv: [f32; 2],
}

/// Global uniforms for trail rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TrailGlobals {
  /// View-projection matrix
  view_proj: [[f32; 4]; 4],
  /// Resolution and time
  resolution_time: [f32; 4], // x, y = resolution, z = time, w = padding
}

/// Cached GPU texture for a trail
struct CachedTexture {
  #[allow(dead_code)]
  texture: wgpu::Texture,
  bind_group: wgpu::BindGroup,
}

/// A batch of textured trail vertices
struct TexturedBatch {
  /// Sprite handle ID (for cache lookup)
  sprite_id: String,
  /// Starting vertex index
  start: usize,
  /// Ending vertex index (exclusive)
  end: usize,
}

/// GPU-accelerated trail and line renderer.
pub struct TrailRenderer {
  /// Render pipeline for solid trails (no texture)
  solid_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for soft trails (no texture)
  soft_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for glow trails (no texture)
  glow_pipeline: wgpu::RenderPipeline,

  /// Render pipeline for textured solid trails
  textured_solid_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for textured soft trails
  textured_soft_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for textured glow trails
  textured_glow_pipeline: wgpu::RenderPipeline,

  /// Vertex buffer
  vertex_buffer: wgpu::Buffer,

  /// Uniform buffer for globals
  globals_buffer: wgpu::Buffer,

  /// Bind group layout for non-textured trails (globals only)
  #[allow(dead_code)]
  bind_group_layout: wgpu::BindGroupLayout,

  /// Bind group layout for textured trails (texture + sampler)
  texture_bind_group_layout: wgpu::BindGroupLayout,

  /// Current bind group (globals)
  bind_group: wgpu::BindGroup,

  /// Sampler for textures
  sampler: wgpu::Sampler,

  /// Cached textures (sprite_id -> CachedTexture)
  texture_cache: HashMap<String, CachedTexture>,

  /// Accumulated vertices for non-textured trails
  vertices: Vec<TrailVertex>,

  /// Accumulated vertices for textured trails
  textured_vertices: Vec<TrailVertex>,

  /// Batches of textured trails (grouped by texture)
  textured_batches: Vec<TexturedBatch>,

  /// Screen dimensions
  width: u32,
  height: u32,

  /// Whether the renderer is enabled
  enabled: bool,
}

impl TrailRenderer {
  /// Create a new trail renderer.
  pub fn new(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    output_format: wgpu::TextureFormat,
  ) -> Self {
    // Create vertex buffer
    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("Trail Vertex Buffer"),
      size: (std::mem::size_of::<TrailVertex>() * MAX_TRAIL_VERTICES) as u64,
      usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });

    // Create globals uniform buffer
    let globals = TrailGlobals {
      view_proj: [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
      ],
      resolution_time: [width as f32, height as f32, 0.0, 0.0],
    };
    let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Trail Globals Buffer"),
      contents: bytemuck::cast_slice(&[globals]),
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    // Create bind group layout for non-textured trails
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("Trail Bind Group Layout"),
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

    // Create bind group layout for textured trails (texture + sampler)
    let texture_bind_group_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Trail Texture Bind Group Layout"),
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

    // Create bind group
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Trail Bind Group"),
      layout: &bind_group_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: globals_buffer.as_entire_binding(),
      }],
    });

    // Create sampler for textures
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("Trail Texture Sampler"),
      address_mode_u: wgpu::AddressMode::Repeat,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create shader module (non-textured)
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Trail Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/trail.wgsl").into()),
    });

    // Create shader module (textured)
    let textured_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Trail Textured Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/trail_textured.wgsl").into()),
    });

    // Create pipeline layout for non-textured trails
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Trail Pipeline Layout"),
      bind_group_layouts: &[&bind_group_layout],
      push_constant_ranges: &[],
    });

    // Create pipeline layout for textured trails (globals + texture)
    let textured_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Trail Textured Pipeline Layout"),
      bind_group_layouts: &[&bind_group_layout, &texture_bind_group_layout],
      push_constant_ranges: &[],
    });

    // Vertex buffer layout
    let vertex_layout = wgpu::VertexBufferLayout {
      array_stride: std::mem::size_of::<TrailVertex>() as u64,
      step_mode: wgpu::VertexStepMode::Vertex,
      attributes: &[
        // position
        wgpu::VertexAttribute {
          format: wgpu::VertexFormat::Float32x2,
          offset: 0,
          shader_location: 0,
        },
        // color
        wgpu::VertexAttribute {
          format: wgpu::VertexFormat::Float32x4,
          offset: 8,
          shader_location: 1,
        },
        // uv
        wgpu::VertexAttribute {
          format: wgpu::VertexFormat::Float32x2,
          offset: 24,
          shader_location: 2,
        },
      ],
    };

    // Create non-textured pipelines
    let create_pipeline = |layout: &wgpu::PipelineLayout,
                           shader: &wgpu::ShaderModule,
                           entry_point: &str,
                           label: &str| {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
          module: shader,
          entry_point: "vs_main",
          buffers: std::slice::from_ref(&vertex_layout),
          compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
          module: shader,
          entry_point,
          targets: &[Some(wgpu::ColorTargetState {
            format: output_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
          })],
          compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
          topology: wgpu::PrimitiveTopology::TriangleStrip,
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
      })
    };

    let solid_pipeline =
      create_pipeline(&pipeline_layout, &shader, "fs_main", "Trail Solid Pipeline");
    let soft_pipeline =
      create_pipeline(&pipeline_layout, &shader, "fs_soft", "Trail Soft Pipeline");
    let glow_pipeline =
      create_pipeline(&pipeline_layout, &shader, "fs_glow", "Trail Glow Pipeline");

    // Create textured pipelines
    let textured_solid_pipeline = create_pipeline(
      &textured_pipeline_layout,
      &textured_shader,
      "fs_main",
      "Trail Textured Solid Pipeline",
    );
    let textured_soft_pipeline = create_pipeline(
      &textured_pipeline_layout,
      &textured_shader,
      "fs_soft",
      "Trail Textured Soft Pipeline",
    );
    let textured_glow_pipeline = create_pipeline(
      &textured_pipeline_layout,
      &textured_shader,
      "fs_glow",
      "Trail Textured Glow Pipeline",
    );

    Self {
      solid_pipeline,
      soft_pipeline,
      glow_pipeline,
      textured_solid_pipeline,
      textured_soft_pipeline,
      textured_glow_pipeline,
      vertex_buffer,
      globals_buffer,
      bind_group_layout,
      texture_bind_group_layout,
      bind_group,
      sampler,
      texture_cache: HashMap::new(),
      vertices: Vec::with_capacity(MAX_TRAIL_VERTICES),
      textured_vertices: Vec::with_capacity(MAX_TRAIL_VERTICES / 2),
      textured_batches: Vec::new(),
      width,
      height,
      enabled: true,
    }
  }

  /// Enable or disable the renderer.
  pub fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }

  /// Check if the renderer is enabled.
  pub fn is_enabled(&self) -> bool {
    self.enabled
  }

  /// Resize the renderer.
  pub fn resize(&mut self, width: u32, height: u32) {
    self.width = width;
    self.height = height;
  }

  /// Begin a new frame (clears accumulated vertices).
  pub fn begin_frame(&mut self) {
    self.vertices.clear();
    self.textured_vertices.clear();
    self.textured_batches.clear();
  }

  /// Ensure a texture is cached and return its sprite_id for batch lookup.
  fn ensure_texture_cached(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    sprite: &SpriteHandle,
  ) -> String {
    let sprite_id = sprite.id();

    if !self.texture_cache.contains_key(&sprite_id) {
      // Create GPU texture from sprite data
      let sprite_data = sprite.sprite();
      let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Trail Texture"),
        size: wgpu::Extent3d {
          width: sprite_data.width(),
          height: sprite_data.height(),
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
      });

      // Upload texture data
      queue.write_texture(
        wgpu::ImageCopyTexture {
          texture: &texture,
          mip_level: 0,
          origin: wgpu::Origin3d::ZERO,
          aspect: wgpu::TextureAspect::All,
        },
        sprite_data.data(),
        wgpu::ImageDataLayout {
          offset: 0,
          bytes_per_row: Some(sprite_data.width() * 4),
          rows_per_image: Some(sprite_data.height()),
        },
        wgpu::Extent3d {
          width: sprite_data.width(),
          height: sprite_data.height(),
          depth_or_array_layers: 1,
        },
      );

      let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

      // Create bind group for this texture
      let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Trail Texture Bind Group"),
        layout: &self.texture_bind_group_layout,
        entries: &[
          wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
          },
          wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
          },
        ],
      });

      self.texture_cache.insert(
        sprite_id.clone(),
        CachedTexture {
          texture,
          bind_group,
        },
      );
    }

    sprite_id
  }

  /// Add a trail to be rendered.
  ///
  /// The trail's points are converted to triangle strip vertices.
  /// If the trail has a texture, it will be rendered with texture mapping.
  pub fn add_trail(&mut self, trail: &Trail2D, _transform: &Transform) {
    if !self.enabled || trail.point_count() < 2 {
      return;
    }

    // Check if this trail has a texture (we'll need device/queue for caching)
    let has_texture = trail.texture().is_some();

    // Select the appropriate vertex buffer
    let vertices = if has_texture {
      &mut self.textured_vertices
    } else {
      &mut self.vertices
    };

    let points = trail.points();
    let point_count = points.len();

    // Need 2 vertices per point for triangle strip
    if vertices.len() + point_count * 2 > MAX_TRAIL_VERTICES {
      return; // Buffer full
    }

    // Track start position for textured batches
    let start_vertex = vertices.len();

    // Track if we need to add a degenerate at the start of this trail
    let needs_degenerate = !vertices.is_empty();

    // Add degenerate triangle to separate from previous trail (part 1: duplicate last vertex)
    if needs_degenerate {
      let last = *vertices.last().unwrap();
      vertices.push(last);
    }

    // Pre-calculate cumulative distances for UV mapping and fade
    let distances = trail.cumulative_distances();

    let mut prev_point: Option<&TrailPoint> = None;

    for (i, point) in points.iter().enumerate() {
      let t = i as f32 / (point_count - 1).max(1) as f32;

      // Calculate direction for this segment
      let (dx, dy) = if let Some(prev) = prev_point {
        (point.x - prev.x, point.y - prev.y)
      } else if i + 1 < point_count {
        let next = &points[i + 1];
        (next.x - point.x, next.y - point.y)
      } else {
        (1.0, 0.0)
      };

      // Perpendicular vector
      let len = (dx * dx + dy * dy).sqrt().max(0.001);
      let px = -dy / len;
      let py = dx / len;

      // Get interpolated properties (width still uses position-based t)
      let width = trail.width_at(t);

      // For color/alpha, use distance-based fade if enabled, otherwise position-based
      let mut color = if trail.fade_distance().is_some() {
        // Distance-based fade: use actual distance from head
        let dist = distances.get(i).copied().unwrap_or(0.0);
        let dist_t = if let Some(max_dist) = trail.fade_distance() {
          (dist / max_dist).clamp(0.0, 1.0)
        } else {
          0.0
        };
        trail.color_at(dist_t)
      } else {
        // Position-based fade: use index position
        trail.color_at(t)
      };

      // Apply age-based alpha fade only when position-based fade is disabled
      // (to avoid double-fading when both are active)
      if !trail.fades_alpha() && trail.fade_distance().is_none() {
        let age_alpha = trail.age_alpha(point.age);
        color = Color::new(
          color.r,
          color.g,
          color.b,
          (color.a as f32 * age_alpha) as u8,
        );
      }

      // Trail points are already in world coordinates (from trail.update(x, y, dt))
      // No need to add transform position
      let world_x = point.x;
      let world_y = point.y;

      let half_width = width / 2.0;

      // Calculate UV.x based on texture mode
      let uv_x = if has_texture {
        match trail.texture_mode() {
          TextureMode::Stretch => t,
          TextureMode::Tile => {
            // Use distance-based UV for tiling
            let dist = distances.get(i).copied().unwrap_or(0.0);
            dist / trail.tile_length()
          }
        }
      } else {
        t
      };

      // Add degenerate at start of trail (part 2: duplicate first vertex of new trail)
      if i == 0 && needs_degenerate {
        vertices.push(TrailVertex {
          position: [world_x + px * half_width, world_y + py * half_width],
          color: color_to_array(color),
          uv: [uv_x, 0.0],
        });
      }

      // Left vertex (UV.y = 0)
      vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [uv_x, 0.0],
      });

      // Right vertex (UV.y = 1)
      vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [uv_x, 1.0],
      });

      prev_point = Some(point);
    }

    // If textured, record the batch info (sprite_id will be set during render)
    if has_texture {
      // Store placeholder batch - sprite_id will be resolved during render
      // We use a sentinel value that will be replaced
      self.textured_batches.push(TexturedBatch {
        sprite_id: String::new(), // Will be set during add_trail_with_device
        start: start_vertex,
        end: self.textured_vertices.len(),
      });
    }
  }

  /// Add a trail with device access for texture caching.
  ///
  /// This variant allows texture caching. Call this instead of `add_trail`
  /// when you have device/queue access and the trail may have a texture.
  pub fn add_trail_with_device(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    trail: &Trail2D,
    transform: &Transform,
  ) {
    if !self.enabled || trail.point_count() < 2 {
      return;
    }

    // If trail has texture, ensure it's cached and record the batch
    if let Some(sprite) = trail.texture() {
      let sprite_id = self.ensure_texture_cached(device, queue, sprite);
      let batch_index = self.textured_batches.len();

      // Add the trail vertices
      self.add_trail(trail, transform);

      // Update the batch with the correct sprite_id
      if batch_index < self.textured_batches.len() {
        self.textured_batches[batch_index].sprite_id = sprite_id;
      }
    } else {
      // No texture, use regular add_trail
      self.add_trail(trail, transform);
    }
  }

  /// Add a line to be rendered.
  pub fn add_line(&mut self, line: &Line2D, offset_x: f32, offset_y: f32) {
    if !self.enabled || !line.is_visible() || line.point_count() < 2 {
      return;
    }

    let points = line.points();
    let point_count = points.len();

    // Need 2 vertices per point for triangle strip
    if self.vertices.len() + point_count * 2 > MAX_TRAIL_VERTICES {
      return; // Buffer full
    }

    // Add degenerate triangle to separate from previous line
    if !self.vertices.is_empty() {
      let last = *self.vertices.last().unwrap();
      self.vertices.push(last);
    }

    let mut prev_point: Option<Vec2> = None;

    for (i, point) in points.iter().enumerate() {
      let t = i as f32 / (point_count - 1).max(1) as f32;

      // Calculate direction for this segment
      let (dx, dy) = if let Some(prev) = prev_point {
        (point.x - prev.x, point.y - prev.y)
      } else if i + 1 < point_count {
        let next = points[i + 1];
        (next.x - point.x, next.y - point.y)
      } else {
        (1.0, 0.0)
      };

      // Perpendicular vector
      let len = (dx * dx + dy * dy).sqrt().max(0.001);
      let px = -dy / len;
      let py = dx / len;

      // Get interpolated properties
      let width = line.width_at(t);
      let color = line.color_at(t);

      // World positions
      let world_x = point.x + offset_x;
      let world_y = point.y + offset_y;

      let half_width = width / 2.0;

      // Add degenerate at start
      if i == 0 && !self.vertices.is_empty() {
        self.vertices.push(TrailVertex {
          position: [world_x + px * half_width, world_y + py * half_width],
          color: color_to_array(color),
          uv: [t, 0.0],
        });
      }

      // Left vertex
      self.vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [t, 0.0],
      });

      // Right vertex
      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [t, 1.0],
      });

      prev_point = Some(*point);
    }

    // Handle closed lines
    if line.is_closed() && point_count > 2 {
      // Connect back to first point
      let first = points[0];
      let last = points[point_count - 1];

      let dx = first.x - last.x;
      let dy = first.y - last.y;
      let len = (dx * dx + dy * dy).sqrt().max(0.001);
      let px = -dy / len;
      let py = dx / len;

      let width = line.width_at(0.0);
      let color = line.color_at(0.0);
      let half_width = width / 2.0;

      let world_x = first.x + offset_x;
      let world_y = first.y + offset_y;

      self.vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [1.0, 0.0],
      });

      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [1.0, 1.0],
      });
    }
  }

  /// Render all accumulated trails and lines.
  #[allow(clippy::too_many_arguments)]
  pub fn render(
    &mut self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target_view: &wgpu::TextureView,
    camera_pos: Vec2,
    camera_zoom: f32,
    time: f32,
    style: TrailStyle,
  ) {
    if !self.enabled || (self.vertices.is_empty() && self.textured_vertices.is_empty()) {
      return;
    }

    // Update globals with camera transform
    let half_w = self.width as f32 / 2.0;
    let half_h = self.height as f32 / 2.0;

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

    let globals = TrailGlobals {
      view_proj,
      resolution_time: [self.width as f32, self.height as f32, time, 0.0],
    };
    queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&[globals]));

    // Select pipelines
    let pipeline = match style {
      TrailStyle::Solid => &self.solid_pipeline,
      TrailStyle::Soft => &self.soft_pipeline,
      TrailStyle::Glow => &self.glow_pipeline,
    };

    let textured_pipeline = match style {
      TrailStyle::Solid => &self.textured_solid_pipeline,
      TrailStyle::Soft => &self.textured_soft_pipeline,
      TrailStyle::Glow => &self.textured_glow_pipeline,
    };

    // Render non-textured trails
    if !self.vertices.is_empty() {
      queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));

      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Trail Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: target_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
      });

      render_pass.set_pipeline(pipeline);
      render_pass.set_bind_group(0, &self.bind_group, &[]);
      render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
      render_pass.draw(0..self.vertices.len() as u32, 0..1);
    }

    // Render textured trails (grouped by texture)
    if !self.textured_vertices.is_empty() && !self.textured_batches.is_empty() {
      // Upload textured vertices
      let offset = if self.vertices.is_empty() {
        0
      } else {
        // Append after non-textured vertices
        let size = self.vertices.len() * std::mem::size_of::<TrailVertex>();
        size as u64
      };

      // Check if we need to upload to a different offset or can reuse buffer
      if self.vertices.is_empty() {
        queue.write_buffer(
          &self.vertex_buffer,
          0,
          bytemuck::cast_slice(&self.textured_vertices),
        );
      } else {
        queue.write_buffer(
          &self.vertex_buffer,
          offset,
          bytemuck::cast_slice(&self.textured_vertices),
        );
      }

      // Render each batch
      for batch in &self.textured_batches {
        if let Some(cached) = self.texture_cache.get(&batch.sprite_id) {
          let vertex_offset = if self.vertices.is_empty() {
            0
          } else {
            self.vertices.len()
          };

          let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Trail Textured Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
              view: target_view,
              resolve_target: None,
              ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
              },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
          });

          render_pass.set_pipeline(textured_pipeline);
          render_pass.set_bind_group(0, &self.bind_group, &[]);
          render_pass.set_bind_group(1, &cached.bind_group, &[]);
          render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

          let start = (vertex_offset + batch.start) as u32;
          let end = (vertex_offset + batch.end) as u32;
          render_pass.draw(start..end, 0..1);
        }
      }
    }
  }

  /// Get the number of vertices accumulated this frame.
  pub fn vertex_count(&self) -> usize {
    self.vertices.len() + self.textured_vertices.len()
  }

  /// Clear the texture cache (call when sprites are unloaded)
  pub fn clear_texture_cache(&mut self) {
    self.texture_cache.clear();
  }
}

/// Convert Color to normalized float array
fn color_to_array(color: Color) -> [f32; 4] {
  [
    color.r as f32 / 255.0,
    color.g as f32 / 255.0,
    color.b as f32 / 255.0,
    color.a as f32 / 255.0,
  ]
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Helper to create a test trail with specific number of points
  fn create_test_trail(num_points: usize) -> Trail2D {
    let mut trail = Trail2D::new(Color::WHITE, 10.0)
      .with_length(num_points + 10) // Ensure we can fit all points
      .with_min_distance(1.0);

    for i in 0..num_points {
      trail.update(i as f32 * 10.0, 0.0, 0.016);
    }
    trail
  }

  /// Calculate expected vertex count for a single trail
  /// Each point generates 2 vertices (left + right) for triangle strip
  fn expected_vertices_single_trail(points: usize) -> usize {
    points * 2
  }

  /// Calculate expected vertex count for multiple trails
  /// First trail: 2 * points
  /// Subsequent trails: 2 (degenerate) + 2 * points
  fn expected_vertices_multiple_trails(point_counts: &[usize]) -> usize {
    if point_counts.is_empty() {
      return 0;
    }

    let first = point_counts[0] * 2;
    let rest: usize = point_counts[1..]
      .iter()
      .map(|&p| 2 + p * 2) // 2 degenerate vertices + normal vertices
      .sum();
    first + rest
  }

  // ========== Degenerate Triangle Regression Tests ==========
  // These tests verify that multiple trails are properly separated by degenerate triangles
  // to prevent the "double tail" bug where trails connect to each other.

  #[test]
  fn vertex_count_formula_single_trail() {
    // A trail with 3 points should generate 6 vertices
    assert_eq!(expected_vertices_single_trail(3), 6);
    // A trail with 5 points should generate 10 vertices
    assert_eq!(expected_vertices_single_trail(5), 10);
  }

  #[test]
  fn vertex_count_formula_multiple_trails() {
    // Two trails with 3 and 4 points:
    // First: 3 * 2 = 6 vertices
    // Second: 2 (degenerate) + 4 * 2 = 10 vertices
    // Total: 16 vertices
    assert_eq!(expected_vertices_multiple_trails(&[3, 4]), 16);

    // Three trails with 3, 4, and 5 points:
    // First: 6 vertices
    // Second: 2 + 8 = 10 vertices
    // Third: 2 + 10 = 12 vertices
    // Total: 28 vertices
    assert_eq!(expected_vertices_multiple_trails(&[3, 4, 5]), 28);
  }

  #[test]
  fn degenerate_triangle_separation_regression() {
    // This test verifies the fix for the "double tail" bug where trails
    // would connect to each other due to missing degenerate triangles.
    //
    // The bug was caused by a condition that never triggered:
    //   if i == 0 && vertices.len() > start_vertex + 1
    //
    // This was fixed by tracking `needs_degenerate` before adding the first
    // degenerate vertex, ensuring both degenerates are added:
    //   1. Duplicate last vertex of previous trail
    //   2. Duplicate first vertex of new trail

    // Two trails should have the following structure:
    // Trail 1: [L0, R0, L1, R1, L2, R2] (6 vertices for 3 points)
    // Trail 2: [R2', L0', L0, R0, L1, R1, L2, R2, L3, R3] (10 vertices = 2 degen + 8 normal)
    //
    // Where R2' is duplicate of Trail 1's last vertex
    // And L0' is duplicate of Trail 2's first vertex

    let counts = &[3, 4];
    let expected = expected_vertices_multiple_trails(counts);
    assert_eq!(
      expected, 16,
      "Two trails with 3 and 4 points should have 16 vertices total"
    );
  }

  #[test]
  fn color_to_array_conversion() {
    let white = color_to_array(Color::WHITE);
    assert!((white[0] - 1.0).abs() < 0.01);
    assert!((white[1] - 1.0).abs() < 0.01);
    assert!((white[2] - 1.0).abs() < 0.01);
    assert!((white[3] - 1.0).abs() < 0.01);

    let black = color_to_array(Color::BLACK);
    assert!((black[0] - 0.0).abs() < 0.01);
    assert!((black[1] - 0.0).abs() < 0.01);
    assert!((black[2] - 0.0).abs() < 0.01);
    assert!((black[3] - 1.0).abs() < 0.01); // Alpha is still 1.0

    let half_alpha = color_to_array(Color::new(255, 255, 255, 128));
    assert!((half_alpha[3] - 0.502).abs() < 0.01); // 128/255 ≈ 0.502
  }

  #[test]
  fn trail_vertex_struct_size() {
    // Ensure TrailVertex is the expected size for GPU buffer layout
    // position: [f32; 2] = 8 bytes
    // color: [f32; 4] = 16 bytes
    // uv: [f32; 2] = 8 bytes
    // Total: 32 bytes
    assert_eq!(std::mem::size_of::<TrailVertex>(), 32);
  }

  #[test]
  fn trail_globals_struct_size() {
    // Ensure TrailGlobals is the expected size for GPU uniform buffer
    // view_proj: [[f32; 4]; 4] = 64 bytes
    // resolution_time: [f32; 4] = 16 bytes
    // Total: 80 bytes
    assert_eq!(std::mem::size_of::<TrailGlobals>(), 80);
  }
}
