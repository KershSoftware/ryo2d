//! Profiler window - A dedicated window showing puffin profiling data.
//!
//! This module provides a separate window that displays real-time flamegraphs
//! and profiling statistics using puffin data. Built on the generic dev_window
//! framework for consistent rendering and resource sharing.

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use super::dev_window::{DevWindowConfig, DevWindowRenderer, colors};
use crate::engine_info;

/// Colors for the profiler UI
mod profiler_colors {
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
  renderer: DevWindowRenderer,
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
  // Hovered scope name for tooltip
  hovered_scope: Option<String>,
  // Frame counter for "waiting" state
  frames_without_data: u32,
  // Track if we've ever received data
  has_received_data: bool,
  // Track actual wall-clock frame time for accurate FPS display
  last_new_frame_time: Option<web_time::Instant>,
  actual_frame_duration_ns: u64,
  // Pause state - when true, stop collecting new data
  paused: bool,
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
    let config = DevWindowConfig {
      title: "Ryo2D Profiler".to_string(),
      width: 600,
      height: 400,
      render_throttle_ms: 100, // ~10 FPS for stable display
    };

    let renderer = DevWindowRenderer::new(event_loop, device, queue, instance, adapter, config)?;

    // Create FrameView and register as a sink to receive frame data
    let frame_view = Arc::new(std::sync::Mutex::new(puffin::FrameView::default()));
    let frame_view_clone = Arc::clone(&frame_view);
    let sink_id = {
      let mut profiler = puffin::GlobalProfiler::lock();
      let sink_id = profiler.add_sink(Box::new(move |frame| {
        if let Ok(mut fv) = frame_view_clone.lock() {
          fv.add_frame(frame);
        }
      }));
      // Request all existing scope details to be sent to our sink.
      // This is needed because scopes registered before the window opened
      // won't have their names in our FrameView's scope_collection otherwise.
      profiler.emit_scope_snapshot();
      sink_id
    };

    engine_info!(module: "Profiler", "Window opened");

    Some(Self {
      renderer,
      frame_view,
      _sink_id: sink_id,
      last_frame_scopes: Vec::new(),
      last_frame_duration_ns: 0,
      hovered_scope: None,
      frames_without_data: 0,
      has_received_data: false,
      last_new_frame_time: None,
      actual_frame_duration_ns: 0,
      display_scopes: Vec::new(),
      display_frame_duration_ns: 0,
      scope_age: std::collections::HashMap::new(),
      paused: false,
    })
  }

  /// Get the window ID for event routing.
  pub fn window_id(&self) -> WindowId {
    self.renderer.window_id()
  }

  /// Handle a window event.
  pub fn handle_event(&mut self, event: &WindowEvent) -> bool {
    // Check for clicks on the pause button
    if let WindowEvent::MouseInput {
      state: winit::event::ElementState::Pressed,
      button: winit::event::MouseButton::Left,
      ..
    } = event
    {
      // Check if click is in the pause button area (right side of header)
      let btn_width = 70;
      let btn_x = self.renderer.width() as i32 - btn_width - 10;
      let mouse_x = self.renderer.mouse_x() as i32;
      let mouse_y = self.renderer.mouse_y() as i32;

      if mouse_x >= btn_x && mouse_x <= btn_x + btn_width && (5..=25).contains(&mouse_y) {
        self.paused = !self.paused;
        return true;
      }
    }
    self.renderer.handle_event(event)
  }

  /// Request a redraw.
  pub fn request_redraw(&self) {
    self.renderer.request_redraw();
  }

  /// Track wall clock time between frames for accurate FPS display.
  pub fn track_frame_time(&mut self) {
    // Don't update frame time when paused - keeps the display frozen
    if self.paused {
      return;
    }
    let now = web_time::Instant::now();
    if let Some(last) = self.last_new_frame_time {
      self.actual_frame_duration_ns = last.elapsed().as_nanos() as u64;
    }
    self.last_new_frame_time = Some(now);
  }

  /// Collect scope data from puffin and render the profiler.
  pub fn render(&mut self) {
    // Check throttling
    if !self.renderer.should_render() {
      return;
    }
    self.renderer.mark_rendered();

    // Collect puffin data
    self.collect_puffin_data();

    // Clear pixel buffer with background color
    self.renderer.clear(colors::BACKGROUND);

    // Draw header
    self.draw_header();

    // Draw flamegraph
    self.draw_flamegraph();

    // Draw footer with help info
    self.draw_footer();

    // Draw tooltip if hovering
    if let Some(ref scope_name) = self.hovered_scope.clone() {
      self
        .renderer
        .draw_tooltip(scope_name, self.renderer.mouse_x(), self.renderer.mouse_y());
    }

    // Upload and present
    self.renderer.present();
  }

  /// Collect scope data from puffin's frame view.
  fn collect_puffin_data(&mut self) {
    // Skip collection when paused - keep displaying frozen data
    if self.paused {
      return;
    }

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

      match frame_view.latest_frame() {
        Some(f) => (f.clone(), frame_view.scope_collection().clone()),
        None => {
          self.frames_without_data += 1;
          return;
        }
      }
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
    for stream_info in unpacked.thread_streams.values() {
      Self::collect_scopes_recursive(
        &stream_info.stream,
        0,
        0,
        &scope_collection,
        &mut self.last_frame_scopes,
      );
    }

    // Update display data
    self.update_display_scopes();
  }

  /// Update display scopes - persist scopes so they don't disappear.
  fn update_display_scopes(&mut self) {
    // Smooth the frame duration to prevent bar width jumps when focus changes
    // (puffin frame.duration_ns() can vary based on parallel work and timing)
    if self.display_frame_duration_ns > 0 && self.last_frame_duration_ns > 0 {
      self.display_frame_duration_ns = ((self.display_frame_duration_ns as f64 * 0.7)
        + (self.last_frame_duration_ns as f64 * 0.3)) as u64;
    } else {
      self.display_frame_duration_ns = self.last_frame_duration_ns;
    }

    // Update existing scopes or add new ones
    for scope in &self.last_frame_scopes {
      // Reset age for scopes we see this frame
      self.scope_age.insert(scope.name.clone(), 0);

      // Update or insert into display_scopes
      if let Some(existing) = self
        .display_scopes
        .iter_mut()
        .find(|s| s.name == scope.name)
      {
        if scope.duration_ns > 0 {
          existing.duration_ns =
            ((existing.duration_ns as f64 * 0.5) + (scope.duration_ns as f64 * 0.5)) as u64;
        }
        existing.duration_ns = existing.duration_ns.max(1000);
        existing.depth = scope.depth;
      } else {
        let mut new_scope = scope.clone();
        new_scope.duration_ns = new_scope.duration_ns.max(1000);
        self.display_scopes.push(new_scope);
      }
    }

    // Age scopes not seen this frame
    for scope in &mut self.display_scopes {
      if let Some(age) = self.scope_age.get_mut(&scope.name) {
        if *age > 0 {
          scope.duration_ns = ((scope.duration_ns as f64 * 0.95) as u64).max(1000);
        }
        *age += 1;
      }
    }

    // Remove old scopes
    self.display_scopes.retain(|s| {
      self
        .scope_age
        .get(&s.name)
        .map(|&age| age < 60)
        .unwrap_or(false)
    });
    self.scope_age.retain(|_, age| *age < 60);

    // Sort by depth
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
      let name = scope_collection
        .fetch_by_id(&scope.id)
        .map(|details| details.name().to_string())
        .unwrap_or_else(|| format!("scope_{}", scope.id.0));

      scopes.push(ScopeEntry {
        name,
        duration_ns: scope.record.duration_ns as u64,
        depth,
      });

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

  /// Draw the header with FPS and frame time.
  fn draw_header(&mut self) {
    let header_height = 30;
    let width = self.renderer.width() as i32;

    // Draw header background
    self
      .renderer
      .fill_rect(0, 0, width, header_height, colors::HEADER_BG);

    // Use actual wall-clock frame duration for FPS
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

    let work_ms = self.display_frame_duration_ns as f64 / 1_000_000.0;
    let text = format!("{:.0} FPS | Work: {:.2}ms", fps, work_ms);
    self.renderer.draw_text(&text, 10, 8, colors::TEXT);

    // Draw pause/play button on the right side
    let btn_width = 70;
    let btn_height = 20;
    let btn_x = width - btn_width - 10;
    let btn_y = 5;

    // Check if mouse is hovering over button
    let mouse_x = self.renderer.mouse_x() as i32;
    let mouse_y = self.renderer.mouse_y() as i32;
    let hovering = mouse_x >= btn_x
      && mouse_x <= btn_x + btn_width
      && mouse_y >= btn_y
      && mouse_y <= btn_y + btn_height;

    // Button colors
    let (btn_bg, btn_text) = if self.paused {
      // Paused state - show "Play" with green tint
      if hovering {
        ([60, 90, 60, 255], [150, 255, 150, 255])
      } else {
        ([50, 70, 50, 255], [120, 200, 120, 255])
      }
    } else {
      // Running state - show "Freeze" with blue tint
      if hovering {
        ([70, 70, 90, 255], [180, 180, 255, 255])
      } else {
        ([55, 55, 75, 255], [140, 140, 220, 255])
      }
    };

    // Draw button background
    self
      .renderer
      .fill_rect(btn_x, btn_y, btn_width, btn_height, btn_bg);
    self
      .renderer
      .draw_rect_outline(btn_x, btn_y, btn_width, btn_height, colors::DIVIDER);

    // Draw button text
    let btn_label = if self.paused { "Resume" } else { "Freeze" };
    let label_x = btn_x + (btn_width - self.renderer.measure_text(btn_label)) / 2;
    self
      .renderer
      .draw_text(btn_label, label_x, btn_y + 4, btn_text);

    // Draw divider
    self
      .renderer
      .fill_rect(0, header_height - 1, width, 1, colors::DIVIDER);
  }

  /// Draw the footer with scope count and help.
  fn draw_footer(&mut self) {
    let footer_height = 24;
    let footer_y = self.renderer.height() as i32 - footer_height;
    let padding = 10;
    let width = self.renderer.width() as i32;

    self
      .renderer
      .fill_rect(0, footer_y, width, footer_height, colors::HEADER_BG);
    self
      .renderer
      .fill_rect(0, footer_y, width, 1, colors::DIVIDER);

    let scope_count = self.display_scopes.len();
    if self.paused {
      // Show frozen indicator
      let text = format!(
        "FROZEN - {} scope(s) | Click Resume to continue",
        scope_count
      );
      self
        .renderer
        .draw_text(&text, padding, footer_y + 6, [255, 200, 100, 255]);
    } else if scope_count > 0 {
      let text = format!("{} scope(s) | Hover for details", scope_count);
      self
        .renderer
        .draw_text(&text, padding, footer_y + 6, colors::TEXT_DIM);
    } else if self.has_received_data {
      self.renderer.draw_text(
        "Add profile_function!() to see more data",
        padding,
        footer_y + 6,
        colors::TEXT_DIM,
      );
    }
  }

  /// Draw the flamegraph with CPU and GPU sections.
  fn draw_flamegraph(&mut self) {
    let header_height = 30;
    let padding = 10;
    let bar_height = 20;
    let bar_spacing = 2;
    let section_header_height = 22;
    let width = self.renderer.width() as i32;

    if self.display_scopes.is_empty() || self.display_frame_duration_ns == 0 {
      let mut y = header_height + padding;
      let line_height = 16;

      if !self.has_received_data && self.frames_without_data < 180 {
        self.renderer.draw_text(
          "Waiting for profiling data...",
          padding,
          y,
          colors::TEXT_DIM,
        );
      } else {
        self
          .renderer
          .draw_text("No profiling data detected", padding, y, colors::TEXT);
        y += line_height * 2;

        self
          .renderer
          .draw_text("To instrument your code:", padding, y, colors::TEXT_DIM);
        y += line_height;

        self.renderer.draw_text(
          "  profile_scope!(\"name\")",
          padding + 10,
          y,
          profiler_colors::SCOPE_COLORS[0],
        );
        y += line_height * 2;

        self
          .renderer
          .draw_text("Example:", padding, y, colors::TEXT_DIM);
        y += line_height;
        self
          .renderer
          .draw_text("  fn update() {", padding, y, colors::TEXT_DIM);
        y += line_height;
        self.renderer.draw_text(
          "    profile_scope!(\"physics\")",
          padding + 20,
          y,
          profiler_colors::SCOPE_COLORS[1],
        );
        y += line_height;
        self
          .renderer
          .draw_text("    // your code here", padding + 20, y, colors::TEXT_DIM);
        y += line_height;
        self.renderer.draw_text("  }", padding, y, colors::TEXT_DIM);
      }

      return;
    }

    let graph_width = (width - padding * 2) as f64;
    let max_x = width - padding;
    // Always use display_frame_duration_ns for bar calculations - this comes from
    // actual puffin profiling data and matches the scope durations.
    // (actual_frame_duration_ns reflects profiler window's render interval, not game frame time)
    let frame_duration = self.display_frame_duration_ns as f64;

    // Reset hovered scope
    self.hovered_scope = None;

    // Separate scopes by category
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

    // Normalize depths within each section so they start at 0
    fn normalize_depths(scopes: &mut [ScopeEntry]) {
      if let Some(min_depth) = scopes.iter().map(|s| s.depth).min() {
        for scope in scopes.iter_mut() {
          scope.depth = scope.depth.saturating_sub(min_depth);
        }
      }
    }
    normalize_depths(&mut cpu_scopes);
    normalize_depths(&mut gpu_scopes);
    normalize_depths(&mut other_scopes);

    let mut current_y = header_height + padding;

    // Draw Game section first
    if !other_scopes.is_empty() {
      self.renderer.fill_rect(
        0,
        current_y,
        width,
        section_header_height,
        profiler_colors::GAME_HEADER,
      );
      self
        .renderer
        .draw_text("Game", padding, current_y + 5, colors::TEXT);

      let game_total_ns: u64 = other_scopes
        .iter()
        .filter(|s| s.depth == 0)
        .map(|s| s.duration_ns)
        .sum();
      let game_ms = game_total_ns as f64 / 1_000_000.0;
      let game_text = format!("{:.2}ms", game_ms);
      self
        .renderer
        .draw_text(&game_text, 60, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      current_y = self.draw_scope_section(
        &other_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &profiler_colors::SCOPE_COLORS,
      );

      current_y += padding;
    }

    // Draw Engine - CPU section
    if !cpu_scopes.is_empty() {
      self.renderer.fill_rect(
        0,
        current_y,
        width,
        section_header_height,
        profiler_colors::ENGINE_CPU_HEADER,
      );
      self
        .renderer
        .draw_text("Engine - CPU", padding, current_y + 5, colors::TEXT);

      let cpu_total_ns: u64 = cpu_scopes
        .iter()
        .filter(|s| s.depth == 0)
        .map(|s| s.duration_ns)
        .sum();
      let cpu_ms = cpu_total_ns as f64 / 1_000_000.0;
      let cpu_text = format!("{:.2}ms", cpu_ms);
      self
        .renderer
        .draw_text(&cpu_text, 120, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      current_y = self.draw_scope_section(
        &cpu_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &profiler_colors::CPU_COLORS,
      );

      current_y += padding;
    }

    // Draw Engine - GPU section
    if !gpu_scopes.is_empty() {
      self.renderer.fill_rect(
        0,
        current_y,
        width,
        section_header_height,
        profiler_colors::ENGINE_GPU_HEADER,
      );
      self
        .renderer
        .draw_text("Engine - GPU", padding, current_y + 5, colors::TEXT);

      let gpu_total_ns: u64 = gpu_scopes
        .iter()
        .filter(|s| s.depth == 0)
        .map(|s| s.duration_ns)
        .sum();
      let gpu_ms = gpu_total_ns as f64 / 1_000_000.0;
      let gpu_text = format!("{:.2}ms", gpu_ms);
      self
        .renderer
        .draw_text(&gpu_text, 120, current_y + 5, colors::TEXT_DIM);

      current_y += section_header_height + 2;

      self.draw_scope_section(
        &gpu_scopes,
        current_y,
        padding,
        max_x,
        bar_height,
        bar_spacing,
        graph_width,
        frame_duration,
        &profiler_colors::GPU_COLORS,
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
    let mut depth_x: std::collections::HashMap<usize, i32> = std::collections::HashMap::new();
    let mut max_y = start_y;
    let mouse_x = self.renderer.mouse_x();
    let mouse_y = self.renderer.mouse_y();

    for scope in scopes.iter() {
      let y = start_y + (scope.depth as i32 * (bar_height + bar_spacing));
      max_y = max_y.max(y + bar_height);

      let x = *depth_x.get(&scope.depth).unwrap_or(&padding);

      let mut width = ((scope.duration_ns as f64 / frame_duration) * graph_width) as i32;
      width = width.max(8);
      width = width.min(max_x - x);

      if width <= 0 {
        continue;
      }

      depth_x.insert(scope.depth, x + width + 2);

      let display_name = scope
        .name
        .strip_prefix("cpu:")
        .or_else(|| scope.name.strip_prefix("gpu:"))
        .unwrap_or(&scope.name);

      let name_hash: usize = display_name
        .bytes()
        .fold(0, |acc, b| acc.wrapping_add(b as usize));
      let color = color_palette[name_hash % color_palette.len()];

      let mouse_in_bar = mouse_x >= x as f32
        && mouse_x <= (x + width) as f32
        && mouse_y >= y as f32
        && mouse_y <= (y + bar_height) as f32;

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

      self.renderer.fill_rect(x, y, width, bar_height, draw_color);

      if width > 40 {
        let duration_ms = scope.duration_ns as f64 / 1_000_000.0;
        let label = if width > 100 {
          format!("{} ({:.2}ms)", display_name, duration_ms)
        } else {
          display_name.to_string()
        };
        self
          .renderer
          .draw_text_shadowed(&label, x + 4, y + 4, colors::TEXT);
      }
    }

    max_y
  }
}
