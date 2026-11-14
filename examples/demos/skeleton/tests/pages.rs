//! The demo's pages without a window: each steps for 120 frames with its
//! skeletons posed and no warning logged, from the folder and from the files
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
const MESHES: usize = 7;
const PAGES: usize = 8;

/// The skeleton files the pages show: the hero on every page, and the
/// banner beside it on the Meshes page
const HERO: &str = "hero.skeleton.ron";
const BANNER: &str = "banner.skeleton.ron";

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
fn skeletons(engine: &mut HeadlessEngine) -> Vec<Entity> {
  let world = engine.world_mut();
  let mut skeletons: Vec<Entity> = world
    .query_filtered::<Entity, With<Skeleton>>()
    .iter(world)
    .collect();
  skeletons.sort();
  skeletons
}

/// The skeleton entities that show the skeleton file at `path`, in entity
/// order
fn showing(engine: &mut HeadlessEngine, path: &str) -> Vec<Entity> {
  let mut skeletons = skeletons(engine);
  let world = engine.world();
  let server = world.resource::<AssetServer>();
  skeletons.retain(|&entity| {
    let data = world.get::<Skeleton>(entity).unwrap().data();
    server
      .path(data)
      .is_some_and(|loaded| loaded.as_str() == path)
  });
  skeletons
}

/// The heroes on the page, in entity order
fn heroes(engine: &mut HeadlessEngine) -> Vec<Entity> {
  showing(engine, HERO)
}

/// The names of the bones `skeleton`'s data has
fn bones(engine: &mut HeadlessEngine, skeleton: Entity) -> Vec<String> {
  let data = engine
    .world()
    .get::<Skeleton>(skeleton)
    .unwrap()
    .data()
    .clone();
  let assets = engine.assets();
  let data = assets.get(&data).expect("the skeleton's data is loaded");
  data
    .bones()
    .iter()
    .map(|bone| bone.name.to_string())
    .collect()
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

/// Steps every page for two seconds, checking that it has a hero, that
/// each of its skeletons (the heroes, and the Meshes page's banner) is
/// posed and drawn as the engine asks of a figure of many parts, and that
/// nothing warns
fn every_page_steps(mut engine_on: impl FnMut(usize) -> Option<HeadlessEngine>) {
  for page in 0..PAGES {
    // A capture for each page, opened before its engine
    let log = ryo2d::testing::capture_log();
    let Some(mut engine) = engine_on(page) else {
      return;
    };
    engine.step_frames(FRAMES).unwrap();
    assert!(
      !heroes(&mut engine).is_empty(),
      "page {}: no hero",
      page + 1
    );
    for skeleton in skeletons(&mut engine) {
      let bones = bones(&mut engine, skeleton);
      let world = engine.world();
      // Posed: every bone its data has is somewhere in the world
      let pose = world.get::<Skeleton>(skeleton).unwrap();
      assert!(
        !bones.is_empty(),
        "page {}: a skeleton of no bones",
        page + 1
      );
      for bone in &bones {
        assert!(
          pose.bone_world(bone).is_some(),
          "page {}: a skeleton's {bone} bone has no pose",
          page + 1
        );
      }
      // Smooth bones, and a shadow that suits a figure of many parts
      assert!(
        world.get::<Interpolated>(skeleton).is_some(),
        "page {}: a skeleton is not interpolated",
        page + 1
      );
      let renderable = world.get::<Renderable>(skeleton).unwrap();
      assert!(renderable.is_skeleton());
      assert!(
        renderable.has_blob_shadow() || renderable.has_contact_shadow(),
        "page {}: a skeleton casts no ground shadow",
        page + 1
      );
      assert!(
        !renderable.has_drop_shadow(),
        "page {}: a skeleton casts a drop shadow",
        page + 1
      );
    }
    // The cape is the Meshes page's own: on every other page no hero wears
    // its skin, so the cape's slot shows nothing there
    for hero in heroes(&mut engine) {
      let wears = worn(&engine, hero).iter().any(|skin| skin == "cape");
      assert_eq!(wears, page == MESHES, "page {}: the cape's skin", page + 1);
      assert_eq!(
        shown(&engine, hero, "cape").is_some(),
        page == MESHES,
        "page {}: the cape's slot",
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

// --- The Meshes page ---

/// The page's one skeleton of the file at `path`
fn one(engine: &mut HeadlessEngine, path: &str) -> Entity {
  let shown = showing(engine, path);
  assert_eq!(shown.len(), 1, "one skeleton of {path} on the page");
  shown[0]
}

/// The mesh `skin` of `skeleton`'s data has in `slot` under `name`
fn mesh(
  engine: &mut HeadlessEngine,
  skeleton: Entity,
  skin: &str,
  slot: &str,
  name: &str,
) -> MeshAttachment {
  let data = engine
    .world()
    .get::<Skeleton>(skeleton)
    .unwrap()
    .data()
    .clone();
  let assets = engine.assets();
  let data = assets.get(&data).expect("the skeleton's data is loaded");
  let slot_id = data.slot(slot).unwrap_or_else(|| panic!("the {slot} slot"));
  match data
    .skin(skin)
    .and_then(|skin| skin.attachment(slot_id, name))
  {
    Some(SkeletonAttachment::Mesh(mesh)) => mesh.clone(),
    other => panic!("{name} in {slot} of the {skin} skin is {other:?}, not a mesh"),
  }
}

/// What `skeleton`'s slot shows
fn shown(engine: &HeadlessEngine, skeleton: Entity, slot: &str) -> Option<String> {
  let skeleton = engine.world().get::<Skeleton>(skeleton).unwrap();
  skeleton.attachment(slot).map(str::to_owned)
}

/// The skins `skeleton` wears
fn worn(engine: &HeadlessEngine, skeleton: Entity) -> Vec<String> {
  let skeleton = engine.world().get::<Skeleton>(skeleton).unwrap();
  skeleton.skins().map(str::to_owned).collect()
}

/// The engine's last frame: its size and its pixels, four bytes each
struct Shot {
  width: u32,
  height: u32,
  pixels: Vec<u8>,
}

fn shot(engine: &HeadlessEngine) -> Shot {
  let frame = engine.frame().unwrap();
  Shot {
    width: frame.width(),
    height: frame.height(),
    pixels: frame.into_raw(),
  }
}

/// A rectangle of the frame, in its pixels: `right` and `bottom` are one
/// past its last column and row
#[derive(Debug, Clone, Copy)]
struct Area {
  left: u32,
  top: u32,
  right: u32,
  bottom: u32,
}

impl Area {
  fn points(self) -> impl Iterator<Item = (u32, u32)> {
    (self.top..self.bottom).flat_map(move |y| (self.left..self.right).map(move |x| (x, y)))
  }
}

impl Shot {
  fn at(&self, x: u32, y: u32) -> [u8; 3] {
    let index = (y as usize * self.width as usize + x as usize) * 4;
    [
      self.pixels[index],
      self.pixels[index + 1],
      self.pixels[index + 2],
    ]
  }

  /// The whole frame
  fn all(&self) -> Area {
    Area {
      left: 0,
      top: 0,
      right: self.width,
      bottom: self.height,
    }
  }

  /// The box round `points` (in the frame's pixels) grown by `margin`,
  /// kept within the frame
  fn round(&self, points: impl IntoIterator<Item = Vec2>, margin: f32) -> Area {
    let (mut low, mut high) = (Vec2::MAX, Vec2::MIN);
    for point in points {
      low = low.min(point);
      high = high.max(point);
    }
    let (low, high) = (low - margin, high + margin);
    Area {
      left: low.x.max(0.0) as u32,
      top: low.y.max(0.0) as u32,
      right: (high.x.max(0.0) as u32).min(self.width),
      bottom: (high.y.max(0.0) as u32).min(self.height),
    }
  }

  /// The pixels of `area` that `is` holds for
  fn count(&self, area: Area, is: impl Fn([u8; 3]) -> bool) -> usize {
    area.points().filter(|&(x, y)| is(self.at(x, y))).count()
  }

  /// The box round the pixels of `area` that `is` holds for
  fn bounds(&self, area: Area, is: impl Fn([u8; 3]) -> bool) -> Option<Area> {
    let mut bounds: Option<Area> = None;
    for (x, y) in area.points().filter(|&(x, y)| is(self.at(x, y))) {
      let b = bounds.get_or_insert(Area {
        left: x,
        top: y,
        right: x + 1,
        bottom: y + 1,
      });
      b.left = b.left.min(x);
      b.top = b.top.min(y);
      b.right = b.right.max(x + 1);
      b.bottom = b.bottom.max(y + 1);
    }
    bounds
  }

  /// The first row down column `x` of `area` whose pixel `is` holds for
  fn top(&self, area: Area, x: u32, is: impl Fn([u8; 3]) -> bool) -> Option<u32> {
    (area.top..area.bottom).find(|&y| is(self.at(x, y)))
  }

  /// The pixels of `area` that differ from `other`'s by more than `by` on
  /// some channel
  fn differing(&self, other: &Shot, area: Area, by: u8) -> usize {
    area
      .points()
      .filter(|&(x, y)| {
        let (a, b) = (self.at(x, y), other.at(x, y));
        (0..3).any(|channel| a[channel].abs_diff(b[channel]) > by)
      })
      .count()
  }
}

/// Whether `pixel` is within `tolerance` of `color` on every channel
fn near(pixel: [u8; 3], color: [u8; 3], tolerance: u8) -> bool {
  (0..3).all(|channel| pixel[channel].abs_diff(color[channel]) <= tolerance)
}

/// The cape's cloth, its folds and its hem, as `assets/README.md` draws
/// them, and the red of the hero's shirt
const CAPE: [[u8; 3]; 3] = [[242, 182, 50], [224, 159, 31], [201, 134, 26]];
const SHIRT: [u8; 3] = [224, 88, 72];

fn is_cape(pixel: [u8; 3]) -> bool {
  CAPE.iter().any(|&color| near(pixel, color, 6))
}

/// The overlay's colours: a mesh's triangles, and its hull (which a
/// region's outline shares)
const TRIANGLES: [u8; 3] = [0x30, 0x50, 0xb0];
const HULL: [u8; 3] = [0x60, 0x90, 0xff];

/// Whether `pixel` is `color` laid over `under`, wholly or in part: the
/// overlay's lines are a pixel wide and smooth, so one covers the pixels it
/// crosses in part, and what shows is between the line's colour and what
/// was there
fn laid_over(pixel: [u8; 3], under: [u8; 3], color: [u8; 3]) -> bool {
  let channel = |c: usize| {
    (
      f32::from(pixel[c]),
      f32::from(under[c]),
      f32::from(color[c]),
    )
  };
  // How much of the line covers the pixel, by the channel the line changes
  // most
  let most = (0..3)
    .max_by_key(|&c| under[c].abs_diff(color[c]))
    .unwrap_or(0);
  let (shown, was, line) = channel(most);
  if (line - was).abs() < 24.0 {
    return false;
  }
  let cover = (shown - was) / (line - was);
  (0.3..=1.1).contains(&cover)
    && (0..3).all(|c| {
      let (shown, was, line) = channel(c);
      (shown - (was + cover * (line - was))).abs() <= 10.0
    })
}

/// Where `at` in the world is in the frame
fn to_frame(engine: &mut HeadlessEngine, at: Vec2) -> Vec2 {
  CameraMut::main(engine.world_mut())
    .expect("the main camera")
    .world_to_screen(at)
}

/// The part of the frame the banner's flag flies in: the box round its
/// mesh's vertices where its bone puts them, with room for the ripple
fn flag_area(engine: &mut HeadlessEngine, banner: Entity, frame: &Shot) -> Area {
  let flag = mesh(engine, banner, "default", "flag", "flag");
  let MeshVertices::Unweighted(vertices) = &flag.vertices else {
    panic!("the flag is weighted");
  };
  let bone = bone(engine, banner, "flag");
  let corners: Vec<Vec2> = vertices
    .iter()
    .map(|&vertex| to_frame(engine, bone.transform_point2(vertex)))
    .collect();
  frame.round(corners, 24.0)
}

/// The part of the frame the hero and its cape are in
fn hero_area(engine: &mut HeadlessEngine, hero: Entity, frame: &Shot) -> Area {
  let feet = engine.world().get::<Transform>(hero).unwrap().translation;
  let feet = to_frame(engine, feet);
  // From below the head to the ground, and as far to each side as the
  // cape streams
  frame.round(
    [
      feet + Vec2::new(-150.0, -170.0),
      feet + Vec2::new(150.0, 0.0),
    ],
    0.0,
  )
}

/// The frames one loop of the banner's ripple lasts, at the engine's 60
/// steps a second
fn ripple_frames(engine: &mut HeadlessEngine, banner: Entity) -> u32 {
  let data = engine
    .world()
    .get::<Skeleton>(banner)
    .unwrap()
    .data()
    .clone();
  let assets = engine.assets();
  let ripple = assets
    .get(&data)
    .and_then(|data| data.animation("ripple"))
    .expect("the banner's ripple");
  let frames = ripple.duration * 60.0;
  assert!(
    (frames - frames.round()).abs() < 1e-3,
    "the ripple loops on a step: {frames} frames"
  );
  frames.round() as u32
}

#[test]
fn the_meshes_page_shows_a_weighted_cape_and_a_deformed_banner() {
  let Some(mut engine) = engine_on(MESHES) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let hero = one(&mut engine, HERO);
  let banner = one(&mut engine, BANNER);

  // The hero walks in the cape's skin, and the cape's slot shows its mesh:
  // weighted, over the torso and the two cape bones
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));
  assert_eq!(worn(&engine, hero), ["cape"]);
  assert_eq!(shown(&engine, hero, "cape").as_deref(), Some("cape"));
  let cape = mesh(&mut engine, hero, "cape", "cape", "cape");
  let MeshVertices::Weighted(weights) = &cape.vertices else {
    panic!("the cape is not weighted");
  };
  let names = bones(&mut engine, hero);
  let mut pulling: Vec<&str> = (0..weights.vertex_count())
    .flat_map(|vertex| weights.influences(vertex))
    .map(|influence| names[influence.bone.index()].as_str())
    .collect();
  pulling.sort_unstable();
  pulling.dedup();
  assert_eq!(pulling, ["cape_lower", "cape_upper", "torso"]);
  // Some of its vertices bend between two bones
  assert!((0..weights.vertex_count()).any(|vertex| weights.influences(vertex).len() > 1));
  // It is drawn: its cloth is in the frame, round where the hero stands
  let frame = shot(&engine);
  let around = hero_area(&mut engine, hero, &frame);
  let cloth = frame.count(around, is_cape);
  assert!(cloth > 400, "the cape's cloth: {cloth} pixels");

  // The banner's slot shows its mesh: unweighted, on the one bone, with
  // the ripple's deform keys moving a vertex each
  assert_eq!(playing(&engine, banner, 0).as_deref(), Some("ripple"));
  assert_eq!(shown(&engine, banner, "flag").as_deref(), Some("flag"));
  let flag = mesh(&mut engine, banner, "default", "flag", "flag");
  assert!(matches!(flag.vertices, MeshVertices::Unweighted(_)));
  let data = engine
    .world()
    .get::<Skeleton>(banner)
    .unwrap()
    .data()
    .clone();
  let assets = engine.assets();
  let data = assets.get(&data).unwrap();
  let slot = data.slot("flag").unwrap();
  let ripple = data.animation("ripple").unwrap();
  let keyed = ripple.timelines.iter().any(|timeline| {
    matches!(
      timeline,
      SkeletonTimeline::Deform { slot: keyed, attachment, frames }
        if *keyed == slot
          && &**attachment == "flag"
          && frames.iter().any(|(_, offsets, _)| offsets.len() == flag.positions() * 2)
    )
  });
  assert!(keyed, "the ripple keys the flag's shape, a vertex each");
}

#[test]
fn the_cape_s_bones_are_keyed_in_idle_walk_and_run_and_the_gait_row_picks_one() {
  let Some(mut engine) = engine_on(MESHES) else {
    return;
  };
  engine.step_frames(10).unwrap();
  let hero = one(&mut engine, HERO);
  {
    let data = engine.world().get::<Skeleton>(hero).unwrap().data().clone();
    let assets = engine.assets();
    let data = assets.get(&data).unwrap();
    for animation in ["idle", "walk", "run"] {
      let timelines = &data.animation(animation).unwrap().timelines;
      for bone in ["cape_upper", "cape_lower"] {
        let id = data.bone(bone).unwrap_or_else(|| panic!("the {bone} bone"));
        let keyed = timelines
          .iter()
          .any(|timeline| matches!(timeline, SkeletonTimeline::Rotate { bone, .. } if *bone == id));
        assert!(keyed, "{animation} keys {bone}");
      }
    }
  }
  // The panel's first row is the gait: walk, run, idle
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("walk"));
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("run"));
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  assert_eq!(playing(&engine, hero, 0).as_deref(), Some("idle"));
  // Standing, the cape hangs: its lower bone points down, near enough
  engine.step_frames(60).unwrap();
  let hangs = bone(&engine, hero, "cape_lower").matrix2.x_axis.normalize();
  assert!(hangs.y > 0.95, "the cape hangs {hangs}");
}

#[test]
fn the_cape_is_a_skin_the_page_takes_off_and_puts_on() {
  let Some(mut engine) = engine_on(MESHES) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let hero = one(&mut engine, HERO);
  let cloth = |engine: &mut HeadlessEngine| {
    let frame = shot(engine);
    let around = hero_area(engine, hero, &frame);
    frame.count(around, is_cape)
  };
  assert!(cloth(&mut engine) > 400);

  // Rows: the gait, the cape, the ripple's speed
  engine.tap_key(KeyCode::ArrowDown);
  engine.step_frames(2).unwrap();
  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  // Without its skin the cape's slot shows nothing: the mesh is no skin's
  // that the hero wears
  assert!(worn(&engine, hero).is_empty());
  assert_eq!(shown(&engine, hero, "cape"), None);
  assert_eq!(cloth(&mut engine), 0, "the cape is off");

  engine.tap_key(KeyCode::ArrowRight);
  engine.step_frames(5).unwrap();
  assert_eq!(worn(&engine, hero), ["cape"]);
  assert!(cloth(&mut engine) > 400, "the cape is on again");
}

#[test]
fn the_banner_s_pixels_change_through_its_deform_and_return_at_the_loop() {
  let Some(mut engine) = engine_on(MESHES) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let banner = one(&mut engine, BANNER);
  let cycle = ripple_frames(&mut engine, banner);
  let start = shot(&engine);
  let flag = flag_area(&mut engine, banner, &start);
  let bone_at_start = bone(&engine, banner, "flag");

  // One loop of the ripple, a step at a time: how many of the flag's
  // pixels each step changes, and the flag a third of the way round
  let mut steps = Vec::new();
  let mut last = shot(&engine);
  let mut part = None;
  for step in 1..=cycle {
    engine.step().unwrap();
    let frame = shot(&engine);
    steps.push(last.differing(&frame, flag, 24));
    if step == cycle / 3 {
      part = Some(start.differing(&frame, flag, 24));
    }
    last = frame;
  }

  // A third of the way round its loop the flag is another shape
  let moved = part.unwrap();
  assert!(
    moved > 300,
    "the flag moved {moved} pixels in a third of a loop"
  );
  // A whole loop on it is the shape it was, to the pixel
  let off = start.differing(&last, flag, 4);
  assert_eq!(off, 0, "the flag after one loop differs in {off} pixels");
  // And no step of the loop jumps, the step that wraps round to the first
  // key among them: the last key is the first again, so the flag moves
  // about as far in every step
  let most = *steps.iter().max().unwrap();
  steps.sort_unstable();
  let usual = steps[steps.len() / 2];
  assert!(usual > 0, "the flag moves every step");
  assert!(
    most <= usual * 2,
    "a step changed {most} of the flag's pixels, where a step usually changes {usual}"
  );

  // The bone it hangs on stands still: only the deform's keys move it
  let after = bone(&engine, banner, "flag");
  assert!(
    bone_at_start.abs_diff_eq(after, 1e-4),
    "the flag's bone moved"
  );
}

#[test]
fn a_hit_flash_whitens_the_cape_with_the_body() {
  let Some(mut engine) = engine_on(MESHES) else {
    return;
  };
  engine.step_frames(20).unwrap();
  let hero = one(&mut engine, HERO);
  let colours = |engine: &mut HeadlessEngine| {
    let frame = shot(engine);
    let around = hero_area(engine, hero, &frame);
    (
      frame.count(around, is_cape),
      frame.count(around, |pixel| near(pixel, SHIRT, 6)),
    )
  };
  let (cape, shirt) = colours(&mut engine);
  assert!(cape > 400 && shirt > 400, "cape {cape}, shirt {shirt}");

  // Space flashes the hero: a mesh takes the entity's effect as its
  // regions do, so the cape whitens with the shirt
  engine.tap_key(KeyCode::Space);
  engine.step_frames(2).unwrap();
  assert_eq!(colours(&mut engine), (0, 0), "cape and shirt, flashing");

  // And both are back when the flash is over
  engine.step_frames(60).unwrap();
  let (cape, shirt) = colours(&mut engine);
  assert!(cape > 400 && shirt > 400, "cape {cape}, shirt {shirt}");
  assert!(
    engine
      .world()
      .get::<Renderable>(hero)
      .unwrap()
      .shader
      .is_none(),
    "the flash is taken off when it is over"
  );
}

#[test]
fn the_console_s_overlay_draws_each_mesh_s_triangles_and_hull_on_the_mesh() {
  // The console and its overlays are developer tools: a build without
  // them has neither
  if !ryo2d::DEV_TOOLS {
    return;
  }
  // The page twice over, in step: one as it is, and one that will have
  // the overlay on, so what differs between their frames is the overlay
  let (Some(mut off), Some(mut on)) = (engine_on(MESHES), engine_on(MESHES)) else {
    return;
  };
  off.step_frames(20).unwrap();
  on.step_frames(20).unwrap();
  let before = shot(&off);
  assert_eq!(
    before.differing(&shot(&on), before.all(), 0),
    0,
    "the two draw one frame before the overlay is on"
  );
  let hero = one(&mut off, HERO);
  let banner = one(&mut off, BANNER);
  let cycle = ripple_frames(&mut off, banner);
  let flag = flag_area(&mut off, banner, &before);
  let sky = before.at(flag.right - 1, flag.top);
  let width = flag.right - flag.left;
  // Clear of the pole, whose outline is the hull's colour, and of the
  // fly's end, where the hull turns down
  let columns: Vec<u32> = (flag.left + width / 4..flag.right - width / 5)
    .step_by(8)
    .collect();

  on.console_run("debug:skeletons");
  // Two moments of the ripple, and of the walk
  let mut edges = Vec::new();
  for wait in [20, cycle / 3] {
    off.step_frames(wait).unwrap();
    on.step_frames(wait).unwrap();
    let (plain, drawn) = (shot(&off), shot(&on));
    // A line of the overlay's `color` crosses the pixel
    let line = |x: u32, y: u32, color: [u8; 3]| laid_over(drawn.at(x, y), plain.at(x, y), color);
    let lines = |area: Area, color: [u8; 3]| -> Vec<(u32, u32)> {
      area.points().filter(|&(x, y)| line(x, y, color)).collect()
    };

    // The banner. Its triangles are drawn across the flag
    let across = lines(flag, TRIANGLES).len();
    assert!(across > 150, "the flag's triangles: {across} pixels");
    // And its hull lies along the flag's own edge: down each column, the
    // hull's line is where the cloth begins
    let edge: Vec<u32> = columns
      .iter()
      .map(|&x| {
        let cloth = plain
          .top(flag, x, |pixel| !near(pixel, sky, 24))
          .expect("the flag's cloth");
        let hull = (flag.top..flag.bottom)
          .find(|&y| line(x, y, HULL))
          .expect("the flag's hull");
        assert!(
          hull.abs_diff(cloth) <= 4,
          "column {x}: the hull is at {hull}, the cloth begins at {cloth}"
        );
        hull
      })
      .collect();
    edges.push(edge);

    // The cape. Its triangles are drawn across it
    let around = hero_area(&mut off, hero, &plain);
    let across = lines(around, TRIANGLES).len();
    assert!(across > 150, "the cape's triangles: {across} pixels");
    // And its hull runs along its hem. Where the cape trails behind the
    // hero, clear of the legs, the lowest cloth down a column is the hem,
    // and the hull's line is just under it: the hull is a straight edge 3
    // to 16 of the cape's units outside a hem that is scalloped, which is
    // up to 7 pixels at the hero's size, and more down a column where the
    // cape slants
    let cloth = plain.bounds(around, is_cape).expect("the cape's cloth");
    let third = (cloth.right - cloth.left) / 3;
    let flipped = off.world().get::<Renderable>(hero).unwrap().flip_x;
    // Short of the hem's corner, where the hull turns up the cape's side
    let trailing = if flipped {
      cloth.right - third..cloth.right - 6
    } else {
      cloth.left + 6..cloth.left + third
    };
    let mut hemmed = 0;
    for x in trailing.step_by(3) {
      let hem = (cloth.top..cloth.bottom)
        .rev()
        .find(|&y| is_cape(plain.at(x, y)))
        .expect("the cape's hem");
      assert!(
        (hem..=hem + 12).any(|y| line(x, y, HULL)),
        "column {x}: the cape's hem is at {hem}, and no hull is drawn under it"
      );
      hemmed += 1;
    }
    assert!(hemmed >= 6, "columns of the cape's hem looked at: {hemmed}");
  }
  // The hull moves with the deform: its line is elsewhere at the second
  // moment
  let moved = edges[0]
    .iter()
    .zip(&edges[1])
    .map(|(a, b)| a.abs_diff(*b))
    .max()
    .unwrap();
  assert!(moved >= 3, "the hull moved {moved} pixels with the ripple");

  // Off again, the lines go
  on.console_run("debug:skeletons");
  off.step_frames(2).unwrap();
  on.step_frames(2).unwrap();
  let after = shot(&off);
  assert_eq!(
    after.differing(&shot(&on), after.all(), 0),
    0,
    "the two draw one frame again with the overlay off"
  );
}
