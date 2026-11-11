//! One frame: clocks, input, the UI pass, the fixed steps, then render.

use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use super::{Engine, FrameOutcome, PresentOutcome, QuitRequested, SceneHost};
use crate::core::asset::AssetsView;
use crate::core::audio::AudioEngine;
use crate::core::config::DisplayMode;
#[cfg(dev_tools)]
use crate::core::console::Console;
use crate::core::diagnostics as overlays;
use crate::core::diagnostics::FrameCost;
use crate::core::diagnostics::OverlayMode;
use crate::core::ecs::kernel::{CheckChangeTicks, DetectChanges};
use crate::core::ecs::{On, Res, ResMut, Resource, World};
use crate::core::input::{Binding, Input, InputEvent};
use crate::core::messages::update_messages;
use crate::core::scene::SceneAction;
use crate::core::schedule::Phase;
use crate::core::settings::Settings;
use crate::core::settings::apply::diff;
use crate::core::settings::persist::SettingsWriter;
use crate::core::shaders::EffectsManager;
use crate::core::text::TextIcons;
use crate::core::time::Time;
use crate::core::ui::interact::{
  PointerInput, PointerOutcome, apply_scroll_requests, resolve_pointer, route_wheel,
};
use crate::core::ui::memory::UiSound;
use crate::core::ui::nav::{NavInput, resolve_focus};
use crate::core::ui::theme::{Theme, load_skin, refresh_skins, use_followed};
use crate::core::ui::widgets::rebind::note_swap;
use crate::core::ui::{ThemeSound, ThemeSoundSource, UiContext, UiInput, UiTheme};
use crate::core::window::WindowRequest;
use crate::profile_scope;

/// The UI's direction actions: Up, Down, Left, Right
const NAV_DIRECTIONS: [&str; 4] = ["ui.up", "ui.down", "ui.left", "ui.right"];

/// What the frame's UI pass did besides drawing, kept for
/// `HeadlessEngine::ui_snapshot`: the input the last pass consumed, the
/// presses its rebinding rows captured, and the UI actions navigation used
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct UiPassLog {
  pub(crate) consumed: UiInput,
  pub(crate) rebinds: Vec<crate::core::ui::immediate::PendingRebind>,
  pub(crate) used: Vec<&'static str>,
}

/// The real delta the loop chose for this frame: `Some` for headless runs
/// and tests, `None` for the wall clock.
#[derive(Resource)]
pub(crate) struct PendingDelta(pub(crate) Option<Duration>);

impl Engine {
  /// One frame. `real_delta` of `None` means wall clock.
  ///
  /// It runs in the engine's log scope, and its records carry its number:
  /// the game clock's frame count once the frame has advanced it.
  pub(crate) fn frame(&mut self, real_delta: Option<Duration>) -> FrameOutcome {
    let number = self
      .world
      .get_resource::<Time>()
      .map_or(0, Time::frame_count)
      + 1;
    crate::core::logging::set_frame(number);
    crate::core::logging::in_scope(self.log_scope, || self.frame_in_scope(real_delta))
  }

  fn frame_in_scope(&mut self, real_delta: Option<Duration>) -> FrameOutcome {
    if let Some(message) = self.renderer.as_ref().and_then(|r| r.take_fault()) {
      log::error!("{}", message);
      return FrameOutcome::Fatal(message);
    }
    let frame_start = Instant::now();
    self.frame_timings = Default::default();
    self.world.insert_resource(PendingDelta(real_delta));

    // Scripted presses and gamepad events are queued before First publishes input
    let next_frame = self.capture.presented_frames() + 1;
    self
      .input_script
      .before_frame(next_frame, &mut self.world.resource_mut::<Input>());
    self.poll_gamepads();

    self.schedules.run(Phase::First, &mut self.world);
    self.handle_debug_keys();
    self.schedules.run(Phase::PreUpdate, &mut self.world);
    #[cfg(dev_tools)]
    self.console_tick();

    let update_start = Instant::now();
    {
      profile_scope!("cpu:update");
      self.ui_pass_index = 0;
      self.ui_pass();
      let mut step = 0;
      // A quit (from the UI pass or a step) ends the steps
      while !self.world.contains_resource::<QuitRequested>()
        && self.world.resource_mut::<Time>().try_expend_fixed_step()
      {
        self.world.resource_mut::<Input>().set_fixed_step(step);
        self.world.resource_mut::<Time>().set_in_fixed(true);
        self.schedules.run(Phase::FixedUpdate, &mut self.world);
        self.world.resource_mut::<Time>().set_in_fixed(false);
        step += 1;
      }
      // A press nobody stepped over waits for the next frame's first step
      // (between steps at high refresh rates, or through a hitstop), but not
      // through a pause: gameplay never saw it, so it is dropped rather than
      // replayed on resume
      let paused = self.world.resource::<Time>().game.is_paused();
      self
        .world
        .resource_mut::<Input>()
        .end_fixed_steps(step > 0 || paused);
      self.apply_settings();
      self.save_settings(Instant::now());
    }
    self.frame_timings.update = update_start.elapsed();

    if self.world.remove_resource::<QuitRequested>().is_some() {
      return FrameOutcome::Quit;
    }

    self.schedules.run(Phase::Update, &mut self.world);
    self.schedules.run(Phase::PostUpdate, &mut self.world);

    let outcome = if self.skip_present {
      FrameOutcome::Skipped
    } else {
      match self.present_frame() {
        PresentOutcome::Fatal(error) => return FrameOutcome::Fatal(error),
        PresentOutcome::Presented => FrameOutcome::Presented,
        PresentOutcome::Skipped => FrameOutcome::Skipped,
      }
    };

    self.schedules.run(Phase::Last, &mut self.world);
    self.clamp_schedule_ticks();
    self
      .input_script
      .after_frame(&mut self.world.resource_mut::<Input>());
    self.publish_frame_cost(frame_start);
    outcome
  }

  /// The frame's work, without its wait for the window's next image, into
  /// the smoothed timings (the F1 overlay's) and the world's [`FrameCost`]
  fn publish_frame_cost(&mut self, frame_start: Instant) {
    self.frame_timings.total = frame_start
      .elapsed()
      .saturating_sub(self.frame_timings.wait);
    self.smoothed_timings.update(&self.frame_timings);
    self
      .world
      .insert_resource(FrameCost::of(&self.smoothed_timings));
  }

  /// Applies what changed in `Settings` since the last check: the volumes
  /// at once, the display mode and window size (only while windowed) as
  /// window requests and vsync for the renderer, both after the frame, and
  /// the FPS counter; and tells the settings writer. A frame with no change
  /// costs one change check
  pub(crate) fn apply_settings(&mut self) {
    let Some(ticks) = self.world.get_resource_change_ticks::<Settings>() else {
      return;
    };
    let now = self.world.change_tick();
    let changed = self.applied_settings.check(ticks.changed, now);
    // A change made after this check is newer than `now`
    self.world.increment_change_tick();
    if !changed {
      return;
    }
    let seen = Instant::now();
    self.with_settings_writer(|writer, settings, _| writer.observe(seen, settings));
    let Some(settings) = self.world.get_resource::<Settings>() else {
      return;
    };
    let now = *settings.values();
    let changes = diff(&self.applied_settings.values, &now);
    let applied = &mut self.applied_settings.values;

    if !changes.volumes.is_empty()
      && let Some(mut audio) = self.world.get_resource_mut::<AudioEngine>()
    {
      for &(kind, volume) in &changes.volumes {
        audio.set_volume_setting(kind, volume);
      }
    }
    applied.volumes = now.volumes;

    let mut request = self.world.resource_mut::<WindowRequest>();
    if let Some(mode) = changes.display_mode {
      request.display_mode = Some(mode);
      applied.display_mode = mode;
    }
    // A fullscreen window keeps the size for when it is windowed again
    if let Some((width, height)) = changes.window_size
      && now.display_mode == DisplayMode::Windowed
    {
      let size = winit::dpi::LogicalSize::new(width, height).to_physical::<u32>(self.scale_factor);
      request.width = Some(size.width);
      request.height = Some(size.height);
      applied.window_size = (width, height);
    }

    if let Some(vsync) = changes.vsync {
      self.pending_vsync = Some(vsync);
      applied.vsync = vsync;
    }

    if let Some(show) = changes.show_fps {
      applied.show_fps = show;
      // Wall-clock overlays would make headless frames differ run to run
      if !self.headless {
        let debug = &mut self.config.debug;
        debug.show_fps = show;
        self.overlay_mode =
          OverlayMode::from_flags(show, debug.show_frame_timing, debug.show_stats);
      }
    }
  }

  /// Takes the player's changed bindings into the settings writer, and
  /// writes the settings file once the last change to what it keeps is a
  /// second old. An idle frame costs a lookup and two checks
  pub(crate) fn save_settings(&mut self, now: Instant) {
    let Some(due) = self
      .world
      .get_resource::<SettingsWriter>()
      .map(|writer| writer.is_due(now))
    else {
      return;
    };
    let bindings = self.changed_bindings();
    if !due && bindings.is_none() {
      return;
    }
    self.with_settings_writer(|writer, settings, saves| {
      if let Some(map) = bindings {
        writer.set_bindings(map);
        writer.observe(now, settings);
      }
      writer.tick(now, settings, saves);
    });
  }

  /// Queue gamepad connections, buttons, triggers and sticks from gilrs.
  fn poll_gamepads(&mut self) {
    let Some(gilrs) = self.gilrs.as_mut() else {
      return;
    };
    let mut input = self.world.resource_mut::<Input>();
    if !input.gamepad_connected()
      && let Some((id, gamepad)) = gilrs.gamepads().find(|(_, g)| g.is_connected())
    {
      input.queue(InputEvent::GamepadConnected {
        id,
        name: gamepad.name().to_string(),
      });
    }
    while let Some(gilrs::Event { id, event, .. }) = gilrs.next_event() {
      let event = match event {
        gilrs::EventType::Connected => {
          gilrs
            .connected_gamepad(id)
            .map(|gamepad| InputEvent::GamepadConnected {
              id,
              name: gamepad.name().to_string(),
            })
        }
        gilrs::EventType::Disconnected => Some(InputEvent::GamepadDisconnected(id)),
        gilrs::EventType::ButtonPressed(button, _) => Some(InputEvent::GamepadButton {
          id,
          button,
          pressed: true,
        }),
        gilrs::EventType::ButtonReleased(button, _) => Some(InputEvent::GamepadButton {
          id,
          button,
          pressed: false,
        }),
        // Analog buttons like triggers
        gilrs::EventType::ButtonChanged(button, value, _) => {
          Some(InputEvent::GamepadTrigger { id, button, value })
        }
        gilrs::EventType::AxisChanged(axis, value, _) => {
          Some(InputEvent::GamepadAxis { id, axis, value })
        }
        _ => None,
      };
      if let Some(event) = event {
        input.queue(event);
      }
    }
  }

  /// The engine's keys, each consumed so the game never sees it: F5
  /// (borderless) in every build, under `disable_debug_keys`; with the
  /// developer tools also the console key, F1-F4 (overlays), F9 and F10 (dev
  /// windows) and F12 (capture). Without them every other key is the game's.
  fn handle_debug_keys(&mut self) {
    #[cfg(dev_tools)]
    self.handle_tool_keys();
    if self.debug_keys_enabled
      && self
        .world
        .resource_mut::<Input>()
        .key_pressed_and_consume(KeyCode::F5)
    {
      self.pending_toggle_borderless = true;
    }
  }

  /// The developer tools' keys: the console key, F1-F4, F9, F10 and F12
  #[cfg(dev_tools)]
  fn handle_tool_keys(&mut self) {
    // The console key opens the console, and closes it while its line is
    // empty; with text in a focused console's line it types instead. The
    // press that toggled it, held, neither types nor toggles again, its
    // auto-repeats included, whatever character the layout gives it
    let console = self.world.get_resource::<Console>().map(|console| {
      (
        console.visible && console.focused && console.has_input(),
        console.key_held(),
      )
    });
    let mut input = self.world.resource_mut::<Input>();
    let mut toggle_console = false;
    let mut console_key_held = None;
    if self.config.debug.dev_console
      && let Some((typing, held)) = console
    {
      let key = KeyCode::Backquote;
      let released = input.key_released(key);
      if held {
        if input.key_repeat(key) {
          input.consume_key(key);
          input.consume_text();
        }
        if released {
          console_key_held = Some(false);
        }
      } else if !typing && input.key_pressed(key) {
        input.consume_key(key);
        input.consume_text();
        toggle_console = true;
        // Pressed and let go within the frame, it is not held
        console_key_held = Some(!released);
      }
    }
    let mut pressed = |key: KeyCode| input.key_pressed_and_consume(key);

    let mut next_overlay = false;
    if self.debug_keys_enabled {
      next_overlay = pressed(KeyCode::F1);
      if pressed(KeyCode::F2) {
        self.config.debug.show_colliders = !self.config.debug.show_colliders;
      }
      if pressed(KeyCode::F3) {
        self.config.debug.show_velocities = !self.config.debug.show_velocities;
      }
      if pressed(KeyCode::F4) {
        self.config.debug.show_origins = !self.config.debug.show_origins;
      }
    }

    if self.config.debug.dev_window_keys {
      // The dev windows are native only
      #[cfg(not(target_arch = "wasm32"))]
      if pressed(KeyCode::F9) {
        if self.logger_window.take().is_some() {
          self.logger_window_focused = false;
        } else {
          // Opened in the event loop, which has the ActiveEventLoop
          self.pending_open_logger = true;
        }
      }
      #[cfg(all(feature = "profiler", not(target_arch = "wasm32")))]
      if pressed(KeyCode::F10) {
        if self.profiler_window.take().is_some() {
          self.profiler_window_focused = false;
        } else {
          self.pending_open_profiler = true;
        }
      }
      if pressed(KeyCode::F12) {
        self
          .capture
          .request(crate::core::capture::CaptureTarget::Default);
      }
    }
    // A closed console is touched only when its key does something
    if (toggle_console || console_key_held.is_some())
      && let Some(mut console) = self.world.get_resource_mut::<Console>()
    {
      if toggle_console {
        console.toggle();
      }
      if let Some(held) = console_key_held {
        console.set_key_held(held);
      }
    }
    if next_overlay {
      self.next_overlay();
    }
  }

  /// F1: the next overlay mode. The FPS counter is the player's setting, so
  /// a change to it goes into `Settings` and the applied values together, as
  /// F5's display mode does, and is not applied again
  #[cfg(dev_tools)]
  pub(super) fn next_overlay(&mut self) {
    self.overlay_mode = self.overlay_mode.next();
    let (show_fps, show_frame_timing, show_stats) = self.overlay_mode.to_flags();
    let debug = &mut self.config.debug;
    debug.show_fps = show_fps;
    debug.show_frame_timing = show_frame_timing;
    debug.show_stats = show_stats;
    if let Some(mut settings) = self.world.get_resource_mut::<Settings>()
      && settings.show_fps() != show_fps
    {
      settings.set_show_fps(show_fps);
    }
    self.applied_settings.values.show_fps = show_fps;
  }

  /// The developer console: animation, input while open, and the line
  /// typed, run as a command.
  #[cfg(dev_tools)]
  fn console_tick(&mut self) {
    // A closed console does nothing a frame: it is neither touched nor
    // taken out of the world (its clock and scroll ease wait for it)
    if !self
      .world
      .get_resource::<Console>()
      .is_some_and(|console| console.visible)
    {
      self.console_cursor = None;
      return;
    }
    let real_dt = self.world.resource::<Time>().real.delta();
    let to_screen = self.console_pixel_scale();
    let to_ui = self.ui_space.mouse_scale;
    let log_scope = self.log_scope;
    let submitted =
      self
        .world
        .resource_scope(|world, mut console: crate::core::ecs::Mut<Console>| {
          // The log's records since it last looked (all of them since it
          // closed, when it has just opened)
          console.mirror_log(log_scope);
          console.update(real_dt);
          // A trackpad moves the console as far as the pointer would, and the
          // pointer is hit-tested in UI units, both converted as the UI pass
          // converts the pointer
          console.set_pixel_scale(to_screen);
          console.set_mouse_scale(to_ui);
          let mut input = world.resource_mut::<Input>();
          let line = console.handle_input(&mut input);
          // A focused console keeps the keyboard from the game; an unfocused
          // one can stay open while playing
          if console.is_focused() {
            input.clear_keyboard_state();
          }
          if console.take_tab()
            && let Some(commands) = world.get_resource::<crate::core::console::ConsoleCommands>()
          {
            console.complete(|prefix| commands.completions(prefix));
          }
          line
        });
    if let Some(line) = submitted {
      self.run_console_line(&line);
    }

    // The console closed this frame ends its override here too
    let (mouse_x, mouse_y) = self.world.resource::<Input>().mouse_position();
    self.console_cursor = self
      .world
      .get_resource::<Console>()
      .and_then(|console| console.desired_cursor(mouse_x * to_ui, mouse_y * to_ui));
  }

  /// Run `line` as a console command, with the engine's own state for the
  /// built-ins (the debug flags, the overlay mode, capture and quit)
  #[cfg(dev_tools)]
  pub(crate) fn run_console_line(&mut self, line: &str) {
    let mut capture = None;
    let mut quit = false;
    let mut hooks = crate::core::console::EngineHooks {
      debug: &mut self.config.debug,
      overlay: &mut self.overlay_mode,
      capture: &mut capture,
      quit: &mut quit,
      game_name: self.game_name.as_deref(),
      game_version: self.game_version.as_deref(),
    };
    crate::core::console::run_line(&mut self.world, &mut hooks, line);
    if quit {
      self.world.insert_resource(QuitRequested);
    }
    if let Some(target) = capture {
      self.capture.request(target);
    }
  }

  /// Run `line` as if typed into the console (echoed, then run), and give
  /// the lines it printed after the echo
  #[cfg(all(dev_tools, not(target_arch = "wasm32")))]
  pub(crate) fn console_run(&mut self, line: &str) -> Vec<String> {
    let Some(mut console) = self.world.get_resource_mut::<Console>() else {
      return Vec::new();
    };
    console.print_command(format!("> {line}"));
    console.capture();
    self.run_console_line(line);
    self
      .world
      .get_resource_mut::<Console>()
      .map(|mut console| console.take_captured())
      .unwrap_or_default()
  }

  /// The UI pass: the scenes' `ui` (visible scenes, bottom-up), then the
  /// debug overlays and the console, before any fixed step. What it consumes
  /// is hidden from gameplay for the frame, and the scene action it returns
  /// applies at once. When that action brings a new scene to the top, the UI
  /// is drawn again so the frame shows the new scene's UI, not none; once
  /// only, so a scene that switches on every call cannot loop.
  ///
  /// The UI memory and layout keep one frame per engine frame: they begin
  /// with the first pass and end after the last.
  fn ui_pass(&mut self) {
    for _ in 0..2 {
      let Some(action) = self.draw_ui() else {
        break;
      };
      if matches!(action, SceneAction::Continue) {
        break;
      }
      let (keep_running, top_changed) = with_host(&mut self.world, |world, host| {
        let scenes = &mut host.scenes;
        let before = scenes.top_scope();
        let keep_running = scenes.handle_action(action, world);
        scenes.sync_pauses(world);
        (keep_running, scenes.top_scope() != before)
      });
      if !keep_running {
        self.world.insert_resource(QuitRequested);
        break;
      }
      if !top_changed {
        break;
      }
    }
    if self.ui_pass_index > 0 {
      self.ui_memory.end_frame();
      self.ui_layout.end_frame();
      // Node records not drawn this frame go, as widget state does
      crate::core::ui::nodes::end_frame(&mut self.world, &self.ui_memory);
    }
  }

  /// A trackpad's logical pixels in the console's units
  #[cfg(dev_tools)]
  fn console_pixel_scale(&self) -> f32 {
    self.ui_space.mouse_scale * self.logical_to_screen()
  }

  /// Screen units per logical window pixel (1 without a renderer, where
  /// the pointer is not mapped either)
  fn logical_to_screen(&self) -> f32 {
    self.renderer.as_ref().map_or(1.0, |renderer| {
      let scale = renderer.mapping().scale();
      if scale.is_finite() && scale > 0.0 {
        self.scale_factor as f32 / scale
      } else {
        1.0
      }
    })
  }

  /// Resolve the frame's pointer (`input`, in UI units) against last
  /// frame's records of the top scene, the console blocking it while shown
  fn resolve_pointer(&mut self, input: &UiInput, theme: &Theme) -> PointerOutcome {
    let buttons = [MouseButton::Left, MouseButton::Right];
    let (x, y) = input.mouse_position();
    let pointer = PointerInput {
      position: Vec2::new(x, y),
      pressed: buttons.map(|b| input.mouse_pressed(b)),
      released: buttons.map(|b| input.mouse_released(b)),
      down: buttons.map(|b| input.mouse_down(b)),
      time: f64::from(self.world.resource::<Time>().real.elapsed()),
      double_click: f64::from(theme.metrics.double_click),
    };
    let top = with_host(&mut self.world, |_, host| host.scenes.top_scope());
    let console = self.console_rect();
    let outcome = resolve_pointer(
      &mut self.ui_memory,
      &pointer,
      top,
      console.as_slice(),
      theme.metrics.drag_threshold,
    );
    // Then the wheel: the scroll areas under the pointer take it, never
    // under the console. The wheel over the UI is consumed whether or not
    // an area moved (`outcome.consume_wheel`), as a press over it is.
    apply_scroll_requests(&mut self.ui_memory);
    let (wheel_x, wheel_y) = input.scroll_wheel_lines();
    // A trackpad moves content as far as the pointer would move: logical
    // pixels to screen units to UI units, the pointer's scales with no offset
    let (pixel_x, pixel_y) = input.scroll_pixels();
    let to_ui = self.ui_space.mouse_scale * self.logical_to_screen();
    let pixels = Vec2::new(pixel_x, pixel_y) * to_ui;
    let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
    let blocked = console.iter().any(|c| c.contains(x, y));
    let taken = !blocked
      && route_wheel(
        &mut self.ui_memory,
        pointer.position,
        Vec2::new(wheel_x, wheel_y),
        pixels,
        shift,
        top,
        theme.metrics.scroll_line,
      );
    PointerOutcome {
      consume_wheel: outcome.consume_wheel || taken,
      ..outcome
    }
  }

  /// Where the open console is, which takes the pointer from the UI
  #[cfg(dev_tools)]
  fn console_rect(&self) -> Option<crate::core::ui::Rect> {
    self
      .world
      .get_resource::<Console>()
      .filter(|console| console.visible)
      .map(Console::window_rect)
  }

  /// No console: a build without tools
  #[cfg(not(dev_tools))]
  fn console_rect(&self) -> Option<crate::core::ui::Rect> {
    None
  }

  /// The console's cursor over the game's this frame (`None`: no opinion)
  #[cfg(dev_tools)]
  pub(super) fn console_cursor(&self) -> Option<crate::core::cursor::CursorStyle> {
    self.console_cursor
  }

  /// No console: a build without tools
  #[cfg(not(dev_tools))]
  pub(super) fn console_cursor(&self) -> Option<crate::core::cursor::CursorStyle> {
    None
  }

  /// Resolve the keyboard and gamepad focus from the UI actions, after the
  /// pointer, and hide the actions it used from gameplay
  fn resolve_navigation(&mut self, theme: &Theme) {
    let top = with_host(&mut self.world, |_, host| host.scenes.top_scope());
    let dt = self.world.resource::<Time>().real.delta();
    let input = self.world.resource::<Input>();
    let nav = NavInput {
      pressed: NAV_DIRECTIONS.map(|action| input.action_pressed(action)),
      held: NAV_DIRECTIONS.map(|action| input.action_held(action)),
      confirm: input.action_pressed("ui.confirm"),
      back: input.action_pressed("ui.back"),
      next: input.action_pressed("ui.next"),
      shift: input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight),
      tab_next: input.action_pressed("ui.tab_next"),
      tab_prev: input.action_pressed("ui.tab_prev"),
      right_stick: Vec2::from(input.right_stick()),
      device: input.last_device(),
      dt,
      repeat_delay: theme.metrics.repeat_delay,
      repeat_rate: theme.metrics.repeat_rate,
    };
    let pointer = &self.ui_memory.interact;
    let moved = pointer.pointer != pointer.prev_pointer;
    let outcome = resolve_focus(&mut self.ui_memory, &nav, top, moved);
    take_actions(
      &mut self.world.resource_mut::<Input>(),
      &outcome.used,
      &mut self.ui_binding_capture,
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
      self.ui_pass_log.used = outcome.used;
    }
  }

  /// The theme for this UI pass: a followed theme asset that changed is
  /// used, and the images of new skins are loaded
  fn refresh_ui_theme(&mut self) -> Arc<Theme> {
    let loaded = &mut self.theme_generation;
    self
      .world
      .resource_scope(|world, mut theme: crate::core::ecs::Mut<UiTheme>| {
        use_followed(&mut theme, world);
        let mut assets = AssetsView::new(world);
        refresh_skins(&mut theme, loaded, |source| load_skin(&mut assets, source));
        theme.shared()
      })
  }

  /// What the theme's UI sounds play, found again when its generation
  /// changes (so a starting theme's on the first pass): what a theme file's
  /// loader gave, or the file a theme made in code names, loaded through
  /// the registry, which warns once about one that does not load (that
  /// sound stays silent)
  fn load_ui_sounds(&mut self, theme: &Theme) {
    let generation = self.world.resource::<UiTheme>().generation();
    if self.sounds_generation == Some(generation) {
      return;
    }
    self.sounds_generation = Some(generation);
    let assets = AssetsView::new(&mut self.world);
    for (slot, event) in self.ui_sounds.iter_mut().zip(UiSound::ALL) {
      let sound = theme.sounds.get(event);
      *slot = match (&sound.sound, sound.path.as_deref()) {
        (Some(source), _) => Some(source.clone()),
        (None, Some(path)) if ThemeSound::is_definition(path) => {
          Some(ThemeSoundSource::Sound(assets.load(path)))
        }
        (None, Some(path)) => Some(ThemeSoundSource::Clip(assets.load(path))),
        (None, None) => None,
      };
    }
  }

  /// Play the theme's sound for each event this frame's UI raised, through
  /// the audio engine on the `ui` bus at the theme's volume. One still
  /// loading is refused (logged), with no warning
  fn play_ui_sounds(&mut self, theme: &Theme) {
    let raised = &self.ui_memory.interact.sounds;
    let from = self.sounds_played.min(raised.len());
    let fresh = &raised[from..];
    self.sounds_played = raised.len();
    let Some(volume_db) = theme.sounds.gain() else {
      return;
    };
    if fresh.is_empty() {
      return;
    }
    let sounds = &self.ui_sounds;
    self
      .world
      .try_resource_scope(|world, mut engine: crate::core::ecs::Mut<AudioEngine>| {
        for &event in fresh {
          let source = UiSound::ALL
            .iter()
            .zip(sounds)
            .find_map(|(kind, source)| (*kind == event).then_some(source.as_ref()))
            .flatten();
          if let Some(source) = source {
            engine.play_ui(world, source.playable(), volume_db);
          }
        }
      });
  }

  /// Bring what prompts read up to date: the last device and pad family,
  /// every action's bindings (copied again only when they changed) and the
  /// game's glyphs (when `TextIcons` changed)
  fn refresh_prompts(&mut self) {
    let input = self.world.resource::<Input>();
    let prompts = Arc::make_mut(&mut self.prompts);
    prompts.sync_input(input.last_device(), input.pad_family(), input.actions());
    match self.world.get_resource_ref::<TextIcons>() {
      Some(icons) if self.prompt_icons != Some(icons.last_changed()) => {
        prompts.icons = TextIcons::clone(&icons);
        prompts.generation += 1;
        self.prompt_icons = Some(icons.last_changed());
      }
      Some(_) => {}
      None => {
        if self.prompt_icons.is_some() {
          prompts.generation += 1;
        }
        prompts.icons = TextIcons::default();
        self.prompt_icons = None;
      }
    }
  }

  /// Record one UI pass into the frame's UI commands (replacing any earlier
  /// pass's) and apply what it consumed; `None` before the GPU is up.
  fn draw_ui(&mut self) -> Option<SceneAction> {
    self.renderer.as_ref()?;
    let started = Instant::now();
    self.ui_commands.clear();
    // Fonts added since the pump (a scene's `on_enter`) are laid out with
    crate::core::text::fonts_follow_assets(&mut self.world);
    self
      .text
      .watch_loads(self.world.get_resource::<crate::core::asset::AssetServer>());
    if let Some(fonts) = self.world.get_resource::<crate::core::text::Fonts>() {
      self.text.sync(fonts);
    }
    // Scenes that exited leave nothing behind in the UI's memory
    for scope in with_host(&mut self.world, |_, host| host.scenes.take_exited()) {
      self.ui_memory.forget_scene(scope);
      crate::core::ui::nodes::despawn_scene_roots(&mut self.world, scope);
    }
    // Trees whose UI file changed are brought in line before they are drawn
    crate::core::ui::nodes::reload_trees(&mut self.world, &mut self.ui_sources);
    // The node trees replay their clean subtrees unless caching is off, and
    // copy those they kept unless fragments are off
    crate::core::ui::nodes::set_cache_enabled(&mut self.world, self.config.debug.ui_cache);
    crate::core::ui::nodes::set_fragments_enabled(&mut self.world, self.config.debug.ui_fragments);
    let theme = self.refresh_ui_theme();
    let theme_generation = self.world.resource::<UiTheme>().generation();
    self.load_ui_sounds(&theme);
    self.refresh_prompts();
    let space = self.ui_space;
    let mut snapshot =
      UiInput::snapshot(self.world.resource::<Input>()).with_mouse_scale(space.mouse_scale);
    // A rerun pass replaces the first pass's records; the pointer is
    // resolved once a frame, against last frame's records
    let first_pass = self.ui_pass_index == 0;
    if first_pass {
      self.sounds_played = 0;
      self.ui_memory.begin_frame();
      self.ui_layout.begin_frame();
      let outcome = self.resolve_pointer(&snapshot, &theme);
      for (button, consume) in [MouseButton::Left, MouseButton::Right]
        .into_iter()
        .zip(outcome.consume_press)
      {
        if consume {
          snapshot.consume_mouse(button);
        }
      }
      if outcome.consume_wheel {
        snapshot.consume_scroll();
      }
      self.resolve_navigation(&theme);
      // A node list's thumb drag is followed before its pool is chosen; its
      // `show` follows it again
      crate::core::ui::widgets::scroll::drag_ahead(
        &mut self.ui_memory,
        theme.metrics.sizes.scrollbar,
      );
      // List pools follow this frame's scrolling before any tree is drawn
      let frame = self.ui_memory.frame();
      crate::core::ui::nodes::update_lists(&mut self.world, &mut self.ui_memory, frame, space.size);
    } else {
      self.ui_memory.restart_pass();
    }
    self.ui_pass_index += 1;
    // A press on the console is the console's, whatever the button:
    // neither the scenes' widgets nor gameplay see it, and it stays hidden
    // through its holds and release (`ui_mouse_capture`). A drag begun in
    // the game keeps its holds and release over the console
    #[cfg(dev_tools)]
    if let Some(console) = self.world.get_resource::<Console>()
      && console.visible
    {
      let (mouse_x, mouse_y) = snapshot.mouse_position();
      if console.window_rect().contains(mouse_x, mouse_y) {
        snapshot.consume_mouse_presses();
      }
    }
    let real_dt = self.world.resource::<Time>().real.delta();
    // Overlays and the console still lay out in whole UI units
    let height = space.size.y.ceil() as u32;
    // The UI sizes images as the store holds them now
    self.ui_images.refresh(&mut self.world);
    let (action, consumed, rebinds) = {
      let mut ui = UiContext::new(
        &mut self.ui_commands,
        &mut self.text,
        &mut self.ui_memory,
        &mut self.ui_layout,
        space,
        snapshot,
        Arc::clone(&theme),
        theme_generation,
        &self.ui_images,
      );
      ui.set_dt(real_dt);
      ui.set_prompts(Arc::clone(&self.prompts));
      let action = with_host(&mut self.world, |world, host| {
        host.scenes.ui_pass(world, &mut ui)
      });

      // Overlays draw over game UI, its open lists included: stats (top),
      // FPS, frame timing (bottom). They are the engine's, scene 0.
      ui.begin_scene(0, false);
      ui.set_layer(crate::core::ui::immediate::OVERLAY_LAYER);
      let debug = &self.config.debug;
      if debug.show_stats {
        overlays::render_stats(&mut ui, &self.smoothed_stats, height);
      }
      if debug.show_fps {
        overlays::render_fps_counter(
          &mut ui,
          self.smoothed_timings.fps as u64,
          height,
          debug.show_frame_timing,
        );
      }
      if debug.show_frame_timing {
        overlays::render_frame_timing(&mut ui, &self.smoothed_timings, height);
      }
      if debug.show_ui_debug {
        crate::core::ui::debug::draw_debug_overlay(&mut ui);
      }
      if debug.show_ui_rebuilds {
        let dt = if self.ui_pass_index == 1 {
          real_dt
        } else {
          0.0
        };
        let top = overlays::overlay_stack_top(
          &ui,
          (debug.show_stats, debug.show_fps, debug.show_frame_timing),
          height,
        );
        self
          .ui_rebuilds
          .draw(&mut ui, &self.world, dt, self.ui_retain.stats, top);
      }
      // The console over everything
      #[cfg(dev_tools)]
      if self
        .world
        .get_resource::<Console>()
        .is_some_and(|c| c.visible)
        && let Some(mut console) = self.world.get_resource_mut::<Console>()
      {
        console.render(&mut ui, space.size.x.ceil() as u32, height);
      }
      ui.end_scene();
      let rebinds = ui.take_rebinds();
      (action, ui.into_input(), rebinds)
    };
    self.ui_commands.end_pass();
    // A rerun pass adds to the frame's first
    if first_pass {
      self.ui_pass_time = started.elapsed();
    } else {
      self.ui_pass_time += started.elapsed();
    }

    let mut input = self.world.resource_mut::<Input>();
    consumed.apply_consumed(&mut input);
    // Back a scene took with `ui.back()`, and the tab keys a tabs widget took
    let interact = &self.ui_memory.interact;
    let capture = &mut self.ui_binding_capture;
    if interact.back_taken {
      take_actions(&mut input, &["ui.back"], capture);
    }
    if interact.tab_taken {
      take_actions(&mut input, &["ui.tab_next", "ui.tab_prev"], capture);
    }
    // The presses rebinding rows captured are bound now (and saved, through
    // the bindings' dirty flag); each stays the UI's until it is released,
    // and a row whose binding came from another action says so
    let now = interact.now;
    for rebind in &rebinds {
      let swapped = input
        .actions_mut()
        .rebind(&rebind.action, rebind.slot, rebind.binding);
      input.consume_binding(&rebind.binding);
      if input.binding_down_raw(&rebind.binding) && !capture.contains(&rebind.binding) {
        capture.push(rebind.binding);
      }
      if let Some(other) = swapped {
        note_swap(&mut self.ui_memory, rebind, &other, now);
      }
    }
    // A binding of an action the UI used stays the UI's until it is
    // released, even once the UI that used it is gone (a Confirm held after
    // Resume closed the menu)
    for binding in capture.iter() {
      input.consume_binding(binding);
    }
    capture.retain(|binding| input.binding_down_raw(binding));
    // A press the UI took stays the UI's until the button is released: its
    // held frames and the release are hidden from gameplay as well
    for button in consumed.mouse_presses_taken() {
      if !self.ui_mouse_capture.contains(&button) {
        self.ui_mouse_capture.push(button);
      }
    }
    for &button in &self.ui_mouse_capture {
      input.consume_mouse(button);
    }
    self
      .ui_mouse_capture
      .retain(|&button| consumed.mouse_held_raw(button));
    self.play_ui_sounds(&theme);
    #[cfg(not(target_arch = "wasm32"))]
    {
      self.ui_pass_log.consumed = consumed;
      self.ui_pass_log.rebinds = rebinds;
    }
    Some(action)
  }

  /// Wait out the rest of the frame at `target_fps` (sleep, then spin for
  /// the last stretch, which sleep cannot hit exactly).
  pub(super) fn pace_frame(&self) {
    // The browser paces frames, and the web cannot sleep
    if cfg!(target_arch = "wasm32") {
      return;
    }
    let Some(start) = self.world.resource::<Time>().frame_start() else {
      return;
    };
    let target = Duration::from_secs_f64(1.0 / f64::from(self.config.target_fps.max(1)));
    let elapsed = start.elapsed();
    if elapsed >= target {
      return;
    }
    let spin_margin = Duration::from_micros(1500);
    let remaining = target - elapsed;
    if remaining > spin_margin {
      std::thread::sleep(remaining - spin_margin);
    }
    while start.elapsed() < target {
      std::hint::spin_loop();
    }
  }
}

/// Hide every binding of the `actions` from the rest of the frame, and add
/// each one that is down to `capture`, which hides it until its release
fn take_actions(input: &mut Input, actions: &[&str], capture: &mut Vec<Binding>) {
  for action in actions {
    let bindings = input.actions().bindings(action).to_vec();
    for binding in &bindings {
      input.consume_binding(binding);
      if input.binding_down_raw(binding) && !capture.contains(binding) {
        capture.push(*binding);
      }
    }
  }
}

/// Frames are paced in software when presenting did not wait for the
/// display: vsync off, or no frame was presented (a hidden window, or a
/// surface with nothing to give), which would otherwise spin.
pub(super) fn should_pace_frames(presentation_blocks: bool, presented: bool) -> bool {
  !presentation_blocks || !presented
}

// =============================================================================
// Engine systems
// =============================================================================

/// `First`: advance the clocks by the frame's real time.
pub(crate) fn advance_clock_system(mut time: ResMut<Time>, pending: Option<Res<PendingDelta>>) {
  match pending.and_then(|pending| pending.0) {
    Some(delta) => time.advance(delta),
    None => time.begin_frame(),
  }
}

/// `First`: apply the input queued since the last frame.
pub(crate) fn publish_input_system(mut input: ResMut<Input>) {
  input.begin_frame();
}

/// Where the fixed clock stood when the message buffers last advanced.
#[derive(Resource, Default)]
pub(crate) struct MessagesAdvancedAt(u64);

/// `First`: advance every message buffer, but only once a fixed step has run
/// since the last advance. Buffers keep two advances' worth of messages, so
/// advancing on frames without a fixed step (any frame rate above twice the
/// fixed rate) would drop a message before a fixed-step reader saw it.
pub(crate) fn update_messages_system(world: &mut World) {
  let steps = world.resource::<Time>().fixed.step_count();
  let mut advanced_at = world.get_resource_or_insert_with(MessagesAdvancedAt::default);
  if advanced_at.0 == steps {
    return;
  }
  advanced_at.0 = steps;
  update_messages(world);
}

/// `PreUpdate`, after the scene transition: the asset registry's pump puts
/// finished loads and reloads in their stores, frees unheld assets and sends
/// their `AssetEvent`s; then the font registry follows the font stores.
pub(crate) fn asset_registry_system(world: &mut World) {
  crate::core::asset::pump(world);
  // Right after the pump: a freed font's face goes before any text is laid
  // out this frame, and a loaded one is there for it
  crate::core::text::fonts_follow_assets(world);
}

/// `PostUpdate`: timed post-process effects run on real time.
pub(crate) fn update_effects_system(time: Res<Time>, mut effects: ResMut<EffectsManager>) {
  effects.update(time.real.delta());
}

/// The last change-tick check the world ran, waiting for the engine to
/// clamp the schedules it owns: bevy clamps only the schedules in its
/// `Schedules` resource, and the engine's, the scenes' and physics' own are
/// not there. Every schedule run checks (`Schedule::run`), so the check is
/// observed rather than called.
#[derive(Resource, Default)]
pub(crate) struct PendingTickCheck(pub(crate) Option<CheckChangeTicks>);

/// Observer: remember a change-tick check for `Engine::clamp_schedule_ticks`.
pub(crate) fn record_tick_check(
  check: On<CheckChangeTicks>,
  mut pending: ResMut<PendingTickCheck>,
) {
  pending.0 = Some(*check);
}

impl Engine {
  /// Clamp the change ticks of the schedules the engine owns, and the tick
  /// the UI node cache keeps, after the world checked its own.
  fn clamp_schedule_ticks(&mut self) {
    let Some(check) = self
      .world
      .get_resource_mut::<PendingTickCheck>()
      .and_then(|mut pending| pending.0.take())
    else {
      return;
    };
    self.schedules.check_change_ticks(check);
    with_host(&mut self.world, |_, host| {
      host.scenes.check_change_ticks(check)
    });
    if let Some(mut physics) = self
      .world
      .get_resource_mut::<crate::core::physics::sync::PhysicsSchedule>()
    {
      physics.check_change_ticks(check);
    }
    crate::core::ui::nodes::check_cache_ticks(&mut self.world, check);
  }
}

/// `Last`: end the frame's change tracking.
pub(crate) fn clear_trackers_system(world: &mut World) {
  world.clear_trackers();
  if world.resource::<Time>().frame_count().is_multiple_of(1024) {
    world.check_change_ticks();
  }
}

/// Run `f` with the scene stack taken out of the world.
pub(crate) fn with_host<R>(
  world: &mut World,
  f: impl FnOnce(&mut World, &mut SceneHost) -> R,
) -> R {
  let mut host = world
    .remove_non_send::<SceneHost>()
    .expect("SceneHost is inserted at engine start and only taken by these systems");
  let out = f(world, &mut host);
  world.insert_non_send(host);
  out
}

/// `FixedSet::Scene`: transitions, `Scene::update`, then the action.
pub(crate) fn scene_update_system(world: &mut World) {
  with_host(world, |world, host| {
    let scenes = &mut host.scenes;
    let action = scenes.update(world);
    if !scenes.handle_action(action, world) {
      world.insert_resource(QuitRequested);
    }
    scenes.sync_pauses(world);
  });
}

/// `PreUpdate`: advance a scene transition once a frame on real time, so a
/// fade runs while the game clock is paused or slowed.
pub(crate) fn scene_transition_system(world: &mut World) {
  let dt = world.resource::<Time>().real.delta();
  with_host(world, |world, host| {
    let scenes = &mut host.scenes;
    if !scenes.is_transitioning() {
      return;
    }
    scenes.update_transition(dt, world);
    scenes.sync_pauses(world);
    if scenes.is_empty() {
      world.insert_resource(QuitRequested);
    }
  });
}

/// `FixedSet::Game`: the scenes' fixed-step systems.
pub(crate) fn scene_fixed_systems(world: &mut World) {
  with_host(world, |world, host| host.scenes.run_fixed_systems(world));
}

/// `UpdateSet::Game`: the scenes' per-frame systems.
pub(crate) fn scene_update_systems(world: &mut World) {
  with_host(world, |world, host| host.scenes.run_update_systems(world));
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Component, Interpolated, Query, ResMut, Resource, Transform, With};
  use crate::core::engine::EngineBuilder;
  use crate::core::graphics::WgpuRenderer;
  use crate::core::scene::{Scene, SceneAction, SceneContext};
  use crate::core::schedule::Phase;
  use std::time::Duration;

  #[derive(Resource, Default)]
  struct Steps(u32);

  #[derive(Component)]
  struct Mover;

  struct Fixture;
  impl Scene for Fixture {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      ctx
        .world
        .spawn((Transform::from_xy(0.0, 0.0), Mover, Interpolated));
      ctx
        .add_systems(
          Phase::FixedUpdate,
          |mut q: Query<&mut Transform, With<Mover>>, mut steps: ResMut<Steps>| {
            for mut t in &mut q {
              t.translation.x += 10.0;
            }
            steps.0 += 1;
          },
        )
        .unwrap();
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      SceneAction::Continue
    }
  }

  fn engine() -> Engine {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      panic!("GPU required for this test; skipped by the harness without one");
    };
    let mut engine = EngineBuilder::new()
      .resolution(64, 48)
      .fixed_hz(50)
      .insert_resource(Steps::default())
      .start_scene(Fixture)
      .build_headless();
    engine.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 48), 1.0);
    engine
  }

  #[test]
  fn a_body_spawned_on_a_floor_starts_one_collision_through_the_frame_loop() {
    use crate::core::ecs::DetectChangesMut;
    use crate::core::physics::CollisionStarted;
    use crate::core::physics::{Collider, RigidBody};

    #[derive(Resource, Default)]
    struct Starts(u32);
    struct Floor {
      spawned: bool,
    }
    impl Scene for Floor {
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if !self.spawned {
          self.spawned = true;
          ctx.world.spawn((
            Transform::from_xy(0.0, 100.0),
            RigidBody::kinematic(),
            Collider::rect(200.0, 20.0),
          ));
          ctx.world.spawn((
            Transform::from_xy(0.0, 86.0),
            RigidBody::dynamic(),
            Collider::rect(10.0, 10.0),
          ));
        }
        let seen = ctx.messages::<CollisionStarted>().len() as u32;
        ctx.world.resource_mut::<Starts>().0 += seen;
        SceneAction::Continue
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .fixed_hz(60)
      .insert_resource(Starts::default())
      // A game that writes the body every step (same value) must not rebuild it
      .add_systems(Phase::FixedUpdate, |mut bodies: Query<&mut RigidBody>| {
        for mut body in &mut bodies {
          body.set_changed();
        }
      })
      .start_scene(Floor { spawned: false })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    for _ in 0..30 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    assert_eq!(e.world().resource::<Starts>().0, 1);
  }

  #[test]
  fn a_fade_started_while_the_game_clock_is_paused_finishes() {
    use crate::core::scene::Transition;
    use crate::core::ui::UiContext;

    struct Arrived;
    impl Scene for Arrived {}
    struct PausesAndLeaves {
      left: bool,
    }
    impl Scene for PausesAndLeaves {
      fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        if self.left {
          return SceneAction::Continue;
        }
        self.left = true;
        // A pause menu's "quit to title": the game clock is stopped
        ctx.time.game.pause();
        SceneAction::SwitchWith(
          Box::new(Arrived),
          Transition::fade(Duration::from_millis(100)),
        )
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .start_scene(PausesAndLeaves { left: false })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    for _ in 0..30 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    let host = e.world().non_send::<SceneHost>();
    assert!(
      !host.scenes.is_transitioning(),
      "the fade ends on real time"
    );
    assert_eq!(host.scenes.scene_names(), ["Arrived"]);
  }

  #[test]
  fn the_asset_pump_runs_after_the_scene_transition_in_its_frame() {
    use crate::core::asset::{AssetId, AssetServer, Assets, Handle, MemorySource};
    use crate::core::graphics::Image;
    use crate::core::scene::Transition;
    use crate::core::ui::UiContext;

    /// What the entering scene's first UI pass saw: whether its own load had
    /// landed, and whether the image the scene it replaced held was still in
    /// the store
    #[derive(Resource, Default)]
    struct FirstPass(Option<(bool, bool)>);
    #[derive(Resource)]
    struct Dropped(AssetId<Image>);

    /// Holds an image it made, and leaves at once through a fade
    #[derive(Default)]
    struct Leaving {
      image: Option<Handle<Image>>,
      left: bool,
    }
    impl Scene for Leaving {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let image = ctx.assets().add(Image::solid_rect(2, 2, 9, 9, 9, 255));
        ctx.world.insert_resource(Dropped(image.id()));
        self.image = Some(image);
      }
      fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        if std::mem::replace(&mut self.left, true) {
          return SceneAction::Continue;
        }
        // The switch happens in the next frame's transition, in `PreUpdate`
        SceneAction::SwitchWith(
          Box::new(Entering::default()),
          Transition::fade(Duration::ZERO),
        )
      }
    }
    #[derive(Default)]
    struct Entering {
      image: Option<Handle<Image>>,
    }
    impl Scene for Entering {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        self.image = Some(ctx.assets().load::<Image>("dot.png"));
      }
      fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        let loaded = self
          .image
          .as_ref()
          .is_some_and(|image| ctx.assets().state(image).is_loaded());
        let dropped = ctx.world.resource::<Dropped>().0;
        let kept = ctx.world.resource::<Assets<Image>>().get(dropped).is_some();
        ctx
          .world
          .resource_mut::<FirstPass>()
          .0
          .get_or_insert((loaded, kept));
        SceneAction::Continue
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let files = MemorySource::new();
    let mut png = Vec::new();
    image::RgbaImage::new(2, 2)
      .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
      .unwrap();
    files.insert("dot.png", png);
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .asset_source(files)
      .insert_resource(FirstPass::default())
      .start_scene(Leaving::default())
      .build_headless();
    // No worker threads: the pump runs the loads itself, as on the web
    e.world().resource::<AssetServer>().limit_workers(0);
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    for _ in 0..4 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    assert_eq!(
      e.world().resource::<FirstPass>().0,
      Some((true, false)),
      "in the frame of the switch, the pump runs after the transition: the \
       entering scene's load lands and the leaving scene's image is freed \
       before its first UI pass"
    );
  }

  #[test]
  fn presses_made_while_the_game_clock_is_paused_do_not_reach_the_next_step() {
    use crate::core::input::{Input, InputEvent};
    use winit::keyboard::KeyCode;

    #[derive(Resource, Default)]
    struct Presses(u32);
    struct CountsSpace;
    impl Scene for CountsSpace {
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Space) {
          ctx.world.resource_mut::<Presses>().0 += 1;
        }
        SceneAction::Continue
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .fixed_hz(60)
      .insert_resource(Presses::default())
      .start_scene(CountsSpace)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    let frame = Duration::from_micros(16_667);
    e.frame(Some(frame));
    e.world_mut().resource_mut::<Time>().game.pause();
    for pressed in [true, false] {
      e.world_mut()
        .resource_mut::<Input>()
        .queue(InputEvent::Key {
          key: KeyCode::Space,
          pressed,
        });
      e.frame(Some(frame));
    }
    e.world_mut().resource_mut::<Time>().game.resume();
    e.frame(Some(frame));
    e.frame(Some(frame));
    assert_eq!(e.world().resource::<Presses>().0, 0);
  }

  /// What gameplay saw of the left button and the W key.
  #[derive(Resource, Default)]
  struct GameplaySaw {
    mouse: u32,
    w: u32,
  }

  /// Draws one widget in `ui` and records what gameplay sees in `update`.
  struct WidgetOverGameplay {
    widget: fn(&mut crate::core::ui::UiContext, &mut WidgetState),
    state: WidgetState,
  }

  #[derive(Default)]
  struct WidgetState {
    slider: f32,
    text: String,
  }

  impl Scene for WidgetOverGameplay {
    fn ui(&mut self, ui: &mut crate::core::ui::UiContext, _ctx: &mut SceneContext) -> SceneAction {
      (self.widget)(ui, &mut self.state);
      SceneAction::Continue
    }
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      use winit::event::MouseButton;
      let mouse = ctx.input.mouse_down(MouseButton::Left)
        || ctx.input.mouse_pressed(MouseButton::Left)
        || ctx.input.mouse_released(MouseButton::Left);
      let w = ctx.input.key_down(winit::keyboard::KeyCode::KeyW)
        || ctx.input.key_pressed(winit::keyboard::KeyCode::KeyW);
      let mut saw = ctx.world.resource_mut::<GameplaySaw>();
      saw.mouse += mouse as u32;
      saw.w += w as u32;
      SceneAction::Continue
    }
  }

  /// Run `script` (input events per frame) under `widget`; what gameplay saw.
  fn gameplay_under(
    widget: fn(&mut crate::core::ui::UiContext, &mut WidgetState),
    state: WidgetState,
    script: Vec<Vec<crate::core::input::InputEvent>>,
  ) -> Option<(u32, u32)> {
    let (device, queue) = crate::core::testing::shared_gpu().ok()?;
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .fixed_hz(60)
      .insert_resource(GameplaySaw::default())
      .start_scene(WidgetOverGameplay { widget, state })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    for events in script {
      for event in events {
        e.world_mut()
          .resource_mut::<crate::core::input::Input>()
          .queue(event);
      }
      e.frame(Some(Duration::from_micros(16_667)));
    }
    let saw = e.world().resource::<GameplaySaw>();
    Some((saw.mouse, saw.w))
  }

  fn press_hold_release_at(x: f64, y: f64) -> Vec<Vec<crate::core::input::InputEvent>> {
    use crate::core::input::InputEvent;
    use winit::event::MouseButton;
    vec![
      vec![InputEvent::MouseMove { x, y }],
      vec![InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
      }],
      vec![],
      vec![],
      vec![InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
      }],
      vec![],
    ]
  }

  #[test]
  fn a_press_a_button_takes_stays_the_ui_s_until_released() {
    let Some((mouse, _)) = gameplay_under(
      |ui, _| {
        ui.button("Go")
          .at(crate::core::ui::Rect::new(0.0, 0.0, 32.0, 16.0));
      },
      WidgetState::default(),
      press_hold_release_at(8.0, 8.0),
    ) else {
      return;
    };
    assert_eq!(mouse, 0, "gameplay saw the button's press, hold or release");
  }

  #[test]
  fn a_ui_button_clicks_on_release_and_the_press_never_reaches_gameplay() {
    use std::cell::Cell;
    thread_local! {
      static CLICKS: Cell<u32> = const { Cell::new(0) };
    }
    fn counted(ui: &mut crate::core::ui::UiContext, _: &mut WidgetState) {
      if ui
        .button("Go")
        .at(crate::core::ui::Rect::new(0.0, 0.0, 32.0, 16.0))
        .clicked()
      {
        CLICKS.with(|clicks| clicks.set(clicks.get() + 1));
      }
    }
    // Frames: move, press, held, held, release, after
    let script = press_hold_release_at(8.0, 8.0);
    CLICKS.with(|clicks| clicks.set(0));
    let Some((mouse, _)) = gameplay_under(counted, WidgetState::default(), script[..2].to_vec())
    else {
      return;
    };
    assert_eq!(mouse, 0, "gameplay saw the press");
    assert_eq!(CLICKS.with(Cell::get), 0, "after the press frame");
    CLICKS.with(|clicks| clicks.set(0));
    let Some((mouse, _)) = gameplay_under(counted, WidgetState::default(), script[..5].to_vec())
    else {
      return;
    };
    assert_eq!(mouse, 0, "gameplay saw the press, hold or release");
    assert_eq!(CLICKS.with(Cell::get), 1, "after the release frame");
  }

  #[test]
  fn a_slider_drag_is_the_ui_s() {
    let Some((mouse, _)) = gameplay_under(
      |ui, state| {
        ui.slider(&mut state.slider, 0.0..=1.0)
          .at(crate::core::ui::Rect::new(10.0, 20.0, 40.0, 8.0))
          .show();
      },
      WidgetState::default(),
      press_hold_release_at(30.0, 24.0),
    ) else {
      return;
    };
    assert_eq!(mouse, 0, "gameplay saw the slider drag");
  }

  #[test]
  fn typing_into_a_focused_field_is_the_ui_s() {
    use crate::core::input::InputEvent;
    use std::cell::RefCell;
    use winit::event::MouseButton;
    use winit::keyboard::KeyCode;
    thread_local! {
      static TYPED: RefCell<String> = const { RefCell::new(String::new()) };
    }
    fn field(ui: &mut crate::core::ui::UiContext, state: &mut WidgetState) {
      ui.text_input(&mut state.text)
        .at(crate::core::ui::Rect::new(0.0, 0.0, 60.0, 16.0))
        .show();
      TYPED.with(|typed| typed.replace(state.text.clone()));
    }
    let key = |pressed| InputEvent::Key {
      key: KeyCode::KeyW,
      pressed,
    };
    let button = |pressed| InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed,
    };
    // A click focuses the field, then W is typed into it, held and released
    let script = vec![
      vec![InputEvent::MouseMove { x: 8.0, y: 8.0 }],
      vec![button(true)],
      vec![button(false)],
      vec![key(true), InputEvent::Text("w".into())],
      vec![],
      vec![key(false)],
    ];
    let Some((_, w)) = gameplay_under(field, WidgetState::default(), script) else {
      return;
    };
    assert_eq!(w, 0, "gameplay saw W typed into the field");
    assert_eq!(TYPED.with(|typed| typed.borrow().clone()), "w");
  }

  #[test]
  fn an_overlay_s_button_takes_the_click_over_the_scene_below() {
    use crate::core::input::{Input, InputEvent};
    use crate::core::ui::{Rect, UiContext};
    use winit::event::MouseButton;

    #[derive(Resource, Default)]
    struct Clicks {
      below: u32,
      overlay: u32,
    }
    struct Below {
      pushed: bool,
    }
    impl Scene for Below {
      fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        if ui
          .button("Below")
          .at(Rect::new(0.0, 0.0, 32.0, 16.0))
          .clicked()
        {
          ctx.world.resource_mut::<Clicks>().below += 1;
        }
        if !self.pushed {
          self.pushed = true;
          return SceneAction::Push(Box::new(Overlay));
        }
        SceneAction::Continue
      }
    }
    struct Overlay;
    impl Scene for Overlay {
      fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        if ui
          .button("Over")
          .at(Rect::new(0.0, 0.0, 32.0, 16.0))
          .clicked()
        {
          ctx.world.resource_mut::<Clicks>().overlay += 1;
        }
        SceneAction::Continue
      }
      fn is_transparent(&self) -> bool {
        true
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .insert_resource(Clicks::default())
      .start_scene(Below { pushed: false })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    let frame = Duration::from_micros(16_667);
    e.frame(Some(frame));
    for event in [
      InputEvent::MouseMove { x: 8.0, y: 8.0 },
      InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
      },
    ] {
      e.world_mut().resource_mut::<Input>().queue(event);
    }
    e.frame(Some(frame));
    // A click lands on release
    e.world_mut()
      .resource_mut::<Input>()
      .queue(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
      });
    e.frame(Some(frame));
    let clicks = e.world().resource::<Clicks>();
    assert_eq!((clicks.below, clicks.overlay), (0, 1));
  }

  #[test]
  fn a_message_written_in_the_ui_pass_reaches_the_next_fixed_step() {
    use crate::core::ecs::{Message, MessageReader};
    use crate::core::ui::UiContext;
    #[derive(Message, Clone)]
    struct Clicked;
    #[derive(Resource, Default)]
    struct Heard(u32);
    struct WritesOnce {
      written: bool,
    }
    impl Scene for WritesOnce {
      fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        if !self.written {
          self.written = true;
          ctx.write_message(Clicked);
        }
        SceneAction::Continue
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .fixed_hz(60)
      .add_message::<Clicked>()
      .insert_resource(Heard::default())
      .add_systems(
        Phase::FixedUpdate,
        |mut clicks: MessageReader<Clicked>, mut heard: ResMut<Heard>| {
          heard.0 += clicks.read().count() as u32;
        },
      )
      .start_scene(WritesOnce { written: false })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    // Written in a frame with no fixed step, then frames without one
    for _ in 0..3 {
      e.frame(Some(Duration::from_micros(4_166)));
    }
    e.frame(Some(Duration::from_micros(16_667)));
    e.frame(Some(Duration::from_micros(16_667)));
    assert_eq!(e.world().resource::<Heard>().0, 1);
  }

  #[test]
  fn a_message_from_a_fixed_step_survives_frames_without_one() {
    use crate::core::ecs::{IntoScheduleConfigs, Message, MessageReader, MessageWriter};
    #[derive(Message, Clone)]
    struct Ping;
    #[derive(Resource, Default)]
    struct Heard(u32);
    struct Idle;
    impl Scene for Idle {}

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut written = false;
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .fixed_hz(60)
      .add_message::<Ping>()
      .insert_resource(Heard::default())
      .add_systems(
        Phase::FixedUpdate,
        (
          |mut pings: MessageReader<Ping>, mut heard: ResMut<Heard>| {
            heard.0 += pings.read().count() as u32;
          },
          move |mut pings: MessageWriter<Ping>| {
            if !written {
              written = true;
              pings.write(Ping);
            }
          },
        )
          .chain(),
      )
      .start_scene(Idle)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    // One fixed step writes the message after the reader ran
    e.frame(Some(Duration::from_micros(16_667)));
    // Two fast frames (240 fps against 60 Hz) run no fixed step
    e.frame(Some(Duration::from_micros(4_166)));
    e.frame(Some(Duration::from_micros(4_166)));
    assert_eq!(e.world().resource::<Heard>().0, 0);
    // The next fixed step's reader still gets it
    e.frame(Some(Duration::from_micros(16_667)));
    assert_eq!(e.world().resource::<Heard>().0, 1);
  }

  #[test]
  fn two_fixed_steps_in_one_frame_snapshot_between_them() {
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.frame(Some(Duration::from_millis(40)));
    assert_eq!(e.world().resource::<Steps>().0, 2);
    // The mover's (the main camera is interpolated too)
    let mut query = e.world_mut().query_filtered::<(
      &crate::core::ecs::GlobalTransform,
      &crate::core::ecs::PreviousGlobalTransform,
    ), With<Mover>>();
    let (global, prev) = query.iter(e.world()).next().map(|(g, p)| (*g, *p)).unwrap();
    assert!((global.translation().x - 20.0).abs() < 1e-4);
    assert!(
      (prev.global.translation().x - 10.0).abs() < 1e-4,
      "previous is the state after step one"
    );
  }

  #[test]
  fn a_frame_with_no_step_keeps_the_previous_snapshot() {
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.frame(Some(Duration::from_millis(20))); // exactly one step
    e.frame(Some(Duration::from_millis(5))); // none
    assert_eq!(e.world().resource::<Steps>().0, 1);
    let alpha = e
      .world()
      .resource::<crate::core::time::Time>()
      .fixed
      .overstep_fraction();
    assert!((alpha - 0.25).abs() < 1e-3, "{alpha}");
  }

  #[test]
  fn input_is_seen_once_per_frame_by_fixed_logic() {
    // Covered end to end by the headless input tests; here: the loop sets the fixed step index
    if crate::core::testing::shared_gpu().is_err() {
      return;
    }
    let mut e = engine();
    e.world_mut()
      .resource_mut::<crate::core::input::Input>()
      .queue(crate::core::input::InputEvent::Key {
        key: winit::keyboard::KeyCode::Space,
        pressed: true,
      });
    e.frame(Some(Duration::from_millis(40)));
    // after the frame the one-shot is cleared by the next begin_frame, so check the step index reset
    assert_eq!(
      e.world()
        .resource::<crate::core::input::Input>()
        .fixed_step(),
      0
    );
  }

  #[test]
  fn a_despawned_body_leaves_physics_even_when_no_fixed_step_runs_for_frames() {
    use crate::core::physics::{Collider, Physics, RigidBody};
    struct Idle;
    impl Scene for Idle {}
    let mut e = EngineBuilder::new()
      .resolution(32, 32)
      .fixed_hz(50)
      .start_scene(Idle)
      .build_headless();
    let body = e
      .world_mut()
      .spawn((
        Transform::IDENTITY,
        RigidBody::dynamic(),
        Collider::circle(4.0),
      ))
      .id();
    e.frame(Some(Duration::from_millis(20))); // one step registers it
    assert!(e.world().resource::<Physics>().body_pose(body).is_some());
    e.world_mut().despawn(body);
    // A high frame rate: frames with no fixed step, while removal events
    // only last two of them
    for _ in 0..3 {
      e.frame(Some(Duration::from_millis(1)));
    }
    e.frame(Some(Duration::from_millis(20)));
    let physics = e.world().resource::<Physics>();
    assert_eq!((physics.body_count(), physics.collider_count()), (0, 0));
  }

  #[test]
  fn the_ui_pass_runs_first_and_what_it_consumes_never_reaches_update() {
    use crate::core::ui::UiContext;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use winit::keyboard::KeyCode;

    struct UiFirst {
      ui_saw: Arc<AtomicBool>,
      update_saw: Arc<AtomicBool>,
    }
    impl Scene for UiFirst {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        if ui.input().key_pressed(KeyCode::Space) {
          self.ui_saw.store(true, Ordering::SeqCst);
          ui.consume_key(KeyCode::Space);
        }
        SceneAction::Continue
      }
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::Space) {
          self.update_saw.store(true, Ordering::SeqCst);
        }
        SceneAction::Continue
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let (ui_saw, update_saw) = (
      Arc::new(AtomicBool::new(false)),
      Arc::new(AtomicBool::new(false)),
    );
    let mut e = EngineBuilder::new()
      .resolution(64, 48)
      .fixed_hz(50)
      .start_scene(UiFirst {
        ui_saw: ui_saw.clone(),
        update_saw: update_saw.clone(),
      })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 48), 1.0);
    e.world_mut()
      .resource_mut::<crate::core::input::Input>()
      .queue(crate::core::input::InputEvent::Key {
        key: KeyCode::Space,
        pressed: true,
      });
    e.frame(Some(Duration::from_millis(20)));
    assert!(
      ui_saw.load(Ordering::SeqCst),
      "the scene's ui ran this frame"
    );
    assert!(
      !update_saw.load(Ordering::SeqCst),
      "the press the UI took never reached the fixed update"
    );
  }

  #[test]
  fn the_debug_switch_turns_the_node_cache_off_and_on() {
    use crate::core::ui::UiNode;
    struct Menu;
    impl Scene for Menu {
      fn on_enter(&mut self, ctx: &mut SceneContext) {
        let menu = UiNode::column()
          .child(UiNode::text("Title"))
          .child(UiNode::button("Play"));
        ctx.spawn_ui(menu);
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(64, 48)
      .ui_cache(false)
      .start_scene(Menu)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 48), 1.0);
    let replayed = |e: &mut Engine| {
      e.frame(Some(Duration::from_millis(16)));
      crate::core::ui::nodes::cache_stats(&e.world).map_or(0, |stats| stats.replayed)
    };
    for _ in 0..3 {
      assert_eq!(replayed(&mut e), 0, "off, every node is built");
    }
    e.config.debug.ui_cache = true;
    replayed(&mut e);
    assert!(replayed(&mut e) > 0, "on, the unchanged tree replays");
    e.config.debug.ui_cache = false;
    assert_eq!(replayed(&mut e), 0, "off again");
  }

  /// A menu of a title and a button, spawned on enter
  struct NodeMenu;

  impl Scene for NodeMenu {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      let menu = crate::core::ui::UiNode::column()
        .child(crate::core::ui::UiNode::text("Title"))
        .child(crate::core::ui::UiNode::button("Play"));
      ctx.spawn_ui(menu);
    }
  }

  fn node_menu_engine() -> Option<Engine> {
    let (device, queue) = crate::core::testing::shared_gpu().ok()?;
    let mut e = EngineBuilder::new()
      .resolution(64, 48)
      .start_scene(NodeMenu)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 48), 1.0);
    Some(e)
  }

  #[test]
  fn the_engine_clamps_the_tick_the_node_cache_keeps() {
    use crate::core::ecs::kernel::Tick;
    use crate::core::ui::nodes::UiNodes;
    let Some(mut e) = node_menu_engine() else {
      return;
    };
    for _ in 0..3 {
      e.frame(Some(Duration::from_millis(16)));
    }
    // Off, no pass moves the tick the cache keeps: it ages with the session
    e.config.debug.ui_cache = false;
    e.frame(Some(Duration::from_millis(16)));
    for _ in 0..=Tick::MAX.get() {
      e.world.increment_change_tick();
    }
    // The world's check (`clear_trackers_system` runs it every 1024 frames)
    // waits for the end of the next frame
    let check = e.world.check_change_ticks().expect("past bevy's threshold");
    e.frame(Some(Duration::from_millis(16)));
    let oldest = check.present_tick().get().wrapping_sub(Tick::MAX.get());
    assert_eq!(
      e.world.resource::<UiNodes>().cache.checked,
      Some(Tick::new(oldest)),
      "clamped to the oldest age bevy compares"
    );
    // On again: built once, then replayed
    e.config.debug.ui_cache = true;
    let stats = |e: &mut Engine| {
      e.frame(Some(Duration::from_millis(16)));
      crate::core::ui::nodes::cache_stats(&e.world).map(|stats| (stats.built, stats.replayed))
    };
    assert_eq!(stats(&mut e), Some((3, 0)));
    assert_eq!(stats(&mut e).map(|(built, _)| built), Some(0));
  }

  #[test]
  fn the_spot_check_runs_in_the_engine_in_debug_builds() {
    let Some(mut e) = node_menu_engine() else {
      return;
    };
    let mut checked = Vec::new();
    for _ in 0..8 {
      e.frame(Some(Duration::from_millis(16)));
      if let Some(stats) = crate::core::ui::nodes::cache_stats(&e.world) {
        checked.extend(stats.spot_checked);
        assert_eq!(stats.spot_differed, 0);
      }
    }
    assert_eq!(
      !checked.is_empty(),
      cfg!(debug_assertions),
      "on by default in debug builds only: {checked:?}"
    );
  }

  #[test]
  fn a_scene_switched_from_ui_draws_its_ui_the_same_frame() {
    use crate::core::ui::immediate::UiCommand;
    use crate::core::ui::{Color, Rect, UiContext};
    struct Leaves;
    impl Scene for Leaves {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        ui.draw_rect(Rect::new(8.0, 8.0, 4.0, 4.0), Color::rgb(0, 0, 255));
        SceneAction::Switch(Box::new(Arrives))
      }
    }
    struct Arrives;
    impl Scene for Arrives {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        ui.draw_rect(Rect::new(0.0, 0.0, 4.0, 4.0), Color::rgb(255, 0, 0));
        SceneAction::Continue
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(16, 16)
      .start_scene(Leaves)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 16, 16), 1.0);
    e.frame(Some(Duration::from_millis(16)));
    // The arriving scene's UI is in the switching frame, and only its own:
    // the first pass's commands are gone
    let drawn: Vec<(Rect, Color)> = e
      .ui_commands
      .commands()
      .map(|command| match command {
        UiCommand::Rect { rect, fill, .. } => (*rect, *fill),
        other => panic!("only the new scene's rect: {other:?}"),
      })
      .collect();
    assert_eq!(
      drawn,
      [(Rect::new(0.0, 0.0, 4.0, 4.0), Color::rgb(255, 0, 0))]
    );
  }

  #[cfg(dev_tools)]
  #[test]
  fn the_console_draws_above_the_top_scenes_open_lists() {
    use crate::core::ui::immediate::UiCommand;
    use crate::core::ui::{Rect, UiContext};
    /// A menu button with its popup list open
    struct OpenList;
    impl Scene for OpenList {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        let anchor = ui.button("Menu").at(Rect::new(4.0, 4.0, 60.0, 16.0)).show();
        ui.popup(anchor.id).show(|ui| {
          ui.button("a");
          ui.button("b");
        });
        SceneAction::Continue
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(OpenList)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
    e.world.resource_mut::<Console>().toggle();
    e.frame(Some(Duration::from_millis(16)));
    // Layers sort before submission: every console command must be on a
    // layer above the open list's
    let layers: Vec<u8> = e
      .ui_commands
      .commands()
      .map(UiCommand::draw_layer)
      .collect();
    let popup = layers.iter().copied().max().unwrap_or(0);
    let last_popup = layers
      .iter()
      .rposition(|&layer| layer == 1)
      .expect("the open list is on the popup layer");
    let console = &layers[last_popup + 1..];
    assert!(!console.is_empty(), "the console drew");
    assert!(
      console.iter().all(|&layer| layer > 1) && popup > 1,
      "console layers {console:?}"
    );
  }

  type Ids = std::sync::Arc<std::sync::Mutex<Vec<crate::core::ui::UiId>>>;

  /// Draws a panel, keeping its id; returns `action` from `ui` once, and
  /// `from_update` from `update`
  struct Panel {
    ids: Ids,
    action: Option<SceneAction>,
    from_update: fn() -> SceneAction,
  }

  impl Scene for Panel {
    fn ui(&mut self, ui: &mut crate::core::ui::UiContext, _ctx: &mut SceneContext) -> SceneAction {
      let id = ui.panel().width(20.0).height(20.0).show(|_| {}).id;
      self.ids.lock().unwrap().push(id);
      self.action.take().unwrap_or(SceneAction::Continue)
    }
    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      (self.from_update)()
    }
  }

  fn panel(ids: &Ids, action: Option<SceneAction>, from_update: fn() -> SceneAction) -> Panel {
    Panel {
      ids: ids.clone(),
      action,
      from_update,
    }
  }

  #[test]
  fn a_rerun_ui_pass_keeps_only_its_own_records() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let (below, above) = (Ids::default(), Ids::default());
    // The lower scene pushes an opaque one from `ui`: the second pass draws
    // the new scene alone, so the first pass's record of the lower panel goes
    let pushed = panel(&above, None, || SceneAction::Continue);
    let base = panel(&below, Some(SceneAction::Push(Box::new(pushed))), || {
      SceneAction::Continue
    });
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .start_scene(base)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    e.frame(Some(Duration::from_millis(16)));
    let below = below.lock().unwrap().clone();
    let above = above.lock().unwrap().clone();
    assert_eq!((below.len(), above.len()), (1, 1), "one pass each");
    assert!(
      e.ui_memory.current(below[0]).is_none(),
      "the first pass's record is discarded"
    );
    let record = e.ui_memory.current(above[0]).copied();
    assert_eq!(
      record.map(|r| r.rect),
      Some(crate::core::ui::Rect::new(0.0, 0.0, 20.0, 20.0))
    );
  }

  #[test]
  fn a_scene_that_exits_leaves_no_records_behind() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let ids = Ids::default();
    struct Empty;
    impl Scene for Empty {}
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .start_scene(Empty)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    with_host(&mut e.world, |world, host| {
      // Draws once, then pops itself in its first fixed step
      host
        .scenes
        .push(Box::new(panel(&ids, None, || SceneAction::Pop)), world);
    });
    e.frame(Some(Duration::from_millis(20)));
    let id = ids.lock().unwrap()[0];
    assert!(e.ui_memory.current(id).is_some(), "drawn before it popped");
    e.frame(Some(Duration::from_millis(20)));
    assert!(e.ui_memory.prev(id).is_none(), "forgotten once it exited");
  }

  #[test]
  fn a_menu_takes_the_arrow_it_uses_and_gameplay_keeps_it_without_one() {
    use crate::core::input::InputEvent;
    use crate::core::ui::UiContext;
    use winit::keyboard::KeyCode;

    #[derive(Resource, Default)]
    struct Downs(u32);
    struct Menu {
      scoped: bool,
    }
    impl Scene for Menu {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        let mut column = ui.column();
        if self.scoped {
          column = column.focus_scope();
        }
        column.show(|ui| {
          ui.button("A");
          ui.button("B");
        });
        SceneAction::Continue
      }
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        if ctx.input.key_pressed(KeyCode::ArrowDown) || ctx.input.key_down(KeyCode::ArrowDown) {
          ctx.world.resource_mut::<Downs>().0 += 1;
        }
        SceneAction::Continue
      }
    }

    for scoped in [true, false] {
      let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
        return;
      };
      let mut e = EngineBuilder::new()
        .resolution(64, 64)
        .fixed_hz(60)
        .insert_resource(Downs::default())
        .start_scene(Menu { scoped })
        .build_headless();
      e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
      let frame = Duration::from_micros(16_667);
      // Drawn, then its scope takes focus
      for _ in 0..3 {
        e.frame(Some(frame));
      }
      // Pressed, held for a few frames (no repeat yet), released
      for pressed in [Some(true), None, None, None, None, Some(false)] {
        if let Some(pressed) = pressed {
          e.world_mut()
            .resource_mut::<Input>()
            .queue(InputEvent::Key {
              key: KeyCode::ArrowDown,
              pressed,
            });
        }
        e.frame(Some(frame));
      }
      let downs = e.world().resource::<Downs>().0;
      assert_eq!(downs == 0, scoped, "scoped: {scoped}, gameplay saw {downs}");
    }
  }

  #[test]
  fn one_back_press_pops_one_scene() {
    use crate::core::input::InputEvent;
    use crate::core::ui::UiContext;
    use winit::keyboard::KeyCode;

    /// A menu that pops on Back and pushes the next of `rest` once
    struct Menu {
      name: &'static str,
      rest: &'static [&'static str],
    }
    impl Scene for Menu {
      fn name(&self) -> &str {
        self.name
      }
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        if ui.back() {
          return SceneAction::Pop;
        }
        SceneAction::Continue
      }
      fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        let Some((next, rest)) = self.rest.split_first() else {
          return SceneAction::Continue;
        };
        self.rest = &[];
        SceneAction::Push(Box::new(Menu { name: next, rest }))
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .fixed_hz(60)
      .start_scene(Menu {
        name: "A",
        rest: &["B", "C"],
      })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    let frame = Duration::from_micros(16_667);
    for _ in 0..5 {
      e.frame(Some(frame));
    }
    let names = |e: &Engine| {
      let host = e.world().non_send::<SceneHost>();
      host
        .scenes
        .scene_names()
        .iter()
        .map(|name| name.to_string())
        .collect::<Vec<_>>()
    };
    assert_eq!(names(&e), ["A", "B", "C"]);
    for pressed in [true, false] {
      e.world_mut()
        .resource_mut::<Input>()
        .queue(InputEvent::Key {
          key: KeyCode::Escape,
          pressed,
        });
      e.frame(Some(frame));
    }
    assert_eq!(names(&e), ["A", "B"], "C popped, and only C");
  }

  /// Frames gameplay saw its key held or pressed
  #[derive(Resource, Default)]
  struct KeySeen(u32);

  /// Gameplay that pushes `menu` on its first update, then counts the
  /// frames it sees `key`
  struct UnderMenu {
    key: winit::keyboard::KeyCode,
    menu: Option<Box<dyn Scene>>,
  }

  impl Scene for UnderMenu {
    fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      if let Some(menu) = self.menu.take() {
        return SceneAction::Push(menu);
      }
      if ctx.input.key_down(self.key) || ctx.input.key_pressed(self.key) {
        ctx.world.resource_mut::<KeySeen>().0 += 1;
      }
      SceneAction::Continue
    }
  }

  /// Run `menu` over gameplay watching `key`: press `key` once the menu has
  /// its focus and hold it six frames, release it, then press it again.
  /// What gameplay saw while it was held, the scenes left then, and what
  /// gameplay saw of the second press.
  fn key_held_through_a_menu(
    key: winit::keyboard::KeyCode,
    menu: Box<dyn Scene>,
  ) -> Option<(u32, usize, u32)> {
    use crate::core::input::InputEvent;
    let (device, queue) = crate::core::testing::shared_gpu().ok()?;
    let mut e = EngineBuilder::new()
      .resolution(64, 64)
      .fixed_hz(60)
      .insert_resource(KeySeen::default())
      .start_scene(UnderMenu {
        key,
        menu: Some(menu),
      })
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 64, 64), 1.0);
    let frame = Duration::from_micros(16_667);
    // Pushed, drawn, then its scope takes the focus
    for _ in 0..4 {
      e.frame(Some(frame));
    }
    let step = |e: &mut Engine, pressed: Option<bool>| {
      if let Some(pressed) = pressed {
        e.world_mut()
          .resource_mut::<Input>()
          .queue(InputEvent::Key { key, pressed });
      }
      e.frame(Some(frame));
    };
    step(&mut e, Some(true));
    for _ in 0..5 {
      step(&mut e, None);
    }
    step(&mut e, Some(false));
    let held = e.world().resource::<KeySeen>().0;
    let scenes = e.world().non_send::<SceneHost>().scenes.scene_names().len();
    step(&mut e, Some(true));
    step(&mut e, None);
    let again = e.world().resource::<KeySeen>().0 - held;
    Some((held, scenes, again))
  }

  #[test]
  fn a_confirm_held_after_a_button_closes_its_menu_never_reaches_gameplay() {
    use crate::core::ui::UiContext;
    use winit::keyboard::KeyCode;

    /// A pause menu whose Resume pops it
    struct Pause;
    impl Scene for Pause {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        let mut resume = false;
        ui.column().focus_scope().show(|ui| {
          resume = ui.button("Resume").clicked();
        });
        if resume {
          SceneAction::Pop
        } else {
          SceneAction::Continue
        }
      }
    }

    // Space is `ui.confirm`
    let Some((held, scenes, again)) = key_held_through_a_menu(KeyCode::Space, Box::new(Pause))
    else {
      return;
    };
    assert_eq!(scenes, 1, "Resume closed the menu");
    assert_eq!(held, 0, "gameplay saw the Confirm that closed the menu");
    assert!(again > 0, "a fresh press is gameplay's");
  }

  #[test]
  fn a_direction_held_as_a_menu_closes_stays_the_ui_s() {
    use crate::core::ui::UiContext;
    use winit::keyboard::KeyCode;

    /// A menu that closes once Down moves its focus to B
    struct Menu;
    impl Scene for Menu {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        let mut moved = false;
        ui.column().focus_scope().show(|ui| {
          ui.button("A").default_focus();
          moved = ui.button("B").show().gained_focus();
        });
        if moved {
          SceneAction::Pop
        } else {
          SceneAction::Continue
        }
      }
    }

    let Some((held, scenes, again)) = key_held_through_a_menu(KeyCode::ArrowDown, Box::new(Menu))
    else {
      return;
    };
    assert_eq!(scenes, 1, "the move closed the menu");
    assert_eq!(held, 0, "gameplay saw the Down the menu used");
    assert!(again > 0, "a fresh press is gameplay's");
  }

  #[test]
  fn a_scene_action_from_ui_applies_before_the_fixed_steps() {
    use crate::core::ui::UiContext;
    struct QuitFromUi;
    impl Scene for QuitFromUi {
      fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        SceneAction::Quit
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(32, 32)
      .insert_resource(Steps::default())
      .start_scene(QuitFromUi)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 32, 32), 1.0);
    assert_eq!(
      e.frame(Some(Duration::from_millis(100))),
      FrameOutcome::Quit
    );
    assert_eq!(
      e.world()
        .resource::<crate::core::time::Time>()
        .fixed
        .step_count(),
      0,
      "no fixed step runs after the UI quit"
    );
  }

  #[test]
  fn frames_are_paced_only_when_presenting_does_not_wait() {
    assert!(
      should_pace_frames(false, true),
      "vsync off: pace to target_fps"
    );
    assert!(
      !should_pace_frames(true, true),
      "vsync on: the display paces"
    );
    assert!(
      should_pace_frames(true, false),
      "nothing presented waited for the display: pace to target_fps"
    );
  }

  #[test]
  fn a_ui_sound_that_cannot_load_warns_once_and_the_frames_run() {
    use crate::core::ui::theme::{Theme, UiTheme};
    let log = crate::core::testing::capture_log();
    let mut e = engine();
    let mut theme = Theme::dark();
    theme.sounds.confirm = crate::core::ui::ThemeSound::from_path("sounds/missing-confirm.ogg");
    e.world_mut().resource_mut::<UiTheme>().set(theme);
    for _ in 0..3 {
      e.frame(Some(Duration::from_millis(16)));
    }
    let warnings = log.containing("missing-confirm.ogg");
    assert_eq!(warnings, 1, "a warning naming the sound, once");
  }

  /// A button to click (at 20, 20, 80 x 30), Back taken, and the theme
  /// file `ui/sounds.theme.ron` followed when there is one
  struct Themed;

  impl Scene for Themed {
    fn on_enter(&mut self, ctx: &mut SceneContext) {
      if let Ok(theme) = ctx
        .assets()
        .load_now::<crate::core::ui::Theme>("ui/sounds.theme.ron")
      {
        ctx.follow_ui_theme(theme);
      }
    }

    fn ui(&mut self, ui: &mut crate::core::ui::UiContext, _ctx: &mut SceneContext) -> SceneAction {
      ui.button("Ok")
        .at(crate::core::ui::Rect::new(20.0, 20.0, 80.0, 30.0));
      ui.back();
      SceneAction::Continue
    }
  }

  /// A headless engine reading `files`, on [`Themed`]
  fn themed(
    files: crate::core::asset::MemorySource,
  ) -> Option<crate::core::testing::HeadlessEngine> {
    crate::core::testing::HeadlessEngine::new_or_skip(
      EngineBuilder::new()
        .resolution(320, 240)
        .asset_source(files)
        .start_scene(Themed),
    )
  }

  /// Click [`Themed`]'s button
  fn click_themed(e: &mut crate::core::testing::HeadlessEngine) {
    use winit::event::MouseButton;
    e.mouse_move(40.0, 30.0);
    e.step_frames(1).unwrap();
    e.press_mouse(MouseButton::Left);
    e.step_frames(1).unwrap();
    e.release_mouse(MouseButton::Left);
    e.step_frames(1).unwrap();
  }

  /// The voices started so far: what each plays, its bus and its gain
  fn starts(e: &crate::core::testing::HeadlessEngine) -> Vec<(String, String, f32)> {
    e.audio_log()
      .into_iter()
      .filter(|entry| entry.kind == crate::core::audio::AudioLogKind::Start)
      .map(|entry| (entry.source, entry.bus, entry.gain_db))
      .collect()
  }

  /// Step `e` until `done`, at most 300 frames
  fn step_until(
    e: &mut crate::core::testing::HeadlessEngine,
    mut done: impl FnMut(&mut crate::core::testing::HeadlessEngine) -> bool,
  ) {
    for _ in 0..300 {
      if done(e) {
        return;
      }
      e.step().unwrap();
    }
    panic!("not done after 300 frames");
  }

  /// A short click, as a WAV file of `frames` frames
  fn click_wav(frames: usize) -> Vec<u8> {
    crate::core::testing::wav(22_050, frames, 4_000)
  }

  #[test]
  fn a_theme_ui_sound_plays_on_the_ui_bus() {
    let files = crate::core::asset::MemorySource::new();
    files.insert("ui/click.wav", click_wav(2_205));
    // A definition naming no bus, which alone would play on `sfx`
    files.insert("ui/back.sound.ron", r#"(clips: [(path: "ui/click.wav")])"#);
    files.insert(
      "ui/sounds.theme.ron",
      r#"(sounds: (
        confirm: (path: "ui/click.wav"),
        back: (path: "ui/back.sound.ron"),
        volume_db: -6.0,
      ))"#,
    );
    let Some(mut e) = themed(files) else {
      return;
    };
    e.step_frames(2).unwrap();
    // A click on the button confirms, and Escape goes back
    click_themed(&mut e);
    e.tap_key(KeyCode::Escape);
    e.step_frames(2).unwrap();
    let starts = starts(&e);
    assert_eq!(starts.len(), 2, "the confirm and the back: {starts:?}");
    for (source, bus, gain_db) in &starts {
      assert_eq!(
        (source.as_str(), bus.as_str()),
        ("ui/click.wav", "ui"),
        "{starts:?}"
      );
      // The theme's volume, in dB
      assert_eq!(*gain_db, -6.0, "{starts:?}");
    }
  }

  #[test]
  fn a_code_made_theme_s_ui_sound_plays_on_the_ui_bus() {
    use crate::core::audio::AudioClip;
    use crate::core::ui::{Theme, ThemeSound, UiTheme};
    let files = crate::core::asset::MemorySource::new();
    files.insert("ui/click.wav", click_wav(2_205));
    let Some(mut e) = themed(files) else {
      return;
    };
    let mut theme = Theme::dark();
    theme.sounds.confirm = ThemeSound::from_path("ui/click.wav");
    e.world_mut().resource_mut::<UiTheme>().set(theme);
    // The engine loads it, off the main thread
    let clip = e.assets().load::<AudioClip>("ui/click.wav");
    step_until(&mut e, |e| e.assets().is_ready(&clip));
    click_themed(&mut e);
    assert_eq!(
      starts(&e),
      [("ui/click.wav".to_string(), "ui".to_string(), 0.0)]
    );
  }

  #[test]
  fn a_reloaded_theme_file_swaps_its_ui_sounds() {
    use crate::core::asset::AssetServer;
    use crate::core::ui::UiTheme;
    let theme = |confirm: &str| format!(r#"(sounds: (confirm: (path: "{confirm}")))"#);
    let files = crate::core::asset::MemorySource::new();
    files.insert("ui/click.wav", click_wav(2_205));
    files.insert("ui/clack.wav", click_wav(1_100));
    files.insert("ui/sounds.theme.ron", theme("ui/click.wav"));
    let Some(mut e) = themed(files.clone()) else {
      return;
    };
    e.step_frames(2).unwrap();
    click_themed(&mut e);
    files.insert("ui/sounds.theme.ron", theme("ui/clack.wav"));
    e.world()
      .resource::<AssetServer>()
      .reload("ui/sounds.theme.ron");
    step_until(&mut e, |e| {
      let ui = e.world().resource::<UiTheme>();
      ui.get().sounds.confirm.path.as_deref() == Some("ui/clack.wav")
    });
    e.step_frames(1).unwrap();
    click_themed(&mut e);
    let played: Vec<String> = starts(&e).into_iter().map(|(source, ..)| source).collect();
    assert_eq!(played, ["ui/click.wav", "ui/clack.wav"]);
  }

  #[cfg(dev_tools)]
  #[test]
  fn a_click_on_the_console_reaches_neither_widgets_nor_gameplay() {
    use crate::core::input::{Input, InputEvent};
    use crate::core::ui::{Rect, UiContext};
    use winit::event::MouseButton;

    #[derive(Resource, Default)]
    struct Seen {
      at: Rect,
      button: u32,
      gameplay: u32,
    }
    struct UnderConsole;
    impl Scene for UnderConsole {
      fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
        let at = ctx.world.resource::<Seen>().at;
        if ui.button("Under").at(at).clicked() {
          ctx.world.resource_mut::<Seen>().button += 1;
        }
        SceneAction::Continue
      }
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        let pressed = ctx.input.mouse_pressed(MouseButton::Left);
        ctx.world.resource_mut::<Seen>().gameplay += u32::from(pressed);
        SceneAction::Continue
      }
    }
    fn click(e: &mut Engine, x: f32, y: f32) {
      let frames = [
        vec![InputEvent::MouseMove {
          x: f64::from(x),
          y: f64::from(y),
        }],
        vec![InputEvent::MouseButton {
          button: MouseButton::Left,
          pressed: true,
        }],
        vec![InputEvent::MouseButton {
          button: MouseButton::Left,
          pressed: false,
        }],
        vec![],
      ];
      for events in frames {
        for event in events {
          e.world_mut().resource_mut::<Input>().queue(event);
        }
        e.frame(Some(Duration::from_micros(16_667)));
      }
    }

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .fixed_hz(60)
      .insert_resource(Seen::default())
      .start_scene(UnderConsole)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
    e.world.resource_mut::<Console>().toggle();
    // Let the console finish opening, then put the button under its window
    for _ in 0..60 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    let window = e.world.resource::<Console>().window_rect();
    let at = Rect::new(window.x + 20.0, window.y + 40.0, 60.0, 20.0);
    e.world_mut().resource_mut::<Seen>().at = at;
    e.frame(Some(Duration::from_micros(16_667)));
    click(&mut e, at.x + 10.0, at.y + 10.0);
    let seen = e.world().resource::<Seen>();
    assert_eq!(
      (seen.button, seen.gameplay),
      (0, 0),
      "the console took the click"
    );
    // With the console closed, the same click is the button's
    e.world.resource_mut::<Console>().toggle();
    for _ in 0..60 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    click(&mut e, at.x + 10.0, at.y + 10.0);
    assert_eq!(e.world().resource::<Seen>().button, 1);
  }

  #[cfg(dev_tools)]
  #[test]
  fn every_mouse_button_over_the_console_stays_out_of_the_game() {
    use crate::core::input::{Input, InputEvent};
    use winit::event::MouseButton;

    const BUTTONS: [MouseButton; 6] = [
      MouseButton::Left,
      MouseButton::Right,
      MouseButton::Middle,
      MouseButton::Back,
      MouseButton::Forward,
      MouseButton::Other(7),
    ];
    /// What gameplay saw of any button: presses, holds and releases
    #[derive(Resource, Default)]
    struct Seen(u32);
    struct Watching;
    impl Scene for Watching {
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        let input = &ctx.input;
        let seen = BUTTONS
          .iter()
          .filter(|&&b| input.mouse_pressed(b) || input.mouse_down(b) || input.mouse_released(b))
          .count() as u32;
        ctx.world.resource_mut::<Seen>().0 += seen;
        SceneAction::Continue
      }
    }
    fn frame_with(e: &mut Engine, events: Vec<InputEvent>) {
      for event in events {
        e.world_mut().resource_mut::<Input>().queue(event);
      }
      e.frame(Some(Duration::from_micros(16_667)));
    }
    let mouse = |x: f32, y: f32| InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    };
    let button = |button, pressed| InputEvent::MouseButton { button, pressed };

    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(800, 600)
      .fixed_hz(60)
      .insert_resource(Seen::default())
      .start_scene(Watching)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 800, 600), 1.0);
    e.world.resource_mut::<Console>().toggle();
    for _ in 0..10 {
      frame_with(&mut e, vec![]);
    }
    let window = e.world.resource::<Console>().window_rect();
    let (x, y) = (window.x + 100.0, window.y + 100.0);
    for b in BUTTONS {
      // Pressed over the console, held, dragged out and let go outside
      frame_with(&mut e, vec![mouse(x, y)]);
      frame_with(&mut e, vec![button(b, true)]);
      frame_with(&mut e, vec![]);
      frame_with(&mut e, vec![mouse(790.0, 590.0)]);
      frame_with(&mut e, vec![button(b, false)]);
      frame_with(&mut e, vec![]);
      assert_eq!(e.world().resource::<Seen>().0, 0, "{b:?} reached the game");
    }
    // With the console closed, a press there is the game's
    e.world.resource_mut::<Console>().toggle();
    frame_with(&mut e, vec![mouse(x, y)]);
    frame_with(&mut e, vec![button(MouseButton::Middle, true)]);
    frame_with(&mut e, vec![button(MouseButton::Middle, false)]);
    assert!(e.world().resource::<Seen>().0 > 0);
  }

  #[cfg(dev_tools)]
  /// A GPU engine at 800 x 600 with the console open, whose scene counts
  /// what gameplay sees of the left button; `None` without a GPU
  fn left_button_watcher() -> Option<Engine> {
    struct Watching;
    impl Scene for Watching {
      fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
        use winit::event::MouseButton;
        let left = MouseButton::Left;
        let (pressed, down, released) = (
          ctx.input.mouse_pressed(left),
          ctx.input.mouse_down(left),
          ctx.input.mouse_released(left),
        );
        let mut seen = ctx.world.resource_mut::<LeftSeen>();
        seen.pressed += u32::from(pressed);
        seen.down += u32::from(down);
        seen.released += u32::from(released);
        SceneAction::Continue
      }
    }
    let (device, queue) = crate::core::testing::shared_gpu().ok()?;
    let mut e = EngineBuilder::new()
      .resolution(800, 600)
      .fixed_hz(60)
      .insert_resource(LeftSeen::default())
      .start_scene(Watching)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 800, 600), 1.0);
    e.world.resource_mut::<Console>().toggle();
    for _ in 0..10 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    Some(e)
  }

  #[cfg(dev_tools)]
  #[derive(Resource, Default, Debug, PartialEq)]
  struct LeftSeen {
    pressed: u32,
    down: u32,
    released: u32,
  }

  #[cfg(dev_tools)]
  /// One frame each: move to `from`, press, move to `to`, release
  fn drag(e: &mut Engine, from: (f32, f32), to: (f32, f32)) {
    use crate::core::input::{Input, InputEvent};
    use winit::event::MouseButton;
    let mouse = |(x, y): (f32, f32)| InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    };
    let button = |pressed| InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed,
    };
    for event in [mouse(from), button(true), mouse(to), button(false)] {
      e.world_mut().resource_mut::<Input>().queue(event);
      e.frame(Some(Duration::from_micros(16_667)));
    }
    e.frame(Some(Duration::from_micros(16_667)));
  }

  #[cfg(dev_tools)]
  #[test]
  fn a_drag_begun_in_the_game_keeps_its_release_over_the_console() {
    let Some(mut e) = left_button_watcher() else {
      return;
    };
    let window = e.world.resource::<Console>().window_rect();
    drag(&mut e, (790.0, 590.0), (window.x + 100.0, window.y + 100.0));
    let seen = e.world().resource::<LeftSeen>();
    assert_eq!((seen.pressed, seen.released), (1, 1), "{seen:?}");
    assert!(seen.down >= 2, "held over the console too: {seen:?}");
  }

  #[cfg(dev_tools)]
  #[test]
  fn a_press_on_the_console_released_in_the_game_reaches_neither() {
    let Some(mut e) = left_button_watcher() else {
      return;
    };
    let window = e.world.resource::<Console>().window_rect();
    drag(&mut e, (window.x + 100.0, window.y + 100.0), (790.0, 590.0));
    assert_eq!(*e.world().resource::<LeftSeen>(), LeftSeen::default());
  }

  #[cfg(dev_tools)]
  #[test]
  fn hit_tests_match_the_drawing_under_a_ui_scale() {
    use crate::core::input::{Input, InputEvent};
    use winit::event::MouseButton;
    struct Idle;
    impl Scene for Idle {}
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    // Two render pixels a UI unit: a 700 x 500 UI over 1400 x 1000 screen units
    let mut e = EngineBuilder::new()
      .resolution(1400, 1000)
      .ui_scale(Some(2.0))
      .start_scene(Idle)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 1400, 1000), 1.0);
    assert_eq!(e.ui_space.mouse_scale, 0.5);
    e.world.resource_mut::<Console>().toggle();
    for _ in 0..10 {
      e.frame(Some(Duration::from_micros(16_667)));
    }
    // The close button as drawn, in UI units, and where the pointer is then
    let close = e.world.resource::<Console>().close_button_rect();
    let (x, y) = (
      (close.x + close.width / 2.0) / 0.5,
      (close.y + close.height / 2.0) / 0.5,
    );
    for events in [
      vec![InputEvent::MouseMove {
        x: f64::from(x),
        y: f64::from(y),
      }],
      vec![InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
      }],
      vec![InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
      }],
    ] {
      for event in events {
        e.world_mut().resource_mut::<Input>().queue(event);
      }
      e.frame(Some(Duration::from_micros(16_667)));
    }
    assert!(
      !e.world.resource::<Console>().visible,
      "the click on the close button drawn closed it"
    );
  }

  #[cfg(dev_tools)]
  #[test]
  fn the_console_overrides_the_cursor_only_while_it_is_the_one_under_it() {
    use crate::core::cursor::CursorStyle;
    struct Idle;
    impl Scene for Idle {}
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Idle)
      .build_headless();
    let frame = |e: &mut Engine, events: Vec<InputEvent>| {
      for event in events {
        e.world_mut().resource_mut::<Input>().queue(event);
      }
      e.frame(Some(Duration::from_micros(16_667)));
      e.console_cursor
    };
    let mouse = |x: f32, y: f32| InputEvent::MouseMove {
      x: f64::from(x),
      y: f64::from(y),
    };
    let button = |pressed| InputEvent::MouseButton {
      button: MouseButton::Left,
      pressed,
    };
    assert_eq!(frame(&mut e, vec![mouse(300.0, 200.0)]), None, "closed");
    e.world.resource_mut::<Console>().toggle();
    for _ in 0..60 {
      frame(&mut e, vec![]);
    }
    let rect = e.world.resource::<Console>().window_rect();
    let (hx, hy) = (rect.x + rect.width - 4.0, rect.y + rect.height - 4.0);
    assert_eq!(
      frame(&mut e, vec![mouse(hx, hy)]),
      Some(CursorStyle::ResizeNwSe)
    );
    frame(&mut e, vec![button(true)]);
    // Drag far past the minimum size, then let go with no move
    assert_eq!(
      frame(&mut e, vec![mouse(hx - 900.0, hy - 600.0)]),
      Some(CursorStyle::ResizeNwSe)
    );
    assert_eq!(
      frame(&mut e, vec![button(false)]),
      None,
      "the game's cursor is back the frame the drag ends"
    );
    let rect = e.world.resource::<Console>().window_rect();
    assert_eq!(
      frame(&mut e, vec![mouse(rect.x + 20.0, rect.y + 20.0)]),
      Some(CursorStyle::Default),
      "over the console"
    );
    assert_eq!(frame(&mut e, vec![mouse(319.0, 239.0)]), None, "away");
  }

  #[cfg(dev_tools)]
  #[test]
  fn the_console_moves_by_the_trackpad_as_far_as_the_pointer_would() {
    struct Idle;
    impl Scene for Idle {}
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(Idle)
      .build_headless();
    let plain = e.console_pixel_scale();
    e.ui_space.mouse_scale = 0.5;
    assert_eq!(e.console_pixel_scale(), plain * 0.5);
  }

  #[test]
  fn the_ui_debug_overlay_outlines_each_widget() {
    use crate::core::ui::immediate::{OVERLAY_LAYER, UiCommand};
    use crate::core::ui::{Rect, UiContext};
    const AT: Rect = Rect::new(20.0, 20.0, 80.0, 30.0);
    struct OneButton;
    impl Scene for OneButton {
      fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
        ui.button("Outlined").at(AT).show();
        SceneAction::Continue
      }
    }
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut e = EngineBuilder::new()
      .resolution(320, 240)
      .start_scene(OneButton)
      .build_headless();
    e.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
    let outlined = |e: &Engine| {
      e.ui_commands.commands().any(|c| {
        matches!(c, UiCommand::Rect { rect, layer, border: Some(_), .. } if *layer == OVERLAY_LAYER && *rect == AT)
      })
    };
    e.frame(Some(Duration::from_millis(16)));
    assert!(!outlined(&e), "off by default");
    e.config.debug.show_ui_debug = true;
    e.frame(Some(Duration::from_millis(16)));
    assert!(
      outlined(&e),
      "the button's rect is outlined on the overlay layer"
    );
  }
}

#[cfg(test)]
mod tick_check_tests {
  use super::*;
  use crate::core::ecs::World;
  use crate::core::schedule::{EngineSchedules, SceneSchedules};

  /// Half a billion ticks: bevy only checks change ticks that far apart.
  /// About a second in debug builds.
  #[test]
  fn a_change_tick_check_is_recorded_for_the_engine() {
    let mut world = World::new();
    world.init_resource::<PendingTickCheck>();
    world.add_observer(record_tick_check);
    for _ in 0..520_000_000u32 {
      world.increment_change_tick();
    }
    assert!(world.check_change_ticks().is_some());
    let check = world.resource_mut::<PendingTickCheck>().0.take();
    let check = check.expect("the observer recorded the check");
    // Every schedule the engine owns accepts it
    let mut schedules = EngineSchedules::new();
    schedules.check_change_ticks(check);
    SceneSchedules::default().check_change_ticks(check);
    crate::core::physics::sync::PhysicsSchedule::new().check_change_ticks(check);
  }
}
