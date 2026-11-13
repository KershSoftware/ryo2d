//! The demo's pages without a window: each steps for 120 frames with its
//! heroes posed and no warning logged, from the folder and from the files
//! the web build embeds, and each page's feature does what its page says.
//!
//! The engine is headless: tests skip when no GPU is available.

use ryo2d::core::audio::AudioLogKind;
use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, LogCapture};
use skeleton_demo::{EMBEDDED, demo, embedded};

const CHARACTER: usize = 0;
const LAYERS: usize = 1;
const EVENTS: usize = 2;
const SOCKETS: usize = 3;
const SKINS: usize = 4;
const AIMING: usize = 5;
const CROWD: usize = 6;
const PAGES: usize = 7;

/// The frames each page runs
const FRAMES: u32 = 120;

fn engine_on(page: usize) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(demo().start_page(page).engine_builder())
}

/// Every warning and error `log` captured
fn warnings(log: &LogCapture) -> Vec<String> {
  log.warnings_and_errors()
}

/// The skeleton entities in the engine's world, in entity order
fn heroes(engine: &mut HeadlessEngine) -> Vec<Entity> {
  let world = engine.world_mut();
  let mut heroes: Vec<Entity> = world
    .query_filtered::<Entity, With<Skeleton>>()
    .iter(world)
    .collect();
  heroes.sort();
  heroes
}

/// The page's one hero
fn hero(engine: &mut HeadlessEngine) -> Entity {
  let heroes = heroes(engine);
  assert_eq!(heroes.len(), 1, "one hero on the page");
  heroes[0]
}

/// The animation playing on `track` of `hero`
fn playing(engine: &HeadlessEngine, hero: Entity, track: usize) -> Option<String> {
  let state = engine.world().get::<SkeletonAnimation>(hero)?;
  state.animation(track).map(str::to_owned)
}

fn bone(engine: &HeadlessEngine, hero: Entity, name: &str) -> Affine2 {
  engine
    .world()
    .get::<Skeleton>(hero)
    .and_then(|skeleton| skeleton.bone_world(name))
    .unwrap_or_else(|| panic!("the hero's {name} bone"))
}

/// The pixels of the engine's last frame within `tolerance` of `color` on
/// every channel
fn pixels_near(engine: &mut HeadlessEngine, color: [u8; 3], tolerance: u8) -> usize {
  let frame = engine.frame().unwrap();
  frame
    .pixels()
    .filter(|pixel| (0..3).all(|channel| pixel.0[channel].abs_diff(color[channel]) <= tolerance))
    .count()
}

/// Steps every page for two seconds, checking that its heroes are posed,
/// drawn as the engine asks of a character, and that nothing warns
fn every_page_steps(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
  for page in 0..PAGES {
    // A capture for each page, opened before its engine
    let log = ryo2d::testing::capture_log();
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    let heroes = heroes(&mut engine);
    assert!(!heroes.is_empty(), "page {}: no hero", page + 1);
    for &hero in &heroes {
      let world = engine.world();
      let skeleton = world.get::<Skeleton>(hero).unwrap();
      assert!(
        skeleton.bone_world("hip").is_some(),
        "page {}: a hero has no pose",
        page + 1
      );
      // Smooth bones, and a shadow that suits a figure of many parts
      assert!(
        world.get::<Interpolated>(hero).is_some(),
        "page {}: a hero is not interpolated",
        page + 1
      );
      let renderable = world.get::<Renderable>(hero).unwrap();
      assert!(renderable.is_skeleton());
      assert!(
        renderable.has_blob_shadow() || renderable.has_contact_shadow(),
        "page {}: a hero casts no ground shadow",
        page + 1
      );
      assert!(
        !renderable.has_drop_shadow(),
        "page {}: a hero casts a drop shadow",
        page + 1
      );
    }
    let warned = warnings(&log);
    assert!(warned.is_empty(), "page {}: {warned:#?}", page + 1);
  }
}

#[test]
fn every_page_steps_with_its_heroes_posed_and_without_a_warning() {
  every_page_steps(engine_on);
}

#[test]
fn every_page_steps_from_the_files_the_web_build_embeds() {
  every_page_steps(|page| {
    let builder = demo().start_page(page).engine_builder();
    HeadlessEngine::new_or_skip(builder.asset_source(embedded()))
  });
}

/// What the system leaves in the assets folder, which the web build does
/// not embed: dotfiles (macOS's `.DS_Store`)
fn is_left_beside_the_assets(name: &str) -> bool {
  name.starts_with('.')
}

#[test]
fn the_web_build_embeds_every_asset_file() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
  let mut on_disk = Vec::new();
  let mut folders = vec![root.clone()];
  while let Some(folder) = folders.pop() {
    for entry in std::fs::read_dir(&folder).unwrap() {
      let path = entry.unwrap().path();
      let name = path.file_name().unwrap_or_default().to_string_lossy();
      if is_left_beside_the_assets(&name) {
        continue;
      }
      if path.is_dir() {
        folders.push(path);
      } else if name != "README.md" {
        let relative = path.strip_prefix(&root).unwrap();
        on_disk.push(relative.to_string_lossy().replace('\\', "/"));
      }
    }
  }
  on_disk.sort();
  let mut embedded: Vec<String> = EMBEDDED.iter().map(|(path, _)| path.to_string()).collect();
  embedded.sort();
  assert_eq!(embedded, on_disk);
  for (path, bytes) in EMBEDDED {
    assert_eq!(*bytes, std::fs::read(root.join(path)).unwrap(), "{path}");
  }
}

#[test]
fn the_character_idles_walks_runs_turns_and_jumps_on_keys() {
  let Some(mut engine) = engine_on(CHARACTER) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let hero = hero(&mut engine);
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("idle"));
  let x = |engine: &HeadlessEngine| engine.world().get::<Transform>(hero).unwrap().translation.x;
  let flipped = |engine: &HeadlessEngine| engine.world().get::<Renderable>(hero).unwrap().flip_x;
  let start = x(&engine);

  engine.press_key(KeyCode::KeyD);
  engine.step_frames(30).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));
  let walked = x(&engine) - start;
  assert!(walked > 10.0, "walking right moved it {walked}");
  assert!(!flipped(&engine));

  engine.press_key(KeyCode::ShiftLeft);
  engine.step_frames(30).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("run"));
  // Walk to run mixes over the pair's own time in the file, as run to walk
  // does, not the default
  let state = engine.world().get::<SkeletonAnimation>(hero).unwrap();
  let run = state.entry(state.current(0).unwrap()).unwrap();
  assert_eq!(run.mix_duration, 0.25);
  let ran = x(&engine) - start - walked;
  assert!(ran > walked, "running is faster: {ran} against {walked}");

  engine.release_key(KeyCode::ShiftLeft);
  engine.release_key(KeyCode::KeyD);
  engine.step_frames(5).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("idle"));

  // A jump plays once, then the hero is back to what the keys ask for
  engine.tap_key(KeyCode::Space);
  engine.step_frames(3).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("jump"));
  engine.step_frames(90).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("idle"));

  // Walking left turns it round with the renderable's flip
  engine.press_key(KeyCode::KeyA);
  engine.step_frames(10).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));
  assert!(flipped(&engine));
}

#[test]
fn the_layers_page_plays_a_second_track_over_the_walk_at_its_alpha() {
  let Some(mut engine) = engine_on(LAYERS) else {
    return;
  };
  engine.step_frames(30).unwrap();
  let hero = hero(&mut engine);
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));
  assert_eq!(playing(&engine, hero, 1).as_deref(), Some("wave"));
  let alpha = |engine: &HeadlessEngine| {
    let state = engine.world().get::<SkeletonAnimation>(hero).unwrap();
    state.entry(state.current(1).unwrap()).unwrap().alpha
  };
  assert_eq!(alpha(&engine), 1.0);

  // The settings panel's first row picks the layer, its second the alpha
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  assert_eq!(playing(&engine, hero, 1).as_deref(), Some("aim"));
  engine.tap_key(KeyCode::ArrowDown);
  engine.step_frames(2).unwrap();
  engine.tap_key(KeyCode::ArrowLeft);
  engine.step_frames(5).unwrap();
  assert!(
    alpha(&engine) < 1.0,
    "the alpha row lowers the layer's alpha"
  );

  // Space fades the layer out, and the track clears
  engine.tap_key(KeyCode::Space);
  engine.step_frames(60).unwrap();
  assert_eq!(playing(&engine, hero, 1), None);
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));

  // Space again fades it in, at the alpha set, from the step it starts in
  engine.tap_key(KeyCode::Space);
  for _ in 0..5 {
    engine.step().unwrap();
    if playing(&engine, hero, 1).is_some() {
      break;
    }
  }
  assert_eq!(playing(&engine, hero, 1).as_deref(), Some("aim"));
  assert!(
    alpha(&engine) < 1.0,
    "the layer starts at the alpha set, not at full"
  );
}

#[test]
fn footsteps_play_the_event_s_sound_and_puff_dust() {
  let Some(mut engine) = engine_on(EVENTS) else {
    return;
  };
  let mut dust = 0;
  for _ in 0..FRAMES {
    engine.step().unwrap();
    dust = dust.max(ParticlesView::new(engine.world_mut()).count());
  }
  assert!(dust > 0, "no dust was puffed");
  let steps: Vec<_> = engine
    .audio_log()
    .into_iter()
    .filter(|entry| entry.kind == AudioLogKind::Start)
    .collect();
  assert!(
    steps.len() >= 3,
    "footsteps in two seconds: {}",
    steps.len()
  );
  for step in &steps {
    // The path the skeleton file gives its footstep event
    assert_eq!(step.sound, "audio/footstep.sound.ron");
  }
  // Heard from where the hero is: it walks to and fro, so not always from
  // the listener's own place
  assert!(steps.iter().any(|step| step.pan != 0.0));
}

#[test]
fn the_sword_is_on_the_weapon_bone_and_mirrors_with_a_flipped_hero() {
  let Some(mut engine) = engine_on(SOCKETS) else {
    return;
  };
  engine.step_frames(FRAMES).unwrap();
  let heroes = heroes(&mut engine);
  assert_eq!(heroes.len(), 2, "a hero facing each way");
  let world = engine.world_mut();
  let sockets: Vec<(Entity, Entity)> = world
    .query::<(Entity, &BoneSocket, &ChildOf)>()
    .iter(world)
    .map(|(socket, _, parent)| (socket, parent.parent()))
    .collect();
  assert_eq!(sockets.len(), 2, "a sword each");
  let mut mirrored = 0;
  for (socket, hero) in sockets {
    assert!(heroes.contains(&hero));
    let held = engine
      .world()
      .get::<GlobalTransform>(socket)
      .unwrap()
      .affine();
    let weapon = bone(&engine, hero, "weapon");
    assert!(
      held.translation.abs_diff_eq(weapon.translation, 1e-3),
      "the sword is at {} and the bone at {}",
      held.translation,
      weapon.translation
    );
    let flipped = engine.world().get::<Renderable>(hero).unwrap().flip_x;
    // A flipped hero's sword is mirrored with it
    assert_eq!(held.matrix2.determinant() < 0.0, flipped);
    mirrored += usize::from(flipped);
  }
  assert_eq!(mirrored, 1, "one hero of the two is flipped");
}

#[test]
fn the_skins_page_changes_outfit_and_puts_the_hat_on_and_off() {
  // The cap's yellow and the green of the ranger's sleeves, as the atlas
  // has them; the tolerance takes in what the frame's tone curve does to a
  // bright colour
  const CAP: [u8; 3] = [242, 182, 50];
  const GREEN: [u8; 3] = [99, 180, 72];
  let Some(mut engine) = engine_on(SKINS) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let hero = hero(&mut engine);
  // What the skeleton says it wears, and what its hat's slot shows
  fn worn(engine: &HeadlessEngine, hero: Entity) -> Vec<&str> {
    engine
      .world()
      .get::<Skeleton>(hero)
      .unwrap()
      .skins()
      .collect()
  }
  let hat = |engine: &HeadlessEngine| {
    let skeleton = engine.world().get::<Skeleton>(hero).unwrap();
    skeleton.attachment("hat").map(str::to_owned)
  };
  // The page opens on the ranger with the hat on
  assert_eq!(worn(&engine, hero), ["ranger", "hat"]);
  assert_eq!(hat(&engine).as_deref(), Some("hat"));
  assert!(pixels_near(&mut engine, CAP, 6) > 200, "the hat is on");
  assert!(
    pixels_near(&mut engine, GREEN, 6) > 200,
    "the ranger's shirt"
  );

  // Rows: the hero, the outfit, the hat
  engine.tap_key(KeyCode::ArrowDown);
  engine.step_frames(2).unwrap();
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  assert_eq!(worn(&engine, hero), ["sailor", "hat"]);
  assert_eq!(pixels_near(&mut engine, GREEN, 6), 0, "another outfit");
  assert!(pixels_near(&mut engine, CAP, 6) > 200, "the hat stays on");

  engine.tap_key(KeyCode::ArrowDown);
  engine.step_frames(2).unwrap();
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  // Without the hat's skin its slot shows nothing: no skin worn has the
  // attachment the slot names
  assert_eq!(worn(&engine, hero), ["sailor"]);
  assert_eq!(hat(&engine), None);
  assert_eq!(pixels_near(&mut engine, CAP, 6), 0, "the hat is off");
}

#[test]
fn the_aiming_arm_points_at_the_pointer() {
  let Some(mut engine) = engine_on(AIMING) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let hero = hero(&mut engine);
  for target in [Vec2::new(900.0, 200.0), Vec2::new(120.0, 520.0)] {
    engine.mouse_move(target.x, target.y);
    engine.step_frames(40).unwrap();
    let arm = bone(&engine, hero, "arm_near");
    let pointing = arm.matrix2.x_axis.normalize();
    let wanted = (target - arm.translation).normalize();
    assert!(
      pointing.dot(wanted) > 0.999,
      "the arm points {pointing}, the pointer is {wanted} away"
    );
    // The hero turns to face the pointer
    let flipped = engine.world().get::<Renderable>(hero).unwrap().flip_x;
    let at = engine.world().get::<Transform>(hero).unwrap().translation.x;
    assert_eq!(flipped, target.x < at);
  }
}

#[test]
fn the_crowd_page_animates_two_hundred_heroes() {
  let Some(mut engine) = engine_on(CROWD) else {
    return;
  };
  engine.step_frames(30).unwrap();
  let heroes = heroes(&mut engine);
  assert_eq!(heroes.len(), 200);
  let mut animations: Vec<String> = heroes
    .iter()
    .filter_map(|&hero| playing(&engine, hero, 0))
    .collect();
  assert_eq!(animations.len(), 200, "every hero plays an animation");
  animations.sort();
  animations.dedup();
  assert!(animations.len() >= 3, "varied animations: {animations:?}");
}

#[test]
fn the_crowd_starts_out_of_step() {
  let Some(mut engine) = engine_on(CROWD) else {
    return;
  };
  engine.step_frames(2).unwrap();
  let heroes = heroes(&mut engine);
  // How far through its loop each idling hero is as the page opens: the
  // idle lasts two seconds
  let phases: Vec<f32> = heroes
    .iter()
    .filter_map(|&hero| {
      let state = engine.world().get::<SkeletonAnimation>(hero)?;
      let entry = state.entry(state.current(0)?)?;
      (entry.animation() == "idle").then(|| (entry.track_time() / 2.0).fract())
    })
    .collect();
  assert!(phases.len() > 20, "idlers: {}", phases.len());
  // Each starts somewhere of its own in the loop: there are idlers in
  // every quarter of it
  for quarter in 0..4 {
    let from = quarter as f32 * 0.25;
    let within = phases
      .iter()
      .filter(|&&phase| phase >= from && phase < from + 0.25)
      .count();
    assert!(within > 0, "no idler in quarter {quarter}: {phases:?}");
  }
}
