//! GPU-accelerated trail and line renderer
//!
//! Renders trails and lines efficiently using GPU instancing and batching.

use super::{Line2D, Trail2D, TrailPoint};
use crate::core::ecs::{Transform, Vec2};
use crate::core::ui::Color;
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

/// GPU-accelerated trail and line renderer.
pub struct TrailRenderer {
  /// Render pipeline for solid trails
  solid_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for soft trails
  soft_pipeline: wgpu::RenderPipeline,
  /// Render pipeline for glow trails
  glow_pipeline: wgpu::RenderPipeline,

  /// Vertex buffer
  vertex_buffer: wgpu::Buffer,

  /// Uniform buffer for globals
  globals_buffer: wgpu::Buffer,

  /// Bind group layout (stored for potential future use, e.g., resizing)
  #[allow(dead_code)]
  bind_group_layout: wgpu::BindGroupLayout,

  /// Current bind group
  bind_group: wgpu::BindGroup,

  /// Accumulated vertices for this frame
  vertices: Vec<TrailVertex>,

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

    // Create bind group layout
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

    // Create bind group
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("Trail Bind Group"),
      layout: &bind_group_layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: globals_buffer.as_entire_binding(),
      }],
    });

    // Create shader module
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Trail Shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/core/trail.wgsl").into()),
    });

    // Create pipeline layout
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("Trail Pipeline Layout"),
      bind_group_layouts: &[&bind_group_layout],
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

    // Create pipelines for each style
    let create_pipeline = |entry_point: &str, label: &str| {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
          module: &shader,
          entry_point: "vs_main",
          buffers: std::slice::from_ref(&vertex_layout),
          compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
          module: &shader,
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

    let solid_pipeline = create_pipeline("fs_main", "Trail Solid Pipeline");
    let soft_pipeline = create_pipeline("fs_soft", "Trail Soft Pipeline");
    let glow_pipeline = create_pipeline("fs_glow", "Trail Glow Pipeline");

    Self {
      solid_pipeline,
      soft_pipeline,
      glow_pipeline,
      vertex_buffer,
      globals_buffer,
      bind_group_layout,
      bind_group,
      vertices: Vec::with_capacity(MAX_TRAIL_VERTICES),
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
  }

  /// Add a trail to be rendered.
  ///
  /// The trail's points are converted to triangle strip vertices.
  pub fn add_trail(&mut self, trail: &Trail2D, _transform: &Transform) {
    if !self.enabled || trail.point_count() < 2 {
      return;
    }

    let points = trail.points();
    let point_count = points.len();

    // Need 2 vertices per point for triangle strip
    if self.vertices.len() + point_count * 2 > MAX_TRAIL_VERTICES {
      return; // Buffer full
    }

    // Add degenerate triangle to separate from previous trail
    if !self.vertices.is_empty() {
      // Duplicate last vertex
      let last = *self.vertices.last().unwrap();
      self.vertices.push(last);
    }

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

      // Get interpolated properties
      let width = trail.width_at(t);
      let mut color = trail.color_at(t);

      // Apply age-based alpha fade only when position-based fade is disabled
      // (to avoid double-fading when both are active)
      if !trail.fades_alpha() {
        let age_alpha = trail.age_alpha(point.age);
        color = Color::new(color.r, color.g, color.b, (color.a as f32 * age_alpha) as u8);
      }

      // Trail points are already in world coordinates (from trail.update(x, y, dt))
      // No need to add transform position
      let world_x = point.x;
      let world_y = point.y;

      let half_width = width / 2.0;

      // Add degenerate at start of trail
      if i == 0 && !self.vertices.is_empty() {
        self.vertices.push(TrailVertex {
          position: [world_x + px * half_width, world_y + py * half_width],
          color: color_to_array(color),
          uv: [t, -0.5],
        });
      }

      // Left vertex
      self.vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [t, -0.5],
      });

      // Right vertex
      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [t, 0.5],
      });

      prev_point = Some(point);
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
          uv: [t, -0.5],
        });
      }

      // Left vertex
      self.vertices.push(TrailVertex {
        position: [world_x + px * half_width, world_y + py * half_width],
        color: color_to_array(color),
        uv: [t, -0.5],
      });

      // Right vertex
      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [t, 0.5],
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
        uv: [1.0, -0.5],
      });

      self.vertices.push(TrailVertex {
        position: [world_x - px * half_width, world_y - py * half_width],
        color: color_to_array(color),
        uv: [1.0, 0.5],
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
    if !self.enabled || self.vertices.is_empty() {
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

    // Upload vertices
    queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));

    // Select pipeline
    let pipeline = match style {
      TrailStyle::Solid => &self.solid_pipeline,
      TrailStyle::Soft => &self.soft_pipeline,
      TrailStyle::Glow => &self.glow_pipeline,
    };

    // Render
    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Trail Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: target_view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Load, // Don't clear, render on top
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
  }

  /// Get the number of vertices accumulated this frame.
  pub fn vertex_count(&self) -> usize {
    self.vertices.len()
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
