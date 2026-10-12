//! Generic engine development window framework.
//!
//! This module provides a reusable framework for creating standalone development
//! tool windows (profiler, logger, inspector, etc.) that can communicate with the
//! main game engine without affecting its performance.
//!
//! # Architecture
//!
//! - Each dev window runs as a separate winit window with its own wgpu surface
//! - Windows share the GPU device/queue with the main renderer via Arc
//! - CPU-side pixel buffer rendering for simplicity and custom control
//! - Throttled rendering (configurable FPS) to minimize overhead

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

/// Common colors for dev window UIs.
pub mod colors {
  pub const BACKGROUND: [u8; 4] = [30, 30, 30, 255];
  pub const TEXT: [u8; 4] = [220, 220, 220, 255];
  pub const TEXT_DIM: [u8; 4] = [150, 150, 150, 255];
  pub const HEADER_BG: [u8; 4] = [45, 45, 45, 255];
  pub const DIVIDER: [u8; 4] = [60, 60, 60, 255];
  pub const SCROLLBAR_BG: [u8; 4] = [40, 40, 40, 255];
  pub const SCROLLBAR_THUMB: [u8; 4] = [80, 80, 80, 255];
  pub const SCROLLBAR_THUMB_HOVER: [u8; 4] = [100, 100, 100, 255];

  // Log level colors
  pub const LOG_TRACE: [u8; 4] = [128, 128, 128, 255]; // Gray
  pub const LOG_DEBUG: [u8; 4] = [100, 149, 237, 255]; // Cornflower blue
  pub const LOG_INFO: [u8; 4] = [220, 220, 220, 255]; // White
  pub const LOG_WARN: [u8; 4] = [255, 193, 37, 255]; // Golden yellow
  pub const LOG_ERROR: [u8; 4] = [255, 99, 71, 255]; // Tomato red

  // Context colors
  pub const ENGINE_CONTEXT: [u8; 4] = [86, 156, 214, 255]; // Blue
  pub const GAME_CONTEXT: [u8; 4] = [184, 215, 163, 255]; // Green
}

/// Configuration for creating a dev window.
pub struct DevWindowConfig {
  pub title: String,
  pub width: u32,
  pub height: u32,
  pub render_throttle_ms: u64,
}

impl Default for DevWindowConfig {
  fn default() -> Self {
    Self {
      title: "Dev Window".to_string(),
      width: 600,
      height: 400,
      render_throttle_ms: 100, // ~10 FPS by default
    }
  }
}

/// Base renderer for dev windows providing common GPU and pixel buffer operations.
pub struct DevWindowRenderer {
  window: Arc<Window>,
  surface: wgpu::Surface<'static>,
  surface_config: wgpu::SurfaceConfiguration,
  device: Arc<wgpu::Device>,
  queue: Arc<wgpu::Queue>,
  // CPU pixel buffer for rendering
  pixel_buffer: Vec<u8>,
  width: u32,
  height: u32,
  // Texture for uploading pixel buffer to GPU
  texture: wgpu::Texture,
  texture_view: wgpu::TextureView,
  bind_group: wgpu::BindGroup,
  bind_group_layout: wgpu::BindGroupLayout,
  render_pipeline: wgpu::RenderPipeline,
  // Mouse state
  mouse_x: f32,
  mouse_y: f32,
  // Keyboard modifier state
  modifiers: ModifiersState,
  // Render throttling
  last_render_time: web_time::Instant,
  render_throttle_ms: u64,
}

impl DevWindowRenderer {
  /// Create a new dev window renderer.
  pub fn new(
    event_loop: &ActiveEventLoop,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
    config: DevWindowConfig,
  ) -> Option<Self> {
    let width = config.width;
    let height = config.height;

    // Create the window
    let window_attrs = WindowAttributes::default()
      .with_title(&config.title)
      .with_inner_size(winit::dpi::LogicalSize::new(width, height))
      .with_resizable(true)
      .with_visible(true);

    let window = match event_loop.create_window(window_attrs) {
      Ok(w) => Arc::new(w),
      Err(e) => {
        eprintln!("[DevWindow] Failed to create window: {}", e);
        return None;
      }
    };

    // Create surface
    let surface = match instance.create_surface(Arc::clone(&window)) {
      Ok(s) => s,
      Err(e) => {
        eprintln!("[DevWindow] Failed to create surface: {}", e);
        return None;
      }
    };

    let surface_caps = surface.get_capabilities(adapter);
    let surface_format = surface_caps
      .formats
      .iter()
      .find(|f| f.is_srgb())
      .copied()
      .unwrap_or(surface_caps.formats[0]);

    let surface_config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format: surface_format,
      width,
      height,
      present_mode: wgpu::PresentMode::AutoVsync,
      alpha_mode: surface_caps.alpha_modes[0],
      view_formats: vec![],
      desired_maximum_frame_latency: 2,
      color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&device, &surface_config);

    // Create CPU pixel buffer
    let pixel_buffer = vec![0u8; (width * height * 4) as usize];

    // Create texture for uploading pixel buffer
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("dev_window_texture"),
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

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("dev_window_sampler"),
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create bind group layout and bind group
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("dev_window_bind_group_layout"),
      entries: &[
        wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
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

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("dev_window_bind_group"),
      layout: &bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
      ],
    });

    // Create shader
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("dev_window_shader"),
      source: wgpu::ShaderSource::Wgsl(DEV_WINDOW_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("dev_window_pipeline_layout"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("dev_window_pipeline"),
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
        targets: &[Some(wgpu::ColorTargetState {
          format: surface_format,
          blend: None,
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
      multiview_mask: None,
      cache: None,
    });

    Some(Self {
      window,
      surface,
      surface_config,
      device,
      queue,
      pixel_buffer,
      width,
      height,
      texture,
      texture_view,
      bind_group,
      bind_group_layout,
      render_pipeline,
      mouse_x: 0.0,
      mouse_y: 0.0,
      modifiers: ModifiersState::empty(),
      last_render_time: web_time::Instant::now(),
      render_throttle_ms: config.render_throttle_ms,
    })
  }

  /// Get the window ID for event routing.
  pub fn window_id(&self) -> WindowId {
    self.window.id()
  }

  /// Get current window width.
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get current window height.
  pub fn height(&self) -> u32 {
    self.height
  }

  /// Get current mouse X position.
  pub fn mouse_x(&self) -> f32 {
    self.mouse_x
  }

  /// Get current mouse Y position.
  pub fn mouse_y(&self) -> f32 {
    self.mouse_y
  }

  /// Get current keyboard modifiers state.
  pub fn modifiers(&self) -> ModifiersState {
    self.modifiers
  }

  /// Get mutable access to the pixel buffer for drawing.
  pub fn pixel_buffer_mut(&mut self) -> &mut [u8] {
    &mut self.pixel_buffer
  }

  /// Check if enough time has elapsed for a render (throttling).
  pub fn should_render(&self) -> bool {
    self.last_render_time.elapsed().as_millis() >= self.render_throttle_ms as u128
  }

  /// Mark that a render just occurred.
  pub fn mark_rendered(&mut self) {
    self.last_render_time = web_time::Instant::now();
  }

  /// Request a redraw of the window.
  pub fn request_redraw(&self) {
    self.window.request_redraw();
  }

  /// Handle a window event. Returns false if the window should close.
  pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
    match event {
      WindowEvent::Resized(new_size) => {
        if new_size.width > 0 && new_size.height > 0 {
          self.resize(new_size.width, new_size.height);
        }
        true
      }
      WindowEvent::CursorMoved { position, .. } => {
        self.mouse_x = position.x as f32;
        self.mouse_y = position.y as f32;
        self.window.request_redraw();
        true
      }
      WindowEvent::ModifiersChanged(new_modifiers) => {
        self.modifiers = new_modifiers.state();
        true
      }
      WindowEvent::CloseRequested => false,
      _ => true,
    }
  }

  /// Resize the window and recreate resources.
  fn resize(&mut self, width: u32, height: u32) {
    self.width = width;
    self.height = height;
    self.surface_config.width = width;
    self.surface_config.height = height;
    self.surface.configure(&self.device, &self.surface_config);

    // Recreate pixel buffer
    self.pixel_buffer = vec![0u8; (width * height * 4) as usize];

    // Recreate texture
    self.texture = self.device.create_texture(&wgpu::TextureDescriptor {
      label: Some("dev_window_texture"),
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
    self.texture_view = self
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    // Recreate bind group with new texture view
    let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("dev_window_sampler"),
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("dev_window_bind_group"),
      layout: &self.bind_group_layout,
      entries: &[
        wgpu::BindGroupEntry {
          binding: 0,
          resource: wgpu::BindingResource::TextureView(&self.texture_view),
        },
        wgpu::BindGroupEntry {
          binding: 1,
          resource: wgpu::BindingResource::Sampler(&sampler),
        },
      ],
    });
  }

  /// Clear the pixel buffer with a color.
  pub fn clear(&mut self, color: [u8; 4]) {
    for pixel in self.pixel_buffer.as_chunks_mut::<4>().0 {
      pixel.copy_from_slice(&color);
    }
  }

  /// Fill a rectangle with a color.
  pub fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    for py in y.max(0)..(y + height).min(self.height as i32) {
      for px in x.max(0)..(x + width).min(self.width as i32) {
        let idx = ((py as u32 * self.width + px as u32) * 4) as usize;
        if idx + 3 < self.pixel_buffer.len() {
          self.pixel_buffer[idx..idx + 4].copy_from_slice(&color);
        }
      }
    }
  }

  /// Draw a rectangle outline.
  pub fn draw_rect_outline(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    // Top and bottom
    for px in x..(x + width) {
      self.set_pixel(px, y, color);
      self.set_pixel(px, y + height - 1, color);
    }
    // Left and right
    for py in y..(y + height) {
      self.set_pixel(x, py, color);
      self.set_pixel(x + width - 1, py, color);
    }
  }

  /// Set a single pixel.
  pub fn set_pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32 {
      let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
      if idx + 3 < self.pixel_buffer.len() {
        self.pixel_buffer[idx..idx + 4].copy_from_slice(&color);
      }
    }
  }

  /// Draw text using the built-in bitmap font.
  pub fn draw_text(&mut self, text: &str, x: i32, y: i32, color: [u8; 4]) {
    let char_width = 7;
    let mut cx = x;

    for ch in text.chars() {
      if let Some(glyph) = get_glyph(ch) {
        for (row, bits) in glyph.iter().enumerate() {
          for col in 0..7 {
            if (bits >> (6 - col)) & 1 == 1 {
              self.set_pixel(cx + col, y + row as i32, color);
            }
          }
        }
      }
      cx += char_width;
    }
  }

  /// Draw text with a dark shadow for better readability on colored backgrounds.
  pub fn draw_text_shadowed(&mut self, text: &str, x: i32, y: i32, color: [u8; 4]) {
    const SHADOW: [u8; 4] = [0, 0, 0, 200];
    self.draw_text(text, x + 1, y + 1, SHADOW);
    self.draw_text(text, x, y, color);
  }

  /// Draw a tooltip near a position.
  pub fn draw_tooltip(&mut self, text: &str, near_x: f32, near_y: f32) {
    let padding = 6;
    let char_width = 7;
    let char_height = 12;

    let width = (text.len() as i32 * char_width) + padding * 2;
    let height = char_height + padding * 2;

    let x = (near_x as i32 + 15).min(self.width as i32 - width - 5);
    let y = (near_y as i32 + 15).min(self.height as i32 - height - 5);

    self.fill_rect(x, y, width, height, [50, 50, 50, 240]);
    self.draw_rect_outline(x, y, width, height, colors::DIVIDER);
    self.draw_text(text, x + padding, y + padding, colors::TEXT);
  }

  /// Upload pixel buffer and present to screen.
  pub fn present(&mut self) {
    // Upload pixel buffer to texture
    self.queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture: &self.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &self.pixel_buffer,
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(self.width * 4),
        rows_per_image: Some(self.height),
      },
      wgpu::Extent3d {
        width: self.width,
        height: self.height,
        depth_or_array_layers: 1,
      },
    );

    // Get surface texture
    let output = match self.surface.get_current_texture() {
      wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
      wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
        self.surface.configure(&self.device, &self.surface_config);
        return;
      }
      wgpu::CurrentSurfaceTexture::Validation => {
        eprintln!("[DevWindow] Surface validation error");
        return;
      }
      wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
    };

    let view = output
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dev_window_encoder"),
      });

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dev_window_render_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });

      render_pass.set_pipeline(&self.render_pipeline);
      render_pass.set_bind_group(0, &self.bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }

    self.queue.submit(std::iter::once(encoder.finish()));
    self.queue.present(output);

    // Request continuous redraws
    self.window.request_redraw();
  }
}

/// Simple shader for displaying texture on screen.
const DEV_WINDOW_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    var tex_coords = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 0.0),
    );

    var output: VertexOutput;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    output.tex_coords = tex_coords[vertex_index];
    return output;
}

@group(0) @binding(0) var t_texture: texture_2d<f32>;
@group(0) @binding(1) var t_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_texture, t_sampler, input.tex_coords);
}
"#;

/// Simple bitmap font glyphs (7x12 pixels per character).
/// Returns 12 bytes, each byte represents a row with 7 bits used.
pub fn get_glyph(ch: char) -> Option<[u8; 12]> {
  match ch {
    ' ' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    '!' => Some([
      0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x10, 0x10, 0, 0,
    ]),
    '"' => Some([0x28, 0x28, 0x28, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    '#' => Some([0x28, 0x28, 0x7C, 0x28, 0x28, 0x7C, 0x28, 0x28, 0, 0, 0, 0]),
    '(' => Some([
      0x08, 0x10, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x10, 0x08, 0, 0,
    ]),
    ')' => Some([
      0x20, 0x10, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x10, 0x20, 0, 0,
    ]),
    '*' => Some([0, 0, 0x28, 0x10, 0x7C, 0x10, 0x28, 0, 0, 0, 0, 0]),
    '+' => Some([0, 0, 0, 0x10, 0x10, 0x7C, 0x10, 0x10, 0, 0, 0, 0]),
    ',' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0x18, 0x18, 0x10, 0x20]),
    '-' => Some([0, 0, 0, 0, 0, 0x7C, 0, 0, 0, 0, 0, 0]),
    '.' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0x18, 0x18, 0, 0]),
    '/' => Some([
      0x04, 0x04, 0x08, 0x08, 0x10, 0x10, 0x20, 0x20, 0x40, 0x40, 0, 0,
    ]),
    ':' => Some([0, 0, 0x18, 0x18, 0, 0, 0, 0x18, 0x18, 0, 0, 0]),
    ';' => Some([0, 0, 0x18, 0x18, 0, 0, 0, 0x18, 0x18, 0x10, 0x20, 0]),
    '<' => Some([
      0x04, 0x08, 0x10, 0x20, 0x40, 0x20, 0x10, 0x08, 0x04, 0, 0, 0,
    ]),
    '=' => Some([0, 0, 0, 0x7C, 0, 0x7C, 0, 0, 0, 0, 0, 0]),
    '>' => Some([
      0x40, 0x20, 0x10, 0x08, 0x04, 0x08, 0x10, 0x20, 0x40, 0, 0, 0,
    ]),
    '?' => Some([
      0x38, 0x44, 0x04, 0x08, 0x10, 0x10, 0x10, 0x00, 0x10, 0x10, 0, 0,
    ]),
    '@' => Some([
      0x38, 0x44, 0x4C, 0x54, 0x54, 0x4C, 0x40, 0x40, 0x38, 0, 0, 0,
    ]),
    '[' => Some([
      0x38, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x38, 0, 0,
    ]),
    '\\' => Some([
      0x40, 0x40, 0x20, 0x20, 0x10, 0x10, 0x08, 0x08, 0x04, 0x04, 0, 0,
    ]),
    ']' => Some([
      0x38, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x38, 0, 0,
    ]),
    '^' => Some([0x10, 0x28, 0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    '0' => Some([
      0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    '1' => Some([
      0x10, 0x30, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x38, 0, 0,
    ]),
    '2' => Some([
      0x38, 0x44, 0x04, 0x04, 0x08, 0x10, 0x20, 0x40, 0x44, 0x7C, 0, 0,
    ]),
    '3' => Some([
      0x38, 0x44, 0x04, 0x04, 0x18, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0,
    ]),
    '4' => Some([
      0x08, 0x18, 0x28, 0x28, 0x48, 0x48, 0x7C, 0x08, 0x08, 0x08, 0, 0,
    ]),
    '5' => Some([
      0x7C, 0x40, 0x40, 0x78, 0x04, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0,
    ]),
    '6' => Some([
      0x38, 0x44, 0x40, 0x40, 0x78, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    '7' => Some([
      0x7C, 0x44, 0x04, 0x08, 0x08, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0,
    ]),
    '8' => Some([
      0x38, 0x44, 0x44, 0x44, 0x38, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    '9' => Some([
      0x38, 0x44, 0x44, 0x44, 0x3C, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0,
    ]),
    'A' | 'a' => Some([
      0x38, 0x44, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'B' | 'b' => Some([
      0x78, 0x44, 0x44, 0x44, 0x78, 0x44, 0x44, 0x44, 0x44, 0x78, 0, 0,
    ]),
    'C' | 'c' => Some([
      0x38, 0x44, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x44, 0x38, 0, 0,
    ]),
    'D' | 'd' => Some([
      0x78, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x78, 0, 0,
    ]),
    'E' | 'e' => Some([
      0x7C, 0x40, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x40, 0x7C, 0, 0,
    ]),
    'F' | 'f' => Some([
      0x7C, 0x40, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x40, 0x40, 0, 0,
    ]),
    'G' | 'g' => Some([
      0x38, 0x44, 0x40, 0x40, 0x4C, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    'H' | 'h' => Some([
      0x44, 0x44, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'I' | 'i' => Some([
      0x38, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x38, 0, 0,
    ]),
    'J' | 'j' => Some([
      0x1C, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x48, 0x48, 0x30, 0, 0,
    ]),
    'K' | 'k' => Some([
      0x44, 0x48, 0x50, 0x60, 0x60, 0x50, 0x48, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'L' | 'l' => Some([
      0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7C, 0, 0,
    ]),
    'M' | 'm' => Some([
      0x44, 0x6C, 0x54, 0x54, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'N' | 'n' => Some([
      0x44, 0x64, 0x64, 0x54, 0x54, 0x4C, 0x4C, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'O' | 'o' => Some([
      0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    'P' | 'p' => Some([
      0x78, 0x44, 0x44, 0x44, 0x78, 0x40, 0x40, 0x40, 0x40, 0x40, 0, 0,
    ]),
    'Q' | 'q' => Some([
      0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x54, 0x48, 0x44, 0x3A, 0, 0,
    ]),
    'R' | 'r' => Some([
      0x78, 0x44, 0x44, 0x44, 0x78, 0x50, 0x48, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'S' | 's' => Some([
      0x38, 0x44, 0x40, 0x40, 0x38, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0,
    ]),
    'T' | 't' => Some([
      0x7C, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0,
    ]),
    'U' | 'u' => Some([
      0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0,
    ]),
    'V' | 'v' => Some([
      0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x28, 0x28, 0x10, 0, 0,
    ]),
    'W' | 'w' => Some([
      0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x54, 0x54, 0x6C, 0x44, 0, 0,
    ]),
    'X' | 'x' => Some([
      0x44, 0x44, 0x28, 0x28, 0x10, 0x28, 0x28, 0x44, 0x44, 0x44, 0, 0,
    ]),
    'Y' | 'y' => Some([
      0x44, 0x44, 0x44, 0x28, 0x28, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0,
    ]),
    'Z' | 'z' => Some([
      0x7C, 0x04, 0x04, 0x08, 0x10, 0x20, 0x40, 0x40, 0x40, 0x7C, 0, 0,
    ]),
    '_' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0x7C, 0, 0]),
    '|' => Some([
      0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0,
    ]),
    _ => None,
  }
}

/// Measure text width in pixels.
pub fn measure_text(text: &str) -> i32 {
  text.len() as i32 * 7
}

/// Character height in pixels.
pub const CHAR_HEIGHT: i32 = 12;

/// Character width in pixels.
pub const CHAR_WIDTH: i32 = 7;
