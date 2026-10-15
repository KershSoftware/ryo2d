//! The platformer preset: run, jump and fall from `MoveIntent`, with the
//! forgiveness that makes jumps feel fair, and the optional extras: air
//! jumps, wall slide and wall jump, and dash.

use super::events::{MovementEvent, MovementEventKind};
use super::intent::MoveIntent;
use crate::core::ecs::{
  Component, DetectChangesMut, Entity, EntityHashSet, Knockback, Local, MessageWriter, Query, Res,
  Vec2, Velocity,
};
use crate::core::physics::{CharacterController, CharacterMode, CharacterState};
use crate::core::time::Time;

fn platformer_controller() -> CharacterController {
  CharacterController::platformer()
}

/// A side-view character's movement from its `MoveIntent`: running with
/// acceleration, air control, a jump set by its height and the time to its
/// top, a heavier fall, coyote time, jump buffering and a shorter jump when
/// the button is released early. Down + jump on a one-way platform drops
/// through it. Off by default: jumps in the air (`with_air_jumps`), a capped
/// fall while pressing into a wall (`with_wall_slide`), a jump off a wall
/// that holds the run input off for a moment (`with_wall_jump`), and a dash
/// (`with_dash`, one in the air until landing). It writes the character's
/// `Velocity` every step, with its own gravity (characters are not dynamic
/// bodies). A jump off a moving platform keeps the platform's speed: in the
/// air the run aims past it, until landing, a wall, a wall jump or a dash.
/// It reads each axis of the input on its own (`MoveIntent::clamped_axes`),
/// so holding up or down does not slow the run.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
#[require(MoveIntent, PlatformerState, CharacterController = platformer_controller())]
pub struct PlatformerMovement {
  /// Top running speed, px/s
  pub run_speed: f32,
  /// Seconds from standing to full speed (0 is instant)
  pub accel_time: f32,
  /// Seconds from full speed to standing (0 is instant)
  pub decel_time: f32,
  /// The share of ground acceleration available in the air
  pub air_control: f32,
  /// Jump height, px
  pub jump_height: f32,
  /// Seconds from the jump to its top
  pub time_to_apex: f32,
  /// Gravity multiplier while falling
  pub fall_multiplier: f32,
  /// Fastest fall, px/s
  pub max_fall_speed: f32,
  /// Seconds after leaving a floor a jump still works
  pub coyote_time: f32,
  /// Seconds a jump pressed before landing is kept
  pub jump_buffer: f32,
  /// Upward speed kept when jump is released early (1 keeps full jumps)
  pub jump_cut: f32,
  /// The share of steering kept while knockback is active (0: none)
  pub knockback_control: f32,
  /// Jumps in the air before landing (refilled on landing and by a wall jump)
  pub air_jumps: u32,
  /// Fastest fall while pressing into a wall in the air, px/s
  pub wall_slide: Option<f32>,
  /// Jumping off a wall touched in the air
  pub wall_jump: Option<WallJump>,
  /// A burst of speed along the input, without gravity; one in the air
  /// until landing
  pub dash: Option<Dash>,
}

/// A jump off a wall: the launch away from the wall and up, and how long
/// the run input is held off afterwards so the jump keeps its direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallJump {
  /// Speed away from the wall (x) and up (y), px/s, both positive
  pub launch: Vec2,
  /// Seconds the run input is ignored after the jump
  pub lock_time: f32,
}

/// A dash: a burst of speed along the input (or the facing when there is
/// none), with no gravity while it lasts, leaving no faster than running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dash {
  /// Speed while dashing, px/s
  pub speed: f32,
  /// Seconds the dash lasts
  pub duration: f32,
  /// Seconds from one dash's start to the next
  pub cooldown: f32,
}

impl Default for PlatformerMovement {
  fn default() -> Self {
    Self {
      run_speed: 180.0,
      accel_time: 0.08,
      decel_time: 0.06,
      air_control: 0.65,
      jump_height: 64.0,
      time_to_apex: 0.38,
      fall_multiplier: 1.5,
      max_fall_speed: 600.0,
      coyote_time: 0.1,
      jump_buffer: 0.1,
      jump_cut: 0.5,
      knockback_control: 0.0,
      air_jumps: 0,
      wall_slide: None,
      wall_jump: None,
      dash: None,
    }
  }
}

impl PlatformerMovement {
  pub fn with_run(mut self, speed: f32, accel_time: f32, decel_time: f32) -> Self {
    (self.run_speed, self.accel_time, self.decel_time) = (speed, accel_time, decel_time);
    self
  }

  pub fn with_air_control(mut self, fraction: f32) -> Self {
    self.air_control = fraction;
    self
  }

  pub fn with_jump(mut self, height: f32, time_to_apex: f32) -> Self {
    (self.jump_height, self.time_to_apex) = (height, time_to_apex);
    self
  }

  pub fn with_fall(mut self, multiplier: f32, max_speed: f32) -> Self {
    (self.fall_multiplier, self.max_fall_speed) = (multiplier, max_speed);
    self
  }

  pub fn with_coyote_time(mut self, seconds: f32) -> Self {
    self.coyote_time = seconds;
    self
  }

  pub fn with_jump_buffer(mut self, seconds: f32) -> Self {
    self.jump_buffer = seconds;
    self
  }

  pub fn with_jump_cut(mut self, factor: f32) -> Self {
    self.jump_cut = factor;
    self
  }

  pub fn with_knockback_control(mut self, fraction: f32) -> Self {
    self.knockback_control = fraction;
    self
  }

  pub fn with_air_jumps(mut self, count: u32) -> Self {
    self.air_jumps = count;
    self
  }

  pub fn with_wall_slide(mut self, max_speed: f32) -> Self {
    self.wall_slide = Some(max_speed);
    self
  }

  pub fn with_wall_jump(mut self, launch: Vec2, lock_time: f32) -> Self {
    self.wall_jump = Some(WallJump { launch, lock_time });
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
}

/// A platformer character's movement state, written by the engine.
#[derive(Component, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlatformerState {
  /// -1 facing left, 1 facing right
  pub facing: f32,
  /// Rising from a jump (releasing jump cuts it short)
  pub jumping: bool,
  /// Air jumps left before landing
  pub air_jumps_left: u32,
  /// Pressing into a wall in the air, the fall capped by `wall_slide`
  pub wall_sliding: bool,
  /// In a dash
  pub dashing: bool,
  /// The air dash is unused (refilled on landing)
  pub dash_ready: bool,
  pub(crate) coyote: f32,
  pub(crate) buffer: f32,
  /// Seconds a wall jump still works after the wall was last touched
  pub(crate) wall_coyote: f32,
  /// -1 when that wall was on the right (a wall jump goes left), 1 on the left
  pub(crate) wall_side: f32,
  /// Seconds left of a wall jump's hold on the run input
  pub(crate) lock: f32,
  pub(crate) dash_left: f32,
  pub(crate) dash_cooldown: f32,
  pub(crate) dash_direction: Vec2,
  /// The floor's horizontal speed under the character, px/s, while on it
  pub(crate) floor_speed: f32,
  /// The horizontal momentum a floor left behind, px/s: kept in the air
  /// (the run aims past it) until landing, a wall it runs into, a wall
  /// jump or a dash
  pub(crate) inherited: f32,
}

impl Default for PlatformerState {
  fn default() -> Self {
    Self {
      facing: 1.0,
      jumping: false,
      air_jumps_left: 0,
      wall_sliding: false,
      dashing: false,
      dash_ready: true,
      coyote: 0.0,
      buffer: 0.0,
      wall_coyote: 0.0,
      wall_side: 0.0,
      lock: 0.0,
      dash_left: 0.0,
      dash_cooldown: 0.0,
      dash_direction: Vec2::ZERO,
      floor_speed: 0.0,
      inherited: 0.0,
    }
  }
}

impl PlatformerState {
  /// Forget jumps, timers, dashes in progress and a platform's momentum,
  /// keeping the facing (a respawn).
  pub fn reset(&mut self) {
    *self = Self {
      facing: self.facing,
      ..Self::default()
    };
  }
}

/// Gravity and launch speed for a jump `height` px high that reaches its
/// top after `time` s, exact at the fixed step `dt`: with gravity added
/// before each move, `height = (g / 2)(T² - T·dt)` at `T = v / g`.
pub(crate) fn jump_physics(height: f32, time: f32, dt: f32) -> (f32, f32) {
  let time = time.max(2.0 * dt);
  let gravity = 2.0 * height.max(0.0) / (time * time - time * dt);
  (gravity, gravity * time)
}

/// `value` moved toward `target` by at most `step` (straight there when the
/// step is infinite).
fn approach(value: f32, target: f32, step: f32) -> f32 {
  if (target - value).abs() <= step {
    target
  } else {
    value + step.copysign(target - value)
  }
}

/// Speed change per second toward `target`: accelerating toward a faster
/// speed the same way, decelerating otherwise. A zero time is instant.
fn run_rate(settings: &PlatformerMovement, current: f32, target: f32) -> f32 {
  let accelerating = target != 0.0
    && (current == 0.0 || current.signum() == target.signum())
    && target.abs() >= current.abs();
  let time = if accelerating {
    settings.accel_time
  } else {
    settings.decel_time
  };
  if time > 0.0 {
    settings.run_speed / time
  } else {
    f32::INFINITY
  }
}

type Mover = (
  Entity,
  &'static PlatformerMovement,
  &'static mut PlatformerState,
  &'static mut MoveIntent,
  &'static CharacterState,
  &'static mut CharacterController,
  &'static mut Velocity,
  Option<&'static Knockback>,
);

/// The entities `platformer_movement` already warned about, per warning
/// (one warning does not silence another).
#[derive(Default)]
pub struct Warned {
  mode: EntityHashSet,
  velocity: EntityHashSet,
}

/// `FixedSet::Movement`: every platformer character's `Velocity` from its
/// intent and its last move.
pub fn platformer_movement(
  time: Res<Time>,
  mut movers: Query<Mover>,
  mut events: MessageWriter<MovementEvent>,
  mut warned: Local<Warned>,
) {
  let dt = time.fixed.timestep();
  for (
    entity,
    settings,
    mut state,
    mut intent,
    character,
    mut controller,
    mut velocity,
    knockback,
  ) in &mut movers
  {
    if controller.mode != CharacterMode::Platformer {
      if warned.mode.insert(entity) {
        crate::engine_warn!(
          module: "Gameplay",
          "{entity}: PlatformerMovement needs CharacterController::platformer()"
        );
      }
      continue;
    }
    // Each axis on its own: up or down held while running (W as up and
    // jump) leaves the run at full speed
    let direction = intent.clamped_axes();
    let mut v = velocity.as_vec2();
    if !v.is_finite() {
      if warned.velocity.insert(entity) {
        crate::engine_warn!(
          module: "Gameplay",
          "{entity}: PlatformerMovement read a non-finite Velocity as zero"
        );
      }
      v = Vec2::ZERO;
    }
    let (gravity, jump_speed) = jump_physics(settings.jump_height, settings.time_to_apex, dt);
    if character.landed {
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::Landed,
      });
      // Landing refills the air jumps and the air dash, and ends a wall
      // jump's hold on the run (which would otherwise keep the launch speed
      // along the ground)
      state.air_jumps_left = settings.air_jumps;
      state.dash_ready = true;
      state.lock = 0.0;
    }

    // A moving floor's momentum: the controller adds the floor's velocity
    // to `Velocity` on the step the character leaves it, and in the air the
    // run aims past it, so no input keeps it and steering is relative to it
    if character.on_floor {
      let speed = character.platform_velocity.x;
      state.floor_speed = if speed.is_finite() { speed } else { 0.0 };
      state.inherited = 0.0;
    } else if character.left_floor {
      state.inherited = state.floor_speed;
      state.floor_speed = 0.0;
    }
    if character.on_wall && character.wall_normal.x * state.inherited < 0.0 {
      // A wall it ran into took it
      state.inherited = 0.0;
    }

    // At least a step each, so a zero coyote time or buffer still jumps
    // from the floor (or a wall) at once
    state.coyote = if character.on_floor {
      settings.coyote_time.max(dt)
    } else {
      (state.coyote - dt).max(0.0)
    };
    state.buffer = if intent.jump_pressed {
      settings.jump_buffer.max(dt)
    } else {
      (state.buffer - dt).max(0.0)
    };
    state.lock = (state.lock - dt).max(0.0);
    state.dash_cooldown = (state.dash_cooldown - dt).max(0.0);
    if character.on_wall && !character.on_floor {
      state.wall_coyote = settings.coyote_time.max(dt);
      state.wall_side = character.wall_normal.x.signum();
    } else {
      state.wall_coyote = (state.wall_coyote - dt).max(0.0);
    }

    // Dashing: straight along the input (or the facing), with no running,
    // jumping or gravity while it lasts; one dash in the air until landing
    if let Some(dash) = settings.dash
      && intent.dash_pressed
      && state.dash_ready
      && state.dash_cooldown <= 0.0
      && state.dash_left <= 0.0
    {
      state.dash_direction = direction
        .try_normalize()
        .unwrap_or(Vec2::new(state.facing, 0.0));
      if state.dash_direction.x.abs() > 0.01 {
        state.facing = state.dash_direction.x.signum();
      }
      state.dash_left = dash.duration;
      state.dash_cooldown = dash.cooldown;
      if !character.on_floor {
        state.dash_ready = false;
      }
      state.jumping = false;
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::Dashed,
      });
    }
    if let Some(dash) = settings.dash
      && state.dash_left > 0.0
    {
      // A dash replaces the momentum, and leaves no faster than running
      state.inherited = 0.0;
      v = state.dash_direction * dash.speed;
      state.dash_left -= dt;
      if state.dash_left <= 0.0 {
        // Out of the dash no faster than running
        v = state.dash_direction * dash.speed.min(settings.run_speed);
      }
      state.dashing = state.dash_left > 0.0;
      state.wall_sliding = false;
      intent.jump_pressed = false;
      intent.dash_pressed = false;
      velocity.set_if_neq(Velocity::from_vec2(v));
      continue;
    }
    state.dashing = false;
    // A dash whose settings went away mid-dash does not resume later
    state.dash_left = 0.0;

    // Running, unless a wall jump holds the input off; knockback takes the
    // steering away
    if state.lock <= 0.0 {
      let control = if knockback.is_some_and(|knockback| knockback.active) {
        settings.knockback_control
      } else {
        1.0
      };
      let mut target = direction.x * settings.run_speed * control;
      if !character.on_floor {
        target += state.inherited;
      }
      let mut rate = run_rate(settings, v.x, target);
      if !character.on_floor {
        // No air control is no steering, even with an instant (infinite) rate
        rate = if settings.air_control > 0.0 {
          rate * settings.air_control
        } else {
          0.0
        };
      }
      v.x = approach(v.x, target, rate * dt);
      if direction.x.abs() > 0.01 {
        state.facing = direction.x.signum();
      }
    }

    // Dropping through (no coyote jump back up through the platform), or
    // jumping: from the floor, off a wall, then in the air
    if intent.jump_pressed && direction.y > 0.5 && character.on_floor && character.floor_one_way {
      controller.drop_through = true;
      state.buffer = 0.0;
      state.coyote = 0.0;
    } else if state.buffer > 0.0 && state.coyote > 0.0 {
      v.y = -jump_speed;
      state.jumping = true;
      state.buffer = 0.0;
      state.coyote = 0.0;
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::Jumped,
      });
    } else if let Some(wall) = settings.wall_jump
      && state.buffer > 0.0
      && state.wall_coyote > 0.0
      && !character.on_floor
    {
      v.x = state.wall_side * wall.launch.x;
      v.y = -wall.launch.y;
      // The launch replaces the momentum
      state.inherited = 0.0;
      state.lock = wall.lock_time;
      state.air_jumps_left = settings.air_jumps;
      state.facing = state.wall_side;
      state.jumping = true;
      state.buffer = 0.0;
      state.wall_coyote = 0.0;
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::WallJumped,
      });
    } else if intent.jump_pressed
      && !character.on_floor
      && state.coyote <= 0.0
      && state.air_jumps_left > 0
    {
      v.y = -jump_speed;
      state.air_jumps_left -= 1;
      state.jumping = true;
      state.buffer = 0.0;
      events.write(MovementEvent {
        entity,
        kind: MovementEventKind::AirJumped,
      });
    }

    // A jump released early is cut short, once
    if state.jumping {
      if v.y >= 0.0 {
        state.jumping = false;
      } else if !intent.jump_held {
        v.y *= settings.jump_cut;
        state.jumping = false;
      }
    }

    // Falling, heavier on the way down
    let pull = if v.y > 0.0 {
      gravity * settings.fall_multiplier
    } else {
      gravity
    };
    v.y = (v.y + pull * dt).min(settings.max_fall_speed);

    // Sliding down a wall pressed into in the air
    state.wall_sliding = false;
    if let Some(max) = settings.wall_slide
      && character.on_wall
      && !character.on_floor
      && v.y > 0.0
      && direction.x * character.wall_normal.x < 0.0
    {
      v.y = v.y.min(max);
      state.wall_sliding = true;
    }

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
  use crate::core::physics::{CharacterState, Collider};
  use crate::core::schedule::EngineSchedules;

  fn runner(world: &mut World, settings: PlatformerMovement) -> Entity {
    let e = player(world, 0.0, 79.0);
    world.entity_mut(e).insert(settings);
    e
  }

  fn intent(world: &mut World, e: Entity) -> crate::core::ecs::Mut<'_, MoveIntent> {
    world.get_mut::<MoveIntent>(e).unwrap()
  }

  fn events(world: &World) -> Vec<MovementEventKind> {
    let messages = world.resource::<Messages<MovementEvent>>();
    messages
      .get_cursor()
      .read(messages)
      .map(|m| m.kind)
      .collect()
  }

  fn settle(world: &mut World, s: &mut EngineSchedules) {
    run(world, s, 30);
  }

  #[test]
  fn the_jump_reaches_its_height_at_its_apex_time() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_jump(64.0, 0.38),
    );
    settle(&mut world, &mut s);
    let start = at(&world, e).translation.y;
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    let (mut top, mut top_step) = (start, 0);
    for i in 1..=60 {
      intent(&mut world, e).jump_held = true;
      run(&mut world, &mut s, 1);
      let y = at(&world, e).translation.y;
      if y < top {
        (top, top_step) = (y, i);
      }
    }
    assert!((start - top - 64.0).abs() <= 1.0, "height {}", start - top);
    assert!(
      (top_step as f32 - 0.38 * 60.0).abs() <= 1.0,
      "apex at step {top_step}"
    );
  }

  #[test]
  fn running_reaches_full_speed_in_its_acceleration_time() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_run(180.0, 0.1, 0.05),
    );
    settle(&mut world, &mut s);
    for _ in 0..6 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!((world.get::<Velocity>(e).unwrap().dx - 180.0).abs() < 1e-3);
  }

  /// Walk right off a platform ending at x = 100; press jump `late` steps
  /// after leaving it. Whether it jumped.
  fn coyote_jump(late: usize) -> bool {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, 110.0), Collider::rect(200.0, 20.0)));
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    let mut since = None;
    for _ in 0..120 {
      intent(&mut world, e).direction = Vec2::X;
      if since == Some(late) {
        intent(&mut world, e).jump_pressed = true;
        intent(&mut world, e).jump_held = true;
      }
      run(&mut world, &mut s, 1);
      let st = world.get::<CharacterState>(e).unwrap();
      if since.is_none() && st.left_floor {
        since = Some(0);
      } else if let Some(n) = since.as_mut() {
        *n += 1;
      }
    }
    events(&world).contains(&MovementEventKind::Jumped)
  }

  #[test]
  fn coyote_time_allows_a_late_jump_within_its_window_only() {
    assert!(coyote_jump(3), "0.05 s after the edge");
    assert!(!coyote_jump(9), "0.15 s after the edge");
  }

  /// Drop onto the floor, pressing jump `early` steps before landing.
  fn buffered_jump(early: usize) -> bool {
    let landing = {
      let (mut world, mut s) = world(60);
      floor(&mut world, 100.0);
      let e = player(&mut world, 0.0, -100.0);
      world.entity_mut(e).insert(PlatformerMovement::default());
      (1..200)
        .find(|_| {
          run(&mut world, &mut s, 1);
          world.get::<CharacterState>(e).unwrap().landed
        })
        .unwrap_or(0)
    };
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = player(&mut world, 0.0, -100.0);
    world.entity_mut(e).insert(PlatformerMovement::default());
    for step in 1..landing + 10 {
      if step + early == landing {
        intent(&mut world, e).jump_pressed = true;
        intent(&mut world, e).jump_held = true;
      }
      run(&mut world, &mut s, 1);
    }
    events(&world).contains(&MovementEventKind::Jumped)
  }

  #[test]
  fn a_jump_pressed_just_before_landing_is_kept() {
    assert!(buffered_jump(3), "0.05 s early");
    assert!(!buffered_jump(12), "0.2 s early");
  }

  #[test]
  fn releasing_jump_early_cuts_it_short() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_jump(64.0, 0.38),
    );
    settle(&mut world, &mut s);
    let start = at(&world, e).translation.y;
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    let mut top = start;
    for i in 0..60 {
      intent(&mut world, e).jump_held = i < 6;
      run(&mut world, &mut s, 1);
      top = top.min(at(&world, e).translation.y);
    }
    assert!(start - top < 0.7 * 64.0, "{}", start - top);
  }

  #[test]
  fn a_jump_says_jumped_then_landed() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    let before = events(&world).len();
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 90);
    let kinds = &events(&world)[before..];
    assert_eq!(
      kinds,
      [MovementEventKind::Jumped, MovementEventKind::Landed]
    );
  }

  #[test]
  fn down_and_jump_on_a_one_way_platform_drops_through_instead() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 105.0),
      Collider::rect(200.0, 10.0).one_way(),
    ));
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    intent(&mut world, e).direction = Vec2::Y;
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(!events(&world).contains(&MovementEventKind::Jumped));
    run(&mut world, &mut s, 30);
    assert!(at(&world, e).translation.y > 120.0, "fell through");
  }

  /// The horizontal speed after one step on the floor with an instant run
  /// of 180 px/s, pressing `direction`.
  fn run_speed_pressing(direction: Vec2) -> f32 {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_run(180.0, 0.0, 0.0),
    );
    settle(&mut world, &mut s);
    intent(&mut world, e).direction = direction;
    run(&mut world, &mut s, 1);
    world.get::<Velocity>(e).unwrap().dx
  }

  #[test]
  fn holding_up_or_down_while_running_keeps_full_speed() {
    // W as both up and jump, or down held while running, is no slower
    let speed = run_speed_pressing(Vec2::new(1.0, -1.0));
    assert!((speed - 180.0).abs() < 1e-3, "right and up: {speed}");
    let speed = run_speed_pressing(Vec2::new(-1.0, 1.0));
    assert!((speed + 180.0).abs() < 1e-3, "left and down: {speed}");
  }

  #[test]
  fn an_analog_stick_runs_at_its_share_of_the_speed() {
    let speed = run_speed_pressing(Vec2::new(0.5, 0.5));
    assert!((speed - 90.0).abs() < 1e-3, "half a stick: {speed}");
    let speed = run_speed_pressing(Vec2::new(0.5, -1.0));
    assert!((speed - 90.0).abs() < 1e-3, "half a stick with up: {speed}");
  }

  #[test]
  fn a_non_finite_horizontal_input_reads_as_zero() {
    assert_eq!(run_speed_pressing(Vec2::new(f32::NAN, -1.0)), 0.0);
    assert_eq!(run_speed_pressing(Vec2::new(f32::INFINITY, 0.0)), 0.0);
  }

  // Review focus 5
  #[test]
  fn an_out_of_range_direction_is_clamped() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    for _ in 0..30 {
      intent(&mut world, e).direction = Vec2::new(5.0, 0.0);
      run(&mut world, &mut s, 1);
    }
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 180.0).abs() < 1e-3, "{dx}");
    let x = at(&world, e).translation.x;
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::new(f32::NAN, 0.0);
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().as_vec2().is_finite());
    assert!(at(&world, e).translation.x.is_finite() && at(&world, e).translation.x >= x);
  }

  #[test]
  fn no_coyote_time_still_jumps_from_the_floor() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_coyote_time(0.0),
    );
    settle(&mut world, &mut s);
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::Jumped));
  }

  // Co-op players block each other: one standing flush against another, or
  // running into it, keeps its footing, so its coyote time, and can jump
  #[test]
  fn a_player_against_another_can_jump() {
    for hold in [false, true] {
      for side in [1.0, -1.0] {
        let (mut world, mut s) = world(60);
        floor(&mut world, 100.0);
        let e = runner(&mut world, PlatformerMovement::default());
        // 20 wide each: their sides touch exactly
        let other = player(&mut world, 20.0 * side, 79.0);
        world
          .entity_mut(other)
          .insert(PlatformerMovement::default());
        let direction = if hold {
          Vec2::new(side, 0.0)
        } else {
          Vec2::ZERO
        };
        for _ in 0..60 {
          intent(&mut world, e).direction = direction;
          run(&mut world, &mut s, 1);
        }
        let st = world.get::<CharacterState>(e).unwrap();
        assert!(
          st.on_floor && st.on_wall == hold,
          "hold {hold}, side {side}: {st:?}"
        );
        let y = at(&world, e).translation.y;
        intent(&mut world, e).direction = direction;
        intent(&mut world, e).jump_pressed = true;
        intent(&mut world, e).jump_held = true;
        run(&mut world, &mut s, 5);
        let messages = world.resource::<Messages<MovementEvent>>();
        let jumped = messages
          .get_cursor()
          .read(messages)
          .any(|m| m.entity == e && m.kind == MovementEventKind::Jumped);
        assert!(jumped, "hold {hold}, side {side}: no jump");
        assert!(
          y - at(&world, e).translation.y > 10.0,
          "hold {hold}, side {side}: rose"
        );
        // Not pushed: at most it keeps its skin from the runner
        let moved = (at(&world, other).translation.x - 20.0 * side) * side;
        assert!(
          (-1e-3..0.5).contains(&moved),
          "hold {hold}, side {side}: the other moved by {moved}"
        );
      }
    }
  }

  // Jumping in place beside another player, flush or within the skin of
  // it, clears its head and comes down on its corner, which the sweep keeps
  // its skin from: it stands there (or slips off) and can jump again, where
  // it hung in the air off the floor and could not
  #[test]
  fn a_player_jumping_in_place_beside_another_can_jump_again() {
    for gap in [0.0, 0.02, 0.1] {
      for side in [1.0, -1.0] {
        let (mut world, mut s) = world(60);
        floor(&mut world, 100.0);
        let e = runner(&mut world, PlatformerMovement::default());
        let other = player(&mut world, (20.0 + gap) * side, 79.0);
        world
          .entity_mut(other)
          .insert(PlatformerMovement::default());
        settle(&mut world, &mut s);
        for jump in 0..2 {
          intent(&mut world, e).jump_pressed = true;
          for i in 0..150 {
            // Held: the full 64 px, over the other's 40
            intent(&mut world, e).jump_held = true;
            let y = at(&world, e).translation.y;
            run(&mut world, &mut s, 1);
            let st = world.get::<CharacterState>(e).unwrap();
            let moved = (at(&world, e).translation.y - y).abs();
            assert!(
              st.on_floor || moved > 1e-3,
              "gap {gap}, side {side}, jump {jump}, step {i}: hanging: {st:?}"
            );
          }
          let st = world.get::<CharacterState>(e).unwrap();
          assert!(st.on_floor, "gap {gap}, side {side}, jump {jump}: {st:?}");
          let dy = world.get::<Velocity>(e).unwrap().dy;
          assert!(dy.abs() < 1e-3, "gap {gap}, side {side}: falling at {dy}");
        }
        let messages = world.resource::<Messages<MovementEvent>>();
        let jumps = messages
          .get_cursor()
          .read(messages)
          .filter(|m| m.entity == e && m.kind == MovementEventKind::Jumped)
          .count();
        assert_eq!(jumps, 2, "gap {gap}, side {side}");
      }
    }
  }

  // Rising past a ledge's top or a neighbour's head, flush against it,
  // brings its corner within the floor probe's reach while the player still
  // rises: that is no landing, and gives no coyote time for a second jump
  #[test]
  fn rising_past_a_ledge_gives_no_second_jump() {
    for neighbour in [false, true] {
      for side in [1.0, -1.0] {
        let (mut world, mut s) = world(60);
        floor(&mut world, 100.0);
        let e = runner(&mut world, PlatformerMovement::default());
        if neighbour {
          let other = player(&mut world, 20.0 * side, 79.0);
          world
            .entity_mut(other)
            .insert(PlatformerMovement::default());
        } else {
          // 30 px high, against the player's side
          world.spawn((
            Transform::from_xy(60.0 * side, 85.0),
            Collider::rect(100.0, 30.0),
          ));
        }
        settle(&mut world, &mut s);
        let before = events(&world).len();
        // Pressing jump all the way up (64 px, past the top)
        for _ in 0..60 {
          intent(&mut world, e).jump_pressed = true;
          intent(&mut world, e).jump_held = true;
          run(&mut world, &mut s, 1);
          if world.get::<Velocity>(e).unwrap().dy >= 0.0 {
            break;
          }
        }
        let messages = world.resource::<Messages<MovementEvent>>();
        let kinds: Vec<MovementEventKind> = messages
          .get_cursor()
          .read(messages)
          .skip(before)
          .filter(|m| m.entity == e)
          .map(|m| m.kind)
          .collect();
        assert_eq!(
          kinds,
          [MovementEventKind::Jumped],
          "neighbour {neighbour}, side {side}"
        );
      }
    }
  }

  #[test]
  fn with_no_air_control_a_character_off_a_ledge_falls_and_keeps_its_speed() {
    let (mut world, mut s) = world(60);
    world.spawn((Transform::from_xy(0.0, 110.0), Collider::rect(200.0, 20.0)));
    let settings = PlatformerMovement::default()
      .with_run(180.0, 0.0, 0.0)
      .with_air_control(0.0);
    let e = runner(&mut world, settings);
    settle(&mut world, &mut s);
    let mut left = false;
    for _ in 0..90 {
      // Run to the edge, then let go: nothing steers in the air
      intent(&mut world, e).direction = if left { Vec2::ZERO } else { Vec2::X };
      run(&mut world, &mut s, 1);
      left |= world.get::<CharacterState>(e).unwrap().left_floor;
    }
    assert!(left, "ran off the edge");
    let v = world.get::<Velocity>(e).unwrap().as_vec2();
    assert!((v.x - 180.0).abs() < 1e-3, "{v}");
    assert!(at(&world, e).translation.y > 150.0, "fell");
  }

  #[test]
  fn a_non_finite_velocity_is_read_as_zero_and_warned_once() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    for _ in 0..3 {
      world.get_mut::<Velocity>(e).unwrap().dx = f32::NAN;
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    let x = at(&world, e).translation.x;
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().as_vec2().is_finite());
    assert!(at(&world, e).translation.x > x + 20.0, "runs");
    let prefix = format!("{e}: PlatformerMovement read a non-finite Velocity");
    let warnings = crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| entry.message.starts_with(&prefix))
      .count();
    assert_eq!(warnings, 1);
  }

  #[test]
  fn a_jump_just_after_dropping_through_does_not_jump() {
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(0.0, 105.0),
      Collider::rect(200.0, 10.0).one_way(),
    ));
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    intent(&mut world, e).direction = Vec2::Y;
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    intent(&mut world, e).direction = Vec2::ZERO;
    run(&mut world, &mut s, 1);
    let y = at(&world, e).translation.y;
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    run(&mut world, &mut s, 1);
    assert!(!events(&world).contains(&MovementEventKind::Jumped));
    assert!(at(&world, e).translation.y > y, "keeps falling");
  }

  /// A wall on the right (its face at x = 60) and a character in the air beside it.
  fn beside_a_wall(settings: PlatformerMovement) -> (World, EngineSchedules, Entity) {
    let (mut world, mut s) = world(60);
    floor(&mut world, 400.0);
    world.spawn((Transform::from_xy(70.0, 0.0), Collider::rect(20.0, 1000.0)));
    let e = player(&mut world, 40.0, 0.0);
    world.entity_mut(e).insert(settings);
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    (world, s, e)
  }

  #[test]
  fn air_jumps_count_down_and_refill_on_landing() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default().with_air_jumps(1));
    settle(&mut world, &mut s);
    for _ in 0..3 {
      intent(&mut world, e).jump_pressed = true;
      run(&mut world, &mut s, 10);
    }
    let kinds = events(&world);
    assert_eq!(
      kinds
        .iter()
        .filter(|k| **k == MovementEventKind::Jumped)
        .count(),
      1
    );
    assert_eq!(
      kinds
        .iter()
        .filter(|k| **k == MovementEventKind::AirJumped)
        .count(),
      1
    );
    run(&mut world, &mut s, 90);
    assert_eq!(world.get::<PlatformerState>(e).unwrap().air_jumps_left, 1);
  }

  #[test]
  fn sliding_down_a_wall_caps_the_fall() {
    let (mut world, mut s, e) = beside_a_wall(PlatformerMovement::default().with_wall_slide(80.0));
    for _ in 0..30 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<PlatformerState>(e).unwrap().wall_sliding);
    assert!(world.get::<Velocity>(e).unwrap().dy <= 80.0 + 1e-3);
  }

  #[test]
  fn a_wall_jump_launches_away_and_holds_the_direction() {
    let (mut world, mut s, e) = beside_a_wall(
      PlatformerMovement::default()
        .with_wall_slide(80.0)
        .with_wall_jump(Vec2::new(300.0, 350.0), 0.2),
    );
    intent(&mut world, e).direction = Vec2::X;
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::WallJumped));
    for _ in 0..6 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
      assert!(
        world.get::<Velocity>(e).unwrap().dx < 0.0,
        "input held off during the lock"
      );
    }
  }

  #[test]
  fn a_dash_is_fast_level_and_limited_in_the_air() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    // Cooldown shorter than the dash, so the second try is limited by the
    // air dash alone, while the character is still in the air
    let e = runner(
      &mut world,
      PlatformerMovement::default().with_dash(600.0, 0.15, 0.1),
    );
    settle(&mut world, &mut s);
    intent(&mut world, e).jump_pressed = true;
    intent(&mut world, e).jump_held = true;
    run(&mut world, &mut s, 5);
    let y = at(&world, e).translation.y;
    intent(&mut world, e).direction = Vec2::X;
    intent(&mut world, e).dash_pressed = true;
    run(&mut world, &mut s, 1);
    assert_eq!(
      world.get::<Velocity>(e).unwrap().as_vec2(),
      Vec2::new(600.0, 0.0)
    );
    run(&mut world, &mut s, 3);
    assert!(
      (at(&world, e).translation.y - y).abs() < 1.0,
      "no gravity while dashing"
    );
    run(&mut world, &mut s, 7);
    assert!(
      !world.get::<CharacterState>(e).unwrap().on_floor,
      "still in the air"
    );
    intent(&mut world, e).dash_pressed = true;
    run(&mut world, &mut s, 1);
    let dashes = events(&world)
      .iter()
      .filter(|k| **k == MovementEventKind::Dashed)
      .count();
    assert_eq!(dashes, 1, "one air dash until landing");
  }

  #[test]
  fn knockback_takes_the_steering_away() {
    let (mut world, mut s) = world(60);
    floor(&mut world, 100.0);
    let e = runner(&mut world, PlatformerMovement::default());
    settle(&mut world, &mut s);
    world
      .entity_mut(e)
      .insert(Knockback::new(Vec2::NEG_X, 200.0).with_decay_rate(1.0));
    for _ in 0..10 {
      intent(&mut world, e).direction = Vec2::X;
      run(&mut world, &mut s, 1);
    }
    assert!(world.get::<Velocity>(e).unwrap().dx.abs() < 1e-3);
  }

  #[test]
  fn no_coyote_time_still_jumps_off_a_wall() {
    let (mut world, mut s, e) = beside_a_wall(
      PlatformerMovement::default()
        .with_coyote_time(0.0)
        .with_wall_jump(Vec2::new(300.0, 350.0), 0.2),
    );
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::WallJumped));
  }

  #[test]
  fn the_wall_jump_hold_keeps_the_facing_and_ends_on_landing() {
    // A hold far longer than the fall, so the character lands inside it
    let (mut world, mut s, e) =
      beside_a_wall(PlatformerMovement::default().with_wall_jump(Vec2::new(100.0, 100.0), 10.0));
    intent(&mut world, e).jump_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::WallJumped));
    run(&mut world, &mut s, 5);
    let state = world.get::<PlatformerState>(e).unwrap();
    assert_eq!(
      state.facing, -1.0,
      "facing away from the wall, input held off"
    );
    assert_eq!(world.get::<Velocity>(e).unwrap().dx, -100.0);
    let landed = (0..120).any(|_| {
      run(&mut world, &mut s, 1);
      world.get::<CharacterState>(e).unwrap().landed
    });
    assert!(landed);
    // Still pressing toward the wall: on the ground it runs there
    run(&mut world, &mut s, 10);
    let dx = world.get::<Velocity>(e).unwrap().dx;
    assert!((dx - 180.0).abs() < 1e-3, "{dx}");
    assert_eq!(world.get::<PlatformerState>(e).unwrap().facing, 1.0);
  }

  #[test]
  fn a_dash_off_a_wall_faces_its_way_and_ends_the_slide() {
    let (mut world, mut s, e) = beside_a_wall(
      PlatformerMovement::default()
        .with_wall_slide(80.0)
        .with_dash(600.0, 0.15, 0.1),
    );
    run(&mut world, &mut s, 2);
    assert!(world.get::<PlatformerState>(e).unwrap().wall_sliding);
    intent(&mut world, e).direction = Vec2::NEG_X;
    intent(&mut world, e).dash_pressed = true;
    run(&mut world, &mut s, 1);
    assert!(events(&world).contains(&MovementEventKind::Dashed));
    let state = world.get::<PlatformerState>(e).unwrap();
    assert!(state.dashing && !state.wall_sliding && !state.dash_ready);
    assert_eq!(state.facing, -1.0);
    assert_eq!(
      world.get::<Velocity>(e).unwrap().as_vec2(),
      Vec2::new(-600.0, 0.0)
    );
  }

  /// A platform 600 wide moving right at `speed` px/s by velocity, its top
  /// at y = 100, and a runner settled on it.
  fn on_moving_platform(
    world: &mut World,
    s: &mut EngineSchedules,
    speed: f32,
  ) -> (Entity, Entity) {
    on_moving_platform_with(world, s, speed, PlatformerMovement::default())
  }

  fn on_moving_platform_with(
    world: &mut World,
    s: &mut EngineSchedules,
    speed: f32,
    settings: PlatformerMovement,
  ) -> (Entity, Entity) {
    use crate::core::physics::RigidBody;
    let platform = world
      .spawn((
        Transform::from_xy(0.0, 110.0),
        RigidBody::kinematic(),
        Collider::rect(600.0, 20.0),
        Velocity::new(speed, 0.0),
      ))
      .id();
    let e = runner(world, settings);
    settle(world, s);
    assert!(world.get::<CharacterState>(e).unwrap().on_floor);
    (platform, e)
  }

  fn offset(world: &World, e: Entity, platform: Entity) -> f32 {
    at(world, e).translation.x - at(world, platform).translation.x
  }

  #[test]
  fn a_jump_off_a_moving_platform_lands_where_it_left() {
    let (mut world, mut s) = world(60);
    let (platform, e) = on_moving_platform(&mut world, &mut s, 120.0);
    let before = offset(&world, e, platform);
    intent(&mut world, e).jump_pressed = true;
    let mut air = 0;
    for _ in 0..90 {
      intent(&mut world, e).jump_held = true;
      run(&mut world, &mut s, 1);
      air += usize::from(!world.get::<CharacterState>(e).unwrap().on_floor);
    }
    assert!(air > 30, "in the air for {air} steps");
    let state = world.get::<CharacterState>(e).unwrap();
    assert!(state.on_floor && state.floor == Some(platform));
    let drift = offset(&world, e, platform) - before;
    assert!(drift.abs() < 3.0, "landed {drift} px from where it left");
  }

  #[test]
  fn steering_off_a_moving_platform_is_relative_to_it() {
    let (mut world, mut s) = world(60);
    let (_, e) = on_moving_platform(&mut world, &mut s, 120.0);
    intent(&mut world, e).jump_pressed = true;
    for _ in 0..20 {
      let mut intent = intent(&mut world, e);
      intent.jump_held = true;
      intent.direction = Vec2::NEG_X;
      run(&mut world, &mut s, 1);
    }
    assert!(!world.get::<CharacterState>(e).unwrap().on_floor);
    let vx = world.get::<Velocity>(e).unwrap().dx;
    assert!((vx - (120.0 - 180.0)).abs() < 1.0, "{vx}");
  }

  #[test]
  fn a_wall_takes_the_momentum_a_jump_kept() {
    let (mut world, mut s) = world(60);
    let (_, e) = on_moving_platform(&mut world, &mut s, 120.0);
    // Hanging over the platform's path 30 px ahead: clear of a character
    // standing on it, in the way of one jumping
    let x = at(&world, e).translation.x + 10.0 + 30.0;
    world.spawn((
      Transform::from_xy(x + 10.0, -100.0),
      Collider::rect(20.0, 260.0),
    ));
    intent(&mut world, e).jump_pressed = true;
    let mut hit = false;
    let mut fastest_after: f32 = 0.0;
    for _ in 0..40 {
      intent(&mut world, e).jump_held = true;
      run(&mut world, &mut s, 1);
      let state = world.get::<CharacterState>(e).unwrap();
      if hit && !state.on_floor {
        fastest_after = fastest_after.max(world.get::<Velocity>(e).unwrap().dx);
      }
      hit |= state.on_wall;
    }
    assert!(hit, "it reached the wall");
    assert!(
      fastest_after < 1.0,
      "pressed on into the wall at {fastest_after} px/s"
    );
  }

  #[test]
  fn a_dash_replaces_the_momentum_a_jump_kept() {
    let (mut world, mut s) = world(60);
    let settings = PlatformerMovement::default().with_dash(400.0, 0.1, 0.5);
    let (_, e) = on_moving_platform_with(&mut world, &mut s, 120.0, settings);
    intent(&mut world, e).jump_pressed = true;
    for _ in 0..10 {
      intent(&mut world, e).jump_held = true;
      run(&mut world, &mut s, 1);
    }
    intent(&mut world, e).dash_pressed = true;
    // Six steps of dash, then ten of falling with no input
    for _ in 0..16 {
      run(&mut world, &mut s, 1);
    }
    assert!(!world.get::<CharacterState>(e).unwrap().on_floor);
    let vx = world.get::<Velocity>(e).unwrap().dx;
    assert!(vx.abs() < 1.0, "{vx}");
  }

  #[test]
  fn reset_keeps_only_the_facing() {
    let mut state = PlatformerState {
      facing: -1.0,
      jumping: true,
      air_jumps_left: 2,
      wall_sliding: true,
      dashing: true,
      dash_ready: false,
      lock: 0.1,
      dash_left: 0.1,
      dash_cooldown: 0.2,
      ..PlatformerState::default()
    };
    state.reset();
    assert_eq!(
      state,
      PlatformerState {
        facing: -1.0,
        ..PlatformerState::default()
      }
    );
    assert!(state.dash_ready);
  }
}
