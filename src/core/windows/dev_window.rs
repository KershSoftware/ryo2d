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

use crate::core::text::CpuText;

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
  // Monospace text for the pixel buffer (None without the default fonts)
  text: Option<CpuText>,
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

    let bind_group = crate::core::graphics::bind_groups::create(
      &device,
      &wgpu::BindGroupDescriptor {
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
      },
    );

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
      text: CpuText::mono(FONT_PX),
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

    self.bind_group = crate::core::graphics::bind_groups::create(
      &self.device,
      &wgpu::BindGroupDescriptor {
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
      },
    );
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

  /// Draw text in JetBrains Mono, its top-left at (x, y). Draws nothing
  /// without the `default-fonts` feature.
  pub fn draw_text(&mut self, text: &str, x: i32, y: i32, color: [u8; 4]) {
    if let Some(font) = &mut self.text {
      font.draw(
        &mut self.pixel_buffer,
        self.width,
        self.height,
        text,
        x,
        y,
        color,
      );
    }
  }

  /// The width of `text` in pixels (characters, not bytes).
  pub fn measure_text(&self, text: &str) -> i32 {
    self
      .text
      .as_ref()
      .map_or(text.chars().count() as i32 * CHAR_WIDTH, |font| {
        font.measure(text)
      })
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
    let width = self.measure_text(text) + padding * 2;
    let line = self.text.as_ref().map_or(CHAR_HEIGHT, CpuText::line_height);
    let height = line + padding * 2;

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

/// The pixel size the dev windows' text is drawn at.
const FONT_PX: f32 = 12.0;

/// Character height in pixels (the font's ascent: a text's top to its baseline).
pub const CHAR_HEIGHT: i32 = 12;

/// Character width in pixels (the font's advance).
pub const CHAR_WIDTH: i32 = 7;

#[cfg(all(test, feature = "default-fonts"))]
mod tests {
  use super::*;

  #[test]
  fn the_char_constants_match_the_font() {
    let text = CpuText::mono(FONT_PX).unwrap();
    assert_eq!(text.advance(), CHAR_WIDTH);
    assert!(
      text.line_height() >= CHAR_HEIGHT,
      "a row holds the ascent and descent"
    );
    assert_eq!(text.measure("\u{00E9}\u{2500}x"), 3 * CHAR_WIDTH);
  }

  /// Renders a logger-window-style layout with the real constants into
  /// `target/dev-window-text.png` for looking at (the windows themselves
  /// need an OS window)
  #[test]
  fn a_logger_layout_renders_to_a_png() {
    let mut text = CpuText::mono(FONT_PX).unwrap();
    let (w, h) = (640u32, 200u32);
    let mut buffer: Vec<u8> = colors::BACKGROUND.repeat((w * h) as usize);
    let fill = |buffer: &mut [u8], x0: i32, y0: i32, rw: i32, rh: i32, c: [u8; 4]| {
      for y in y0..y0 + rh {
        for x in x0..x0 + rw {
          let i = ((y as u32 * w + x as u32) * 4) as usize;
          buffer[i..i + 4].copy_from_slice(&c);
        }
      }
    };
    fill(&mut buffer, 0, 0, w as i32, 30, colors::HEADER_BG);
    for (i, label) in ["All", "Trace", "Debug", "Info", "Warn", "Error"]
      .iter()
      .enumerate()
    {
      let x = 8 + i as i32 * 70;
      fill(
        &mut buffer,
        x,
        6,
        text.measure(label) + 8,
        18,
        colors::DIVIDER,
      );
      text.draw(&mut buffer, w, h, label, x + 4, 8, colors::TEXT);
    }
    let rows = [
      (
        colors::LOG_INFO,
        "INFO ",
        "ryo2d::engine",
        "Engine started \u{2014} 1280x720",
      ),
      (
        colors::LOG_DEBUG,
        "DEBUG",
        "ryo2d::text",
        "Loaded fonts: Inter, JetBrains Mono, m5x7",
      ),
      (
        colors::LOG_WARN,
        "WARN ",
        "ryo2d::audio",
        "No audio device; caf\u{00E9} muted \u{2500}\u{2500}",
      ),
      (
        colors::LOG_ERROR,
        "ERROR",
        "game",
        "Failed to load level 3: gjg(){}[]|~ 0Oo1lI",
      ),
      (
        colors::LOG_TRACE,
        "TRACE",
        "game",
        "pitch 14 rows, g y p q j descenders",
      ),
    ];
    for (i, (color, level, module, message)) in rows.iter().enumerate() {
      let y = 36 + i as i32 * (CHAR_HEIGHT + 2);
      let mut x = 8;
      text.draw(&mut buffer, w, h, "12:00:01.250", x, y, colors::TEXT_DIM);
      x += 13 * CHAR_WIDTH;
      text.draw(&mut buffer, w, h, level, x, y, *color);
      x += 6 * CHAR_WIDTH;
      text.draw(&mut buffer, w, h, module, x, y, colors::ENGINE_CONTEXT);
      x += (module.len() as i32 + 1) * CHAR_WIDTH;
      text.draw(&mut buffer, w, h, message, x, y, colors::TEXT);
    }
    // A profiler-style row of labels
    for (i, label) in ["Frame 16.6 ms", "Update 2.1", "Render 4.8", "Freeze"]
      .iter()
      .enumerate()
    {
      text.draw(
        &mut buffer,
        w,
        h,
        label,
        8 + i as i32 * 150,
        140,
        colors::TEXT,
      );
      text.draw(
        &mut buffer,
        w,
        h,
        label,
        8 + i as i32 * 150,
        160,
        colors::TEXT_DIM,
      );
    }
    assert!(buffer.as_chunks::<4>().0.iter().any(|p| p[0] > 200));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/dev-window-text.png");
    image::save_buffer(&path, &buffer, w, h, image::ColorType::Rgba8).unwrap();
  }
}
