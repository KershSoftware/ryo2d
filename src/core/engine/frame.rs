//! One frame: clocks, input, the UI pass, the fixed steps, then render.

use std::time::{Duration, Instant};

use winit::keyboard::KeyCode;

use super::{Engine, FrameOutcome, PresentOutcome, QuitRequested, SceneHost};
use crate::core::asset::AssetManager;
use crate::core::capture::CaptureTarget;
use crate::core::config::debug as overlays;
use crate::core::console::{CommandContext, CommandRegistry, CommandResult};
use crate::core::cursor::CursorRequest;
use crate::core::ecs::{MessageWriter, Res, ResMut, Resource, World};
use crate::core::graphics::Camera;
use crate::core::input::{Input, InputEvent};
use crate::core::messages::{AssetLoadFailed, AssetReloaded, update_messages};
use crate::core::scene::SceneAction;
use crate::core::schedule::Phase;
use crate::core::shaders::EffectsManager;
use crate::core::time::Time;
use crate::core::ui::{UiContext, UiInput};
use crate::profile_scope;

/// The real delta the loop chose for this frame: `Some` for headless runs
/// and tests, `None` for the wall clock.
#[derive(Resource)]
pub(crate) struct PendingDelta(pub(crate) Option<Duration>);

impl Engine {
  /// One frame. `real_delta` of `None` means wall clock.
  pub(crate) fn frame(&mut self, real_delta: Option<Duration>) -> FrameOutcome {
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
    self.console_tick();

    let update_start = Instant::now();
    {
      profile_scope!("cpu:update");
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
    }
    self.frame_timings.update = update_start.elapsed();

    if self.world.remove_resource::<QuitRequested>().is_some() || self.console_quit_requested {
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
    self
      .input_script
      .after_frame(&mut self.world.resource_mut::<Input>());
    outcome
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

  /// F1-F5 overlays and borderless, F9/F10 dev windows, F12 capture, and the
  /// console key. Each consumes its key so the game never sees it.
  fn handle_debug_keys(&mut self) {
    let mut input = self.world.resource_mut::<Input>();
    let mut pressed = |key: KeyCode| {
      let hit = input.key_pressed(key);
      if hit {
        input.consume_key(key);
      }
      hit
    };

    if self.debug_keys_enabled {
      if pressed(KeyCode::F1) {
        self.overlay_mode = self.overlay_mode.next();
        let (show_fps, show_frame_timing, show_stats) = self.overlay_mode.to_flags();
        self.config.debug.show_fps = show_fps;
        self.config.debug.show_frame_timing = show_frame_timing;
        self.config.debug.show_stats = show_stats;
      }
      if pressed(KeyCode::F2) {
        self.config.debug.show_colliders = !self.config.debug.show_colliders;
      }
      if pressed(KeyCode::F3) {
        self.config.debug.show_velocities = !self.config.debug.show_velocities;
      }
      if pressed(KeyCode::F4) {
        self.config.debug.show_origins = !self.config.debug.show_origins;
      }
      if pressed(KeyCode::F5) {
        self.pending_toggle_borderless = true;
      }
    }

    if self.config.debug.dev_console && pressed(KeyCode::Backquote) {
      self.console_state.toggle();
    }

    if self.config.debug.dev_window_keys {
      if pressed(KeyCode::F9) {
        if self.logger_window.take().is_some() {
          self.logger_window_focused = false;
        } else {
          // Opened in the event loop, which has the ActiveEventLoop
          self.pending_open_logger = true;
        }
      }
      #[cfg(feature = "profiler")]
      if pressed(KeyCode::F10) {
        if self.profiler_window.take().is_some() {
          self.profiler_window_focused = false;
        } else {
          self.pending_open_profiler = true;
        }
      }
      if pressed(KeyCode::F12) {
        self.capture.request(CaptureTarget::Default);
      }
    }
  }

  /// The developer console: animation, input while open, commands.
  fn console_tick(&mut self) {
    let real_dt = self.world.resource::<Time>().real.delta();
    self.console_state.update(real_dt);

    if self.console_state.visible {
      let command = self
        .console_state
        .handle_input(&mut self.world.resource_mut::<Input>());
      if let Some(command) = command {
        let mut speed = self.world.resource::<Time>().game.speed();
        let (quit, capture) = {
          let registry = self.world.resource::<CommandRegistry>();
          let mut cmd_ctx = CommandContext {
            console: &mut self.console_state,
            debug_config: &mut self.config.debug,
            quit_requested: false,
            capture_requested: None,
            time_scale: &mut speed,
            commands: registry.command_info_list(),
            game_name: self.game_name.clone(),
            game_version: self.game_version.clone(),
          };
          let result = registry.execute(&command, &mut cmd_ctx);
          (
            matches!(result, CommandResult::Quit) || cmd_ctx.quit_requested,
            cmd_ctx.capture_requested.take(),
          )
        };
        if quit {
          self.console_quit_requested = true;
        }
        if let Some(target) = capture {
          self.capture.request(target);
        }
        let mut time = self.world.resource_mut::<Time>();
        if time.game.speed() != speed {
          time.game.set_speed(speed);
        }
      }

      // A focused console keeps the keyboard from the game; an unfocused one
      // can stay open while playing
      if self.console_state.is_focused() {
        self.world.resource_mut::<Input>().clear_keyboard_state();
      }
    }

    // Runs while closed too, so the cursor is reset
    let (mouse_x, mouse_y) = self.world.resource::<Input>().mouse_position();
    if let Some(style) = self.console_state.desired_cursor(mouse_x, mouse_y) {
      self.world.resource_mut::<CursorRequest>().cursor = Some(style);
    }
  }

  /// The UI pass: the scenes' `ui` (visible scenes, bottom-up), then the
  /// debug overlays and the console, before any fixed step. What it consumes
  /// is hidden from gameplay for the frame, and the scene action it returns
  /// applies at once. When that action brings a new scene to the top, the UI
  /// is drawn again so the frame shows the new scene's UI, not none; once
  /// only, so a scene that switches on every call cannot loop.
  fn ui_pass(&mut self) {
    for _ in 0..2 {
      let Some(action) = self.draw_ui() else {
        return;
      };
      if matches!(action, SceneAction::Continue) {
        return;
      }
      let (keep_running, top_changed) = with_host(&mut self.world, |world, host| {
        let SceneHost { scenes, audio } = host;
        let before = scenes.top_scope();
        let keep_running = scenes.handle_action(action, world, audio);
        scenes.sync_physics_paused(world);
        (keep_running, scenes.top_scope() != before)
      });
      if !keep_running {
        self.world.insert_resource(QuitRequested);
        return;
      }
      if !top_changed {
        return;
      }
    }
  }

  /// Draw one UI pass into the UI buffer and apply what it consumed; `None`
  /// before the GPU is up.
  fn draw_ui(&mut self) -> Option<SceneAction> {
    let (width, height) = self
      .renderer
      .as_ref()
      .map(|renderer| (renderer.game_width(), renderer.game_height()))?;
    self.ui_buffer.fill(0);
    let snapshot = UiInput::snapshot(self.world.resource::<Input>());
    let real_dt = self.world.resource::<Time>().real.delta();
    let (action, consumed) = {
      let mut ui = UiContext::new_scaled(
        &mut self.ui_buffer,
        width,
        height,
        snapshot,
        &self.ui_font,
        self.ui_scale,
      );
      ui.set_dt(real_dt);
      // The console blocks the mouse for everything under it
      if self.console_state.visible {
        let (mouse_x, mouse_y) = ui.input().mouse_position();
        if self.console_state.window_rect().contains(mouse_x, mouse_y) {
          ui.set_mouse_occluded();
          ui.consume_click();
        }
      }

      let action = with_host(&mut self.world, |world, host| {
        host.scenes.ui_pass(world, &mut host.audio, &mut ui)
      });

      // Overlays draw over game UI: stats (top), FPS, frame timing (bottom)
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
      // The console over everything
      self.console_state.render(&mut ui, width, height);
      (action, ui.into_input())
    };

    let mut input = self.world.resource_mut::<Input>();
    consumed.apply_consumed(&mut input);
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
    Some(action)
  }

  /// Wait out the rest of the frame at `target_fps` (sleep, then spin for
  /// the last stretch, which sleep cannot hit exactly).
  pub(super) fn pace_frame(&self) {
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

/// Frames are paced in software only when presenting does not wait for the
/// display already (vsync off).
pub(super) fn should_pace_frames(presentation_blocks: bool) -> bool {
  !presentation_blocks
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

/// `PreUpdate`: finish async loads and (dev builds) hot reloads.
pub(crate) fn asset_pump_system(
  mut assets: ResMut<AssetManager>,
  mut reloaded: MessageWriter<AssetReloaded>,
  mut failed: MessageWriter<AssetLoadFailed>,
) {
  assets.process_async_loads();
  #[cfg(debug_assertions)]
  assets.process_reloads();
  for outcome in assets.take_reload_outcomes() {
    match outcome {
      Ok(path) => {
        reloaded.write(AssetReloaded { path });
      }
      Err((path, error)) => {
        failed.write(AssetLoadFailed { path, error });
      }
    }
  }
}

/// `PostUpdate`: camera shake runs on real time.
pub(crate) fn camera_shake_system(time: Res<Time>, mut camera: ResMut<Camera>) {
  camera.update_shake(time.real.delta());
}

/// `PostUpdate`: timed post-process effects run on real time.
pub(crate) fn update_effects_system(time: Res<Time>, mut effects: ResMut<EffectsManager>) {
  effects.update(time.real.delta());
}

/// `Last`: end the frame's change tracking.
pub(crate) fn clear_trackers_system(world: &mut World) {
  world.clear_trackers();
  if world.resource::<Time>().frame_count().is_multiple_of(1024) {
    world.check_change_ticks();
  }
}

/// Run `f` with the scene stack and audio taken out of the world.
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
    let SceneHost { scenes, audio } = host;
    let action = scenes.update(world, audio);
    if !scenes.handle_action(action, world, audio) {
      world.insert_resource(QuitRequested);
    }
    scenes.sync_physics_paused(world);
  });
}

/// `PreUpdate`: advance a scene transition once a frame on real time, so a
/// fade runs while the game clock is paused or slowed.
pub(crate) fn scene_transition_system(world: &mut World) {
  let dt = world.resource::<Time>().real.delta();
  with_host(world, |world, host| {
    let SceneHost { scenes, audio } = host;
    if !scenes.is_transitioning() {
      return;
    }
    scenes.update_transition(dt, world, audio);
    scenes.sync_physics_paused(world);
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
    use crate::core::messages::CollisionStarted;
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
            Collider::box_collider(200.0, 20.0),
          ));
          ctx.world.spawn((
            Transform::from_xy(0.0, 86.0),
            RigidBody::new(),
            Collider::box_collider(10.0, 10.0),
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
    text: crate::core::ui::TextInputState,
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
        ui.button("Go", crate::core::ui::Rect::new(0.0, 0.0, 32.0, 16.0));
      },
      WidgetState::default(),
      press_hold_release_at(8.0, 8.0),
    ) else {
      return;
    };
    assert_eq!(mouse, 0, "gameplay saw the button's press, hold or release");
  }

  #[test]
  fn a_slider_drag_is_the_ui_s() {
    let Some((mouse, _)) = gameplay_under(
      |ui, state| {
        ui.slider(
          crate::core::ui::Rect::new(10.0, 20.0, 40.0, 8.0),
          &mut state.slider,
          0.0,
          1.0,
        );
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
    use winit::keyboard::KeyCode;
    let mut state = WidgetState::default();
    state.text.set_focused(true);
    let Some((_, w)) = gameplay_under(
      |ui, state| {
        ui.text_input(
          &mut state.text,
          crate::core::ui::Rect::new(0.0, 0.0, 60.0, 16.0),
          "",
          0.0,
        );
      },
      state,
      vec![
        vec![
          InputEvent::Key {
            key: KeyCode::KeyW,
            pressed: true,
          },
          InputEvent::Text("w".into()),
        ],
        vec![],
        vec![InputEvent::Key {
          key: KeyCode::KeyW,
          pressed: false,
        }],
      ],
    ) else {
      return;
    };
    assert_eq!(w, 0, "gameplay saw W typed into the field");
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
        if ui.button("Below", Rect::new(0.0, 0.0, 32.0, 16.0)) {
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
        if ui.button("Over", Rect::new(0.0, 0.0, 32.0, 16.0)) {
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
    let clicks = e.world().resource::<Clicks>();
    assert_eq!((clicks.below, clicks.overlay), (0, 1));
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
    let mut query = e.world_mut().query::<(
      &crate::core::ecs::GlobalTransform,
      &crate::core::ecs::PreviousGlobalTransform,
    )>();
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
    use crate::core::physics::{Collider, RapierPhysicsWorld, RigidBody};
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
        RigidBody::new(),
        Collider::circle_collider(4.0),
      ))
      .id();
    e.frame(Some(Duration::from_millis(20))); // one step registers it
    assert!(
      e.world()
        .resource::<RapierPhysicsWorld>()
        .is_registered(body)
    );
    e.world_mut().despawn(body);
    // A high frame rate: frames with no fixed step, while removal events
    // only last two of them
    for _ in 0..3 {
      e.frame(Some(Duration::from_millis(1)));
    }
    e.frame(Some(Duration::from_millis(20)));
    assert!(
      !e.world()
        .resource::<RapierPhysicsWorld>()
        .is_registered(body)
    );
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
  fn a_scene_switched_from_ui_draws_its_ui_the_same_frame() {
    use crate::core::ui::{Color, Rect, UiContext};
    struct Leaves;
    impl Scene for Leaves {
      fn ui(&mut self, _ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
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
    assert_eq!(
      &e.ui_buffer[..4],
      &[255, 0, 0, 255],
      "the arriving scene's UI is in the switching frame"
    );
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
    assert!(should_pace_frames(false), "vsync off: pace to target_fps");
    assert!(!should_pace_frames(true), "vsync on: the display paces");
  }
}
