//! The top-down preset: eight-direction movement from `MoveIntent`, with
//! acceleration, analog speed and an optional dash.

use super::events::{MovementEvent, MovementEventKind};
use super::intent::MoveIntent;
use super::platformer::Dash;
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, EntityHashSet, Knockback, Local, MessageWriter, Query, Res,
  Vec2, Velocity,
};
use crate::core::physics::{CharacterController, CharacterMode};
use crate::core::time::Time;

fn top_down_controller() -> CharacterController {
  CharacterController::top_down()
}

/// A top-down character's movement from its `MoveIntent`: acceleration and
/// deceleration in any direction, diagonals no faster than straight, and a
/// partial stick giving a proportional speed. Off by default: a dash
/// (`with_dash`). It writes the character's `Velocity` every step.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(MoveIntent, TopDownState, CharacterController = top_down_controller())]
pub struct TopDownMovement {
  /// Top speed, px/s
  pub max_speed: f32,
  /// Seconds from standing to full speed (0 is instant)
  pub accel_time: f32,
  /// Seconds from full speed to standing (0 is instant)
  pub decel_time: f32,
  /// A burst of speed along the facing, ignoring the input while it lasts
  pub dash: Option<Dash>,
  /// The share of steering kept while knockback is active (0: none)
  pub knockback_control: f32,
}

impl Default for TopDownMovement {
  fn default() -> Self {
    Self {
      max_speed: 160.0,
      accel_time: 0.08,
      decel_time: 0.1,
      dash: None,
      knockback_control: 0.0,
    }
  }
}

impl TopDownMovement {
  pub fn with_speed(mut self, max_speed: f32, accel_time: f32, decel_time: f32) -> Self {
    (self.max_speed, self.accel_time, self.decel_time) = (max_speed, accel_time, decel_time);
    self
  }

  pub fn with_dash(mut self, speed: f32, duration: f32, cooldown: f32) -> Self {
    self.dash = Some(Dash {
      speed,
      duration,
      cooldown,
    });
    self
  }

  pub fn with_knockback_control(mut self, fraction: f32) -> Self {
    self.knockback_control = fraction;
    self
  }
}

/// A top-down character's movement state, written by the engine.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TopDownState {
  /// The last direction steered in (unit length)
  pub facing: Vec2,
  /// In a dash
  pub dashing: bool,
  pub(crate) dash_left: f32,
  pub(crate) dash_cooldown: f32,
  pub(crate) dash_direction: Vec2,
}

impl Default for TopDownState {
  fn default() -> Self {
    Self {
      facing: Vec2::Y,
      dashing: false,
      dash_left: 0.0,
      dash_cooldown: 0.0,
      dash_direction: Vec2::ZERO,
    }
  }
}

impl TopDownState {
  /// Forget a dash and its cooldown, keeping the facing (a respawn).
  pub fn reset(&mut self) {
    *self = Self {
      facing: self.facing,
      ..Self::default()
    };
  }
}

type Mover = (
  Entity,
  &'static TopDownMovement,
  &'static mut TopDownState,
  &'static mut MoveIntent,
  &'static CharacterController,
  &'static mut Velocity,
  Option<&'static Knockback>,
);

/// The entities `top_down_movement` already warned about, per warning.
#[derive(Default)]
pub struct Warned {
  mode: EntityHashSet,
  velocity: EntityHashSet,
}

/// `FixedSet::Movement`: every top-down character's `Velocity` from its
/// intent.
pub fn top_down_movement(
  time: Res<Time>,
  mut movers: Query<Mover>,
  mut events: MessageWriter<MovementEvent>,
  mut warned: Local<Warned>,
) {
  let dt = time.fixed.timestep();
  for (entity, settings, mut state, mut intent, controller, mut velocity, knockback) in &mut movers
  {
    if controller.mode != CharacterMode::TopDown {
      if warned.mode.insert(entity) {
        crate::engine_warn!(
          module: "Gameplay",
          "{entity}: TopDownMovement needs CharacterController::top_down()"
        );
      }
      continue;
    }
    let direction = intent.clamped_direction();
    let mut v = velocity.as_vec2();
    if !v.is_finite() {
      if warned.velocity.insert(entity) {
        crate::engine_warn!(
          module: "Gameplay",
          "{entity}: TopDownMovement read a non-finite Velocity as zero"
        );
      }
      v = Vec2::ZERO;
    }
    if let Some(facing) = direction.try_normalize() {
      state.facing = facing;
    }
    state.dash_cooldown = (state.dash_cooldown - dt).max(0.0);
    if let Some(dash) = settings.dash
      && intent.dash_pressed
      && state.dash_cooldown <= 0.0
      && state.dash_left <= 0.0
    {
      state.dash_direction = state.facing;
      state.dash_left = dash.duration;
      state.dash_cooldown = dash.cooldown;
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::Dashed,
      });
    }
    if let Some(dash) = settings.dash
      && state.dash_left > 0.0
    {
      v = state.dash_direction * dash.speed;
      state.dash_left -= dt;
      if state.dash_left <= 0.0 {
        // Out of the dash no faster than walking
        v = state.dash_direction * dash.speed.min(settings.max_speed);
      }
    } else {
      // A dash whose settings went away mid-dash does not resume later
      state.dash_left = 0.0;
      let control = if knockback.is_some_and(|knockback| knockback.active) {
        settings.knockback_control
      } else {
        1.0
      };
      let target = direction * settings.max_speed * control;
      let time = if target != Vec2::ZERO {
        settings.accel_time
      } else {
        settings.decel_time
      };
      // A zero time (or no speed to move at) is instant: the target at
      // once, never an infinite or frozen rate
      let step = settings.max_speed / time * dt;
      let gap = target - v;
      v = if time > 0.0 && step > 0.0 && step.is_finite() && gap.length() > step {
        v + gap.normalize() * step
      } else {
        target
      };
    }
    state.dashing = state.dash_left > 0.0;
    intent.jump_pressed = false;
    intent.dash_pressed = false;
    velocity.set_if_neq(Velocity::from_vec2(v));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::{Messages, Transform, World};
  use crate::core::gameplay::test_util::*;
  use crate::core::physics::Collider;

  fn walker(world: &mut World) -> Entity {
    world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default(),
      ))
      .id()
  }

  fn steer(world: &mut World, e: Entity, direction: Vec2) {
    world.get_mut::<MoveIntent>(e).unwrap().direction = direction;
  }

  #[test]
  fn reaches_full_speed_in_its_acceleration_time() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..5 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().dx - 160.0).abs() < 1e-3);
  }

  #[test]
  fn diagonals_are_no_faster_and_half_a_stick_is_half_speed() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::ONE);
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().as_vec2().length() - 160.0).abs() < 1e-2);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::new(0.5, 0.0));
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().as_vec2().length() - 80.0).abs() < 1e-2);
  }

  #[test]
  fn letting_go_stops_in_its_deceleration_time() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    for _ in 0..7 {
      steer(&mut world, e, Vec2::ZERO);
      run(&mut world, &mut s, 1);
    }
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::ZERO);
  }

  #[test]
  fn a_dash_bursts_in_the_facing_direction() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_dash(500.0, 0.1, 0.4),
      ))
      .id();
    steer(&mut world, e, Vec2::Y);
    run(&mut world, &mut s, 3);
    steer(&mut world, e, Vec2::ZERO);
    world.get_mut::<MoveIntent>(e).unwrap().dash_pressed = true;
    run(&mut world, &mut s, 1);
    assert_eq!(
      world.get::<Velocity>(e).unwrap().as_vec2(),
      Vec2::new(0.0, 500.0)
    );
  }

  #[test]
  fn instant_times_reach_the_target_at_once() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_speed(160.0, 0.0, 0.0),
      ))
      .id();
    steer(&mut world, e, Vec2::X);
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Velocity>(e).unwrap().dx, 160.0);
    steer(&mut world, e, Vec2::ZERO);
    run(&mut world, &mut s, 1);
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::ZERO);
  }

  #[test]
  fn an_out_of_range_direction_is_clamped() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::new(5.0, 0.0));
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().dx - 160.0).abs() < 1e-3);
    for _ in 0..10 {
      steer(&mut world, e, Vec2::new(f32::NAN, 0.0));
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().as_vec2().is_finite());
  }

  #[test]
  fn a_non_finite_velocity_is_read_as_zero_and_warned_once() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..3 {
      world.get_mut::<Velocity>(e).unwrap().dx = f32::NAN;
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().as_vec2().is_finite());
    let prefix = format!("{e}: TopDownMovement read a non-finite Velocity");
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.starts_with(&prefix))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn a_platformer_controller_is_warned_once_and_left_alone() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default(),
        CharacterController::platformer(),
      ))
      .id();
    for _ in 0..3 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::ZERO);
    let prefix = format!("{e}: TopDownMovement needs");
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.starts_with(&prefix))
      .count();
    assert_eq!(warnings, 1);
  }

  fn dash_count(world: &World) -> usize {
    let messages = world.resource::<Messages<MovementEvent>>();
    messages
      .get_cursor()
      .read(messages)
      .filter(|m| m.kind == MovementEventKind::Dashed)
      .count()
  }

  /// Face right at rest, then press dash; the velocity of each of the next
  /// `steps` steps, steering up the whole time when `steer_away`.
  fn dash_run(steps: usize, steer_away: bool) -> (Vec<Vec2>, usize, bool) {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_dash(300.0, 0.1, 0.4),
      ))
      .id();
    for _ in 0..20 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    steer(&mut world, e, Vec2::ZERO);
    run(&mut world, &mut s, 20);
    let mut speeds = Vec::new();
    for i in 0..steps {
      // The press itself faces the input; steer away after it
      let away = steer_away && i > 0;
      steer(&mut world, e, if away { Vec2::NEG_Y } else { Vec2::ZERO });
      world.get_mut::<MoveIntent>(e).unwrap().dash_pressed = i == 0;
      run(&mut world, &mut s, 1);
      speeds.push(world.get::<Velocity>(e).unwrap().as_vec2());
    }
    let dashing = world.get::<TopDownState>(e).unwrap().dashing;
    (speeds, dash_count(&world), dashing)
  }

  #[test]
  fn a_dash_lasts_its_duration_at_speed_and_leaves_at_walking_speed() {
    let (speeds, dashes, dashing) = dash_run(12, false);
    // 0.1 s is 6 steps; the f32 timer may run one step over
    let fast = speeds
      .iter()
      .take_while(|v| **v == Vec2::new(300.0, 0.0))
      .count();
    assert!(
      (5..=6).contains(&fast),
      "{fast} steps at dash speed: {speeds:?}"
    );
    assert_eq!(speeds[fast], Vec2::new(160.0, 0.0), "leaves at max_speed");
    assert!(!dashing);
    assert_eq!(dashes, 1, "one Dashed per dash");
  }

  #[test]
  fn steering_is_ignored_while_dashing() {
    let (speeds, _, _) = dash_run(5, true);
    assert!(
      speeds.iter().all(|v| *v == Vec2::new(300.0, 0.0)),
      "{speeds:?}"
    );
  }

  #[test]
  fn the_cooldown_blocks_a_second_dash_and_then_allows_one() {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_dash(300.0, 0.1, 0.4),
      ))
      .id();
    for step in 0..40 {
      // Presses at step 0 (dashes), 12 (cooling down) and 30 (allowed again)
      steer(&mut world, e, Vec2::X);
      world.get_mut::<MoveIntent>(e).unwrap().dash_pressed = matches!(step, 0 | 12 | 30);
      run(&mut world, &mut s, 1);
      if step == 12 {
        assert_eq!(dash_count(&world), 1, "blocked by the cooldown");
      }
    }
    assert_eq!(dash_count(&world), 2);
  }

  /// Hold right with knockback pushing left and `control` steering kept.
  fn steered_under_knockback(control: f32) -> f32 {
    let (mut world, mut s) = world(60);
    let e = world
      .spawn((
        Transform::IDENTITY,
        Collider::circle(8.0),
        TopDownMovement::default().with_knockback_control(control),
      ))
      .id();
    for _ in 0..20 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    world
      .entity_mut(e)
      .insert(Knockback::new(Vec2::NEG_X, 200.0).with_decay_rate(1.0));
    for _ in 0..10 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    world.get::<Velocity>(e).unwrap().dx
  }

  #[test]
  fn knockback_control_scales_the_steering_target() {
    assert!(steered_under_knockback(0.0).abs() < 1e-3, "no steering");
    let half = steered_under_knockback(0.5);
    assert!((half - 80.0).abs() < 1e-3, "{half}");
  }

  #[test]
  fn a_moving_character_given_zero_speed_stops() {
    let (mut world, mut s) = world(60);
    let e = walker(&mut world);
    for _ in 0..20 {
      steer(&mut world, e, Vec2::X);
      run(&mut world, &mut s, 1);
    }
    world.get_mut::<TopDownMovement>(e).unwrap().max_speed = 0.0;
    for _ in 0..7 {
      run(&mut world, &mut s, 1);
    }
    assert_eq!(world.get::<Velocity>(e).unwrap().as_vec2(), Vec2::ZERO);
  }

  #[test]
  fn reset_keeps_only_the_facing() {
    let mut state = TopDownState {
      facing: Vec2::X,
      dashing: true,
      dash_left: 0.1,
      dash_cooldown: 0.2,
      ..TopDownState::default()
    };
    state.reset();
    assert_eq!(
      state,
      TopDownState {
        facing: Vec2::X,
        ..TopDownState::default()
      }
    );
  }
}
