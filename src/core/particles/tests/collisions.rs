//! Particles meeting the world's colliders on the engine's step: every
//! response, the filters, the events and a tile map's edges.

use glam::Vec2;

use crate::core::ecs::kernel::Disabled;
use crate::core::ecs::{ChildOf, Entity, GlobalTransform, Transform, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::collision::{CollisionConfig, CollisionResponse, ParticleCollision};
use crate::core::particles::contact::{CONTACT_SKIN, ColliderGeometry, Contact, ParticleCollider};
use crate::core::particles::effect::{Burst, ParticleEffect};
use crate::core::particles::grid::{Candidates, ColliderGrid};
use crate::core::particles::rng::ParticleRng;
use crate::core::particles::testing::{
  self, Seen, collisions, count, edit, frame, particles, step, steps,
};
use crate::core::physics::{Collider, TileColliders, TilePart};
use crate::core::time::Time;

const DT: f32 = 1.0 / 60.0;
const RADIUS: f32 = 2.0;

/// One motionless particle living 60 s, burst once by an emitter at `at`
/// whose one cycle then ends, falling under `gravity_y` and colliding by
/// `config`
fn faller(world: &mut World, config: CollisionConfig, at: Vec2, gravity_y: f32) -> Entity {
  let effect = ParticleEffect::new(testing::image(world))
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(60.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 1.0))
    .with_gravity(0.0, gravity_y)
    .with_collision(config);
  let effect = testing::effect(world, effect);
  world
    .spawn((
      Transform::from_translation(at),
      ParticleEmitter::new(effect),
    ))
    .id()
}

/// One motionless particle at (100, 0) falling under gravity onto a collider below it.
///
/// The collider at (100, 60) is centred under the particle, as colliders are centred on their
/// entity's origin.
struct FallScene {
  world: World,
  emitter: Entity,
  collider: Entity,
  /// Top edge of the collider's world bounds
  top: f32,
  /// Bottom edge of the collider's world bounds
  bottom: f32,
}

impl FallScene {
  fn new(config: CollisionConfig, collider: Collider) -> Self {
    Self::with_motion(config, collider, 600.0, DT)
  }

  /// A fixed step `dt` long
  fn with_motion(config: CollisionConfig, collider: Collider, gravity_y: f32, dt: f32) -> Self {
    let mut world = testing::world();
    world.insert_resource(Time::new((1.0 / dt).round() as u32));
    let emitter = faller(&mut world, config, Vec2::new(100.0, 0.0), gravity_y);
    let transform = Transform::from_xy(100.0, 60.0);
    let (min, max) = collider.world_bounds(&GlobalTransform::from_transform(&transform));
    let collider = world.spawn((transform, collider)).id();
    Self {
      world,
      emitter,
      collider,
      top: min.y,
      bottom: max.y,
    }
  }

  /// A 200x20 floor centred under the particle.
  fn floor(config: CollisionConfig) -> Self {
    Self::new(config, Collider::rect(200.0, 20.0))
  }

  fn step(&mut self) {
    step(&mut self.world);
  }

  fn particle(&self) -> Option<Seen> {
    particles(&self.world, self.emitter).first().copied()
  }

  fn particle_count(&self) -> usize {
    count(&self.world, self.emitter)
  }

  fn edit(&mut self, f: impl FnOnce(&mut crate::core::particles::pool::Pool)) {
    edit(&mut self.world, self.emitter, f);
  }

  /// Every collision written so far: these tests never advance the buffers.
  fn collisions(&self) -> Vec<ParticleCollision> {
    collisions(&self.world)
  }
}

// Responses

#[test]
fn die_on_hit_removes_particle_at_floor() {
  let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
  scene.step();
  assert_eq!(scene.particle_count(), 1);

  let mut last_y = 0.0;
  for _ in 0..60 {
    scene.step();
    if let Some(p) = scene.particle() {
      last_y = p.y;
    }
  }

  assert_eq!(
    scene.particle_count(),
    0,
    "particle should die on the floor"
  );
  assert!(
    last_y < scene.top,
    "particle died at y={last_y}, past the floor top {}",
    scene.top
  );
}

#[test]
fn bounce_reflects_off_floor() {
  let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5));
  let rest_limit = scene.top - RADIUS + 1e-3;

  let mut bounced = false;
  for step in 0..240 {
    scene.step();
    let p = scene.particle().expect("bouncing particle stays alive");
    assert!(
      p.y <= rest_limit,
      "step {step}: y={} sank past {rest_limit}",
      p.y
    );
    bounced |= p.vy < 0.0;
  }

  assert!(bounced, "velocity should reverse off the floor");
}

#[test]
fn stick_freezes_particle() {
  let mut scene = FallScene::floor(CollisionConfig::sticky(RADIUS));
  let contact_y = scene.top - RADIUS;
  for _ in 0..60 {
    scene.step();
    if scene.particle().is_some_and(|p| p.stuck) {
      break;
    }
  }
  let p = scene.particle().expect("particle reached the floor");
  assert!(p.stuck, "particle never reached the floor");
  // on the floor, its radius above it
  assert!((p.y - contact_y).abs() < 1e-3, "stuck at y = {}", p.y);
  let (stuck_x, stuck_y) = (p.x, p.y);

  for _ in 0..30 {
    scene.step();
  }

  let p = scene.particle().expect("stuck particle stays alive");
  assert_eq!((p.x, p.y), (stuck_x, stuck_y));
  assert_eq!((p.vx, p.vy), (0.0, 0.0));
}

#[test]
fn event_response_passes_through_and_notifies_once() {
  let config = CollisionConfig::new(RADIUS).with_response(CollisionResponse::Event);
  let mut scene = FallScene::floor(config);
  for _ in 0..120 {
    scene.step();
  }

  let p = scene.particle().expect("pass-through particle stays alive");
  assert!(
    p.y > scene.bottom + RADIUS,
    "particle at y={} should fall through",
    p.y
  );
  let collisions = scene.collisions();
  assert_eq!(collisions.len(), 1, "one event per entry");
  let hit = &collisions[0];
  assert_eq!(hit.collider_entity, scene.collider);
  assert_eq!(hit.emitter_entity, scene.emitter);
  assert_eq!((hit.normal_x, hit.normal_y), (0.0, -1.0));
  assert!(
    hit.velocity_y > 0.0,
    "velocity before the response, moving down"
  );
}

#[test]
fn with_events_reports_die() {
  let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS).with_events());
  for _ in 0..60 {
    scene.step();
  }
  assert_eq!(scene.particle_count(), 0);
  let collisions = scene.collisions();
  assert_eq!(collisions.len(), 1);
  assert_eq!(collisions[0].collider_entity, scene.collider);

  let mut silent = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
  for _ in 0..60 {
    silent.step();
  }
  assert_eq!(silent.particle_count(), 0);
  assert!(silent.collisions().is_empty(), "events are opt-in");
}

#[test]
fn with_events_reports_stick_once() {
  let mut scene = FallScene::floor(CollisionConfig::sticky(RADIUS).with_events());
  for _ in 0..120 {
    scene.step();
  }
  assert_eq!(scene.particle_count(), 1);
  assert_eq!(scene.collisions().len(), 1);
}

#[test]
fn bounce_off_circle_collider() {
  // Centred 10px left of the particle, so it lands on the circle's right shoulder
  let circle = Collider::circle(20.0).with_offset(-10.0, 0.0);
  let mut scene = FallScene::new(CollisionConfig::bouncy(RADIUS, 0.5), circle);

  let mut bounce_velocity = None;
  for _ in 0..60 {
    scene.step();
    let p = scene.particle().expect("bouncing particle stays alive");
    if p.vy < 0.0 {
      bounce_velocity = Some((p.vx, p.vy));
      break;
    }
  }

  let (vx, _) = bounce_velocity.expect("velocity should reverse off the circle");
  assert!(
    vx > 0.0,
    "the curved surface deflects the particle away from its centre"
  );
}

#[test]
fn a_stuck_particle_still_ages_and_dies() {
  let mut scene = FallScene::floor(CollisionConfig::sticky(RADIUS));
  for _ in 0..60 {
    scene.step();
  }
  let p = scene.particle().expect("particle reached the floor");
  assert!(p.stuck);
  let age = p.age;
  scene.step();
  let p = scene.particle().expect("still there");
  assert!(
    (p.age - age - DT).abs() < 1e-5,
    "it aged: {age} to {}",
    p.age
  );
  // Its 60 s run out
  steps(&mut scene.world, 60 * 60);
  assert_eq!(scene.particle_count(), 0, "it died at its lifetime");
}

// Filters

#[test]
fn collision_mask_filters_colliders() {
  let floor = || Collider::rect(200.0, 20.0).with_layer(0b10);

  let mut masked = FallScene::new(CollisionConfig::die_on_hit(RADIUS).with_mask(0b01), floor());
  for _ in 0..120 {
    masked.step();
  }
  let p = masked
    .particle()
    .expect("a masked-out floor does not kill the particle");
  assert!(
    p.y > masked.bottom + RADIUS,
    "particle at y={} should fall through",
    p.y
  );

  let mut matching = FallScene::new(CollisionConfig::die_on_hit(RADIUS).with_mask(0b10), floor());
  for _ in 0..120 {
    matching.step();
  }
  assert_eq!(
    matching.particle_count(),
    0,
    "a matching mask still collides"
  );
}

#[test]
fn trigger_colliders_are_ignored() {
  let trigger = Collider::rect(200.0, 20.0).as_trigger();
  let mut scene = FallScene::new(CollisionConfig::die_on_hit(RADIUS), trigger);
  for _ in 0..120 {
    scene.step();
  }
  let p = scene
    .particle()
    .expect("a trigger does not kill the particle");
  assert!(
    p.y > scene.bottom + RADIUS,
    "particle at y={} should fall through",
    p.y
  );
}

#[test]
fn disabled_colliders_are_ignored() {
  let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
  scene.world.entity_mut(scene.collider).insert(Disabled);
  for _ in 0..120 {
    scene.step();
  }
  let p = scene
    .particle()
    .expect("a disabled floor does not kill the particle");
  assert!(
    p.y > scene.bottom + RADIUS,
    "particle at y={} should fall through",
    p.y
  );
}

#[test]
fn negatively_scaled_collider_still_collides() {
  let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
  if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.collider) {
    transform.scale.x = -1.0;
  }
  for _ in 0..60 {
    scene.step();
  }
  assert_eq!(
    scene.particle_count(),
    0,
    "inverted bounds should still collide"
  );
}

#[test]
fn particles_ignore_their_own_emitters_collider() {
  let mut scene = FallScene::floor(CollisionConfig::die_on_hit(RADIUS));
  // A 20x20 box centred on the emitter, so particles spawn inside it
  let own_box = Collider::rect(20.0, 20.0);
  let own_bottom = scene
    .world
    .get::<Transform>(scene.emitter)
    .map(|transform| {
      own_box
        .world_bounds(&GlobalTransform::from_transform(transform))
        .1
        .y
    })
    .expect("emitter has a transform");
  scene.world.entity_mut(scene.emitter).insert(own_box);

  scene.step();
  assert_eq!(scene.particle_count(), 1, "own collider must not kill it");

  let mut last_y = 0.0;
  for _ in 0..60 {
    scene.step();
    if let Some(p) = scene.particle() {
      last_y = p.y;
    }
  }
  assert_eq!(scene.particle_count(), 0, "the floor still kills it");
  assert!(
    last_y > own_bottom && last_y < scene.top,
    "particle should die at the floor, last seen at y={last_y}"
  );
}

// Events

#[test]
fn a_corner_hit_sends_one_bounce_event() {
  let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
  let mut scene = FallScene::with_motion(config, Collider::rect(200.0, 20.0), 0.0, DT);
  // A wall rising from the floor, its left face at x = 150
  scene
    .world
    .spawn((Transform::from_xy(160.0, 50.0), Collider::rect(20.0, 200.0)));
  scene.step();
  // Heading into the corner fast enough to touch floor and wall in one update
  let top = scene.top;
  scene.edit(|p| {
    p.x[0] = 147.0;
    p.y[0] = top - 3.0;
    p.vx[0] = 120.0;
    p.vy[0] = 120.0;
  });

  scene.step();

  assert_eq!(scene.collisions().len(), 1);
}

#[test]
fn settling_bounce_does_not_flood_events() {
  let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5).with_events());
  for _ in 0..600 {
    scene.step();
  }
  assert!(
    scene.particle().is_some(),
    "particle should still be resting on the floor"
  );
  let count = scene.collisions().len();
  assert!(
    (1..10).contains(&count),
    "expected a few impact events, got {count}"
  );
}

#[test]
fn settling_bounce_does_not_flood_events_under_high_gravity() {
  let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
  // ~17px per step: a 200px-thick floor (top still at 50) keeps deep first contacts from
  // resolving through the bottom face
  let floor = Collider::rect(200.0, 200.0).with_offset(0.0, 90.0);
  let mut scene = FallScene::with_motion(config, floor, 3000.0, 1.0 / 30.0);
  assert_eq!((scene.top, scene.bottom), (50.0, 250.0));
  for _ in 0..600 {
    scene.step();
  }
  let p = scene.particle().expect("particle should still be alive");
  assert!(
    p.y <= scene.top - RADIUS + 1e-3,
    "particle at y={} should rest on the floor",
    p.y
  );
  let count = scene.collisions().len();
  assert!(
    (1..10).contains(&count),
    "expected a few impact events, got {count}"
  );
}

#[test]
fn slow_bounce_emits_no_event() {
  let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
  let floor = Collider::rect(200.0, 20.0);
  let mut scene = FallScene::with_motion(config, floor, 0.0, DT);
  // 8px above contact, drifting down well under MIN_BOUNCE_EVENT_SPEED
  let rest = scene.top - RADIUS - 8.0;
  if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.emitter) {
    transform.translation.y = rest;
  }
  scene.step();
  scene.edit(|p| p.vy[0] = 10.0);

  let mut bounced = false;
  for _ in 0..120 {
    scene.step();
    let p = scene.particle().expect("bouncing particle stays alive");
    bounced |= p.vy < 0.0;
  }
  assert!(bounced, "velocity_y should flip off the floor");
  assert!(
    scene.collisions().is_empty(),
    "a 10 px/s impact is below MIN_BOUNCE_EVENT_SPEED"
  );
}

#[test]
fn sliding_particle_bounces_off_wall_gathered_after_floor() {
  let mut scene = FallScene::floor(CollisionConfig::bouncy(RADIUS, 0.5));
  // Start resting on the floor (x 0..200), then slide right into a wall spawned after it
  let rest_y = scene.top - RADIUS - CONTACT_SKIN;
  if let Some(mut transform) = scene.world.get_mut::<Transform>(scene.emitter) {
    transform.translation.y = rest_y;
  }
  let wall = Collider::rect(20.0, 100.0);
  let wall_transform = Transform::from_xy(160.0, scene.top);
  let wall_face = wall
    .world_bounds(&GlobalTransform::from_transform(&wall_transform))
    .0
    .x;
  scene.world.spawn((wall_transform, wall));

  scene.step();
  scene.edit(|p| p.vx[0] = 120.0);

  let face_limit = wall_face - RADIUS + 1e-3;
  let mut bounced = false;
  for step in 0..120 {
    scene.step();
    let p = scene.particle().expect("bouncing particle stays alive");
    assert!(
      p.x <= face_limit,
      "step {step}: x={} went past the wall face at {wall_face}",
      p.x
    );
    bounced |= p.vx < 0.0;
  }
  assert!(bounced, "velocity_x should flip off the wall");
}

// Tile edges

/// A tile part as a map packs a chunk's outlines: a floor (x 0 to 400,
/// y 80 to 100) and a block far off (x 360 to 400, y 0 to 20), two loops
/// in one collider at the origin, whose box (0, 0 to 400, 100) is mostly
/// air. One particle from an emitter at `at` under `gravity_y`
fn tile_part_scene(config: CollisionConfig, at: Vec2, gravity_y: f32) -> (World, Entity) {
  let corners = |x0: f32, y0: f32, x1: f32, y1: f32| {
    [
      Vec2::new(x0, y0),
      Vec2::new(x1, y0),
      Vec2::new(x1, y1),
      Vec2::new(x0, y1),
    ]
  };
  let mut points = corners(0.0, 80.0, 400.0, 100.0).to_vec();
  points.extend(corners(360.0, 0.0, 400.0, 20.0));
  let ring = |base: u32| (0..4).map(move |i| [base + i, base + (i + 1) % 4]);
  let segments: Vec<[u32; 2]> = ring(0).chain(ring(4)).collect();
  parts_scene(
    vec![Collider::segments(points, segments)],
    true,
    (config, at, gravity_y),
  )
}

/// `parts` at the origin and one particle from an emitter at `at` under
/// `gravity_y`, after the step it spawns in. As a map's tile parts
/// (`tile_parts`), they are children of an entity holding the grid they
/// were built from, of 20 px cells, as physics spawns them; otherwise a
/// game's own colliders
fn parts_scene(
  parts: Vec<Collider>,
  tile_parts: bool,
  (config, at, gravity_y): (CollisionConfig, Vec2, f32),
) -> (World, Entity) {
  let mut world = testing::world();
  let grid = world
    .spawn((
      Transform::IDENTITY,
      TileColliders::new(20, 20, Vec2::splat(20.0)),
    ))
    .id();
  for part in parts {
    if tile_parts {
      world.spawn((Transform::IDENTITY, part, TilePart, ChildOf(grid)));
    } else {
      world.spawn((Transform::IDENTITY, part));
    }
  }
  let emitter = faller(&mut world, config, at, gravity_y);
  step(&mut world);
  (world, emitter)
}

/// The particle of `emitter`, its only one
fn only(world: &World, emitter: Entity) -> Seen {
  let seen = particles(world, emitter);
  assert_eq!(seen.len(), 1, "one particle");
  seen[0]
}

/// A closed polyline round the rectangle (x0, y0) to (x1, y1): clockwise
/// on screen, as a tile map winds the loop round a solid, or the other way,
/// as it winds a hole's
fn closed_loop(x0: f32, y0: f32, x1: f32, y1: f32, clockwise: bool) -> Collider {
  let mut points = vec![
    Vec2::new(x0, y0),
    Vec2::new(x1, y0),
    Vec2::new(x1, y1),
    Vec2::new(x0, y1),
  ];
  if !clockwise {
    points.reverse();
  }
  points.push(points[0]);
  Collider::polyline(points)
}

#[test]
fn a_particle_in_a_room_whose_wall_s_outline_lies_in_another_part_drifts_untouched() {
  // A solid mass 200 px square whose outer loop is one part, with a room
  // (60 to 140) carved in it whose loop is another part, as two chunks
  // pack them: a particle drifting in the room meets nothing
  let mass = closed_loop(0.0, 0.0, 200.0, 200.0, true);
  let room = closed_loop(60.0, 60.0, 140.0, 140.0, false);
  let config = CollisionConfig::bouncy(RADIUS, 0.5);
  let (mut world, emitter) = parts_scene(
    vec![mass, room],
    true,
    (config, Vec2::new(90.0, 100.0), 0.0),
  );
  edit(&mut world, emitter, |p| p.vx[0] = 60.0);
  for step_n in 0..30 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(
      (p.y - 100.0).abs() < 1e-3 && p.vx == 60.0 && !p.touching,
      "step {step_n}: at ({}, {}), velocity {:?}",
      p.x,
      p.y,
      (p.vx, p.vy)
    );
  }
}

#[test]
fn a_particle_in_a_room_alone_in_its_part_bounces_off_its_walls() {
  // The room's loop alone in its part, a closed polyline wound as a
  // hole's: the particle bounces off the walls from inside, and stays in
  let room = closed_loop(60.0, 60.0, 140.0, 140.0, false);
  let config = CollisionConfig::bouncy(RADIUS, 1.0);
  let (mut world, emitter) = parts_scene(vec![room], true, (config, Vec2::new(100.0, 100.0), 0.0));
  edit(&mut world, emitter, |p| p.vx[0] = 300.0);
  let mut bounced = false;
  for step_n in 0..120 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(
      p.x > 60.0 + RADIUS - 1e-3 && p.x < 140.0 - RADIUS + 1e-3,
      "step {step_n}: x = {}",
      p.x
    );
    bounced |= p.vx < 0.0;
  }
  assert!(bounced, "it bounced off the wall");
}

#[test]
fn particles_stay_in_a_game_s_closed_polyline_arena_and_bounce() {
  // A game's arena wall: a closed polyline, hollow as physics treats it
  let arena = closed_loop(0.0, 0.0, 200.0, 200.0, true);
  let config = CollisionConfig::bouncy(RADIUS, 1.0);
  let (mut world, emitter) =
    parts_scene(vec![arena], false, (config, Vec2::new(100.0, 100.0), 0.0));
  edit(&mut world, emitter, |p| p.vx[0] = 400.0);
  let mut bounced = false;
  for step_n in 0..120 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(
      p.x > RADIUS - 1e-3 && p.x < 200.0 - RADIUS + 1e-3 && (p.y - 100.0).abs() < 1e-3,
      "step {step_n}: at ({}, {})",
      p.x,
      p.y
    );
    bounced |= p.vx < 0.0;
  }
  assert!(bounced, "it bounced off the wall");
}

#[test]
fn particles_meet_a_tile_part_s_edges_not_its_box() {
  // Drifting through the air inside the part's box: untouched
  let config = CollisionConfig::bouncy(RADIUS, 0.5).with_events();
  let (mut world, emitter) = tile_part_scene(config.clone(), Vec2::new(100.0, 40.0), 0.0);
  edit(&mut world, emitter, |p| p.vx[0] = 120.0);
  for step_n in 0..60 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(
      (p.y - 40.0).abs() < 1e-3 && p.vx == 120.0 && !p.touching,
      "step {step_n}: at ({}, {}), velocity {:?}",
      p.x,
      p.y,
      (p.vx, p.vy)
    );
  }
  assert!(only(&world, emitter).x > 200.0, "it drifted on");
  assert!(collisions(&world).is_empty(), "nothing was told");

  // Falling onto the floor: it bounces off its top edge, and never sinks
  // into the floor
  let (mut world, emitter) = tile_part_scene(config, Vec2::new(100.0, 40.0), 600.0);
  let mut bounced = false;
  for step_n in 0..240 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(p.y <= 80.0 - RADIUS + 1e-3, "step {step_n}: y = {}", p.y);
    bounced |= p.vy < 0.0;
  }
  assert!(bounced, "it bounced off the floor");
  let first = collisions(&world).first().cloned();
  let first = first.expect("the bounce was told");
  assert_eq!((first.normal_x, first.normal_y), (0.0, -1.0));
}

#[test]
fn a_raised_floor_s_edge_is_no_surface_for_particles() {
  // The floor's grid made a tile map storey's edge, no surface: a
  // particle falls through it
  let config = CollisionConfig::bouncy(RADIUS, 0.5);
  let (mut world, emitter) = tile_part_scene(config, Vec2::new(100.0, 40.0), 600.0);
  for mut grid in world.query::<&mut TileColliders>().iter_mut(&mut world) {
    grid.surface = false;
  }
  steps(&mut world, 60);
  let p = only(&world, emitter);
  assert!(p.y > 100.0 && !p.touching, "it fell through: y = {}", p.y);
}

#[test]
fn a_particle_crossing_an_edge_within_an_update_still_bounces() {
  // 30 px an update, through the floor's 20 px: it is never within its
  // radius of an edge, so only the edge it crossed can stop it
  let config = CollisionConfig::bouncy(RADIUS, 0.5);
  let (mut world, emitter) = tile_part_scene(config, Vec2::new(100.0, 50.0), 0.0);
  edit(&mut world, emitter, |p| p.vy[0] = 1800.0);
  for step_n in 0..20 {
    step(&mut world);
    let p = only(&world, emitter);
    assert!(p.y <= 80.0 - RADIUS + 1e-3, "step {step_n}: y = {}", p.y);
  }
  assert!(only(&world, emitter).vy < 0.0, "it bounced up");
}

#[test]
fn a_fast_stick_particle_sticks_on_the_surface_it_crossed() {
  // 30 px an update through the floor's top: it sticks where it crossed,
  // its radius out, not where its move ended inside the floor
  let (mut world, emitter) =
    tile_part_scene(CollisionConfig::sticky(RADIUS), Vec2::new(100.0, 50.0), 0.0);
  edit(&mut world, emitter, |p| p.vy[0] = 1800.0);
  steps(&mut world, 4);
  let p = only(&world, emitter);
  assert!(p.stuck, "it stuck");
  assert!((p.y - (80.0 - RADIUS)).abs() < 1e-3, "y = {}", p.y);
}

#[test]
fn a_particle_starting_just_inside_tile_ground_ends_on_top_of_it() {
  // Spawned 4 px under the floor's top (as landing dust around the feet
  // is), nearer the top than the bottom: out through the top, where it
  // lands and rests
  let config = CollisionConfig::bouncy(RADIUS, 0.3);
  let (mut world, emitter) = tile_part_scene(config, Vec2::new(100.0, 84.0), 600.0);
  steps(&mut world, 240);
  let p = only(&world, emitter);
  assert!(
    p.y <= 80.0 - RADIUS + 1e-3 && p.y > 70.0,
    "it rests on the floor's top, not inside it: y = {}",
    p.y
  );
  // inside the block far off, a pixel from its right side: out to the
  // right, not further in
  let (mut world, emitter) = tile_part_scene(
    CollisionConfig::bouncy(RADIUS, 0.3),
    Vec2::new(399.0, 10.0),
    0.0,
  );
  step(&mut world);
  let p = only(&world, emitter);
  assert!(p.x >= 400.0 + RADIUS - 1e-3, "x = {}", p.x);
}

#[test]
fn a_particle_falls_onto_a_flipped_hex_tile_s_shape_and_stops_on_it() {
  use crate::core::ecs::IVec2;
  use crate::core::physics::{CellShape, HexCells, TileCell};
  use crate::core::tilemap::{StaggerAxis, StaggerIndex};
  // A hexagonal map's rock: in cell (0, 0) of 120 x 140 hexagons of side
  // 70 (its box from the origin), a triangle as a tile flipped
  // horizontally places it, its points running anticlockwise. Its ring,
  // as physics builds it, is the map's own part
  let cell = glam::UVec2::new(120, 140);
  let hex = HexCells::new(StaggerAxis::Y, StaggerIndex::Odd, cell, 70);
  let mut tiles = TileColliders::unbounded(cell.as_vec2()).with_hex(hex, Vec2::ZERO);
  let flipped = [(48.0, 84.0), (24.0, 100.0), (50.0, 108.0)]
    .map(|(x, y): (f32, f32)| Vec2::new(x / 120.0, y / 140.0));
  let rock = tiles.add_shape(CellShape {
    polygons: vec![flipped.to_vec()],
    one_way: false,
  });
  assert!(tiles.set(IVec2::ZERO, TileCell::Shape(rock)));
  let parts = tiles.traced_parts();
  let mut world = testing::world();
  let grid = world.spawn((Transform::IDENTITY, tiles)).id();
  for (at, collider) in parts {
    world.spawn((
      Transform::from_xy(at.x, at.y),
      collider,
      TilePart,
      ChildOf(grid),
    ));
  }
  // A particle above the rock's top edge, from (24, 100) up to (48, 84):
  // at x 36 the edge is at y 92
  let emitter = faller(
    &mut world,
    CollisionConfig::sticky(RADIUS),
    Vec2::new(36.0, 20.0),
    600.0,
  );
  step(&mut world);
  let mut last = only(&world, emitter).y;
  assert!(last < 25.0, "it starts where it was spawned: y = {last}");
  for step_n in 0..120 {
    step(&mut world);
    let p = only(&world, emitter);
    // It falls as gravity takes it, never jumping onto the rock
    assert!(
      p.y >= last - 1e-3 && p.y - last < 8.0,
      "step {step_n}: from y {last} to {}",
      p.y
    );
    last = p.y;
  }
  let p = only(&world, emitter);
  assert!(p.stuck, "it stuck on the rock");
  assert!(
    p.y > 92.0 - RADIUS - 2.0 && p.y < 92.0,
    "on the rock's top edge, outside it: ({}, {})",
    p.x,
    p.y
  );
}

// Stopped and unscaled emitters

/// A stream of a particle a step from (100, 0), standing still and living
/// 60 s, falling under `gravity_y`, colliding by `config` if given
fn stream(world: &mut World, gravity_y: f32, config: Option<CollisionConfig>) -> ParticleEffect {
  let mut effect = ParticleEffect::new(testing::image(world))
    .with_rate(60.0)
    .with_speed(0.0)
    .with_lifetime(60.0)
    .with_gravity(0.0, gravity_y);
  effect.collision = config;
  effect
}

fn stop(world: &mut World, emitter: Entity) {
  world
    .get_mut::<ParticleEmitter>(emitter)
    .expect("the entity has an emitter")
    .stop();
}

#[test]
fn a_stopped_emitter_s_particles_keep_colliding() {
  // A stream above a floor, stopped before any particle lands: they fall
  // on, and stick to the floor
  let mut world = testing::world();
  let effect = stream(&mut world, 600.0, Some(CollisionConfig::sticky(RADIUS)));
  let effect = testing::effect(&mut world, effect);
  let emitter = world
    .spawn((Transform::from_xy(100.0, 0.0), ParticleEmitter::new(effect)))
    .id();
  world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
  steps(&mut world, 5);
  stop(&mut world, emitter);
  let live = count(&world, emitter);
  assert!(live > 0);
  steps(&mut world, 120);
  let seen = particles(&world, emitter);
  assert_eq!(seen.len(), live, "none spawned after the stop, none lost");
  for p in &seen {
    assert!(
      p.stuck && (p.y - (50.0 - RADIUS)).abs() < 1e-3,
      "stuck on the floor: {p:?}"
    );
  }
}

#[test]
fn a_stopped_emitter_s_particles_keep_their_gravity() {
  let mut world = testing::world();
  let effect = stream(&mut world, 100.0, None);
  let effect = testing::effect(&mut world, effect);
  let emitter = world
    .spawn((Transform::from_xy(100.0, 0.0), ParticleEmitter::new(effect)))
    .id();
  steps(&mut world, 5);
  stop(&mut world, emitter);
  let before = particles(&world, emitter);
  assert!(!before.is_empty());
  steps(&mut world, 30);
  let after = particles(&world, emitter);
  assert_eq!(after.len(), before.len());
  for (was, now) in before.iter().zip(&after) {
    assert!(
      (now.vy - was.vy - 100.0 * 30.0 * DT).abs() < 1e-3,
      "gravity kept pulling: {} to {}",
      was.vy,
      now.vy
    );
    assert!(now.y > was.y);
  }
}

#[test]
fn an_unscaled_emitter_s_particles_collide_too() {
  // Its own pass meets the same colliders, and tells of the hit
  let mut world = testing::world();
  let config = CollisionConfig::die_on_hit(RADIUS).with_events();
  let effect = ParticleEffect::new(testing::image(&mut world))
    .with_rate(0.0)
    .with_speed(0.0)
    .with_lifetime(60.0)
    .with_looping(false)
    .with_burst(Burst::at(0.0, 1.0))
    .with_gravity(0.0, 600.0)
    .with_collision(config);
  let effect = testing::effect(&mut world, effect);
  let emitter = world
    .spawn((
      Transform::from_xy(100.0, 0.0),
      ParticleEmitter::new(effect).unscaled(),
    ))
    .id();
  let floor = world
    .spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)))
    .id();
  let length = world.resource::<Time>().fixed.timestep_duration();
  frame(&mut world, length);
  assert_eq!(count(&world, emitter), 1);
  for _ in 0..60 {
    frame(&mut world, length);
  }
  assert_eq!(count(&world, emitter), 0, "it died on the floor");
  let hits = collisions(&world);
  assert_eq!(hits.len(), 1);
  assert_eq!(hits[0].emitter_entity, emitter);
  assert_eq!(hits[0].collider_entity, floor);
}

// The collision grid

/// A contact field by field, bit for bit
fn exactly(contact: Option<Contact>) -> Option<(Entity, [u32; 4])> {
  contact.map(|c| {
    let fields = [c.normal_x, c.normal_y, c.point_x, c.point_y];
    (c.collider, fields.map(f32::to_bits))
  })
}

/// Which kind of geometry a collider is, for counting what a test met
fn kind(collider: &ParticleCollider) -> usize {
  match collider.geometry {
    ColliderGeometry::Box { .. } => 0,
    ColliderGeometry::Circle { .. } => 1,
    ColliderGeometry::Edges { solid: Some(_), .. } => 2,
    ColliderGeometry::Edges { solid: None, .. } => 3,
  }
}

#[test]
fn the_grid_finds_what_brute_force_finds() {
  // Seeded moves over a busy level, mostly short, some long: every
  // collider's contact over the grid's candidates (none for a collider it
  // leaves out) is the contact over all of its edges
  let mut world = testing::world();
  testing::busy_level(&mut world);
  let (colliders, edges) = testing::gathered(&mut world);
  let mut grid = ColliderGrid::default();
  grid.rebuild(&colliders, &edges);
  let mut near = Candidates::default();
  let mut rng = ParticleRng::new(0x6_121D);
  const MOVES: usize = 10_000;
  for radius in [2.0, 6.0] {
    let (mut met, mut kinds, mut read) = (0, [0; 4], 0);
    for n in 0..MOVES {
      let from = Vec2::new(rng.range(-80.0, 1130.0), rng.range(-80.0, 870.0));
      let longest = match rng.next_f32() {
        roll if roll < 0.7 => 12.0,
        roll if roll < 0.95 => 60.0,
        _ => 400.0,
      };
      let along = Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU));
      let to = from + along * rng.range(0.0, longest);
      grid.query_move(from, to, radius, &mut near);
      read += near.edges.len();
      for (k, collider) in colliders.iter().enumerate() {
        let all = collider.contact(&edges, from, (to.x, to.y), radius);
        let candidate = near.colliders.binary_search(&(k as u32)).is_ok();
        let among = candidate
          .then(|| collider.contact_among(&edges, &near.edges, from, (to.x, to.y), radius))
          .flatten();
        assert_eq!(
          exactly(among),
          exactly(all),
          "move {n} at radius {radius}, from {from} to {to}: collider {k}, {:?}",
          collider.geometry
        );
        if all.is_some() {
          met += 1;
          kinds[kind(collider)] += 1;
        }
      }
    }
    assert!(met > 1_000, "radius {radius}: only {met} contacts");
    assert!(
      kinds.iter().all(|&n| n > 0),
      "radius {radius}: contacts by box, circle, solid and two-sided edges: {kinds:?}"
    );
    assert!(
      read < MOVES * edges.len() / 20,
      "the grid leaves most edges out: {read} read of {} edges a move",
      edges.len()
    );
  }
}
