//! The window: winit events in, window and cursor requests out, dev windows,
//! and frames while the window is unfocused.

use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{Ime, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use super::frame::should_pace_frames;
use super::{Engine, EngineEvent, FrameOutcome};
use crate::core::config::DisplayMode;
use crate::core::cursor::CursorRequest;
use crate::core::graphics::WgpuRenderer;
use crate::core::graphics::renderer::RendererSettings;
use crate::core::graphics::screen::initial_window_size;
use crate::core::input::{Input, InputEvent};
use crate::core::render::RenderPipeline;
use crate::core::window::{WindowInfo, WindowRequest};
use crate::{engine_debug, engine_info, engine_warn};

/// Cursor grab mode and visibility for a focus change, or `None` when the
/// cursor isn't locked and nothing changes.
fn cursor_grab_for_focus(
  cursor_locked: bool,
  focused: bool,
) -> Option<(winit::window::CursorGrabMode, bool)> {
  use winit::window::CursorGrabMode;
  if !cursor_locked {
    return None;
  }
  if focused {
    // Regained focus: re-lock cursor
    Some((CursorGrabMode::Confined, false))
  } else {
    // Lost focus: release cursor so user can interact with other windows
    Some((CursorGrabMode::None, true))
  }
}

impl Engine {
  /// Show the window and start the engine on its new renderer (native at
  /// once, the web when its async GPU setup arrives).
  fn finish_gpu_setup(
    &mut self,
    event_loop: &ActiveEventLoop,
    window: Arc<winit::window::Window>,
    mut renderer: WgpuRenderer,
  ) {
    // Present a first frame before showing the window, to avoid a white flash
    let _ = renderer.render_blank_frame(&mut RenderPipeline::default_2d());
    window.set_visible(true);

    let scale_factor = window.scale_factor();
    self.window = Some(window);
    self.init_gpu(renderer, scale_factor);

    // The profiler opens on its own only for `ryo2d dev --profiler`
    #[cfg(feature = "profiler")]
    if std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok()
      && let Some(renderer) = &self.renderer
      && let (Some(instance), Some(adapter)) = (renderer.instance(), renderer.adapter())
    {
      self.profiler_window = crate::core::windows::profiler_window::ProfilerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        instance,
        adapter,
      );
    }

    // A dev window on the web would be a second canvas on the page
    if self.config.debug.logger_window
      && !cfg!(target_arch = "wasm32")
      && let Some(renderer) = &self.renderer
      && let (Some(instance), Some(adapter)) = (renderer.instance(), renderer.adapter())
    {
      self.logger_window = crate::core::windows::logger_window::LoggerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        instance,
        adapter,
      );
    }
  }

  fn renderer_settings(&self) -> RendererSettings {
    RendererSettings {
      presentation: self.config.presentation,
      vsync: Some(self.vsync),
    }
  }

  /// Apply a requested UI scale (`None`: the automatic scale).
  fn apply_ui_scale(&mut self, scale: Option<f32>) {
    self.config.ui_scale = scale;
    self.refresh_ui_space();
  }

  /// Apply the window requests that still make sense without a window.
  /// Only UI scale does; resolution and display mode requests are ignored.
  #[cfg(not(target_arch = "wasm32"))]
  pub(crate) fn apply_headless_requests(&mut self) {
    let request = std::mem::take(&mut *self.world.resource_mut::<WindowRequest>());
    if let Some(scale) = request.ui_scale {
      self.apply_ui_scale(scale);
    }
  }

  /// Apply pending window configuration changes from the game.
  fn apply_window_request(&mut self, window: &Arc<Window>, request: WindowRequest) {
    // A resolution request sizes the window; the game's resolution stays
    if let (Some(width), Some(height)) = (request.width, request.height) {
      let new_size = winit::dpi::PhysicalSize::new(width, height);
      if let Some(actual_size) = window.request_inner_size(new_size) {
        self.last_window_size = (actual_size.width, actual_size.height);
      }
    }

    if let Some(mode) = request.display_mode {
      match mode {
        DisplayMode::Windowed => {
          window.set_fullscreen(None);
          window.set_decorations(true);
        }
        DisplayMode::ExclusiveFullscreen => {
          // The current monitor's native resolution
          if let Some(monitor) = window.current_monitor()
            && let Some(video_mode) = monitor.video_modes().next()
          {
            window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
          }
        }
        DisplayMode::BorderlessFullscreen => {
          // Clear maximized first (see the F5 toggle below)
          window.set_maximized(false);
          window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
            window.current_monitor(),
          )));
        }
      }
      self.set_display_mode(mode);
      self.surface_needs_reconfigure = true;
    }

    if let Some(scale) = request.ui_scale {
      self.apply_ui_scale(scale);
    }
  }

  fn set_display_mode(&mut self, mode: DisplayMode) {
    self.config.display_mode = mode;
    self.world.resource_mut::<WindowInfo>().display_mode = mode;
  }

  /// Apply pending cursor configuration changes from the game.
  fn apply_cursor_request(
    &mut self,
    window: &Arc<Window>,
    event_loop: &ActiveEventLoop,
    request: CursorRequest,
  ) {
    use winit::window::CursorGrabMode;

    // Custom cursor takes precedence over system cursor
    if let Some(custom_data) = &request.custom_cursor {
      if let Some(cached) = self.custom_cursor_cache.get(&custom_data.id) {
        window.set_cursor(cached.clone());
      } else {
        match winit::window::CustomCursor::from_rgba(
          custom_data.rgba.clone(),
          custom_data.width,
          custom_data.height,
          custom_data.hotspot_x,
          custom_data.hotspot_y,
        ) {
          Ok(source) => {
            let cursor = event_loop.create_custom_cursor(source);
            window.set_cursor(cursor.clone());
            self
              .custom_cursor_cache
              .insert(custom_data.id.clone(), cursor);
          }
          Err(e) => {
            engine_warn!(module: "Cursor", "Failed to create custom cursor: {:?}", e);
          }
        }
      }
    } else if let Some(style) = &request.cursor {
      window.set_cursor(winit::window::Cursor::Icon(style.to_winit()));
    }

    if let Some(visible) = request.visible {
      window.set_cursor_visible(visible);
    }

    // Lock (FPS-style: confined and hidden, raw motion via DeviceEvent)
    if let Some(locked) = request.locked {
      if locked {
        // Confined (ClipCursor on Windows) reliably keeps the cursor off other
        // monitors; raw deltas arrive whatever the grab mode
        let _ = window.set_cursor_grab(CursorGrabMode::Confined);
        window.set_cursor_visible(false);
        self.cursor_locked = true;
      } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
        self.cursor_locked = false;
      }
    } else if let Some(confined) = request.confined {
      let mode = if confined {
        CursorGrabMode::Confined
      } else {
        CursorGrabMode::None
      };
      if window.set_cursor_grab(mode).is_err() {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
      }
    }
  }

  /// Load a window icon from a file path or use the embedded default logo.
  fn load_window_icon(icon_path: Option<&std::path::Path>) -> Option<winit::window::Icon> {
    if let Some(path) = icon_path {
      if let Ok(img) = image::open(path) {
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();
        return winit::window::Icon::from_rgba(rgba.into_raw(), width, height).ok();
      }
      engine_warn!(
        module: "Icon",
        "Failed to load custom icon from {:?}, using default",
        path
      );
    }

    let logo_bytes = include_bytes!("../../assets/logo.png");
    let img = image::load_from_memory(logo_bytes).ok()?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    winit::window::Icon::from_rgba(rgba.into_raw(), width, height).ok()
  }

  /// Bookkeeping after a frame: timings, the once-a-second log line.
  fn after_frame(&mut self, frame_start: Instant) {
    self.frame_timings.total = frame_start.elapsed();
    self.smoothed_timings.update(&self.frame_timings);

    // Log when a frame takes too long (>33ms = <30fps)
    #[cfg(debug_assertions)]
    {
      let total_ms = self.frame_timings.total.as_secs_f64() * 1000.0;
      if total_ms > 33.0 {
        engine_warn!(
          module: "Perf",
          "SLOW FRAME - Total: {:.1}ms | Update: {:.1}ms | Render: {:.1}ms | Present: {:.1}ms",
          total_ms,
          self.frame_timings.update.as_secs_f64() * 1000.0,
          self.frame_timings.render.as_secs_f64() * 1000.0,
          self.frame_timings.present.as_secs_f64() * 1000.0,
        );
      }
    }

    if self.last_fps_log.elapsed() >= Duration::from_secs(1) {
      self.last_fps_log = Instant::now();
      if self.config.debug.show_frame_timing {
        engine_debug!(
          module: "FrameTiming",
          "FPS: {:.0} | Update: {:.2}ms | Render: {:.2}ms | Present: {:.2}ms | Total: {:.2}ms",
          self.smoothed_timings.fps,
          self.frame_timings.update.as_secs_f64() * 1000.0,
          self.frame_timings.render.as_secs_f64() * 1000.0,
          self.frame_timings.present.as_secs_f64() * 1000.0,
          self.frame_timings.total.as_secs_f64() * 1000.0,
        );
      }
    }
  }

  /// A frame while the game window is unfocused (no redraw events arrive).
  ///
  /// `throttle` (no window of ours has focus) caps the rate to the background
  /// FPS and draws only with `background_rendering`. Otherwise a dev window
  /// has focus: draw at full rate and refresh the dev windows.
  fn unfocused_tick(&mut self, event_loop: &ActiveEventLoop, throttle: bool) {
    let frame_start = Instant::now();
    #[cfg(feature = "profiler")]
    if !throttle {
      if let Some(profiler) = &mut self.profiler_window {
        profiler.track_frame_time();
      }
      crate::core::profiler::new_frame();
    }

    self.skip_present = throttle && !self.config.background_rendering;
    let outcome = self.frame(None);
    self.skip_present = false;
    if matches!(outcome, FrameOutcome::Quit | FrameOutcome::Fatal(_)) {
      event_loop.exit();
      return;
    }
    self.after_frame(frame_start);

    let fps = if throttle && self.config.target_background_fps > 0 {
      self.config.target_background_fps
    } else {
      self.config.target_fps
    };
    let target = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
    let elapsed = frame_start.elapsed();
    // The browser paces frames, and the web cannot sleep
    if elapsed < target && !cfg!(target_arch = "wasm32") {
      std::thread::sleep(target - elapsed);
    }

    if !throttle {
      // After the sleep, so the profiler reads the whole frame's duration
      #[cfg(feature = "profiler")]
      if let Some(profiler) = &mut self.profiler_window {
        profiler.render();
      }
      if let Some(logger) = &mut self.logger_window {
        logger.render();
      }
    }
  }
}

impl ApplicationHandler<EngineEvent> for Engine {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    // The web can resume a page that already has its canvas
    if self.window.is_some() {
      return;
    }
    let (width, height) = initial_window_size(
      self.config.presentation,
      (self.config.width, self.config.height),
      self.config.window_size,
    );

    // Hidden until the first frame is presented, to avoid a white flash
    let window_attrs = WindowAttributes::default()
      .with_title(&self.config.title)
      .with_inner_size(winit::dpi::LogicalSize::new(width, height))
      .with_resizable(self.config.resizable)
      .with_decorations(self.config.display_mode != DisplayMode::BorderlessFullscreen)
      .with_visible(false);
    // On the web the window is a canvas appended to the page, sized by the
    // page's CSS rather than an inline size
    #[cfg(target_arch = "wasm32")]
    let window_attrs = {
      use winit::platform::web::WindowAttributesExtWebSys;
      let mut window_attrs = window_attrs.with_append(true).with_prevent_default(true);
      window_attrs.inner_size = None;
      window_attrs
    };

    let window = match event_loop.create_window(window_attrs) {
      Ok(window) => Arc::new(window),
      Err(e) => {
        crate::engine_error!(module: "Window", "Failed to create window: {}", e);
        event_loop.exit();
        return;
      }
    };

    match self.config.display_mode {
      DisplayMode::Windowed => {}
      DisplayMode::ExclusiveFullscreen => {
        if let Some(monitor) = window.current_monitor()
          && let Some(video_mode) = monitor.video_modes().next()
        {
          window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
        }
      }
      DisplayMode::BorderlessFullscreen => {
        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
          window.current_monitor(),
        )));
      }
    }

    window.set_ime_allowed(true);
    if let Some(icon) = Self::load_window_icon(self.config.icon_path.as_deref()) {
      window.set_window_icon(Some(icon));
    }

    let settings = self.renderer_settings();
    #[cfg(not(target_arch = "wasm32"))]
    match pollster::block_on(WgpuRenderer::new(Arc::clone(&window), settings)) {
      Ok(renderer) => self.finish_gpu_setup(event_loop, window, renderer),
      Err(error) => {
        crate::engine_error!(module: "Render", "{}", error);
        self.startup_error = Some(error);
        event_loop.exit();
      }
    }
    // The web sets the GPU up asynchronously; `user_event` finishes it
    #[cfg(target_arch = "wasm32")]
    {
      let proxy = self.proxy.clone();
      let gpu_window = Arc::clone(&window);
      wasm_bindgen_futures::spawn_local(async move {
        let result = WgpuRenderer::new(gpu_window, settings).await;
        if let Some(proxy) = proxy {
          let _ = proxy.send_event(EngineEvent::GpuReady(Box::new(result)));
        }
      });
      self.window = Some(window);
    }
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: EngineEvent) {
    match event {
      EngineEvent::GpuReady(result) => match *result {
        Ok(renderer) => {
          if let Some(window) = self.window.clone() {
            let size = window.inner_size();
            self.finish_gpu_setup(event_loop, window, renderer);
            // The page sized the canvas while the GPU was on its way
            if size.width > 0 && size.height > 0 {
              self.resize_window(size.width, size.height);
            }
          }
        }
        Err(error) => {
          crate::engine_error!(module: "Render", "{}", error);
          self.startup_error = Some(error);
          event_loop.exit();
        }
      },
    }
  }

  #[allow(unused_variables)] // window_id used only when profiler feature is enabled
  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    window_id: WindowId,
    event: WindowEvent,
  ) {
    #[cfg(feature = "profiler")]
    if let Some(profiler) = &mut self.profiler_window
      && window_id == profiler.window_id()
    {
      match &event {
        WindowEvent::CloseRequested => self.profiler_window = None,
        WindowEvent::RedrawRequested => profiler.render(),
        WindowEvent::Focused(focused) => self.profiler_window_focused = *focused,
        _ => {
          profiler.handle_event(&event);
        }
      }
      return;
    }

    if let Some(logger) = &mut self.logger_window
      && window_id == logger.window_id()
    {
      match &event {
        WindowEvent::CloseRequested => self.logger_window = None,
        WindowEvent::RedrawRequested => logger.render(),
        WindowEvent::Focused(focused) => self.logger_window_focused = *focused,
        _ => {
          logger.handle_event(&event);
        }
      }
      return;
    }

    match &event {
      WindowEvent::KeyboardInput { event, .. } => {
        let mut input = self.world.resource_mut::<Input>();
        if let PhysicalKey::Code(key) = event.physical_key {
          input.queue(InputEvent::Key {
            key,
            pressed: event.state == winit::event::ElementState::Pressed,
          });
        }
        // Typed text (not control characters, not Ctrl/Alt shortcuts)
        if event.state == winit::event::ElementState::Pressed
          && let Some(text) = &event.text
        {
          let ctrl_held =
            input.key_down(KeyCode::ControlLeft) || input.key_down(KeyCode::ControlRight);
          let alt_held = input.key_down(KeyCode::AltLeft) || input.key_down(KeyCode::AltRight);
          if !ctrl_held && !alt_held && !text.chars().all(|c| c.is_control()) {
            input.queue(InputEvent::Text(text.to_string()));
          }
        }
      }
      WindowEvent::MouseInput { state, button, .. } => {
        self
          .world
          .resource_mut::<Input>()
          .queue(InputEvent::MouseButton {
            button: *button,
            pressed: *state == winit::event::ElementState::Pressed,
          });
      }
      WindowEvent::CursorMoved { position, .. } => {
        // While locked, motion comes from DeviceEvent::MouseMotion instead
        if !self.cursor_locked {
          self.cursor_moved(position.x, position.y);
        }
      }
      WindowEvent::MouseWheel { delta, .. } => {
        self
          .world
          .resource_mut::<Input>()
          .queue(InputEvent::Scroll(*delta));
      }
      WindowEvent::Ime(ime_event) => {
        let event = match ime_event {
          Ime::Enabled => InputEvent::ImeEnabled,
          Ime::Disabled => InputEvent::ImeDisabled,
          Ime::Preedit(text, cursor) => InputEvent::ImePreedit {
            text: text.clone(),
            cursor: *cursor,
          },
          Ime::Commit(text) => InputEvent::ImeCommit(text.clone()),
        };
        self.world.resource_mut::<Input>().queue(event);
      }
      WindowEvent::Resized(new_size) => {
        self.resize_window(new_size.width, new_size.height);
        if let Some(window) = &self.window {
          window.request_redraw();
        }
      }
      WindowEvent::Focused(focused) => {
        self.game_window_focused = *focused;
        // Release the grab on Alt-Tab and restore it on return
        if let Some((grab, visible)) = cursor_grab_for_focus(self.cursor_locked, *focused)
          && let Some(window) = &self.window
        {
          let _ = window.set_cursor_grab(grab);
          window.set_cursor_visible(visible);
        }
      }
      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        // A resize event follows; screen space changes with the factor now
        self.scale_factor = *scale_factor;
        self.world.resource_mut::<WindowInfo>().scale_factor = *scale_factor;
        // The new mapping brings a new UI space
        let (width, height) = self.last_window_size;
        self.resize_window(width, height);
      }
      _ => {}
    }

    match event {
      WindowEvent::CloseRequested => {
        engine_info!(module: "Engine", "Close requested, exiting...");
        event_loop.exit();
      }
      // Nothing to draw until the GPU is ready (the web sets it up late)
      WindowEvent::RedrawRequested if self.renderer.is_some() => {
        #[cfg(feature = "profiler")]
        if let Some(profiler) = &mut self.profiler_window {
          profiler.track_frame_time();
        }
        crate::core::profiler::new_frame();

        if self.surface_needs_reconfigure {
          self.surface_needs_reconfigure = false;
          if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
            let size = window.inner_size();
            if size.width > 0 && size.height > 0 {
              renderer.resize(size.width, size.height);
            }
          }
        }

        let frame_start = Instant::now();
        if matches!(
          self.frame(None),
          FrameOutcome::Quit | FrameOutcome::Fatal(_)
        ) {
          event_loop.exit();
          return;
        }

        // Dev windows draw from here while the game has focus (unfocused
        // windows get no redraw events on Windows)
        #[cfg(feature = "profiler")]
        if self.game_window_focused
          && let Some(profiler) = &mut self.profiler_window
        {
          profiler.render();
        }
        if self.game_window_focused
          && let Some(logger) = &mut self.logger_window
        {
          logger.render();
        }

        self.after_frame(frame_start);
        let presentation_blocks = self
          .renderer
          .as_ref()
          .is_some_and(WgpuRenderer::presentation_blocks);
        if should_pace_frames(presentation_blocks) {
          self.pace_frame();
        }
      }
      _ => {}
    }
  }

  fn device_event(
    &mut self,
    _event_loop: &ActiveEventLoop,
    _device_id: winit::event::DeviceId,
    event: winit::event::DeviceEvent,
  ) {
    // While the cursor is locked, raw mouse motion is the mouse delta
    if self.cursor_locked
      && let winit::event::DeviceEvent::MouseMotion { delta } = event
    {
      self
        .world
        .resource_mut::<Input>()
        .queue(InputEvent::RawMouseMotion {
          dx: delta.0,
          dy: delta.1,
        });
    }
  }

  fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
    // Last event on every exit path. On macOS, Cmd+Q exits the process right
    // after this without returning from `run_app`, so captures are flushed here.
    let reports = self.capture.finish();
    self.report_captures(reports);
    #[cfg(not(target_arch = "wasm32"))]
    if self.capture.exit_with_failure() {
      std::process::exit(1);
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    // The browser wakes the loop for each animation frame it is asked for
    event_loop.set_control_flow(if cfg!(target_arch = "wasm32") {
      winit::event_loop::ControlFlow::Wait
    } else {
      winit::event_loop::ControlFlow::Poll
    });

    // Report finished captures and honour RYO_CAPTURE_EXIT
    let reports = self.capture.poll_writes();
    self.report_captures(reports);
    if self.capture.exit_requested() {
      event_loop.exit();
      return;
    }

    if let Some(window) = self.window.clone() {
      let request = std::mem::take(&mut *self.world.resource_mut::<WindowRequest>());
      if request.has_changes() {
        self.apply_window_request(&window, request);
      }
      let request = std::mem::take(&mut *self.world.resource_mut::<CursorRequest>());
      if request.has_changes() {
        self.apply_cursor_request(&window, event_loop, request);
      }
    }

    // Dev window requests (F9/F10) are used up whether or not a window can
    // open, so they are not retried every frame
    #[cfg(feature = "profiler")]
    if std::mem::take(&mut self.pending_open_profiler)
      && self.profiler_window.is_none()
      && let Some(renderer) = &self.renderer
      && let (Some(instance), Some(adapter)) = (renderer.instance(), renderer.adapter())
    {
      // Collect scopes even when enable_profiler wasn't set
      puffin::set_scopes_on(true);
      self.profiler_window = crate::core::windows::profiler_window::ProfilerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        instance,
        adapter,
      );
    }

    if std::mem::take(&mut self.pending_open_logger)
      && !cfg!(target_arch = "wasm32")
      && self.logger_window.is_none()
      && let Some(renderer) = &self.renderer
      && let (Some(instance), Some(adapter)) = (renderer.instance(), renderer.adapter())
    {
      self.logger_window = crate::core::windows::logger_window::LoggerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        instance,
        adapter,
      );
    }

    // F5: toggle borderless
    if std::mem::take(&mut self.pending_toggle_borderless)
      && let Some(window) = self.window.clone()
    {
      match self.config.display_mode {
        DisplayMode::Windowed => {
          // Clear maximized first: on Windows a maximized window's client area
          // is held to the work area, which keeps fullscreen off the taskbar
          window.set_maximized(false);
          window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
          self.set_display_mode(DisplayMode::BorderlessFullscreen);
          engine_info!(module: "Window", "Switched to borderless fullscreen");
        }
        DisplayMode::BorderlessFullscreen | DisplayMode::ExclusiveFullscreen => {
          window.set_fullscreen(None);
          window.set_decorations(true);
          self.set_display_mode(DisplayMode::Windowed);
          engine_info!(module: "Window", "Switched to windowed mode");
        }
      }
      self.surface_needs_reconfigure = true;
    }

    // An unfocused game window gets no redraw events (Windows): run its frame
    // here, throttled unless a dev window has focus. The browser paces the
    // web's frames, focused or not.
    if !self.game_window_focused && !cfg!(target_arch = "wasm32") {
      #[cfg(feature = "profiler")]
      let throttle = !self.profiler_window_focused && !self.logger_window_focused;
      #[cfg(not(feature = "profiler"))]
      let throttle = !self.logger_window_focused;
      self.unfocused_tick(event_loop, throttle);
    } else if let Some(window) = &self.window {
      window.request_redraw();
    }

    #[cfg(feature = "profiler")]
    if self.profiler_window_focused
      && let Some(profiler) = &self.profiler_window
    {
      profiler.request_redraw();
    }
    if self.logger_window_focused
      && let Some(logger) = &self.logger_window
    {
      logger.request_redraw();
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use winit::window::CursorGrabMode;

  #[test]
  fn cursor_grab_for_focus_no_action_when_not_locked() {
    assert_eq!(cursor_grab_for_focus(false, true), None);
    assert_eq!(cursor_grab_for_focus(false, false), None);
  }

  #[test]
  fn cursor_grab_for_focus_releases_on_focus_lost() {
    assert_eq!(
      cursor_grab_for_focus(true, false),
      Some((CursorGrabMode::None, true))
    );
  }

  #[test]
  fn cursor_grab_for_focus_restores_on_focus_gained() {
    assert_eq!(
      cursor_grab_for_focus(true, true),
      Some((CursorGrabMode::Confined, false))
    );
  }
}
