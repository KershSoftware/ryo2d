//! A saved `.skeleton.ron` file reloads in place, and a broken save keeps the
//! last good skeleton, as the particle effects' files do

use std::time::{Duration, Instant};

use ryo2d::core::asset::pump;
use ryo2d::prelude::*;
use ryo2d::testing::{asset_world_over, capture_log};

const PATH: &str = "rigs/hero.skeleton.ron";
const IMAGE: &str = "rigs/hero.png";

fn skeleton(bones: &str) -> String {
  format!(
    r#"(
      images: {{ "parts": "hero.png" }},
      regions: {{ "torso": (image: "parts", rect: (0, 0, 8, 8)) }},
      bones: [{bones}],
    )"#
  )
}

fn png() -> Vec<u8> {
  let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([255; 4]));
  let mut bytes = Vec::new();
  image
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("encoding a png");
  bytes
}

fn bone_names(world: &World, handle: &Handle<SkeletonData>) -> Vec<String> {
  world
    .resource::<Assets<SkeletonData>>()
    .get(handle)
    .map(|data| {
      data
        .bones()
        .iter()
        .map(|bone| bone.name.to_string())
        .collect()
    })
    .unwrap_or_default()
}

/// Loads the skeleton at [`PATH`] on the workers and waits for it
fn load(world: &mut World) -> Handle<SkeletonData> {
  let handle = world.resource::<AssetServer>().load::<SkeletonData>(PATH);
  pump_until(world, |world| {
    world
      .resource::<Assets<SkeletonData>>()
      .get(&handle)
      .is_some()
  });
  handle
}

/// Saves `bytes` at `path` and tells the server, as the watcher does
fn save(world: &World, files: &MemorySource, path: &str, bytes: impl Into<Vec<u8>>) {
  files.insert(path, bytes.into());
  world.resource::<AssetServer>().reload(path);
}

/// Pumps until `done`, for at most 5 s
fn pump_until(world: &mut World, done: impl Fn(&World) -> bool) {
  let start = Instant::now();
  loop {
    pump(world);
    if done(world) {
      return;
    }
    assert!(
      start.elapsed() < Duration::from_secs(5),
      "timed out waiting"
    );
    std::thread::sleep(Duration::from_millis(1));
  }
}

#[test]
fn a_reload_keeps_the_asset_id_and_a_broken_reload_keeps_the_last_good_data() {
  let log = capture_log();
  let files = MemorySource::new();
  files.insert(IMAGE, png());
  files.insert(PATH, skeleton(r#"(name: "root")"#));
  let mut world = asset_world_over(files.clone());
  let handle = load(&mut world);
  let version = |world: &World| world.resource::<Assets<SkeletonData>>().version(&handle);
  let before = version(&world);
  assert_eq!(bone_names(&world, &handle), ["root"]);
  let mut events = world
    .resource::<Messages<AssetEvent<SkeletonData>>>()
    .get_cursor_current();

  // A save that loads: the same id, new data, a new version, `Modified`
  save(
    &world,
    &files,
    PATH,
    skeleton(r#"(name: "root"), (name: "arm", parent: "root")"#),
  );
  pump_until(&mut world, |world| version(world) != before);
  assert_eq!(bone_names(&world, &handle), ["root", "arm"]);
  let sent: Vec<_> = events
    .read(world.resource::<Messages<AssetEvent<SkeletonData>>>())
    .cloned()
    .collect();
  assert!(
    matches!(sent.as_slice(), [AssetEvent::Modified { id }] if *id == handle.id()),
    "{sent:?}"
  );
  let good = version(&world);

  // Saves that do not: a RON error, an unknown field, a bad rig and an image
  // that is gone. Each keeps the last good data and its version
  for broken in [
    "(bones: [(name: ".to_string(),
    skeleton(r#"(name: "root", lenght: 4)"#),
    skeleton(r#"(name: "root"), (name: "root")"#),
    skeleton(r#"(name: "root", parent: "ghost")"#),
  ] {
    save(&world, &files, PATH, broken.clone());
    let mut sent = Vec::new();
    let start = Instant::now();
    while !sent
      .iter()
      .any(|event| matches!(event, AssetEvent::ReloadFailed { .. }))
    {
      assert!(start.elapsed() < Duration::from_secs(5), "{sent:?}");
      std::thread::sleep(Duration::from_millis(1));
      pump(&mut world);
      sent.extend(
        events
          .read(world.resource::<Messages<AssetEvent<SkeletonData>>>())
          .cloned(),
      );
    }
    assert!(
      matches!(sent.as_slice(), [AssetEvent::ReloadFailed { id, .. }] if *id == handle.id()),
      "ReloadFailed, not Modified: {sent:?}"
    );
    assert_eq!(version(&world), good, "{broken}: the version is kept");
    assert_eq!(bone_names(&world, &handle), ["root", "arm"], "{broken}");
  }
  assert_eq!(log.containing(PATH), 4, "one warning per broken save");

  // A good save carries on, under the same id
  save(&world, &files, PATH, skeleton(r#"(name: "hips")"#));
  pump_until(&mut world, |world| version(world) != good);
  assert_eq!(bone_names(&world, &handle), ["hips"]);
}

#[test]
fn a_saved_image_checks_the_skeleton_again() {
  let files = MemorySource::new();
  files.insert(IMAGE, png());
  files.insert(PATH, skeleton(r#"(name: "root")"#));
  let mut world = asset_world_over(files.clone());
  let handle = load(&mut world);
  let version = |world: &World| world.resource::<Assets<SkeletonData>>().version(&handle);
  let before = version(&world);
  let mut events = world
    .resource::<Messages<AssetEvent<SkeletonData>>>()
    .get_cursor_current();
  // The region is 8 by 8: an image that shrinks under it fails the check,
  // and the skeleton keeps what it has
  let small = image::RgbaImage::from_pixel(4, 4, image::Rgba([255; 4]));
  let mut bytes = Vec::new();
  small
    .write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    )
    .expect("encoding a png");
  save(&world, &files, IMAGE, bytes);
  let start = Instant::now();
  let mut failed = false;
  while !failed {
    assert!(start.elapsed() < Duration::from_secs(5), "never rechecked");
    std::thread::sleep(Duration::from_millis(1));
    pump(&mut world);
    failed = events
      .read(world.resource::<Messages<AssetEvent<SkeletonData>>>())
      .any(|event| matches!(event, AssetEvent::ReloadFailed { id, .. } if *id == handle.id()));
  }
  assert_eq!(version(&world), before);
  assert_eq!(bone_names(&world, &handle), ["root"]);
}
