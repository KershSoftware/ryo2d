//! The game scene headless, single player: players leap, land, ride a moving
//! platform, bounce off the walls and respawn at a checkpoint after a fall,
//! all on the engine's character controller.

use leap_king::components::{LeapState, MovingPlatform, Platform, Player};
use leap_king::scenes::GameScene;
use ryo2d::prelude::*;
use ryo2d::testing::HeadlessEngine;

/// The tower every test climbs: its first checkpoint platform's top is at
/// y 0, and its first moving platform is above it.
const SEED: u64 = 42;

fn engine() -> Option<HeadlessEngine> {
  let scene = GameScene::new_single_player()
    .expect("single player")
    .with_seed(SEED);
  let builder = EngineBuilder::new().pixel_art(1280, 720).start_scene(scene);
  HeadlessEngine::new_or_skip(builder)
}

struct Seen {
  entity: Entity,
  position: Vec2,
  velocity: Vec2,
  state: LeapState,
  on_floor: bool,
  floor: Option<Entity>,
}

fn player(engine: &mut HeadlessEngine) -> Seen {
  let world = engine.world_mut();
  let (entity, transform, velocity, player, state) = world
    .query::<(Entity, &Transform, &Velocity, &Player, &CharacterState)>()
    .single(world)
    .expect("one player");
  Seen {
    entity,
    position: transform.translation,
    velocity: velocity.as_vec2(),
    state: player.leap_state,
    on_floor: state.on_floor,
    floor: state.floor,
  }
}

/// Charge for `frames` with `aim` held, release, then run `after` frames;
/// returns how many times the player started a landing.
fn leap(engine: &mut HeadlessEngine, aim: KeyCode, frames: u32, after: u32) -> u32 {
  engine.press_key(KeyCode::Space);
  engine.press_key(aim);
  engine.step_frames(frames).expect("frames");
  engine.release_key(KeyCode::Space);
  engine.release_key(aim);
  let mut landings = 0;
  let mut was_landing = false;
  for _ in 0..after {
    engine.step().expect("frame");
    let landing = matches!(player(engine).state, LeapState::Landing { .. });
    landings += u32::from(landing && !was_landing);
    was_landing = landing;
  }
  landings
}

/// Put the player in the air at `at`, moving at `velocity`, as if it had
/// leapt there. The velocity rises a little, so the controller's report from
/// where it stood cannot read as a landing before its next move.
fn throw(engine: &mut HeadlessEngine, at: Vec2, velocity: Vec2) {
  let entity = player(engine).entity;
  let world = engine.world_mut();
  world.entity_mut(entity).insert((
    Transform::from_xy(at.x, at.y),
    Velocity::new(velocity.x, velocity.y.min(-1.0)),
  ));
  let mut player = world.get_mut::<Player>(entity).expect("player");
  player.grounded = false;
  player.leap_state = LeapState::Airborne;
}

fn platforms(engine: &mut HeadlessEngine) -> Vec<(Entity, Vec2, Platform, bool)> {
  let world = engine.world_mut();
  let mut platforms: Vec<_> = world
    .query::<(Entity, &Transform, &Platform, Option<&MovingPlatform>)>()
    .iter(world)
    .map(|(e, t, p, moving)| (e, t.translation, *p, moving.is_some()))
    .collect();
  // Lowest first
  platforms.sort_by(|a, b| b.1.y.total_cmp(&a.1.y));
  platforms
}

#[test]
fn the_player_leaps_and_lands() {
  let Some(mut engine) = engine() else {
    return;
  };
  // Past the start-up input delay, standing on the spawn platform
  engine.step_frames(20).expect("frames");
  let start = player(&mut engine);
  assert!(
    start.on_floor,
    "on the spawn platform at {}",
    start.position
  );

  // A leap right (KeyA pulls the slingshot left) lands further along it
  assert_eq!(leap(&mut engine, KeyCode::KeyA, 30, 120), 1, "one landing");
  let right = player(&mut engine);
  assert!(right.on_floor, "landed at {}", right.position);
  assert!(right.position.x > start.position.x + 100.0);
  assert!((right.position.y - start.position.y).abs() < 1.0);

  // The brief's leap: Space and KeyD for 30 frames, back onto the platform
  assert_eq!(leap(&mut engine, KeyCode::KeyD, 30, 120), 1, "one landing");
  let back = player(&mut engine);
  assert!(back.on_floor, "landed at {}", back.position);
  assert!(
    (back.position.x - start.position.x).abs() < 5.0,
    "{}",
    back.position
  );
  assert!(
    (back.position.y - start.position.y).abs() < 1.0,
    "on the starting platform at {}, from {}",
    back.position,
    start.position
  );
  assert_eq!(back.state, LeapState::Idle);
}

#[test]
fn a_moving_platform_carries_the_player() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(5).expect("frames");
  // Climbing makes the tower: go up far enough for a moving platform (it
  // takes up its swing the step after it is made)
  throw(&mut engine, Vec2::new(20.0, -1500.0), Vec2::ZERO);
  engine.step_frames(2).expect("frames");
  let (platform, at, size, _) = platforms(&mut engine)
    .into_iter()
    .find(|p| p.3)
    .expect("a moving platform");

  // Drop onto its middle
  throw(
    &mut engine,
    Vec2::new(at.x + size.width / 2.0 - 12.0, at.y - 60.0),
    Vec2::ZERO,
  );
  engine.step_frames(40).expect("frames");
  let landed = player(&mut engine);
  assert!(landed.on_floor, "landed at {}", landed.position);
  assert_eq!(landed.floor, Some(platform));

  let offset = |engine: &mut HeadlessEngine| {
    let platform_x = engine
      .world()
      .get::<Transform>(platform)
      .expect("platform")
      .translation
      .x;
    (player(engine).position.x - platform_x, platform_x)
  };
  let (before, from) = offset(&mut engine);
  engine.step_frames(60).expect("frames");
  let (after, to) = offset(&mut engine);
  assert!(
    (to - from).abs() > 10.0,
    "the platform moved {from} -> {to}"
  );
  assert!(
    (after - before).abs() < 0.5,
    "rode it: offset {before} -> {after}"
  );
  assert!(player(&mut engine).on_floor);
}

#[test]
fn the_player_bounces_off_the_walls() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(5).expect("frames");
  for (x, vx) in [(60.0, -500.0), (1196.0, 500.0)] {
    throw(&mut engine, Vec2::new(x, 300.0), Vec2::new(vx, -300.0));
    engine.step_frames(12).expect("frames");
    let seen = player(&mut engine);
    assert!(
      (0.0..=1280.0 - 24.0).contains(&seen.position.x),
      "stayed on screen at {}",
      seen.position
    );
    assert!(
      (seen.velocity.x + vx * 0.4).abs() < 1e-3,
      "bounced back at 40%: {}",
      seen.velocity
    );
  }
}

#[test]
fn a_fall_respawns_the_player_at_its_checkpoint() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(5).expect("frames");
  let (_, checkpoint, size, _) = platforms(&mut engine)
    .into_iter()
    .find(|p| p.2.is_checkpoint)
    .expect("a checkpoint platform");

  // Onto the checkpoint platform: its zone moves the respawn point there
  throw(
    &mut engine,
    Vec2::new(checkpoint.x + size.width - 40.0, checkpoint.y - 80.0),
    Vec2::ZERO,
  );
  engine.step_frames(40).expect("frames");
  assert!(player(&mut engine).on_floor);

  // Off its edge and down past the death line
  throw(
    &mut engine,
    Vec2::new(checkpoint.x + size.width + 30.0, checkpoint.y - 40.0),
    Vec2::new(100.0, 0.0),
  );
  engine.step_frames(150).expect("frames");

  // Back at slot 0's spot on the checkpoint platform, standing, and the
  // respawn's teleport does not read as a landing
  let spot = Vec2::new(checkpoint.x + 12.0, checkpoint.y - 32.0);
  for _ in 0..30 {
    engine.step().expect("frame");
    let seen = player(&mut engine);
    assert!(
      (seen.position - spot).length() < 1.0,
      "respawned at {}, not {spot}",
      seen.position
    );
    assert_eq!(seen.state, LeapState::Idle);
  }
  assert!(player(&mut engine).on_floor);
}

/// Slot 0's spot on a checkpoint platform whose top-left is `platform`.
fn spot(platform: Vec2) -> Vec2 {
  Vec2::new(platform.x + 12.0, platform.y - 32.0)
}

/// Respawn now (the R key) and say where the player came back.
fn respawn_now(engine: &mut HeadlessEngine) -> Vec2 {
  engine.tap_key(KeyCode::KeyR);
  engine.step().expect("frame");
  player(engine).position
}

#[test]
fn climbing_past_a_checkpoint_beside_its_platform_reaches_it() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(20).expect("frames");
  let all = platforms(&mut engine);
  let (_, checkpoint, size, _) = *all
    .iter()
    .find(|p| p.2.is_checkpoint)
    .expect("a checkpoint platform");
  // A platform a little above the checkpoint's height, clear of its sides
  let (side, at, side_size, _) = *all
    .iter()
    .find(|(_, at, p, moving)| {
      !p.is_checkpoint
        && !moving
        && (checkpoint.y - 250.0..checkpoint.y - 60.0).contains(&at.y)
        && (at.x + p.width < checkpoint.x || at.x > checkpoint.x + size.width)
    })
    .expect("a side platform above the checkpoint's height");

  // Leap from below the checkpoint's height up onto the side platform
  let from = Vec2::new(at.x + side_size.width / 2.0 - 12.0, checkpoint.y + 40.0);
  let rise = from.y + 32.0 - at.y + 20.0;
  throw(
    &mut engine,
    from,
    Vec2::new(0.0, -(2.0 * leap_king::constants::GRAVITY * rise).sqrt()),
  );
  engine.step_frames(90).expect("frames");
  let landed = player(&mut engine);
  assert!(
    landed.on_floor && landed.floor == Some(side),
    "on the side platform: {}",
    landed.position
  );

  let back = respawn_now(&mut engine);
  assert!(
    (back - spot(checkpoint)).length() < 1.0,
    "the checkpoint was reached: respawned at {back}, not {}",
    spot(checkpoint)
  );
}

#[test]
fn falling_through_a_lower_checkpoint_keeps_the_higher_one() {
  let Some(mut engine) = engine() else {
    return;
  };
  engine.step_frames(5).expect("frames");
  // Climb (by teleport) so the second checkpoint platform exists
  throw(&mut engine, Vec2::new(20.0, -1700.0), Vec2::ZERO);
  engine.step_frames(2).expect("frames");
  let checkpoints: Vec<Vec2> = platforms(&mut engine)
    .into_iter()
    .filter(|p| p.2.is_checkpoint)
    .map(|p| p.1)
    .collect();
  let (lower, higher) = (checkpoints[0], checkpoints[1]);

  // Reach the higher one
  throw(
    &mut engine,
    Vec2::new(higher.x + 150.0, higher.y - 80.0),
    Vec2::ZERO,
  );
  engine.step_frames(40).expect("frames");
  assert!(player(&mut engine).on_floor);

  // Fall through the lower one's zone, away from its platform, and respawn
  // on the step after its zone moved the respawn point down
  throw(&mut engine, Vec2::new(1100.0, lower.y - 120.0), Vec2::ZERO);
  let entity = player(&mut engine).entity;
  let mut entered = false;
  for _ in 0..60 {
    engine.step().expect("frame");
    let point = engine
      .world()
      .get::<Respawn>(entity)
      .expect("respawn")
      .point;
    if (point - spot(lower)).length() < 1.0 {
      entered = true;
      break;
    }
  }
  assert!(entered, "fell through the lower checkpoint's zone");
  let back = respawn_now(&mut engine);
  assert!(
    (back - spot(higher)).length() < 1.0,
    "respawned at the higher checkpoint {}, not {back}",
    spot(higher)
  );

  // And a fall past the death line comes back there too
  throw(&mut engine, Vec2::new(1100.0, lower.y - 120.0), Vec2::ZERO);
  engine.step_frames(120).expect("frames");
  let back = player(&mut engine).position;
  assert!(
    (back - spot(higher)).length() < 1.0,
    "after the fall: {back}, not {}",
    spot(higher)
  );
}
