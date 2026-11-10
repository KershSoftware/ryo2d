//! Effects loaded from files and saved while the game runs.

use std::time::{Duration, Instant};

use crate::core::asset::{
  AssetServer, Assets, AssetsView, Handle, MemorySource, insert_registry, pump,
};
use crate::core::ecs::{Entity, Transform, World};
use crate::core::particles::ParticleEmitter;
use crate::core::particles::effect::ParticleEffect;
use crate::core::particles::testing::{
  self, collisions, count, death_particles, particles, step, steps,
};
use crate::core::physics::Collider;

const PATH: &str = "fx/live.particles.ron";

fn png() -> Vec<u8> {
  let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255; 4]));
  let mut bytes = Vec::new();
  if let Err(error) = image.write_to(
    &mut std::io::Cursor::new(&mut bytes),
    image::ImageFormat::Png,
  ) {
    panic!("encoding a png: {error}");
  }
  bytes
}

/// A step world whose effects load from `texts`, with an image beside
/// them, its registry told when a file is saved
fn world_over(texts: &[(&str, &str)]) -> (World, MemorySource) {
  let files = MemorySource::new();
  files.insert("fx/spark.png", png());
  for (path, text) in texts {
    files.insert(path, *text);
  }
  let mut world = testing::world();
  insert_registry(&mut world, AssetServer::new(files.clone()));
  world.resource::<AssetServer>().start_watching();
  (world, files)
}

fn loaded(world: &mut World, path: &str) -> Handle<ParticleEffect> {
  match AssetsView::new(world).load_now::<ParticleEffect>(path) {
    Ok(handle) => handle,
    Err(error) => panic!("{path}: {}", error.report()),
  }
}

fn version(world: &World, effect: &Handle<ParticleEffect>) -> Option<u64> {
  world.resource::<Assets<ParticleEffect>>().version(effect)
}

/// Saves `text` at `path` and pumps until `effect`'s new version lands
fn save(
  world: &mut World,
  files: &MemorySource,
  path: &str,
  text: &str,
  effect: &Handle<ParticleEffect>,
) {
  let before = version(world, effect);
  files.insert(path, text);
  let start = Instant::now();
  while version(world, effect) == before {
    assert!(
      start.elapsed() < Duration::from_secs(5),
      "the save never landed"
    );
    std::thread::sleep(Duration::from_millis(1));
    pump(world);
  }
}

/// An emitter of `effect` at (`x`, `y`)
fn emitter(world: &mut World, effect: &Handle<ParticleEffect>, x: f32, y: f32) -> Entity {
  world
    .spawn((
      Transform::from_xy(x, y),
      ParticleEmitter::new(effect.clone()),
    ))
    .id()
}

#[test]
fn a_reload_changes_live_particles_over_life_values() {
  let v1 = r##"(
    image: "fx/spark.png",
    rate: 0,
    looping: false,
    duration: 10,
    bursts: [(count: 3)],
    speed: 0,
    lifetime: 5,
    size: 8,
    color_over_life: [(0.0, "#ff0000")],
  )"##;
  let (mut world, files) = world_over(&[(PATH, v1)]);
  let effect = loaded(&mut world, PATH);
  let source = emitter(&mut world, &effect, 0.0, 0.0);
  steps(&mut world, 10);
  let before = particles(&world, source);
  assert_eq!(before.len(), 3);
  for particle in &before {
    assert_eq!(particle.color, [1.0, 0.0, 0.0, 1.0], "red: {particle:?}");
    assert_eq!(particle.size, 8.0);
  }

  // Blue over life, smaller, and a new burst later in the same cycle
  let v2 = r##"(
    image: "fx/spark.png",
    rate: 0,
    looping: false,
    duration: 10,
    bursts: [(count: 3), (time: 0.5, count: 2)],
    speed: 0,
    lifetime: 5,
    size: 4,
    color_over_life: [(0.0, "#0000ff")],
  )"##;
  save(&mut world, &files, PATH, v2, &effect);
  // Seen until the step sees the new version: the compiled effect is still v1
  assert_eq!(particles(&world, source)[0].color, [1.0, 0.0, 0.0, 1.0]);
  step(&mut world);
  let after = particles(&world, source);
  assert_eq!(
    after.len(),
    3,
    "the burst already fired does not fire again"
  );
  for (was, now) in before.iter().zip(&after) {
    assert_eq!(now.color, [0.0, 0.0, 1.0, 1.0], "blue at once: {now:?}");
    assert_eq!(now.size, 8.0, "start values stay: {now:?}");
    assert!(now.age > was.age, "still ageing: {now:?}");
  }

  // The new burst is due at its time in the same cycle, and has the new
  // start values
  steps(&mut world, 30);
  let all = particles(&world, source);
  assert_eq!(all.len(), 5, "{all:?}");
  assert_eq!(all[0].size, 8.0, "live particles keep their start size");
  assert_eq!(all[4].size, 4.0, "new particles take the new one");
  assert_eq!(all[4].color, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn a_reload_lowering_the_cap_or_dropping_collision_keeps_live_particles() {
  // Falling from y 0 onto a floor at y 50 in about 0.35 s, sticking there
  let v1 = r#"(
    image: "fx/spark.png",
    rate: 60,
    max_particles: 30,
    lifetime: 3,
    speed: 0,
    gravity: (0, 800),
    collision: (radius: 1, response: Stick, events: true),
  )"#;
  let (mut world, files) = world_over(&[(PATH, v1)]);
  world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
  let effect = loaded(&mut world, PATH);
  let source = emitter(&mut world, &effect, 100.0, 0.0);
  steps(&mut world, 40);
  let before = particles(&world, source);
  assert_eq!(before.len(), 30, "at its cap");
  let stuck = before.iter().filter(|particle| particle.stuck).count();
  assert!(stuck > 0 && stuck < 30, "{stuck} of 30 stuck");
  let told = collisions(&world).len();
  assert_eq!(told, stuck, "each stick was told");

  // A cap below the live count, and no collisions
  let v2 = r#"(
    image: "fx/spark.png",
    rate: 60,
    max_particles: 10,
    lifetime: 3,
    speed: 0,
    gravity: (0, 800),
  )"#;
  save(&mut world, &files, PATH, v2, &effect);
  step(&mut world);
  let after = particles(&world, source);
  assert_eq!(after.len(), 30, "none of the live particles is cut short");
  assert!(
    after.iter().all(|particle| !particle.stuck),
    "released, since collisions have stopped: {after:?}"
  );
  steps(&mut world, 30);
  let fallen = particles(&world, source);
  assert_eq!(fallen.len(), 30, "still nothing cut short, nothing new");
  assert_eq!(
    collisions(&world).len(),
    told,
    "no collision since the save"
  );
  assert!(
    fallen.iter().all(|particle| particle.y > 50.0),
    "every particle passed the floor, the stuck ones moving again: {fallen:?}"
  );

  // Nothing spawns while over the cap; as they die, it refills to the new cap
  let mut most = 30;
  for _ in 0..200 {
    step(&mut world);
    let live = count(&world, source);
    assert!(live <= most.max(10), "{live} over {most}");
    most = live;
  }
  let settled = count(&world, source);
  assert!((1..=10).contains(&settled), "{settled}");
}

/// Stuck particles are held by `Stick`: a reload to another response lets
/// them go, and they fall onto the floor they were stuck to, bouncing
#[test]
fn a_reload_that_stops_sticking_lets_stuck_particles_go() {
  let text = |response: &str| {
    format!(
      r#"(
    image: "fx/spark.png",
    rate: 30,
    max_particles: 10,
    lifetime: 10,
    speed: 0,
    gravity: (0, 800),
    collision: (radius: 1, response: {response}),
  )"#
    )
  };
  let (mut world, files) = world_over(&[(PATH, &text("Stick"))]);
  world.spawn((Transform::from_xy(100.0, 60.0), Collider::rect(200.0, 20.0)));
  let effect = loaded(&mut world, PATH);
  let source = emitter(&mut world, &effect, 100.0, 0.0);
  steps(&mut world, 45);
  let before = particles(&world, source);
  assert!(before.iter().any(|particle| particle.stuck), "{before:?}");

  save(&mut world, &files, PATH, &text("Bounce(0.5)"), &effect);
  steps(&mut world, 5);
  let after = particles(&world, source);
  assert!(after.iter().all(|particle| !particle.stuck), "{after:?}");
  // They still meet the floor
  assert!(
    after.iter().all(|particle| particle.y <= 49.5),
    "still above the floor: {after:?}"
  );
}

/// The file naming a sub-effect that has sub-emitters of its own is told
/// of it as it loads. The warning joins the log the tests that count
/// particle warnings read, so this holds their lock
#[test]
fn a_sub_effect_with_sub_emitters_of_its_own_loads_with_a_warning() {
  let _log = testing::log_lock();
  let parent = "fx/warn-parent.particles.ron";
  let nested = "fx/warn-nested.particles.ron";
  let warned = || {
    testing::particle_loader_warnings()
      .iter()
      .filter(|message| {
        message.contains(parent) && message.contains(nested) && message.contains("own")
      })
      .count()
  };
  let before = warned();
  let (mut world, _files) = world_over(&[
    (
      parent,
      r#"(image: "fx/spark.png", on_death: (effect: "fx/warn-nested.particles.ron", count: 1))"#,
    ),
    (
      nested,
      r#"(image: "fx/spark.png", on_death: (effect: "fx/warn-leaf.particles.ron", count: 1))"#,
    ),
    ("fx/warn-leaf.particles.ron", r#"(image: "fx/spark.png")"#),
  ]);
  // It still loads
  let _effect = loaded(&mut world, parent);
  assert_eq!(warned(), before + 1, "warned once, naming both");
}

#[test]
fn a_sub_effect_s_reload_reaches_its_parent() {
  const PUFF: &str = "fx/puff.particles.ron";
  let parent = r#"(
    image: "fx/spark.png",
    rate: 0,
    looping: false,
    duration: 2,
    bursts: [(count: 1)],
    speed: 0,
    lifetime: 0.05,
    on_death: (effect: "fx/puff.particles.ron", count: 3),
  )"#;
  let puff = r##"(image: "fx/spark.png", rate: 0, speed: 0, lifetime: 100, color_over_life: [(0.0, "#ff0000")])"##;
  let (mut world, files) = world_over(&[(PATH, parent), (PUFF, puff)]);
  let effect = loaded(&mut world, PATH);
  let source = emitter(&mut world, &effect, 0.0, 0.0);
  steps(&mut world, 10);
  let before = death_particles(&world, source);
  assert_eq!(before.len(), 3);
  assert_eq!(before[0].color, [1.0, 0.0, 0.0, 1.0]);

  let blue = r##"(image: "fx/spark.png", rate: 0, speed: 0, lifetime: 100, color_over_life: [(0.0, "#0000ff")])"##;
  // Its parent's version moves with it, so what is built from it is rebuilt
  save(&mut world, &files, PUFF, blue, &effect);
  step(&mut world);
  let after = death_particles(&world, source);
  assert_eq!(after.len(), 3, "the sub-effect's particles are kept");
  for particle in &after {
    assert_eq!(particle.color, [0.0, 0.0, 1.0, 1.0], "{particle:?}");
  }
  assert_eq!(
    world
      .resource::<crate::core::particles::ParticleStore>()
      .live(),
    3
  );
}
