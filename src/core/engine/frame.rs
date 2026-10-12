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
use crate::core::schedule::Phase;
use crate::core::shaders::EffectsManager;
use crate::core::time::Time;
use crate::core::ui::UiContext;
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
      while self.world.resource_mut::<Time>().try_expend_fixed_step() {
        self.world.resource_mut::<Input>().set_fixed_step(step);
        self.world.resource_mut::<Time>().set_in_fixed(true);
        self.schedules.run(Phase::FixedUpdate, &mut self.world);
        self.world.resource_mut::<Time>().set_in_fixed(false);
        step += 1;
        if self.world.contains_resource::<QuitRequested>() {
          break;
        }
      }
      // A press nobody stepped over waits for the next frame's first step
      self.world.resource_mut::<Input>().end_fixed_steps(step > 0);
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

  /// Draw the frame's UI layer: debug overlays and the console.
  fn ui_pass(&mut self) {
    let Some((width, height)) = self
      .renderer
      .as_ref()
      .map(|renderer| (renderer.game_width(), renderer.game_height()))
    else {
      return;
    };
    self.ui_buffer.fill(0);
    let real_dt = self.world.resource::<Time>().real.delta();
    let input = self.world.resource::<Input>();
    let mut ui = UiContext::new_scaled(
      &mut self.ui_buffer,
      width,
      height,
      input,
      &self.ui_font,
      self.ui_scale,
    );
    ui.set_dt(real_dt);
    // MIGRATION: Task 14 runs the scenes' `ui` here, before the overlays

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

/// `First`: advance every message buffer by a frame.
pub(crate) fn update_messages_system(world: &mut World) {
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
    // covered end to end in Task 15's headless tests; here: the loop sets the fixed step index
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
  fn frames_are_paced_only_when_presenting_does_not_wait() {
    assert!(should_pace_frames(false), "vsync off: pace to target_fps");
    assert!(!should_pace_frames(true), "vsync on: the display paces");
  }
}
