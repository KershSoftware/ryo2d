//! Headless tests of the shadow pages

use demo_kit::prelude::*;
use ryo2d::testing::{HeadlessEngine, capture_log};
use shadows_demo::scenes::{Bearer, Lantern, Torch};

const SOFT: usize = 3;
const LANTERN: usize = 8;
const MANY_LIGHTS: usize = 9;
const PAGES: usize = 10;

fn start(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(shadows_demo::demo().start_page(page).engine_builder())
}

fn tap(engine: &mut HeadlessEngine, key: KeyCode) {
  engine.tap_key(key);
  engine.step_frames(2).unwrap();
}

#[test]
fn every_page_builds_and_runs_without_a_warning() {
  let log = capture_log();
  for page in 0..PAGES {
    let Some(mut engine) = start(page) else {
      return;
    };
    engine.step_frames(120).unwrap();
    let frame = engine.frame().unwrap();
    assert!(frame.width() > 0, "page {page} drew a frame");
  }
  let warnings = log.warnings_and_errors();
  assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn the_soft_page_s_size_control_changes_the_light_s_softness() {
  let Some(mut engine) = start(SOFT) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let softness = |engine: &mut HeadlessEngine| {
    let world = engine.world_mut();
    let sizes: Vec<f32> = world
      .query::<&Light2D>()
      .iter(world)
      .map(Light2D::shadow_softness)
      .collect();
    assert_eq!(sizes.len(), 1);
    sizes[0]
  };
  let before = softness(&mut engine);
  tap(&mut engine, KeyCode::ArrowRight);
  let after = softness(&mut engine);
  assert!(after > before, "{before} then {after}");
  tap(&mut engine, KeyCode::ArrowLeft);
  tap(&mut engine, KeyCode::ArrowLeft);
  assert!(softness(&mut engine) < before);
}

#[test]
fn the_lantern_is_inside_its_bearer_s_occluder() {
  let Some(mut engine) = start(LANTERN) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let world = engine.world_mut();
  let mut lanterns =
    world.query_filtered::<(&GlobalTransform, &ChildOf, &Light2D), With<Lantern>>();
  let found: Vec<_> = lanterns
    .iter(world)
    .map(|(at, parent, light)| (at.translation(), parent.parent(), light.casts_shadows()))
    .collect();
  assert_eq!(found.len(), 1);
  let (light, bearer, casts) = found[0];
  assert!(casts, "the lantern casts shadows");
  assert!(
    world.get::<Bearer>(bearer).is_some(),
    "carried by the bearer"
  );
  let (Some(occluder), Some(placed)) = (
    world.get::<LightOccluder2D>(bearer),
    world.get::<GlobalTransform>(bearer),
  ) else {
    panic!("the bearer is an occluder");
  };
  let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
  for v in occluder.vertices() {
    min = min.min(placed.translation() + *v);
    max = max.max(placed.translation() + *v);
  }
  assert!(
    light.cmpgt(min).all() && light.cmplt(max).all(),
    "{light} outside {min} to {max}"
  );
}

#[test]
fn the_many_lights_page_has_sixty_four_shadowed_torches_and_q_cycles_quality() {
  let Some(mut engine) = start(MANY_LIGHTS) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let world = engine.world_mut();
  let torches: Vec<bool> = world
    .query_filtered::<&Light2D, With<Torch>>()
    .iter(world)
    .map(Light2D::casts_shadows)
    .collect();
  assert_eq!(torches.len(), 64);
  assert!(torches.iter().all(|&shadowed| shadowed));
  let world = engine.world_mut();
  let chains = world
    .query_filtered::<(), (With<MapOccluder>, With<LightOccluder2D>)>()
    .iter(world)
    .count();
  assert!(chains > 0, "the maze's walls are map occluder chains");

  let quality = |engine: &HeadlessEngine| {
    engine
      .world()
      .resource::<ryo2d::prelude::Settings>()
      .shadow_quality()
  };
  let start = quality(&engine);
  tap(&mut engine, KeyCode::KeyQ);
  let picked = quality(&engine);
  assert_ne!(picked, start);
  // The choice is the player's: a page change and a reset keep it
  tap(&mut engine, KeyCode::BracketRight);
  assert_eq!(quality(&engine), picked, "kept after a page change");
  tap(&mut engine, KeyCode::BracketLeft);
  tap(&mut engine, KeyCode::KeyR);
  assert_eq!(quality(&engine), picked, "kept after a reset");
  for _ in 0..3 {
    tap(&mut engine, KeyCode::KeyQ);
  }
  assert_eq!(quality(&engine), start, "four presses go round");
}

#[test]
fn the_many_lights_page_s_walls_stay_dark_until_turned_off() {
  let Some(mut engine) = start(MANY_LIGHTS) else {
    return;
  };
  engine.step_frames(4).unwrap();
  let own = |engine: &mut HeadlessEngine| {
    let world = engine.world_mut();
    let chains: Vec<bool> = world
      .query_filtered::<&LightOccluder2D, With<MapOccluder>>()
      .iter(world)
      .map(LightOccluder2D::self_shadow)
      .collect();
    assert!(
      !chains.is_empty(),
      "the maze's walls are map occluder chains"
    );
    chains
  };
  assert!(own(&mut engine).iter().all(|&on| on), "dark by default");
  // The third row, "Walls stay dark", turned off
  tap(&mut engine, KeyCode::ArrowDown);
  tap(&mut engine, KeyCode::ArrowDown);
  tap(&mut engine, KeyCode::ArrowRight);
  assert!(own(&mut engine).iter().all(|&on| !on), "turned off");
  tap(&mut engine, KeyCode::ArrowLeft);
  assert!(own(&mut engine).iter().all(|&on| on), "back on");
}
