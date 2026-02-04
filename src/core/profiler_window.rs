//! Profiler window - A dedicated window showing puffin profiling data.
//!
//! This module provides a separate window that displays real-time flamegraphs
//! and profiling statistics using puffin data. It uses a simple CPU renderer
//! that's independent of the main game's rendering pipeline.

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

/// Colors for the profiler UI
mod colors {
  pub const BACKGROUND: [u8; 4] = [30, 30, 30, 255];
  pub const TEXT: [u8; 4] = [220, 220, 220, 255];
  pub const TEXT_DIM: [u8; 4] = [150, 150, 150, 255];
  pub const HEADER_BG: [u8; 4] = [45, 45, 45, 255];
  pub const DIVIDER: [u8; 4] = [60, 60, 60, 255];

  // CPU scope colors (blue/cyan tones)
  pub const CPU_COLORS: [[u8; 4]; 6] = [
    [86, 156, 214, 255],  // Blue
    [78, 201, 176, 255],  // Teal
    [156, 220, 254, 255], // Cyan
    [100, 149, 237, 255], // Cornflower
    [70, 130, 180, 255],  // Steel blue
    [135, 206, 235, 255], // Sky blue
  ];

  // GPU scope colors (green/orange tones)
  pub const GPU_COLORS: [[u8; 4]; 6] = [
    [184, 215, 163, 255], // Green
    [220, 220, 170, 255], // Yellow
    [206, 145, 120, 255], // Orange
    [244, 135, 113, 255], // Red-orange
    [255, 193, 37, 255],  // Golden
    [152, 251, 152, 255], // Pale green
  ];

  // Section header colors
  pub const ENGINE_CPU_HEADER: [u8; 4] = [60, 80, 100, 255];
  pub const ENGINE_GPU_HEADER: [u8; 4] = [80, 100, 60, 255];
  pub const GAME_HEADER: [u8; 4] = [100, 70, 90, 255];

  // Legacy scope colors (for scopes without prefix)
  pub const SCOPE_COLORS: [[u8; 4]; 8] = [
    [86, 156, 214, 255],  // Blue
    [78, 201, 176, 255],  // Teal
    [184, 215, 163, 255], // Green
    [220, 220, 170, 255], // Yellow
    [206, 145, 120, 255], // Orange
    [197, 134, 192, 255], // Purple
    [156, 220, 254, 255], // Cyan
    [244, 135, 113, 255], // Red
  ];
}

/// A scope entry from puffin for display
#[derive(Clone)]
struct ScopeEntry {
  name: String,
  duration_ns: u64,
  depth: usize,
}

/// Manages the profiler window and its rendering.
pub struct ProfilerWindow {
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
  render_pipeline: wgpu::RenderPipeline,
  // Puffin frame view for collecting profiling data
  frame_view: Arc<std::sync::Mutex<puffin::FrameView>>,
  // Sink ID for cleanup
  _sink_id: puffin::FrameSinkId,
  // Cached scope data from last frame
  last_frame_scopes: Vec<ScopeEntry>,
  last_frame_duration_ns: u64,
  // Display scopes: merged from recent frames for stable display
  display_scopes: Vec<ScopeEntry>,
  display_frame_duration_ns: u64,
  // Track scope "freshness" - how recently each scope was seen
  scope_age: std::collections::HashMap<String, u32>,
  // Mouse position for hover detection
  mouse_x: f32,
  mouse_y: f32,
  // Hovered scope name for tooltip
  hovered_scope: Option<String>,
  // Frame counter for "waiting" state
  frames_without_data: u32,
  // Track if we've ever received data
  has_received_data: bool,
  // Last time we rendered (for throttling to ~30 FPS)
  last_render_time: std::time::Instant,
  // Track actual wall-clock frame time for accurate FPS display
  last_new_frame_time: Option<std::time::Instant>,
  actual_frame_duration_ns: u64,
}

impl ProfilerWindow {
  /// Create a new profiler window.
  pub fn new(
    event_loop: &ActiveEventLoop,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    instance: &wgpu::Instance,
    adapter: &wgpu::Adapter,
  ) -> Option<Self> {
    let width = 600u32;
    let height = 400u32;

    // Create the profiler window
    let window_attrs = WindowAttributes::default()
      .with_title("Ryo2D Profiler")
      .with_inner_size(winit::dpi::LogicalSize::new(width, height))
      .with_resizable(true)
      .with_visible(true);

    let window = match event_loop.create_window(window_attrs) {
      Ok(w) => Arc::new(w),
      Err(e) => {
        eprintln!("[Profiler] Failed to create window: {}", e);
        return None;
      }
    };

    // Create surface
    let surface = match instance.create_surface(Arc::clone(&window)) {
      Ok(s) => s,
      Err(e) => {
        eprintln!("[Profiler] Failed to create surface: {}", e);
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
    };
    surface.configure(&device, &surface_config);

    // Create CPU pixel buffer
    let pixel_buffer = vec![0u8; (width * height * 4) as usize];

    // Create texture for uploading pixel buffer
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some("profiler_texture"),
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
      label: Some("profiler_sampler"),
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    // Create bind group layout and bind group
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("profiler_bind_group_layout"),
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
      label: Some("profiler_bind_group"),
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

    // Create simple shader for displaying texture
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("profiler_shader"),
      source: wgpu::ShaderSource::Wgsl(PROFILER_SHADER.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("profiler_pipeline_layout"),
      bind_group_layouts: &[&bind_group_layout],
      push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("profiler_pipeline"),
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
      multiview: None,
      cache: None,
    });

    // Create FrameView and register as a sink to receive frame data
    let frame_view = Arc::new(std::sync::Mutex::new(puffin::FrameView::default()));
    let frame_view_clone = Arc::clone(&frame_view);
    let sink_id = puffin::GlobalProfiler::lock().add_sink(Box::new(move |frame| {
      if let Ok(mut fv) = frame_view_clone.lock() {
        fv.add_frame(frame);
      }
    }));

    eprintln!("[Profiler] Window opened");

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
      render_pipeline,
      frame_view,
      _sink_id: sink_id,
      last_frame_scopes: Vec::new(),
      last_frame_duration_ns: 0,
      mouse_x: 0.0,
      mouse_y: 0.0,
      hovered_scope: None,
      frames_without_data: 0,
      has_received_data: false,
      last_render_time: std::time::Instant::now(),
      last_new_frame_time: None,
      actual_frame_duration_ns: 0,
      display_scopes: Vec::new(),
      display_frame_duration_ns: 0,
      scope_age: std::collections::HashMap::new(),
    })
  }

  /// Get the window ID for event routing.
  pub fn window_id(&self) -> WindowId {
    self.window.id()
  }

  /// Handle a window event.
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
        // Request redraw to update hover state
        self.window.request_redraw();
        true
      }
      WindowEvent::CloseRequested => {
        // Return false to signal window should close
        false
      }
      _ => true,
    }
  }

  /// Resize the profiler window.
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
      label: Some("profiler_texture"),
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
      label: Some("profiler_sampler"),
      mag_filter: wgpu::FilterMode::Nearest,
      min_filter: wgpu::FilterMode::Nearest,
      ..Default::default()
    });

    let bind_group_layout =
      self
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
          label: Some("profiler_bind_group_layout"),
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

    self.bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("profiler_bind_group"),
      layout: &bind_group_layout,
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

  /// Request a redraw.
  pub fn request_redraw(&self) {
    self.window.request_redraw();
  }

  /// Track wall clock time between frames for accurate FPS display.
  /// Call this when new_frame() is called in the engine.
  pub fn track_frame_time(&mut self) {
    let now = std::time::Instant::now();
    if let Some(last) = self.last_new_frame_time {
      self.actual_frame_duration_ns = last.elapsed().as_nanos() as u64;
    }
    self.last_new_frame_time = Some(now);
  }

  /// Collect scope data from puffin and render the profiler.
  /// Throttled to ~30 FPS to keep it readable.
  pub fn render(&mut self) {
    // Throttle to ~10 FPS (100ms between frames) for stable, readable display
    let elapsed = self.last_render_time.elapsed();
    if elapsed.as_millis() < 100 {
      return;
    }
    self.last_render_time = std::time::Instant::now();

    // Collect puffin data
    self.collect_puffin_data();

    // Clear pixel buffer with background color
    self.clear(colors::BACKGROUND);

    // Draw header
    self.draw_header();

    // Draw flamegraph
    self.draw_flamegraph();

    // Draw footer with help info
    self.draw_footer();

    // Draw tooltip if hovering
    if let Some(ref scope_name) = self.hovered_scope.clone() {
      self.draw_tooltip(scope_name);
    }

    // Upload and present
    self.present();
  }

  /// Collect scope data from puffin's frame view.
  fn collect_puffin_data(&mut self) {
    self.last_frame_scopes.clear();
    self.last_frame_duration_ns = 0;

    // Get data from the frame view while holding the lock, then release it
    let (frame, scope_collection) = {
      let frame_view = match self.frame_view.lock() {
        Ok(fv) => fv,
        Err(_) => {
          self.frames_without_data += 1;
          return;
        }
      };

      let frame = match frame_view.latest_frame() {
        Some(f) => f,
        None => {
          self.frames_without_data += 1;
          return;
        }
      };

      // Clone the scope collection while we have the lock
      (frame.clone(), frame_view.scope_collection().clone())
    };

    self.last_frame_duration_ns = frame.duration_ns() as u64;
    self.has_received_data = true;
    self.frames_without_data = 0;

    // Get unpacked frame data
    let unpacked = match frame.unpacked() {
      Ok(u) => u,
      Err(_) => return,
    };

    // Collect all scopes from all thread streams using recursive helper
    for (_thread_info, stream_info) in unpacked.thread_streams.iter() {
      Self::collect_scopes_recursive(
        &stream_info.stream,
        0,
        0,
        &scope_collection,
        &mut self.last_frame_scopes,
      );
    }

    // Update display data - just use the raw frame data directly
    // but track scope age for fade-out effects
    self.update_display_scopes();
  }

  /// Update display scopes - persist scopes so they don't disappear.
  fn update_display_scopes(&mut self) {
    self.display_frame_duration_ns = self.last_frame_duration_ns;

    // Update existing scopes or add new ones
    for scope in &self.last_frame_scopes {
      // Reset age for scopes we see this frame
      self.scope_age.insert(scope.name.clone(), 0);

      // Update or insert into display_scopes
      if let Some(existing) = self.display_scopes.iter_mut().find(|s| s.name == scope.name) {
        // Use smoothed update: blend toward new value
        // This keeps scopes visible even when they intermittently have 0 duration
        if scope.duration_ns > 0 {
          // Blend toward the new non-zero value
          existing.duration_ns = ((existing.duration_ns as f64 * 0.5) + (scope.duration_ns as f64 * 0.5)) as u64;
        }
        // Always keep at least some minimum display duration so scope stays visible
        existing.duration_ns = existing.duration_ns.max(1000); // 1 microsecond minimum
        existing.depth = scope.depth;
      } else {
        // Add new scope with minimum duration
        let mut new_scope = scope.clone();
        new_scope.duration_ns = new_scope.duration_ns.max(1000);
        self.display_scopes.push(new_scope);
      }
    }

    // Age scopes not seen this frame - slow decay to keep them visible longer
    for scope in &mut self.display_scopes {
      if let Some(age) = self.scope_age.get_mut(&scope.name) {
        if *age > 0 {
          // Scope wasn't in this frame - slow fade but maintain minimum
          scope.duration_ns = ((scope.duration_ns as f64 * 0.95) as u64).max(1000);
        }
        *age += 1;
      }
    }

    // Remove scopes that have been absent for very long (60 frames = 6 seconds at 10 FPS)
    self.display_scopes.retain(|s| {
      self.scope_age.get(&s.name).map(|&age| age < 60).unwrap_or(false)
    });
    self.scope_age.retain(|_, age| *age < 60);

    // Sort by depth for consistent display order
    self.display_scopes.sort_by_key(|s| s.depth);
  }

  /// Recursively collect scopes from a stream.
  fn collect_scopes_recursive(
    stream: &puffin::Stream,
    offset: u64,
    depth: usize,
    scope_collection: &puffin::ScopeCollection,
    scopes: &mut Vec<ScopeEntry>,
  ) {
    let reader = if offset == 0 {
      puffin::Reader::from_start(stream)
    } else {
      match puffin::Reader::with_offset(stream, offset) {
        Ok(r) => r,
        Err(_) => return,
      }
    };

    for scope in reader.flatten() {
      // Get scope name from the scope collection
      let name = scope_collection
        .fetch_by_id(&scope.id)
        .map(|details| details.name().to_string())
        .unwrap_or_else(|| format!("scope_{}", scope.id.0));

      scopes.push(ScopeEntry {
        name,
        duration_ns: scope.record.duration_ns as u64,
        depth,
      });

      // Recursively collect child scopes
      if scope.child_begin_position < scope.child_end_position {
        Self::collect_scopes_recursive(
          stream,
          scope.child_begin_position,
          depth + 1,
          scope_collection,
          scopes,
        );
      }
    }
  }

  /// Clear the pixel buffer with a color.
  fn clear(&mut self, color: [u8; 4]) {
    for pixel in self.pixel_buffer.chunks_exact_mut(4) {
      pixel.copy_from_slice(&color);
    }
  }

  /// Draw the header with FPS and frame time.
  fn draw_header(&mut self) {
    let header_height = 30;

    // Draw header background
    self.fill_rect(0, 0, self.width as i32, header_height, colors::HEADER_BG);

    // Use actual wall-clock frame duration for FPS (includes sleep/vsync wait)
    // Fall back to puffin's duration if we don't have wall clock data yet
    let actual_ms = if self.actual_frame_duration_ns > 0 {
      self.actual_frame_duration_ns as f64 / 1_000_000.0
    } else {
      self.display_frame_duration_ns as f64 / 1_000_000.0
    };
    let fps = if actual_ms > 0.0 {
      1000.0 / actual_ms
    } else {
      0.0
    };

    // Show work time (puffin) separately from actual frame time
    let work_ms = self.display_frame_duration_ns as f64 / 1_000_000.0;
    let text = format!("{:.0} FPS | Work: {:.2}ms", fps, work_ms);
    self.draw_text(&text, 10, 8, colors::TEXT);

    // Draw divider
    self.fill_rect(
      0,
      header_height - 1,
      self.width as i32,
      1,
      colors::DIVIDER,
    );
  }

  /// Draw the footer with scope count and help.
  fn draw_footer(&mut self) {
    let footer_height = 24;
    let footer_y = self.height as i32 - footer_height;
    let padding = 10;

    // Draw footer background
    self.fill_rect(0, footer_y, self.width as i32, footer_height, colors::HEADER_BG);

    // Draw divider at top of footer
    self.fill_rect(0, footer_y, self.width as i32, 1, colors::DIVIDER);

    // Show scope count and hint
    let scope_count = self.display_scopes.len();
    if scope_count > 0 {
      let text = format!("{} scope(s) | Hover for details", scope_count);
      self.draw_text(&text, padding, footer_y + 6, colors::TEXT_DIM);
    } else if self.has_received_data {
      self.draw_text("Add profile_function!() to see more data", padding, footer_y + 6, colors::TEXT_DIM);
    }
  }

  /// Draw the flamegraph with CPU and GPU sections.
  fn draw_flamegraph(&mut self) {
    let header_height = 30;
    let padding = 10;
    let bar_height = 20;
    let bar_spacing = 2;
    let section_header_height = 22;

    if self.display_scopes.is_empty() || self.display_frame_duration_ns == 0 {
      let mut y = header_height + padding;
      let line_height = 16;

      // Show different messages based on state
      if !self.has_received_data && self.frames_without_data < 180 {
        // First ~3 seconds: show waiting message
        self.draw_text("Waiting for profiling data...", padding, y, colors::TEXT_DIM);
      } else {
        // After 3 seconds of no data: show setup instructions
        self.draw_text("No profiling data detected", padding, y, colors::TEXT);
        y += line_height * 2;

        self.draw_text("To instrument your code:", padding, y, colors::TEXT_DIM);
        y += line_height;

        self.draw_text("  profile_scope!(\"name\")", padding + 10, y, colors::SCOPE_COLORS[0]);
        y += line_height * 2;

        self.draw_text("Example:", padding, y, colors::TEXT_DIM);
        y += line_height;
        self.draw_text("  fn update() {", padding, y, colors::TEXT_DIM);
        y += line_height;
        self.draw_text("    profile_scope!(\"physics\")", padding + 20, y, colors::SCOPE_COLORS[1]);
        y += line_height;
        self.draw_text("    // your code here", padding + 20, y, colors::TEXT_DIM);
        y += line_height;
        self.draw_text("  }", padding, y, colors::TEXT_DIM);
      }

      return;
    }

    let graph_width = (self.width as i32 - padding * 2) as f64;
    let max_x = self.width as i32 - padding; // Right boundary
    // Use actual wall-clock frame duration for consistent bar scaling across focus states
    let frame_duration = if self.actual_frame_duration_ns > 0 {
      self.actual_frame_duration_ns as f64
    } else {
      self.display_frame_duration_ns as f64
    };

    // Reset hovered scope
    self.hovered_scope = None;

    // Separate scopes by category (cpu:, gpu:, or uncategorized)
    let mut cpu_scopes: Vec<_> = Vec::new();
    let mut gpu_scopes: Vec<_> = Vec::new();
    let mut other_scopes: Vec<_> = Vec::new();

    for scope in &self.display_scopes {
      if scope.name.starts_with("cpu:") {
        cpu_scopes.push(scope.clone());
      } else if scope.name.starts_with("gpu:") {
        gpu_scopes.push(scope.clone());
      } else {
        other_scopes.push(scope.clone());
      }
    }

    let mut current_y = header_height + padding;

    // Draw Game section first (user code) - most relevant to game devs
    if !other_scopes.is_empty() {
      // Draw section header
      self.fill_rect(0, current_y, self.width as i32, section_header_height, colors::GAME_HEADER);
      self.draw_text("Game", padding, current_y + 5, colors::TEXT);

      // Calculate and show Game time
      let game_total_ns: u64 = other_scopes.iter().filter(|s| s.depth == 0).map(|s| s.duration_ns).sum();
      let game_ms = game_total_ns as f64 / 1_000_000.0;
      let game_text = format!("{:.2}ms", game_ms);
      self.draw_text(&game_text, 60, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      // Draw game scopes (no prefix stripping needed - they don't have prefixes)
      current_y = self.draw_scope_section(
        &other_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &colors::SCOPE_COLORS,
      );

      current_y += padding;
    }

    // Draw Engine - CPU section
    if !cpu_scopes.is_empty() {
      // Draw section header
      self.fill_rect(0, current_y, self.width as i32, section_header_height, colors::ENGINE_CPU_HEADER);
      self.draw_text("Engine - CPU", padding, current_y + 5, colors::TEXT);

      // Calculate and show CPU time
      let cpu_total_ns: u64 = cpu_scopes.iter().filter(|s| s.depth == 0).map(|s| s.duration_ns).sum();
      let cpu_ms = cpu_total_ns as f64 / 1_000_000.0;
      let cpu_text = format!("{:.2}ms", cpu_ms);
      self.draw_text(&cpu_text, 120, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      // Draw CPU scopes
      current_y = self.draw_scope_section(
        &cpu_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &colors::CPU_COLORS,
      );

      current_y += padding;
    }

    // Draw Engine - GPU section
    if !gpu_scopes.is_empty() {
      // Draw section header
      self.fill_rect(0, current_y, self.width as i32, section_header_height, colors::ENGINE_GPU_HEADER);
      self.draw_text("Engine - GPU", padding, current_y + 5, colors::TEXT);

      // Calculate and show GPU time
      let gpu_total_ns: u64 = gpu_scopes.iter().filter(|s| s.depth == 0).map(|s| s.duration_ns).sum();
      let gpu_ms = gpu_total_ns as f64 / 1_000_000.0;
      let gpu_text = format!("{:.2}ms", gpu_ms);
      self.draw_text(&gpu_text, 120, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      // Draw GPU scopes
      self.draw_scope_section(
        &gpu_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &colors::GPU_COLORS,
      );
    }
  }

  /// Draw a section of scopes and return the new y position.
  #[allow(clippy::too_many_arguments)]
  fn draw_scope_section(
    &mut self,
    scopes: &[ScopeEntry],
    start_y: i32,
    padding: i32,
    max_x: i32,
    bar_height: i32,
    bar_spacing: i32,
    graph_width: f64,
    frame_duration: f64,
    color_palette: &[[u8; 4]],
  ) -> i32 {
    // Track x position for each depth level to stack bars side-by-side
    let mut depth_x: std::collections::HashMap<usize, i32> = std::collections::HashMap::new();
    let mut max_y = start_y;

    // Draw each scope as a bar
    for scope in scopes.iter() {
      let y = start_y + (scope.depth as i32 * (bar_height + bar_spacing));
      max_y = max_y.max(y + bar_height);

      // Get current x position for this depth, defaulting to padding
      let x = *depth_x.get(&scope.depth).unwrap_or(&padding);

      // Calculate width based on duration
      let mut width = ((scope.duration_ns as f64 / frame_duration) * graph_width) as i32;
      width = width.max(8); // Minimum width of 8 pixels so brief scopes stay visible

      // Clamp width so bar doesn't exceed right margin
      width = width.min(max_x - x);

      // Skip if no room left (bar would be invisible)
      if width <= 0 {
        continue;
      }

      // Update x position for next bar at this depth
      depth_x.insert(scope.depth, x + width + 2);

      // Strip prefix from display name
      let display_name = scope
        .name
        .strip_prefix("cpu:")
        .or_else(|| scope.name.strip_prefix("gpu:"))
        .unwrap_or(&scope.name);

      // Choose color based on scope name hash for consistency
      let name_hash: usize = display_name.bytes().fold(0, |acc, b| acc.wrapping_add(b as usize));
      let color = color_palette[name_hash % color_palette.len()];

      // Check if mouse is hovering over this bar
      let mouse_in_bar = self.mouse_x >= x as f32
        && self.mouse_x <= (x + width) as f32
        && self.mouse_y >= y as f32
        && self.mouse_y <= (y + bar_height) as f32;

      // Draw bar (brighter if hovered)
      let draw_color = if mouse_in_bar {
        self.hovered_scope = Some(format!(
          "{}: {:.3}ms",
          display_name,
          scope.duration_ns as f64 / 1_000_000.0
        ));
        [
          (color[0] as u16 + 40).min(255) as u8,
          (color[1] as u16 + 40).min(255) as u8,
          (color[2] as u16 + 40).min(255) as u8,
          255,
        ]
      } else {
        color
      };

      self.fill_rect(x, y, width, bar_height, draw_color);

      // Draw scope name if bar is wide enough
      if width > 40 {
        let duration_ms = scope.duration_ns as f64 / 1_000_000.0;
        let label = if width > 100 {
          format!("{} ({:.2}ms)", display_name, duration_ms)
        } else {
          display_name.to_string()
        };
        self.draw_text_shadowed(&label, x + 4, y + 4, colors::TEXT);
      }
    }

    max_y
  }

  /// Draw a tooltip near the mouse.
  fn draw_tooltip(&mut self, text: &str) {
    let padding = 6;
    let char_width = 7;
    let char_height = 12;

    let width = (text.len() as i32 * char_width) + padding * 2;
    let height = char_height + padding * 2;

    let x = (self.mouse_x as i32 + 15).min(self.width as i32 - width - 5);
    let y = (self.mouse_y as i32 + 15).min(self.height as i32 - height - 5);

    // Draw tooltip background
    self.fill_rect(x, y, width, height, [50, 50, 50, 240]);
    self.draw_rect_outline(x, y, width, height, colors::DIVIDER);

    // Draw text
    self.draw_text(text, x + padding, y + padding, colors::TEXT);
  }

  /// Fill a rectangle with a color.
  fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
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
  fn draw_rect_outline(&mut self, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
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
  fn set_pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32 {
      let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
      if idx + 3 < self.pixel_buffer.len() {
        self.pixel_buffer[idx..idx + 4].copy_from_slice(&color);
      }
    }
  }

  /// Draw text (simple 7x12 font).
  fn draw_text(&mut self, text: &str, x: i32, y: i32, color: [u8; 4]) {
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
  fn draw_text_shadowed(&mut self, text: &str, x: i32, y: i32, color: [u8; 4]) {
    const SHADOW: [u8; 4] = [0, 0, 0, 200];
    // Draw shadow offset by 1 pixel
    self.draw_text(text, x + 1, y + 1, SHADOW);
    // Draw main text on top
    self.draw_text(text, x, y, color);
  }

  /// Upload pixel buffer and present to screen.
  fn present(&mut self) {
    // Upload pixel buffer to texture
    self.queue.write_texture(
      wgpu::ImageCopyTexture {
        texture: &self.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      &self.pixel_buffer,
      wgpu::ImageDataLayout {
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
      Ok(t) => t,
      Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
        self.surface.configure(&self.device, &self.surface_config);
        return;
      }
      Err(e) => {
        eprintln!("[Profiler] Surface error: {}", e);
        return;
      }
    };

    let view = output
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("profiler_encoder"),
      });

    {
      let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("profiler_render_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
      });

      render_pass.set_pipeline(&self.render_pipeline);
      render_pass.set_bind_group(0, &self.bind_group, &[]);
      render_pass.draw(0..6, 0..1);
    }

    self.queue.submit(std::iter::once(encoder.finish()));
    output.present();

    // Request continuous redraws to keep profiler updating even when unfocused
    self.window.request_redraw();
  }
}

/// Simple shader for displaying texture on screen.
const PROFILER_SHADER: &str = r#"
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
fn get_glyph(ch: char) -> Option<[u8; 12]> {
  // Simple ASCII subset - numbers, letters, and common punctuation
  match ch {
    ' ' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    '!' => Some([0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x10, 0x10, 0, 0]),
    '(' => Some([0x08, 0x10, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x10, 0x08, 0, 0]),
    ')' => Some([0x20, 0x10, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x10, 0x20, 0, 0]),
    '-' => Some([0, 0, 0, 0, 0, 0x7C, 0, 0, 0, 0, 0, 0]),
    '.' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0x18, 0x18, 0, 0]),
    ':' => Some([0, 0, 0x18, 0x18, 0, 0, 0, 0x18, 0x18, 0, 0, 0]),
    '0' => Some([0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    '1' => Some([0x10, 0x30, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x38, 0, 0]),
    '2' => Some([0x38, 0x44, 0x04, 0x04, 0x08, 0x10, 0x20, 0x40, 0x44, 0x7C, 0, 0]),
    '3' => Some([0x38, 0x44, 0x04, 0x04, 0x18, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0]),
    '4' => Some([0x08, 0x18, 0x28, 0x28, 0x48, 0x48, 0x7C, 0x08, 0x08, 0x08, 0, 0]),
    '5' => Some([0x7C, 0x40, 0x40, 0x78, 0x04, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0]),
    '6' => Some([0x38, 0x44, 0x40, 0x40, 0x78, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    '7' => Some([0x7C, 0x44, 0x04, 0x08, 0x08, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0]),
    '8' => Some([0x38, 0x44, 0x44, 0x44, 0x38, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    '9' => Some([0x38, 0x44, 0x44, 0x44, 0x3C, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0]),
    'A' | 'a' => Some([0x38, 0x44, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0]),
    'B' | 'b' => Some([0x78, 0x44, 0x44, 0x44, 0x78, 0x44, 0x44, 0x44, 0x44, 0x78, 0, 0]),
    'C' | 'c' => Some([0x38, 0x44, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x44, 0x38, 0, 0]),
    'D' | 'd' => Some([0x78, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x78, 0, 0]),
    'E' | 'e' => Some([0x7C, 0x40, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x40, 0x7C, 0, 0]),
    'F' | 'f' => Some([0x7C, 0x40, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x40, 0x40, 0, 0]),
    'G' | 'g' => Some([0x38, 0x44, 0x40, 0x40, 0x4C, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    'H' | 'h' => Some([0x44, 0x44, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0]),
    'I' | 'i' => Some([0x38, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x38, 0, 0]),
    'J' | 'j' => Some([0x1C, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x48, 0x48, 0x30, 0, 0]),
    'K' | 'k' => Some([0x44, 0x48, 0x50, 0x60, 0x60, 0x50, 0x48, 0x44, 0x44, 0x44, 0, 0]),
    'L' | 'l' => Some([0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7C, 0, 0]),
    'M' | 'm' => Some([0x44, 0x6C, 0x54, 0x54, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0, 0]),
    'N' | 'n' => Some([0x44, 0x64, 0x64, 0x54, 0x54, 0x4C, 0x4C, 0x44, 0x44, 0x44, 0, 0]),
    'O' | 'o' => Some([0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    'P' | 'p' => Some([0x78, 0x44, 0x44, 0x44, 0x78, 0x40, 0x40, 0x40, 0x40, 0x40, 0, 0]),
    'Q' | 'q' => Some([0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x54, 0x48, 0x44, 0x3A, 0, 0]),
    'R' | 'r' => Some([0x78, 0x44, 0x44, 0x44, 0x78, 0x50, 0x48, 0x44, 0x44, 0x44, 0, 0]),
    'S' | 's' => Some([0x38, 0x44, 0x40, 0x40, 0x38, 0x04, 0x04, 0x04, 0x44, 0x38, 0, 0]),
    'T' | 't' => Some([0x7C, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0]),
    'U' | 'u' => Some([0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0, 0]),
    'V' | 'v' => Some([0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x28, 0x28, 0x10, 0, 0]),
    'W' | 'w' => Some([0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x54, 0x54, 0x6C, 0x44, 0, 0]),
    'X' | 'x' => Some([0x44, 0x44, 0x28, 0x28, 0x10, 0x28, 0x28, 0x44, 0x44, 0x44, 0, 0]),
    'Y' | 'y' => Some([0x44, 0x44, 0x44, 0x28, 0x28, 0x10, 0x10, 0x10, 0x10, 0x10, 0, 0]),
    'Z' | 'z' => Some([0x7C, 0x04, 0x04, 0x08, 0x10, 0x20, 0x40, 0x40, 0x40, 0x7C, 0, 0]),
    '_' => Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0x7C, 0, 0]),
    _ => None, // Unknown character
  }
}
