//! GPU-accelerated trail and line renderer
//!
//! Renders trails and lines efficiently using GPU instancing and batching.

use super::{Line2D, LineCap, LineJoin, TextureMode, Trail2D, TrailEffect, TrailPoint};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::Vec2;
use crate::core::graphics::TextureId;
use crate::core::ui::Color;
use std::cell::Cell;
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
  /// UV coordinates (x = along trail 0-1, y = across trail 0-1)
  uv: [f32; 2],
  /// Effect parameters: [type_and_style, param1, param2, param3]
  /// type_and_style encodes effect type (bits 0-3) and style (bits 4-5)
  effect_params: [f32; 4],
  /// Normalized perpendicular direction for GPU displacement effects
  perpendicular: [f32; 2],
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
  #[expect(dead_code, reason = "texture kept alive for GPU bind group lifetime")]
  texture: wgpu::Texture,
  bind_group: wgpu::BindGroup,
  /// The sprite content generation uploaded (hot reload bumps it)
  generation: u64,
}

/// A batch of textured trail vertices
struct TexturedBatch {
  /// The texture to draw with (set by `add_trail_with_device`)
  texture: Option<TextureId>,
  /// Starting vertex index
  start: usize,
  /// Ending vertex index (exclusive)
  end: usize,
}

/// The vertices of one geometry group.
#[derive(Default)]
struct Geometry {
  vertices: Vec<TrailVertex>,
  textured_vertices: Vec<TrailVertex>,
  textured_batches: Vec<TexturedBatch>,
}

impl Geometry {
  fn len(&self) -> usize {
    self.vertices.len() + self.textured_vertices.len()
  }

  fn clear(&mut self) {
    self.vertices.clear();
    self.textured_vertices.clear();
    self.textured_batches.clear();
  }
}

/// The geometry of the trails and lines sharing a lighting mode and z-order.
///
/// Groups drawn before lighting are lit; the others (`with_lighting(false)`)
/// draw after it.
struct Group {
  lit: bool,
  z: i32,
  /// Whether anything was added to the group this frame
  used: bool,
  geometry: Geometry,
  /// Where the group's vertices start in the vertex buffer, set by `prepare`
  base: Cell<usize>,
}

impl Group {
  /// Position in draw order: lit groups first, each by z.
  fn order(&self) -> (bool, i32) {
    (!self.lit, self.z)
  }
}

/// GPU-accelerated trail and line renderer.
pub struct TrailRenderer {
  /// Render pipeline for non-textured trails (uber-shader)
  pipeline: wgpu::RenderPipeline,
  /// Render pipeline for textured trails (uber-shader)
  textured_pipeline: wgpu::RenderPipeline,

  /// Vertex buffer
  vertex_buffer: wgpu::Buffer,

  /// Uniform buffer for globals
  globals_buffer: wgpu::Buffer,

  /// Bind group layout for non-textured trails (globals only)
  #[expect(dead_code, reason = "layout kept alive for shader validation")]
  bind_group_layout: wgpu::BindGroupLayout,

  /// Bind group layout for textured trails (texture + sampler)
  texture_bind_group_layout: wgpu::BindGroupLayout,

  /// Current bind group (globals)
  bind_group: wgpu::BindGroup,

  /// Sampler for textures
  sampler: wgpu::Sampler,

  /// Trail textures by the sprite renderer's texture id (sampled repeating
  /// along the trail, so not from the atlas)
  texture_cache: HashMap<TextureId, CachedTexture>,

  /// The group being built: its non-textured vertices
  vertices: Vec<TrailVertex>,

  /// The group being built: its textured vertices
  textured_vertices: Vec<TrailVertex>,

  /// The group being built: its textured batches (grouped by texture)
  textured_batches: Vec<TexturedBatch>,

  /// Vertices held by the groups other than the one being built
  other_vertices: usize,

  /// Vertices held by all groups (the one being built is swapped in only
  /// while it is built, so this counts it once it is done)
  total_vertices: usize,

  /// The frame's geometry by lighting mode and z-order, in draw order (lit
  /// groups first, each by z), which is also their layout in the vertex buffer
  groups: Vec<Group>,

  /// `(lit, z)` to the group's index in `groups`
  group_index: HashMap<(bool, i32), usize>,

  /// Whether geometry was dropped this frame because the vertex buffer is full
  dropped: bool,

  /// Whether the current run of frames with dropped geometry was reported
  drop_warned: std::cell::Cell<bool>,

  /// Screen space's size, which the projection spans
  screen: glam::Vec2,

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
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    // Create pipeline layout for textured trails (globals + texture)
    let textured_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Trail Textured Pipeline Layout"),
      bind_group_layouts: &[Some(&bind_group_layout), Some(&texture_bind_group_layout)],
      immediate_size: 0,
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
        // effect_params
        wgpu::VertexAttribute {
          format: wgpu::VertexFormat::Float32x4,
          offset: 32,
          shader_location: 3,
        },
        // perpendicular
        wgpu::VertexAttribute {
          format: wgpu::VertexFormat::Float32x2,
          offset: 48,
          shader_location: 4,
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
          entry_point: Some("vs_main"),
          buffers: &[Some(vertex_layout.clone())],
          compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
          module: shader,
          entry_point: Some(entry_point),
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
        multiview_mask: None,
        cache: None,
      })
    };

    let pipeline = create_pipeline(&pipeline_layout, &shader, "fs_main", "Trail Pipeline");
    let textured_pipeline = create_pipeline(
      &textured_pipeline_layout,
      &textured_shader,
      "fs_main",
      "Trail Textured Pipeline",
    );

    Self {
      pipeline,
      textured_pipeline,
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
      other_vertices: 0,
      total_vertices: 0,
      groups: Vec::new(),
      group_index: HashMap::new(),
      dropped: false,
      drop_warned: std::cell::Cell::new(false),
      screen: glam::Vec2::new(width as f32, height as f32),
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
    self.screen = glam::Vec2::new(width as f32, height as f32);
  }

  /// Set screen space's size, which trails and lines are projected into.
  pub(crate) fn set_screen_size(&mut self, size: glam::Vec2) {
    self.screen = size;
  }

  /// Begin a new frame (clears accumulated vertices).
  pub fn begin_frame(&mut self) {
    self.vertices.clear();
    self.textured_vertices.clear();
    self.textured_batches.clear();
    // Keep last frame's groups (and their allocations) for reuse
    let before = self.groups.len();
    self.groups.retain(|group| group.used);
    for group in &mut self.groups {
      group.geometry.clear();
      group.used = false;
    }
    if self.groups.len() != before {
      self.reindex_groups();
    }
    self.other_vertices = 0;
    self.total_vertices = 0;
    self.dropped = false;
  }

  fn reindex_groups(&mut self) {
    self.group_index.clear();
    for (index, group) in self.groups.iter().enumerate() {
      self.group_index.insert((group.lit, group.z), index);
    }
  }

  /// Build geometry with `add` into the group of this lighting mode and z.
  fn add_in_group(&mut self, lit: bool, z: i32, add: impl FnOnce(&mut Self)) {
    let index = match self.group_index.get(&(lit, z)) {
      Some(&index) => index,
      None => {
        // Keep the groups in draw order
        let order = (!lit, z);
        let index = self.groups.partition_point(|group| group.order() < order);
        self.groups.insert(
          index,
          Group {
            lit,
            z,
            used: false,
            geometry: Geometry::default(),
            base: Cell::new(0),
          },
        );
        self.reindex_groups();
        index
      }
    };
    let before = self.groups[index].geometry.len();
    self.other_vertices = self.total_vertices - before;
    self.groups[index].used = true;
    self.swap_group(index);
    add(self);
    self.swap_group(index);
    self.total_vertices = self.other_vertices + self.groups[index].geometry.len();
  }

  /// Swap the group at `index` with the working vertices.
  fn swap_group(&mut self, index: usize) {
    let geometry = &mut self.groups[index].geometry;
    std::mem::swap(&mut self.vertices, &mut geometry.vertices);
    std::mem::swap(&mut self.textured_vertices, &mut geometry.textured_vertices);
    std::mem::swap(&mut self.textured_batches, &mut geometry.textured_batches);
  }

  /// Ensure `sprite`'s texture is cached under `id`, rebuilt when its content
  /// changed.
  fn ensure_texture_cached(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    id: TextureId,
    sprite: &SpriteHandle,
  ) {
    let generation = sprite.generation();
    if self
      .texture_cache
      .get(&id)
      .is_none_or(|cached| cached.generation != generation)
    {
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
        wgpu::TexelCopyTextureInfo {
          texture: &texture,
          mip_level: 0,
          origin: wgpu::Origin3d::ZERO,
          aspect: wgpu::TextureAspect::All,
        },
        sprite_data.data(),
        wgpu::TexelCopyBufferLayout {
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
        id,
        CachedTexture {
          texture,
          bind_group,
          generation,
        },
      );
    }
  }

  /// Forget cached textures whose id fails `live` (the sprite renderer
  /// dropped them).
  pub(crate) fn retain_textures(&mut self, live: impl Fn(TextureId) -> bool) {
    self.texture_cache.retain(|id, _| live(*id));
  }

  /// Add a trail to be rendered.
  ///
  /// The trail's points are converted to triangle strip vertices.
  /// If the trail has a texture, it will be rendered with texture mapping.
  pub fn add_trail(&mut self, trail: &Trail2D) {
    self.add_in_group(trail.lighting(), trail.z_order(), |renderer| {
      renderer.push_trail(trail)
    });
  }

  /// Build a trail's geometry into the current (lit or unlit) group.
  fn push_trail(&mut self, trail: &Trail2D) {
    if !self.enabled || !trail.is_visible() || trail.point_count() < 2 {
      return;
    }

    // Check if this trail has a texture (we'll need device/queue for caching)
    let has_texture = trail.texture().is_some();

    // The whole frame shares one vertex buffer
    let used = self.other_vertices + self.vertices.len() + self.textured_vertices.len();

    // Select the appropriate vertex buffer
    let vertices = if has_texture {
      &mut self.textured_vertices
    } else {
      &mut self.vertices
    };

    let points = trail.points();
    let point_count = points.len();

    // Estimate total vertices: 2 per point (strip) + caps overhead
    let cap_overhead = match trail.cap() {
      LineCap::Butt => 0,
      LineCap::Round => 2 * (8 * 2 + 3), // 2 caps, each ~8 segments * 2 verts + degenerate
      LineCap::Square => 2 * 4,          // 2 caps, 4 vertices each
    };
    if used + point_count * 2 + cap_overhead > MAX_TRAIL_VERTICES {
      self.dropped = true;
      return; // Buffer full
    }

    // Track start position for textured batches
    let start_vertex = vertices.len();

    // Track if we need to add a degenerate at the start of this trail
    let needs_degenerate = !vertices.is_empty();

    // Pre-compute effect params once for all vertices in this trail
    let effect_params = build_effect_params(trail.effect(), trail.style());

    // Add degenerate triangle to separate from previous trail (part 1: duplicate last vertex)
    // Must be an exact copy so GPU displacement moves it identically to the source vertex.
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

      // Calculate UV.x based on texture mode or effect presence.
      // For GPU effects, use distance-based UV instead of index-based.
      // Index-based t shifts every frame as new points are added, causing the displacement
      // pattern to "slide" along the trail. Distance-based UV is stable per-point.
      let uv_x = if has_texture {
        match trail.texture_mode() {
          TextureMode::Stretch => t,
          TextureMode::Tile => {
            // Use distance-based UV for tiling
            let dist = distances.get(i).copied().unwrap_or(0.0);
            dist / trail.tile_length()
          }
        }
      } else if trail.effect().is_some() {
        // Distance-based: normalize by total trail length for a stable 0..1 range
        let dist = distances.get(i).copied().unwrap_or(0.0);
        let total = distances.last().copied().unwrap_or(1.0).max(0.001);
        dist / total
      } else {
        t
      };

      // Add degenerate at start of trail (part 2: duplicate first vertex of new trail)
      // Must use same effect_params and perpendicular as the real first-left vertex so GPU
      // displacement moves them identically, keeping the bridge triangle zero-area.
      if i == 0 && needs_degenerate {
        vertices.push(TrailVertex {
          position: [world_x + px * half_width, world_y + py * half_width],
          color: color_to_array(color),
          uv: [uv_x, 0.0],
          effect_params,
          perpendicular: [px, py],
        });
      }

      // Left vertex (UV.y = 0)
      vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [uv_x, 0.0],
        effect_params,
        perpendicular: [px, py],
      });

      // Right vertex (UV.y = 1)
      vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [uv_x, 1.0],
        effect_params,
        perpendicular: [px, py],
      });

      prev_point = Some(point);
    }

    // Generate cap geometry at endpoints if cap != Butt
    let cap = trail.cap();
    if cap != LineCap::Butt && point_count >= 2 {
      // Tail cap (oldest point, last in the VecDeque) — appended first since
      // the strip ends at the tail. The outward direction is AWAY from the trail
      // (from second-to-last toward last point).
      {
        let tail = &points[point_count - 1];
        let prev = &points[point_count - 2];
        let tdx = tail.x - prev.x;
        let tdy = tail.y - prev.y;
        let tlen = (tdx * tdx + tdy * tdy).sqrt().max(0.001);
        let dir_x = tdx / tlen;
        let dir_y = tdy / tlen;
        // Perpendicular at the tail
        let tpx = -dir_y;
        let tpy = dir_x;

        let t_tail = 1.0;
        let tail_width = trail.width_at(t_tail);
        let tail_half_width = tail_width / 2.0;
        let tail_color = if trail.fade_distance().is_some() {
          let dist = distances.get(point_count - 1).copied().unwrap_or(0.0);
          let dist_t = if let Some(max_dist) = trail.fade_distance() {
            (dist / max_dist).clamp(0.0, 1.0)
          } else {
            0.0
          };
          trail.color_at(dist_t)
        } else {
          trail.color_at(t_tail)
        };

        let tail_uv_x = if has_texture {
          match trail.texture_mode() {
            TextureMode::Stretch => t_tail,
            TextureMode::Tile => {
              let dist = distances.get(point_count - 1).copied().unwrap_or(0.0);
              dist / trail.tile_length()
            }
          }
        } else {
          t_tail
        };

        match cap {
          LineCap::Round => {
            generate_round_cap(
              vertices,
              tail.x,
              tail.y,
              dir_x,
              dir_y,
              tpx,
              tpy,
              tail_half_width,
              color_to_array(tail_color),
              tail_uv_x,
              effect_params,
            );
          }
          LineCap::Square => {
            generate_square_cap(
              vertices,
              tail.x,
              tail.y,
              dir_x,
              dir_y,
              tpx,
              tpy,
              tail_half_width,
              color_to_array(tail_color),
              tail_uv_x,
              effect_params,
            );
          }
          LineCap::Butt => {} // Already handled by early return
        }
      }

      // Head cap (newest point, first in the VecDeque)
      // The outward direction is AWAY from the trail (opposite of the first segment direction)
      {
        let head = &points[0];
        let next = &points[1];
        let hdx = next.x - head.x;
        let hdy = next.y - head.y;
        let hlen = (hdx * hdx + hdy * hdy).sqrt().max(0.001);
        // Direction away from the trail at the head (opposite of segment direction)
        let dir_x = -hdx / hlen;
        let dir_y = -hdy / hlen;
        // Perpendicular at the head
        let hpx = -hdy / hlen;
        let hpy = hdx / hlen;

        let t_head = 0.0;
        let head_width = trail.width_at(t_head);
        let head_half_width = head_width / 2.0;
        let head_color = trail.color_at(t_head);

        let head_uv_x = if has_texture {
          match trail.texture_mode() {
            TextureMode::Stretch => t_head,
            TextureMode::Tile => 0.0,
          }
        } else {
          t_head
        };

        match cap {
          LineCap::Round => {
            generate_round_cap(
              vertices,
              head.x,
              head.y,
              dir_x,
              dir_y,
              hpx,
              hpy,
              head_half_width,
              color_to_array(head_color),
              head_uv_x,
              effect_params,
            );
          }
          LineCap::Square => {
            // For square cap at the head, we need a degenerate separator first
            // since the square cap naturally continues the strip but the head
            // is not the end of the strip
            if !vertices.is_empty() {
              let last = *vertices.last().unwrap();
              vertices.push(last);
            }
            let ext_x = head.x + dir_x * head_half_width;
            let ext_y = head.y + dir_y * head_half_width;
            // Degenerate: duplicate first vertex
            vertices.push(TrailVertex {
              position: [ext_x + hpx * head_half_width, ext_y + hpy * head_half_width],
              color: color_to_array(head_color),
              uv: [head_uv_x, 0.0],
              effect_params,
              perpendicular: [hpx, hpy],
            });
            // Left
            vertices.push(TrailVertex {
              position: [ext_x + hpx * head_half_width, ext_y + hpy * head_half_width],
              color: color_to_array(head_color),
              uv: [head_uv_x, 0.0],
              effect_params,
              perpendicular: [hpx, hpy],
            });
            // Right
            vertices.push(TrailVertex {
              position: [ext_x - hpx * head_half_width, ext_y - hpy * head_half_width],
              color: color_to_array(head_color),
              uv: [head_uv_x, 1.0],
              effect_params,
              perpendicular: [hpx, hpy],
            });
            // Connect back to head position
            vertices.push(TrailVertex {
              position: [
                head.x + hpx * head_half_width,
                head.y + hpy * head_half_width,
              ],
              color: color_to_array(head_color),
              uv: [head_uv_x, 0.0],
              effect_params,
              perpendicular: [hpx, hpy],
            });
            vertices.push(TrailVertex {
              position: [
                head.x - hpx * head_half_width,
                head.y - hpy * head_half_width,
              ],
              color: color_to_array(head_color),
              uv: [head_uv_x, 1.0],
              effect_params,
              perpendicular: [hpx, hpy],
            });
          }
          LineCap::Butt => {}
        }
      }
    }

    // If textured, record the batch (its texture is set by add_trail_with_device)
    if has_texture {
      self.textured_batches.push(TexturedBatch {
        texture: None,
        start: start_vertex,
        end: self.textured_vertices.len(),
      });
    }
  }

  /// Add a trail with device access for texture caching.
  ///
  /// This variant allows texture caching. Call this instead of `add_trail`
  /// when you have device/queue access and the trail may have a texture;
  /// `texture` is the sprite renderer's id for it.
  pub(crate) fn add_trail_with_device(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    trail: &Trail2D,
    texture: Option<TextureId>,
  ) {
    self.add_in_group(trail.lighting(), trail.z_order(), |renderer| {
      renderer.push_trail_with_device(device, queue, trail, texture)
    });
  }

  fn push_trail_with_device(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    trail: &Trail2D,
    texture: Option<TextureId>,
  ) {
    if !self.enabled || !trail.is_visible() || trail.point_count() < 2 {
      return;
    }

    // If trail has texture, ensure it's cached and record the batch
    if let (Some(sprite), Some(id)) = (trail.texture(), texture) {
      self.ensure_texture_cached(device, queue, id, sprite);
      let batch_index = self.textured_batches.len();

      // Add the trail vertices
      self.push_trail(trail);

      // Point the batch at its texture
      if let Some(batch) = self.textured_batches.get_mut(batch_index) {
        batch.texture = Some(id);
      }
    } else {
      // No texture, use regular add_trail
      self.push_trail(trail);
    }
  }

  /// Add a line to be rendered.
  pub fn add_line(&mut self, line: &Line2D, offset: Vec2) {
    self.add_in_group(line.lighting(), line.z_order(), |renderer| {
      renderer.push_line(line, offset)
    });
  }

  fn push_line(&mut self, line: &Line2D, offset: Vec2) {
    let (offset_x, offset_y) = (offset.x, offset.y);
    if !self.enabled || !line.is_visible() || line.point_count() < 2 {
      return;
    }

    let points = line.points();
    let point_count = points.len();

    // Estimate total vertices: 2 per point (strip) + caps + joins overhead
    let cap_overhead = match line.cap() {
      LineCap::Butt => 0,
      LineCap::Round => 2 * (8 * 2 + 3), // 2 caps, each ~8 segments * 2 verts + degenerate
      LineCap::Square => 2 * 4,          // 2 caps, 4 vertices each
    };
    let join_overhead = match line.join() {
      LineJoin::None => 0,
      LineJoin::Bevel | LineJoin::Miter => point_count.saturating_sub(2) * 6,
      LineJoin::Round => point_count.saturating_sub(2) * 20,
    };
    let used = self.other_vertices + self.vertices.len() + self.textured_vertices.len();
    if used + point_count * 2 + cap_overhead + join_overhead > MAX_TRAIL_VERTICES {
      self.dropped = true;
      return; // Buffer full
    }

    // Lines don't have effects, so only encode the style
    let effect_params = build_effect_params(None, line.style());

    let join = line.join();
    let cap = line.cap();

    // Add degenerate triangle to separate from previous line
    if !self.vertices.is_empty() {
      let last = *self.vertices.last().unwrap();
      self.vertices.push(last);
    }

    // Pre-compute per-segment perpendiculars for join geometry
    // segment_perps[i] = perpendicular for segment from point[i] to point[i+1]
    let mut segment_perps: Vec<(f32, f32)> = Vec::with_capacity(point_count);
    for i in 0..point_count {
      let (dx, dy) = if i + 1 < point_count {
        let next = points[i + 1];
        (next.x - points[i].x, next.y - points[i].y)
      } else if i > 0 {
        // Last point: use direction from previous
        (points[i].x - points[i - 1].x, points[i].y - points[i - 1].y)
      } else {
        (1.0, 0.0)
      };
      let len = (dx * dx + dy * dy).sqrt().max(0.001);
      segment_perps.push((-dy / len, dx / len));
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
          effect_params,
          perpendicular: [0.0, 0.0],
        });
      }

      // Handle interior join points — all join types use inline vertex pairs
      // embedded directly in the strip. No separate/deferred geometry needed.
      let is_interior_join = join != LineJoin::None && i > 0 && i < point_count - 1;
      let mut join_handled = false;

      if is_interior_join {
        let (ppx, ppy) = segment_perps[i - 1];
        let (npx, npy) = segment_perps[i];
        let cross = ppx * npy - ppy * npx;
        let perp_dot = ppx * npx + ppy * npy;
        let needs_join = perp_dot < 0.999;
        let color_arr = color_to_array(color);

        if needs_join {
          // Determine inside/outside based on turn direction.
          // cross < 0 in screen coords (Y-down) means left turn → outside is +perp side.
          // cross > 0 means right turn → outside is -perp side.
          //
          // IMPORTANT: Vertex pairs must always emit in the same order as normal
          // vertices: +perp side (UV.y=0) FIRST, -perp side (UV.y=1) SECOND.
          // This preserves strip winding. The outside_sign tells us which
          // geometric side is "outside" for positioning the join shape.
          let outside_sign: f32 = if cross < 0.0 { 1.0 } else { -1.0 };

          // Helper: emit a full-width vertex pair in correct strip order
          // (+perp first UV.y=0, -perp second UV.y=1)
          let emit_pair = |verts: &mut Vec<TrailVertex>, px: f32, py: f32, t: f32, ep: [f32; 4]| {
            verts.push(TrailVertex {
              position: [world_x + px * half_width, world_y + py * half_width],
              color: color_arr,
              uv: [t, 0.0],
              effect_params: ep,
              perpendicular: [px, py],
            });
            verts.push(TrailVertex {
              position: [world_x - px * half_width, world_y - py * half_width],
              color: color_arr,
              uv: [t, 1.0],
              effect_params: ep,
              perpendicular: [px, py],
            });
          };

          match join {
            LineJoin::Miter => {
              let mx = ppx + npx;
              let my = ppy + npy;
              let mlen = (mx * mx + my * my).sqrt();
              let mut use_miter = false;

              if mlen > 0.001 {
                let mnx = mx / mlen;
                let mny = my / mlen;
                let dot = mnx * ppx + mny * ppy;
                if dot.abs() > 0.001 {
                  let miter_length = 1.0 / dot;
                  if miter_length.abs() <= 4.0 {
                    use_miter = true;
                    join_handled = true;

                    let spike_x = world_x + outside_sign * mnx * half_width * miter_length;
                    let spike_y = world_y + outside_sign * mny * half_width * miter_length;

                    // Determine which slot the spike goes in based on outside direction
                    // outside_sign=+1 → spike is on +perp side → UV.y=0 (first vertex)
                    // outside_sign=-1 → spike is on -perp side → UV.y=1 (second vertex)
                    let spike_uv_y = if outside_sign > 0.0 { 0.0 } else { 1.0 };
                    let inside_uv_y = if outside_sign > 0.0 { 1.0 } else { 0.0 };
                    let inside_sign = -outside_sign;

                    // Pair 1: incoming perp
                    if outside_sign > 0.0 {
                      // Spike is +perp side (first), inside is -perp side (second)
                      self.vertices.push(TrailVertex {
                        position: [spike_x, spike_y],
                        color: color_arr,
                        uv: [t, spike_uv_y],
                        effect_params,
                        perpendicular: [mnx, mny],
                      });
                      self.vertices.push(TrailVertex {
                        position: [
                          world_x + inside_sign * ppx * half_width,
                          world_y + inside_sign * ppy * half_width,
                        ],
                        color: color_arr,
                        uv: [t, inside_uv_y],
                        effect_params,
                        perpendicular: [ppx, ppy],
                      });
                    } else {
                      // Inside is +perp side (first), spike is -perp side (second)
                      self.vertices.push(TrailVertex {
                        position: [
                          world_x + inside_sign * ppx * half_width,
                          world_y + inside_sign * ppy * half_width,
                        ],
                        color: color_arr,
                        uv: [t, inside_uv_y],
                        effect_params,
                        perpendicular: [ppx, ppy],
                      });
                      self.vertices.push(TrailVertex {
                        position: [spike_x, spike_y],
                        color: color_arr,
                        uv: [t, spike_uv_y],
                        effect_params,
                        perpendicular: [mnx, mny],
                      });
                    }
                    // Pair 2: outgoing perp
                    if outside_sign > 0.0 {
                      self.vertices.push(TrailVertex {
                        position: [spike_x, spike_y],
                        color: color_arr,
                        uv: [t, spike_uv_y],
                        effect_params,
                        perpendicular: [mnx, mny],
                      });
                      self.vertices.push(TrailVertex {
                        position: [
                          world_x + inside_sign * npx * half_width,
                          world_y + inside_sign * npy * half_width,
                        ],
                        color: color_arr,
                        uv: [t, inside_uv_y],
                        effect_params,
                        perpendicular: [npx, npy],
                      });
                    } else {
                      self.vertices.push(TrailVertex {
                        position: [
                          world_x + inside_sign * npx * half_width,
                          world_y + inside_sign * npy * half_width,
                        ],
                        color: color_arr,
                        uv: [t, inside_uv_y],
                        effect_params,
                        perpendicular: [npx, npy],
                      });
                      self.vertices.push(TrailVertex {
                        position: [spike_x, spike_y],
                        color: color_arr,
                        uv: [t, spike_uv_y],
                        effect_params,
                        perpendicular: [mnx, mny],
                      });
                    }
                  }
                }
              }
              // Miter limit exceeded — fall back to bevel
              if !use_miter {
                join_handled = true;
                emit_pair(&mut self.vertices, ppx, ppy, t, effect_params);
                emit_pair(&mut self.vertices, npx, npy, t, effect_params);
              }
            }
            LineJoin::Bevel => {
              join_handled = true;
              emit_pair(&mut self.vertices, ppx, ppy, t, effect_params);
              emit_pair(&mut self.vertices, npx, npy, t, effect_params);
            }
            LineJoin::Round => {
              join_handled = true;
              let segments = ROUND_JOIN_SEGMENTS;

              // Determine rotation direction for the outside arc
              let (start_ox, start_oy, end_ox, end_oy) = if cross < 0.0 {
                (ppx, ppy, npx, npy) // +perp is outside (left turn)
              } else {
                (-ppx, -ppy, -npx, -npy) // -perp is outside
              };
              let start_cross_end = start_ox * end_oy - start_oy * end_ox;
              let dot_se = (start_ox * end_ox + start_oy * end_oy).clamp(-1.0, 1.0);
              let total_angle = dot_se.acos();
              let inside_sign = -outside_sign;

              for k in 0..=segments {
                let frac = k as f32 / segments as f32;
                let angle = total_angle * frac;
                let cos_a = angle.cos();
                let sin_a = angle.sin();

                let (rot_ox, rot_oy) = if start_cross_end >= 0.0 {
                  (
                    start_ox * cos_a - start_oy * sin_a,
                    start_oy * cos_a + start_ox * sin_a,
                  )
                } else {
                  (
                    start_ox * cos_a + start_oy * sin_a,
                    start_oy * cos_a - start_ox * sin_a,
                  )
                };

                // Inside: interpolate between incoming and outgoing perps
                let in_px = ppx + (npx - ppx) * frac;
                let in_py = ppy + (npy - ppy) * frac;

                // Always emit +perp side (UV.y=0) first, -perp side (UV.y=1) second
                if outside_sign > 0.0 {
                  // Outside arc is +perp side (first), inside is -perp side (second)
                  self.vertices.push(TrailVertex {
                    position: [world_x + rot_ox * half_width, world_y + rot_oy * half_width],
                    color: color_arr,
                    uv: [t, 0.0],
                    effect_params,
                    perpendicular: [rot_ox, rot_oy],
                  });
                  self.vertices.push(TrailVertex {
                    position: [
                      world_x + inside_sign * in_px * half_width,
                      world_y + inside_sign * in_py * half_width,
                    ],
                    color: color_arr,
                    uv: [t, 1.0],
                    effect_params,
                    perpendicular: [in_px, in_py],
                  });
                } else {
                  // Inside is +perp side (first), outside arc is -perp side (second)
                  self.vertices.push(TrailVertex {
                    position: [
                      world_x + inside_sign * in_px * half_width,
                      world_y + inside_sign * in_py * half_width,
                    ],
                    color: color_arr,
                    uv: [t, 0.0],
                    effect_params,
                    perpendicular: [in_px, in_py],
                  });
                  self.vertices.push(TrailVertex {
                    position: [world_x + rot_ox * half_width, world_y + rot_oy * half_width],
                    color: color_arr,
                    uv: [t, 1.0],
                    effect_params,
                    perpendicular: [rot_ox, rot_oy],
                  });
                }
              }
            }
            LineJoin::None => {} // Already handled by early check
          }
        }
      }

      // Normal vertices: non-join points, or join with insignificant angle
      if !join_handled {
        let color_arr = color_to_array(color);
        self.vertices.push(TrailVertex {
          position: [world_x + px * half_width, world_y + py * half_width],
          color: color_arr,
          uv: [t, 0.0],
          effect_params,
          perpendicular: [px, py],
        });
        self.vertices.push(TrailVertex {
          position: [world_x - px * half_width, world_y - py * half_width],
          color: color_arr,
          uv: [t, 1.0],
          effect_params,
          perpendicular: [px, py],
        });
      }

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
        effect_params,
        perpendicular: [px, py],
      });

      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [1.0, 1.0],
        effect_params,
        perpendicular: [px, py],
      });
    }

    // Generate cap geometry at endpoints (only for non-closed lines)
    if cap != LineCap::Butt && !line.is_closed() && point_count >= 2 {
      // End cap (last point) — appended to the strip naturally
      {
        let end_pt = points[point_count - 1];
        let prev_pt = points[point_count - 2];
        let edx = end_pt.x - prev_pt.x;
        let edy = end_pt.y - prev_pt.y;
        let elen = (edx * edx + edy * edy).sqrt().max(0.001);
        let dir_x = edx / elen;
        let dir_y = edy / elen;
        let epx = -edy / elen;
        let epy = edx / elen;

        let t_end = 1.0;
        let end_width = line.width_at(t_end);
        let end_half_width = end_width / 2.0;
        let end_color = line.color_at(t_end);
        let end_world_x = end_pt.x + offset_x;
        let end_world_y = end_pt.y + offset_y;

        match cap {
          LineCap::Round => {
            generate_round_cap(
              &mut self.vertices,
              end_world_x,
              end_world_y,
              dir_x,
              dir_y,
              epx,
              epy,
              end_half_width,
              color_to_array(end_color),
              t_end,
              effect_params,
            );
          }
          LineCap::Square => {
            generate_square_cap(
              &mut self.vertices,
              end_world_x,
              end_world_y,
              dir_x,
              dir_y,
              epx,
              epy,
              end_half_width,
              color_to_array(end_color),
              t_end,
              effect_params,
            );
          }
          LineCap::Butt => {}
        }
      }

      // Start cap (first point)
      {
        let start_pt = points[0];
        let next_pt = points[1];
        let sdx = next_pt.x - start_pt.x;
        let sdy = next_pt.y - start_pt.y;
        let slen = (sdx * sdx + sdy * sdy).sqrt().max(0.001);
        // Direction away from the trail at the start
        let dir_x = -sdx / slen;
        let dir_y = -sdy / slen;
        let spx = -sdy / slen;
        let spy = sdx / slen;

        let t_start = 0.0;
        let start_width = line.width_at(t_start);
        let start_half_width = start_width / 2.0;
        let start_color = line.color_at(t_start);
        let start_world_x = start_pt.x + offset_x;
        let start_world_y = start_pt.y + offset_y;

        match cap {
          LineCap::Round => {
            generate_round_cap(
              &mut self.vertices,
              start_world_x,
              start_world_y,
              dir_x,
              dir_y,
              spx,
              spy,
              start_half_width,
              color_to_array(start_color),
              t_start,
              effect_params,
            );
          }
          LineCap::Square => {
            // Square cap at start needs degenerate separator since it's not at
            // the end of the strip
            if !self.vertices.is_empty() {
              let last = *self.vertices.last().unwrap();
              self.vertices.push(last);
            }
            let ext_x = start_world_x + dir_x * start_half_width;
            let ext_y = start_world_y + dir_y * start_half_width;
            vertices_push_square_cap_head(
              &mut self.vertices,
              ext_x,
              ext_y,
              start_world_x,
              start_world_y,
              spx,
              spy,
              start_half_width,
              color_to_array(start_color),
              t_start,
              effect_params,
            );
          }
          LineCap::Butt => {}
        }
      }
    }
  }

  /// Render the trails and lines that scene lighting applies to.
  #[allow(clippy::too_many_arguments)]
  pub fn render(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target_view: &wgpu::TextureView,
    camera_pos: Vec2,
    camera_zoom: f32,
    time: f32,
    _style: TrailStyle,
  ) {
    self.render_groups(
      queue,
      encoder,
      target_view,
      (camera_pos, camera_zoom, time),
      true,
    );
  }

  /// Render the trails and lines created with `with_lighting(false)`.
  ///
  /// Call after lighting, onto the lit scene. Their geometry sits after the lit
  /// geometry in the vertex buffer, so both can be drawn in the same frame.
  pub fn render_unlit(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target_view: &wgpu::TextureView,
    camera_pos: Vec2,
    camera_zoom: f32,
    time: f32,
  ) {
    self.render_groups(
      queue,
      encoder,
      target_view,
      (camera_pos, camera_zoom, time),
      false,
    );
  }

  /// Upload and draw the groups of one lighting mode in a pass of their own.
  fn render_groups(
    &self,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target_view: &wgpu::TextureView,
    camera: (Vec2, f32, f32),
    lit: bool,
  ) {
    let mut zs: Vec<i32> = self.groups(lit).collect();
    if zs.is_empty() {
      return;
    }
    zs.sort_unstable();
    self.prepare(queue, camera);
    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
      label: Some("Trail Render Pass"),
      color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: target_view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
          load: wgpu::LoadOp::Load,
          store: wgpu::StoreOp::Store,
        },
      })],
      depth_stencil_attachment: None,
      occlusion_query_set: None,
      multiview_mask: None,
      timestamp_writes: None,
    });
    for z in zs {
      self.draw_group(&mut render_pass, lit, z);
    }
  }

  /// The z of every group of one lighting mode that has geometry to draw.
  pub(crate) fn groups(&self, lit: bool) -> impl Iterator<Item = i32> + '_ {
    self
      .groups
      .iter()
      .filter(move |group| self.enabled && group.lit == lit && group.geometry.len() > 0)
      .map(|group| group.z)
  }

  /// Upload the frame's globals and every group's vertices.
  ///
  /// Call before the render pass that draws groups with
  /// [`draw_group`](Self::draw_group); buffer writes are not allowed
  /// mid-pass.
  pub(crate) fn prepare(
    &self,
    queue: &wgpu::Queue,
    (camera_pos, camera_zoom, time): (Vec2, f32, f32),
  ) {
    if !self.enabled {
      return;
    }
    let mut warned = self.drop_warned.get();
    let warn = should_warn_about_drops(self.dropped, &mut warned);
    self.drop_warned.set(warned);
    if warn {
      crate::engine_warn!(
        module: "Trails",
        "Trail/line vertex limit ({MAX_TRAIL_VERTICES}) reached; some trails or lines were not drawn this frame"
      );
    }

    // Update globals with camera transform
    let half_w = self.screen.x / 2.0;
    let half_h = self.screen.y / 2.0;

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
      resolution_time: [self.screen.x, self.screen.y, time, 0.0],
    };
    queue.write_buffer(&self.globals_buffer, 0, bytemuck::cast_slice(&[globals]));

    let vertex_size = std::mem::size_of::<TrailVertex>();
    // The groups are in draw order, so each starts where the last one ended
    let mut base = 0;
    for group in &self.groups {
      let geometry = &group.geometry;
      group.base.set(base);
      let start = base;
      base += geometry.len();
      if geometry.len() == 0 || start + geometry.len() > MAX_TRAIL_VERTICES {
        continue;
      }
      let base = start;
      if !geometry.vertices.is_empty() {
        queue.write_buffer(
          &self.vertex_buffer,
          (base * vertex_size) as u64,
          bytemuck::cast_slice(&geometry.vertices),
        );
      }
      if !geometry.textured_vertices.is_empty() {
        let textured_base = base + geometry.vertices.len();
        queue.write_buffer(
          &self.vertex_buffer,
          (textured_base * vertex_size) as u64,
          bytemuck::cast_slice(&geometry.textured_vertices),
        );
      }
    }
  }

  /// Draw one group inside a render pass, after [`prepare`](Self::prepare).
  ///
  /// Draws nothing when the group is empty or does not exist. Leaves the
  /// pipeline and bind groups set for trails, so a caller drawing something
  /// else next sets its own.
  pub(crate) fn draw_group<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, lit: bool, z: i32) {
    if !self.enabled {
      return;
    }
    let Some(group) = self
      .group_index
      .get(&(lit, z))
      .and_then(|&index| self.groups.get(index))
    else {
      return;
    };
    let geometry = &group.geometry;
    let base = group.base.get();
    if geometry.len() == 0 || base + geometry.len() > MAX_TRAIL_VERTICES {
      return;
    }

    pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
    if !geometry.vertices.is_empty() {
      pass.set_pipeline(&self.pipeline);
      pass.set_bind_group(0, &self.bind_group, &[]);
      pass.draw(base as u32..(base + geometry.vertices.len()) as u32, 0..1);
    }

    // Textured trails (grouped by texture) follow the non-textured ones
    let textured_base = base + geometry.vertices.len();
    for batch in &geometry.textured_batches {
      if let Some(cached) = batch.texture.and_then(|id| self.texture_cache.get(&id)) {
        pass.set_pipeline(&self.textured_pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_bind_group(1, &cached.bind_group, &[]);
        let start = (textured_base + batch.start) as u32;
        let end = (textured_base + batch.end) as u32;
        pass.draw(start..end, 0..1);
      }
    }
  }

  /// How many draw calls [`draw_group`](Self::draw_group) issues for all groups.
  pub(crate) fn draw_count(&self) -> usize {
    if !self.enabled {
      return 0;
    }
    self
      .groups
      .iter()
      .map(|group| {
        let geometry = &group.geometry;
        let textured = geometry
          .textured_batches
          .iter()
          .filter(|batch| {
            batch
              .texture
              .is_some_and(|id| self.texture_cache.contains_key(&id))
          })
          .count();
        usize::from(!geometry.vertices.is_empty()) + textured
      })
      .sum()
  }

  /// Get the number of vertices accumulated this frame.
  pub fn vertex_count(&self) -> usize {
    self.total_vertices
  }

  /// Whether any trail or line skips lighting this frame.
  pub fn has_unlit(&self) -> bool {
    self.groups(false).next().is_some()
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

/// Encode effect type and style into a single f32 value.
/// Bits 0-3: effect type (0=none, 1=lightning, 2=noise, 3=scroll, 4=ribbon)
/// Bits 4-5: style (0=solid, 1=soft, 2=glow)
fn encode_type_and_style(effect_type: u32, style: u32) -> f32 {
  (effect_type | (style << 4)) as f32
}

/// Get the effect type index for a TrailEffect.
fn effect_type_index(effect: Option<&TrailEffect>) -> u32 {
  match effect {
    None => 0,
    Some(TrailEffect::Lightning { .. }) => 1,
    Some(TrailEffect::Noise { .. }) => 2,
    Some(TrailEffect::UvScroll { .. }) => 3,
    Some(TrailEffect::Ribbon { .. }) => 4,
  }
}

/// Get the style index for a TrailStyle.
fn style_index(style: TrailStyle) -> u32 {
  match style {
    TrailStyle::Solid => 0,
    TrailStyle::Soft => 1,
    TrailStyle::Glow => 2,
  }
}

/// Build the effect_params array for a trail vertex.
fn build_effect_params(effect: Option<&TrailEffect>, style: TrailStyle) -> [f32; 4] {
  let type_and_style = encode_type_and_style(effect_type_index(effect), style_index(style));
  match effect {
    None => [type_and_style, 0.0, 0.0, 0.0],
    Some(TrailEffect::Lightning {
      jitter,
      speed,
      detail,
    }) => [type_and_style, *jitter, *speed, *detail as f32],
    Some(TrailEffect::Noise {
      frequency,
      amplitude,
      speed,
    }) => [type_and_style, *frequency, *amplitude, *speed],
    Some(TrailEffect::UvScroll { speed }) => [type_and_style, *speed, 0.0, 0.0],
    Some(TrailEffect::Ribbon {
      amplitude,
      frequency,
      speed,
    }) => [type_and_style, *amplitude, *frequency, *speed],
  }
}

// =====================================================================
// Line Cap Geometry Generation
// =====================================================================

/// Number of segments for round caps
const ROUND_CAP_SEGMENTS: u32 = 8;

/// Generate round cap geometry at an endpoint, appending to the vertex buffer.
///
/// # Parameters
/// - `vertices`: vertex buffer to append to
/// - `cx, cy`: center position of the cap (the endpoint)
/// - `dx, dy`: direction AWAY from the trail (outward from this endpoint)
/// - `px, py`: perpendicular direction at the endpoint
/// - `half_width`: half the trail width at this endpoint
/// - `color`: color array for the cap vertices
/// - `uv_x`: UV.x coordinate at this endpoint
/// - `effect_params`: effect parameters for the cap vertices
#[allow(clippy::too_many_arguments)]
fn generate_round_cap(
  vertices: &mut Vec<TrailVertex>,
  cx: f32,
  cy: f32,
  dx: f32,
  dy: f32,
  px: f32,
  py: f32,
  half_width: f32,
  color: [f32; 4],
  uv_x: f32,
  effect_params: [f32; 4],
) {
  let segments = ROUND_CAP_SEGMENTS;

  // Add degenerate separator from main strip
  if !vertices.is_empty() {
    let last = *vertices.last().unwrap();
    vertices.push(last);
  }

  // The semicircle sweeps from +perpendicular to -perpendicular,
  // going through the outward direction (dx, dy).
  // We parameterize the angle from -PI/2 to +PI/2 relative to the outward direction.
  //
  // Start angle: the +perpendicular side (left edge of trail at this endpoint)
  // End angle: the -perpendicular side (right edge of trail at this endpoint)

  // Generate triangle fan as a triangle strip: center, rim0, center, rim1, center, rim2, ...

  // Degenerate: duplicate first vertex of fan
  if !vertices.is_empty() {
    vertices.push(TrailVertex {
      position: [cx, cy],
      color,
      uv: [uv_x, 0.5],
      effect_params,
      perpendicular: [0.0, 0.0],
    });
  }

  for i in 0..=segments {
    let angle = std::f32::consts::PI * (i as f32 / segments as f32);

    // Rotate the +perpendicular direction by angle around the outward direction
    // Start at +perp (angle=0), sweep through outward dir (angle=PI/2), end at -perp (angle=PI)
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    // rim = center + cos(angle) * perp * half_width + sin(angle) * outward * half_width
    let rim_x = cx + (px * cos_a + dx * sin_a) * half_width;
    let rim_y = cy + (py * cos_a + dy * sin_a) * half_width;

    // Determine UV.y based on position in the fan
    let uv_y = 1.0 - (i as f32 / segments as f32);

    // Center vertex
    vertices.push(TrailVertex {
      position: [cx, cy],
      color,
      uv: [uv_x, 0.5],
      effect_params,
      perpendicular: [0.0, 0.0],
    });

    // Rim vertex
    vertices.push(TrailVertex {
      position: [rim_x, rim_y],
      color,
      uv: [uv_x, uv_y],
      effect_params,
      perpendicular: [px, py],
    });
  }
}

/// Generate square cap geometry at an endpoint, appending to the vertex buffer.
///
/// Simply extends the trail by `half_width` beyond the endpoint.
/// These 2 vertices are appended directly to the existing triangle strip
/// (no degenerate needed since they continue the strip naturally).
///
/// # Parameters
/// - `vertices`: vertex buffer to append to
/// - `cx, cy`: center position of the cap (the endpoint)
/// - `dx, dy`: direction AWAY from the trail (outward from this endpoint)
/// - `px, py`: perpendicular direction at the endpoint
/// - `half_width`: half the trail width at this endpoint
/// - `color`: color array for the cap vertices
/// - `uv_x`: UV.x coordinate at this endpoint
/// - `effect_params`: effect parameters for the cap vertices
#[allow(clippy::too_many_arguments)]
fn generate_square_cap(
  vertices: &mut Vec<TrailVertex>,
  cx: f32,
  cy: f32,
  dx: f32,
  dy: f32,
  px: f32,
  py: f32,
  half_width: f32,
  color: [f32; 4],
  uv_x: f32,
  effect_params: [f32; 4],
) {
  // Extend position by half_width in the outward direction
  let ext_x = cx + dx * half_width;
  let ext_y = cy + dy * half_width;

  // Left vertex
  vertices.push(TrailVertex {
    position: [ext_x + px * half_width, ext_y + py * half_width],
    color,
    uv: [uv_x, 0.0],
    effect_params,
    perpendicular: [px, py],
  });

  // Right vertex
  vertices.push(TrailVertex {
    position: [ext_x - px * half_width, ext_y - py * half_width],
    color,
    uv: [uv_x, 1.0],
    effect_params,
    perpendicular: [px, py],
  });
}

/// Helper for square cap at the head/start of a line (emits a separate quad
/// connecting the extended position back to the head position).
#[allow(clippy::too_many_arguments)]
fn vertices_push_square_cap_head(
  vertices: &mut Vec<TrailVertex>,
  ext_x: f32,
  ext_y: f32,
  head_x: f32,
  head_y: f32,
  px: f32,
  py: f32,
  half_width: f32,
  color: [f32; 4],
  uv_x: f32,
  effect_params: [f32; 4],
) {
  // Degenerate: duplicate first vertex
  vertices.push(TrailVertex {
    position: [ext_x + px * half_width, ext_y + py * half_width],
    color,
    uv: [uv_x, 0.0],
    effect_params,
    perpendicular: [px, py],
  });
  // Left at extended position
  vertices.push(TrailVertex {
    position: [ext_x + px * half_width, ext_y + py * half_width],
    color,
    uv: [uv_x, 0.0],
    effect_params,
    perpendicular: [px, py],
  });
  // Right at extended position
  vertices.push(TrailVertex {
    position: [ext_x - px * half_width, ext_y - py * half_width],
    color,
    uv: [uv_x, 1.0],
    effect_params,
    perpendicular: [px, py],
  });
  // Left at head position
  vertices.push(TrailVertex {
    position: [head_x + px * half_width, head_y + py * half_width],
    color,
    uv: [uv_x, 0.0],
    effect_params,
    perpendicular: [px, py],
  });
  // Right at head position
  vertices.push(TrailVertex {
    position: [head_x - px * half_width, head_y - py * half_width],
    color,
    uv: [uv_x, 1.0],
    effect_params,
    perpendicular: [px, py],
  });
}

// =====================================================================
// Line Join Geometry Generation
// =====================================================================

/// Number of segments for round joins
const ROUND_JOIN_SEGMENTS: u32 = 6;

/// Whether to report dropped geometry now: once when drops start, and again
/// only after a frame without drops, so sustained overload doesn't flood the log.
fn should_warn_about_drops(dropped: bool, warned: &mut bool) -> bool {
  let warn = dropped && !*warned;
  *warned = dropped;
  warn
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn drop_warning_fires_once_per_overload() {
    let mut warned = false;
    let frames = [true, true, true, false, true, true];

    let warnings: Vec<bool> = frames
      .iter()
      .map(|&dropped| should_warn_about_drops(dropped, &mut warned))
      .collect();

    assert_eq!(warnings, [true, false, false, false, true, false]);
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
    // effect_params: [f32; 4] = 16 bytes
    // perpendicular: [f32; 2] = 8 bytes
    // Total: 56 bytes
    assert_eq!(std::mem::size_of::<TrailVertex>(), 56);
  }

  #[test]
  fn trail_globals_struct_size() {
    // Ensure TrailGlobals is the expected size for GPU uniform buffer
    // view_proj: [[f32; 4]; 4] = 64 bytes
    // resolution_time: [f32; 4] = 16 bytes
    // Total: 80 bytes
    assert_eq!(std::mem::size_of::<TrailGlobals>(), 80);
  }

  #[test]
  fn effect_type_style_encoding() {
    // No effect + Solid = 0 + (0 << 4) = 0
    let none_solid = encode_type_and_style(0, 0);
    assert!((none_solid - 0.0).abs() < 0.01);

    // Lightning + Glow = 1 + (2 << 4) = 33
    let lightning_glow = encode_type_and_style(1, 2);
    assert!((lightning_glow - 33.0).abs() < 0.01);

    // Noise + Soft = 2 + (1 << 4) = 18
    let noise_soft = encode_type_and_style(2, 1);
    assert!((noise_soft - 18.0).abs() < 0.01);

    // UvScroll + Solid = 3 + (0 << 4) = 3
    let scroll_solid = encode_type_and_style(3, 0);
    assert!((scroll_solid - 3.0).abs() < 0.01);

    // Ribbon + Glow = 4 + (2 << 4) = 36
    let ribbon_glow = encode_type_and_style(4, 2);
    assert!((ribbon_glow - 36.0).abs() < 0.01);
  }

  #[test]
  fn build_effect_params_none() {
    let params = build_effect_params(None, TrailStyle::Solid);
    assert!((params[0] - 0.0).abs() < 0.01); // type=0, style=0
    assert!((params[1] - 0.0).abs() < 0.01);
    assert!((params[2] - 0.0).abs() < 0.01);
    assert!((params[3] - 0.0).abs() < 0.01);
  }

  #[test]
  fn build_effect_params_lightning() {
    let effect = TrailEffect::Lightning {
      jitter: 15.0,
      speed: 8.0,
      detail: 3,
    };
    let params = build_effect_params(Some(&effect), TrailStyle::Glow);
    // type=1, style=2 -> 1 + (2<<4) = 33
    assert!((params[0] - 33.0).abs() < 0.01);
    assert!((params[1] - 15.0).abs() < 0.01); // jitter
    assert!((params[2] - 8.0).abs() < 0.01); // speed
    assert!((params[3] - 3.0).abs() < 0.01); // detail
  }

  #[test]
  fn build_effect_params_noise() {
    let effect = TrailEffect::Noise {
      frequency: 3.0,
      amplitude: 10.0,
      speed: 2.0,
    };
    let params = build_effect_params(Some(&effect), TrailStyle::Soft);
    // type=2, style=1 -> 2 + (1<<4) = 18
    assert!((params[0] - 18.0).abs() < 0.01);
    assert!((params[1] - 3.0).abs() < 0.01); // frequency
    assert!((params[2] - 10.0).abs() < 0.01); // amplitude
    assert!((params[3] - 2.0).abs() < 0.01); // speed
  }

  #[test]
  fn build_effect_params_uv_scroll() {
    let effect = TrailEffect::UvScroll { speed: 1.5 };
    let params = build_effect_params(Some(&effect), TrailStyle::Solid);
    // type=3, style=0 -> 3
    assert!((params[0] - 3.0).abs() < 0.01);
    assert!((params[1] - 1.5).abs() < 0.01); // speed
  }

  #[test]
  fn build_effect_params_ribbon() {
    let effect = TrailEffect::Ribbon {
      amplitude: 10.0,
      frequency: 3.0,
      speed: 2.0,
    };
    let params = build_effect_params(Some(&effect), TrailStyle::Glow);
    // type=4, style=2 -> 4 + (2<<4) = 36
    assert!((params[0] - 36.0).abs() < 0.01);
    assert!((params[1] - 10.0).abs() < 0.01); // amplitude
    assert!((params[2] - 3.0).abs() < 0.01); // frequency
    assert!((params[3] - 2.0).abs() < 0.01); // speed
  }

  // ========== Line Cap Tests ==========

  #[test]
  fn round_cap_generates_vertices() {
    let mut vertices = Vec::new();
    let effect_params = [0.0; 4];
    let color = [1.0, 1.0, 1.0, 1.0];

    generate_round_cap(
      &mut vertices,
      100.0,
      100.0, // center
      1.0,
      0.0, // direction (right)
      0.0,
      -1.0, // perpendicular (up)
      5.0,  // half_width
      color,
      0.5,
      effect_params,
    );

    // Should generate vertices: (segments+1) * 2 for center/rim pairs
    // No degenerate since buffer was empty
    let expected = 2 * (ROUND_CAP_SEGMENTS as usize + 1);
    assert_eq!(vertices.len(), expected);
  }

  #[test]
  fn round_cap_with_existing_vertices_adds_degenerate() {
    let mut vertices = vec![TrailVertex {
      position: [0.0, 0.0],
      color: [1.0, 1.0, 1.0, 1.0],
      uv: [0.0, 0.0],
      effect_params: [0.0; 4],
      perpendicular: [0.0, 0.0],
    }];

    let before_len = vertices.len();

    generate_round_cap(
      &mut vertices,
      100.0,
      100.0,
      1.0,
      0.0,
      0.0,
      -1.0,
      5.0,
      [1.0; 4],
      0.5,
      [0.0; 4],
    );

    // Should have degenerate(1) + degenerate_first_fan(1) + (segments+1)*2 pairs
    let expected_new = 1 + 1 + 2 * (ROUND_CAP_SEGMENTS as usize + 1);
    assert_eq!(vertices.len(), before_len + expected_new);
  }

  #[test]
  fn round_cap_rim_vertices_at_correct_radius() {
    let mut vertices = Vec::new();
    let half_width = 10.0;

    generate_round_cap(
      &mut vertices,
      0.0,
      0.0, // center at origin
      1.0,
      0.0, // direction right
      0.0,
      1.0, // perpendicular up
      half_width,
      [1.0; 4],
      0.0,
      [0.0; 4],
    );

    // All rim vertices (odd indices) should be approximately half_width from center
    for (idx, v) in vertices.iter().enumerate() {
      if idx % 2 == 1 {
        // Rim vertex
        let dist = (v.position[0] * v.position[0] + v.position[1] * v.position[1]).sqrt();
        assert!(
          (dist - half_width).abs() < 0.1,
          "Rim vertex at index {} has distance {} from center, expected {}",
          idx,
          dist,
          half_width
        );
      }
    }
  }

  #[test]
  fn square_cap_generates_vertices() {
    let mut vertices = Vec::new();
    let effect_params = [0.0; 4];
    let color = [1.0, 1.0, 1.0, 1.0];

    generate_square_cap(
      &mut vertices,
      100.0,
      100.0, // center
      1.0,
      0.0, // direction (right)
      0.0,
      -1.0, // perpendicular (up)
      5.0,  // half_width
      color,
      0.5,
      effect_params,
    );

    // Square cap always generates exactly 2 vertices
    assert_eq!(vertices.len(), 2);
  }

  #[test]
  fn square_cap_extends_by_half_width() {
    let mut vertices = Vec::new();
    let half_width = 5.0;

    generate_square_cap(
      &mut vertices,
      100.0,
      100.0, // center
      1.0,
      0.0, // direction (right)
      0.0,
      -1.0, // perpendicular (up)
      half_width,
      [1.0; 4],
      0.5,
      [0.0; 4],
    );

    // The extended position should be at (100 + 5, 100) = (105, 100)
    // Left vertex: (105, 100 - 5) = (105, 95)
    // Right vertex: (105, 100 + 5) = (105, 105)
    assert!((vertices[0].position[0] - 105.0).abs() < 0.01);
    assert!((vertices[1].position[0] - 105.0).abs() < 0.01);
  }

  // ========== Line Join Tests ==========

  /// Helper: simulate inline join vertex generation for a 3-point V-shape.
  /// Returns the vertices produced by the join logic at the corner point.
  /// This mirrors the inline join code in add_line() without needing a GPU.
  fn simulate_join_vertices(
    join: LineJoin,
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    half_width: f32,
  ) -> Vec<TrailVertex> {
    let mut vertices = Vec::new();
    let color_arr = [1.0_f32; 4];
    let effect_params = [0.0_f32; 4];
    let t = 0.5;

    // Compute segment perpendiculars (same as add_line)
    let dx0 = p1.0 - p0.0;
    let dy0 = p1.1 - p0.1;
    let len0 = (dx0 * dx0 + dy0 * dy0).sqrt();
    let ppx = -dy0 / len0;
    let ppy = dx0 / len0;

    let dx1 = p2.0 - p1.0;
    let dy1 = p2.1 - p1.1;
    let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
    let npx = -dy1 / len1;
    let npy = dx1 / len1;

    let cross = ppx * npy - ppy * npx;
    let world_x = p1.0;
    let world_y = p1.1;

    let outside_sign: f32 = if cross < 0.0 { 1.0 } else { -1.0 };

    match join {
      LineJoin::Miter => {
        let mx = ppx + npx;
        let my = ppy + npy;
        let mlen = (mx * mx + my * my).sqrt();
        if mlen > 0.001 {
          let mnx = mx / mlen;
          let mny = my / mlen;
          let dot = mnx * ppx + mny * ppy;
          if dot.abs() > 0.001 {
            let miter_length = 1.0 / dot;
            if miter_length.abs() <= 4.0 {
              let spike_x = world_x + outside_sign * mnx * half_width * miter_length;
              let spike_y = world_y + outside_sign * mny * half_width * miter_length;
              let spike_uv_y = if outside_sign > 0.0 { 0.0 } else { 1.0 };
              let inside_uv_y = if outside_sign > 0.0 { 1.0 } else { 0.0 };
              let inside_sign = -outside_sign;

              // Two pairs, each with spike + inside, ordered by UV.y
              for (px, py) in [(ppx, ppy), (npx, npy)] {
                if outside_sign > 0.0 {
                  vertices.push(TrailVertex {
                    position: [spike_x, spike_y],
                    color: color_arr,
                    uv: [t, spike_uv_y],
                    effect_params,
                    perpendicular: [mnx, mny],
                  });
                  vertices.push(TrailVertex {
                    position: [
                      world_x + inside_sign * px * half_width,
                      world_y + inside_sign * py * half_width,
                    ],
                    color: color_arr,
                    uv: [t, inside_uv_y],
                    effect_params,
                    perpendicular: [px, py],
                  });
                } else {
                  vertices.push(TrailVertex {
                    position: [
                      world_x + inside_sign * px * half_width,
                      world_y + inside_sign * py * half_width,
                    ],
                    color: color_arr,
                    uv: [t, inside_uv_y],
                    effect_params,
                    perpendicular: [px, py],
                  });
                  vertices.push(TrailVertex {
                    position: [spike_x, spike_y],
                    color: color_arr,
                    uv: [t, spike_uv_y],
                    effect_params,
                    perpendicular: [mnx, mny],
                  });
                }
              }
            }
          }
        }
      }
      LineJoin::Bevel => {
        for (px, py) in [(ppx, ppy), (npx, npy)] {
          vertices.push(TrailVertex {
            position: [world_x + px * half_width, world_y + py * half_width],
            color: color_arr,
            uv: [t, 0.0],
            effect_params,
            perpendicular: [px, py],
          });
          vertices.push(TrailVertex {
            position: [world_x - px * half_width, world_y - py * half_width],
            color: color_arr,
            uv: [t, 1.0],
            effect_params,
            perpendicular: [px, py],
          });
        }
      }
      LineJoin::Round => {
        let segments = ROUND_JOIN_SEGMENTS;
        let (start_ox, start_oy, end_ox, end_oy) = if cross < 0.0 {
          (ppx, ppy, npx, npy)
        } else {
          (-ppx, -ppy, -npx, -npy)
        };
        let start_cross_end = start_ox * end_oy - start_oy * end_ox;
        let dot_se = (start_ox * end_ox + start_oy * end_oy).clamp(-1.0, 1.0);
        let total_angle = dot_se.acos();
        let inside_sign = -outside_sign;

        for k in 0..=segments {
          let frac = k as f32 / segments as f32;
          let angle = total_angle * frac;
          let cos_a = angle.cos();
          let sin_a = angle.sin();

          let (rot_ox, rot_oy) = if start_cross_end >= 0.0 {
            (
              start_ox * cos_a - start_oy * sin_a,
              start_oy * cos_a + start_ox * sin_a,
            )
          } else {
            (
              start_ox * cos_a + start_oy * sin_a,
              start_oy * cos_a - start_ox * sin_a,
            )
          };

          let in_px = ppx + (npx - ppx) * frac;
          let in_py = ppy + (npy - ppy) * frac;

          if outside_sign > 0.0 {
            vertices.push(TrailVertex {
              position: [world_x + rot_ox * half_width, world_y + rot_oy * half_width],
              color: color_arr,
              uv: [t, 0.0],
              effect_params,
              perpendicular: [rot_ox, rot_oy],
            });
            vertices.push(TrailVertex {
              position: [
                world_x + inside_sign * in_px * half_width,
                world_y + inside_sign * in_py * half_width,
              ],
              color: color_arr,
              uv: [t, 1.0],
              effect_params,
              perpendicular: [in_px, in_py],
            });
          } else {
            vertices.push(TrailVertex {
              position: [
                world_x + inside_sign * in_px * half_width,
                world_y + inside_sign * in_py * half_width,
              ],
              color: color_arr,
              uv: [t, 0.0],
              effect_params,
              perpendicular: [in_px, in_py],
            });
            vertices.push(TrailVertex {
              position: [world_x + rot_ox * half_width, world_y + rot_oy * half_width],
              color: color_arr,
              uv: [t, 1.0],
              effect_params,
              perpendicular: [rot_ox, rot_oy],
            });
          }
        }
      }
      LineJoin::None => {}
    }
    vertices
  }

  // ---- Vertex ordering tests ----
  // All join types must emit pairs with UV.y=0 first, UV.y=1 second
  // to match normal vertex pair ordering and prevent strip twisting.

  #[test]
  fn miter_join_vertex_ordering() {
    // 90-degree L-shape: right then up
    let verts = simulate_join_vertices(
      LineJoin::Miter,
      (0.0, 0.0),      // left
      (100.0, 0.0),    // corner
      (100.0, -100.0), // up
      10.0,
    );
    assert_eq!(verts.len(), 4, "Miter join emits 2 pairs = 4 vertices");
    // Every pair: first vertex UV.y=0, second UV.y=1
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0, "First vertex in pair must have UV.y=0");
      assert_eq!(pair[1].uv[1], 1.0, "Second vertex in pair must have UV.y=1");
    }
  }

  #[test]
  fn bevel_join_vertex_ordering() {
    let verts = simulate_join_vertices(
      LineJoin::Bevel,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, -100.0),
      10.0,
    );
    assert_eq!(verts.len(), 4, "Bevel join emits 2 pairs = 4 vertices");
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0, "First vertex in pair must have UV.y=0");
      assert_eq!(pair[1].uv[1], 1.0, "Second vertex in pair must have UV.y=1");
    }
  }

  #[test]
  fn round_join_vertex_ordering() {
    let verts = simulate_join_vertices(
      LineJoin::Round,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, -100.0),
      10.0,
    );
    let expected = 2 * (ROUND_JOIN_SEGMENTS as usize + 1);
    assert_eq!(verts.len(), expected, "Round join emits (segments+1) pairs");
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0, "First vertex in pair must have UV.y=0");
      assert_eq!(pair[1].uv[1], 1.0, "Second vertex in pair must have UV.y=1");
    }
  }

  // ---- Vertex ordering with opposite turn direction ----

  #[test]
  fn miter_join_vertex_ordering_right_turn() {
    // Right turn (opposite winding)
    let verts = simulate_join_vertices(
      LineJoin::Miter,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, 100.0), // down instead of up
      10.0,
    );
    assert_eq!(verts.len(), 4);
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0);
      assert_eq!(pair[1].uv[1], 1.0);
    }
  }

  #[test]
  fn bevel_join_vertex_ordering_right_turn() {
    let verts = simulate_join_vertices(
      LineJoin::Bevel,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, 100.0),
      10.0,
    );
    assert_eq!(verts.len(), 4);
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0);
      assert_eq!(pair[1].uv[1], 1.0);
    }
  }

  #[test]
  fn round_join_vertex_ordering_right_turn() {
    let verts = simulate_join_vertices(
      LineJoin::Round,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, 100.0),
      10.0,
    );
    let expected = 2 * (ROUND_JOIN_SEGMENTS as usize + 1);
    assert_eq!(verts.len(), expected);
    for pair in verts.chunks(2) {
      assert_eq!(pair[0].uv[1], 0.0);
      assert_eq!(pair[1].uv[1], 1.0);
    }
  }

  // ---- Miter spike geometry tests ----

  #[test]
  fn miter_spike_extends_outward() {
    // V-shape opening upward: arms go up-left and up-right from bottom apex
    let angle_rad = 35.0_f32.to_radians();
    let arm = 120.0;
    let dx = (angle_rad / 2.0).sin() * arm;
    let dy = (angle_rad / 2.0).cos() * arm;
    let verts = simulate_join_vertices(
      LineJoin::Miter,
      (100.0 - dx, 200.0 - dy), // left arm tip (up-left)
      (100.0, 200.0),           // apex
      (100.0 + dx, 200.0 - dy), // right arm tip (up-right)
      15.0,
    );
    assert_eq!(verts.len(), 4);
    // Spike should extend BELOW the apex (higher Y = outside of V)
    let spike_y = verts[0].position[1].max(verts[1].position[1]);
    assert!(
      spike_y > 200.0,
      "Miter spike should extend below apex (y > 200), got {}",
      spike_y
    );
  }

  #[test]
  fn miter_spike_length_matches_formula() {
    // 90-degree corner: miter_length should be 1/cos(45°) ≈ 1.414
    let verts = simulate_join_vertices(
      LineJoin::Miter,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, -100.0),
      10.0,
    );
    assert_eq!(verts.len(), 4);
    // Find the spike vertex (the one furthest from center)
    let center = (100.0_f32, 0.0_f32);
    let max_dist = verts
      .iter()
      .map(|v| {
        let dx = v.position[0] - center.0;
        let dy = v.position[1] - center.1;
        (dx * dx + dy * dy).sqrt()
      })
      .fold(0.0_f32, f32::max);
    // Expected: half_width * miter_length = 10.0 * 1.414 ≈ 14.14
    assert!(
      (max_dist - 14.14).abs() < 0.5,
      "Spike distance should be ~14.14, got {}",
      max_dist
    );
  }

  // ---- Bevel geometry tests ----

  #[test]
  fn bevel_join_uses_both_perpendiculars() {
    let verts = simulate_join_vertices(
      LineJoin::Bevel,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, -100.0),
      10.0,
    );
    // Pair 1 uses incoming perp, pair 2 uses outgoing perp
    let perp1 = verts[0].perpendicular;
    let perp2 = verts[2].perpendicular;
    // They should be different (incoming vs outgoing)
    assert!(
      (perp1[0] - perp2[0]).abs() > 0.01 || (perp1[1] - perp2[1]).abs() > 0.01,
      "Bevel pairs should use different perpendiculars"
    );
  }

  // ---- Round geometry tests ----

  #[test]
  fn round_join_no_vertices_for_collinear() {
    // Collinear points: no angle between perpendiculars
    let verts = simulate_join_vertices(
      LineJoin::Round,
      (0.0, 0.0),
      (100.0, 0.0),
      (200.0, 0.0), // straight line
      10.0,
    );
    // All vertices should be at the same distance from center (no arc needed)
    // With collinear segments, perps are identical → total_angle ≈ 0
    // The loop still runs but all vertices collapse to the same positions
    for pair in verts.chunks(2) {
      let d0 = ((pair[0].position[0] - 100.0).powi(2) + (pair[0].position[1]).powi(2)).sqrt();
      let d1 = ((pair[1].position[0] - 100.0).powi(2) + (pair[1].position[1]).powi(2)).sqrt();
      assert!(
        (d0 - 10.0).abs() < 0.1,
        "All round join vertices should be at half_width"
      );
      assert!(
        (d1 - 10.0).abs() < 0.1,
        "All round join vertices should be at half_width"
      );
    }
  }

  #[test]
  fn round_join_outside_arc_at_correct_radius() {
    let hw = 10.0;
    let verts = simulate_join_vertices(
      LineJoin::Round,
      (0.0, 0.0),
      (100.0, 0.0),
      (100.0, -100.0),
      hw,
    );
    let center = (100.0, 0.0);
    // Check outside arc vertices (every other vertex in each pair).
    // Inside vertices use linearly interpolated perps so may be shorter.
    for pair in verts.chunks(2) {
      // For this geometry (left turn, outside_sign=+1), the outside arc is first (UV.y=0)
      let outside = &pair[0];
      let dist = ((outside.position[0] - center.0).powi(2)
        + (outside.position[1] - center.1).powi(2))
      .sqrt();
      assert!(
        (dist - hw).abs() < 0.5,
        "Outside arc vertex should be ~{} from center, got {}",
        hw,
        dist
      );
    }
  }

  #[test]
  fn square_cap_head_helper_generates_vertices() {
    let mut vertices = Vec::new();

    vertices_push_square_cap_head(
      &mut vertices,
      95.0,
      100.0, // extended position
      100.0,
      100.0, // head position
      0.0,
      -1.0, // perpendicular
      5.0,  // half_width
      [1.0; 4],
      0.0,
      [0.0; 4],
    );

    // Should generate 5 vertices: degenerate + left_ext + right_ext + left_head + right_head
    assert_eq!(vertices.len(), 5);
  }
}
