//! The UI pass and the image store: a scene's `ui` hook keeps the store, and
//! the UI sizes images as the store holds them when the pass starts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::core::asset::{Assets, DiskSource, Handle};
use crate::core::graphics::{Image, WgpuRenderer};
use crate::core::scene::{SceneAction, SceneContext};
use crate::core::ui::immediate::UiCommand;
use crate::core::ui::{Rect, UiArgs, UiContext, UiNode};
use glam::Vec2;

/// An engine over `scene` with a GPU (`None` without one), its assets read
/// from `source` if given
fn engine(scene: impl Scene + 'static, source: Option<DiskSource>) -> Option<Engine> {
  let (device, queue) = crate::core::testing::shared_gpu().ok()?;
  let mut builder = EngineBuilder::new()
    .resolution(320, 240)
    .fixed_hz(60)
    .start_scene(scene);
  if let Some(source) = source {
    builder = builder.asset_source(source);
  }
  let mut engine = builder.build_headless();
  engine.init_gpu(WgpuRenderer::new_offscreen(device, queue, 320, 240), 1.0);
  Some(engine)
}

fn step(engine: &mut Engine) {
  engine.frame(Some(Duration::from_micros(16_667)));
}

/// Where the last UI pass drew `image`
fn drawn(engine: &Engine, image: &Handle<Image>) -> Vec<Rect> {
  engine
    .ui_commands
    .commands()
    .filter_map(|command| match command {
      UiCommand::Sprite { sprite, rect, .. } if sprite == image => Some(*rect),
      _ => None,
    })
    .collect()
}

/// Where the last UI pass drew the text `text`
fn text_at(engine: &Engine, text: &str) -> Option<Vec2> {
  engine
    .ui_commands
    .commands()
    .find_map(|command| match command {
      UiCommand::Text { layout, origin, .. } if layout.text() == text => Some(*origin),
      _ => None,
    })
}

#[derive(Default)]
struct Seen {
  /// The image `on_enter` loaded, then what `ui` read of it
  loaded: Option<Handle<Image>>,
  got: Option<(u32, u32)>,
  /// What `ui` built from the file
  template: Option<Result<(), String>>,
  spawned: Option<Result<(), String>>,
}

/// Loads `icon.png` on enter; its first `ui` reads it through the store and
/// builds the file's template and root, which name the same image
struct BuildsInUi {
  file: String,
  seen: Arc<Mutex<Seen>>,
  built: bool,
}

impl Scene for BuildsInUi {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let loaded = ctx.assets().load_now::<Image>("icon.png").ok();
    self.seen.lock().unwrap().loaded = loaded;
  }

  fn ui(&mut self, _ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    if self.built {
      return SceneAction::Continue;
    }
    self.built = true;
    let mut seen = self.seen.lock().unwrap();
    if let Some(loaded) = seen.loaded.clone() {
      let store = ctx.world.resource::<Assets<Image>>();
      let in_store = store.get(&loaded).map(|i| (i.width(), i.height()));
      let in_view = ctx
        .assets()
        .get::<Image>(&loaded)
        .map(|i| (i.width(), i.height()));
      assert_eq!(in_store, in_view, "the store and the view agree");
      seen.got = in_view;
    }
    let template = ctx.ui_template(&self.file, "Icon", &UiArgs::new());
    seen.template = Some(template.map(|_| ()).map_err(|e| e.to_string()));
    let spawned = ctx.spawn_ui_file(&self.file);
    seen.spawned = Some(spawned.map(|_| ()).map_err(|e| e.to_string()));
    SceneAction::Continue
  }
}

#[test]
fn a_ui_hook_reads_the_image_store_and_builds_ui_files_that_name_images() {
  let dir = tempfile::tempdir().unwrap();
  image::RgbaImage::from_pixel(6, 4, image::Rgba([255, 0, 0, 255]))
    .save(dir.path().join("icon.png"))
    .unwrap();
  std::fs::write(
    dir.path().join("menu.ui.ron"),
    r#"(templates: {"Icon": (node: Image("icon.png"))}, root: Column(children: [Icon()]))"#,
  )
  .unwrap();
  let seen = Arc::new(Mutex::new(Seen::default()));
  let scene = BuildsInUi {
    file: "menu.ui.ron".to_string(),
    seen: Arc::clone(&seen),
    built: false,
  };
  let Some(mut e) = engine(scene, Some(DiskSource::new(dir.path()))) else {
    return;
  };
  for _ in 0..3 {
    step(&mut e);
  }

  let seen = seen.lock().unwrap();
  let loaded = seen.loaded.clone().expect("on_enter loads the image");
  assert_eq!(seen.got, Some((6, 4)), "ui reads the image");
  assert_eq!(seen.template, Some(Ok(())), "a template naming it builds");
  assert_eq!(seen.spawned, Some(Ok(())), "a file naming it spawns");
  let rects = drawn(&e, &loaded);
  assert_eq!(rects.len(), 1, "the spawned tree draws it: {rects:?}");
  assert_eq!((rects[0].width, rects[0].height), (6.0, 4.0));
}

/// Adds a 6 x 4 image on its first update and draws it at its natural size
/// from then on, beside a label
struct AddsInUpdate {
  image: Arc<Mutex<Option<Handle<Image>>>>,
}

impl Scene for AddsInUpdate {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    let mut image = self.image.lock().unwrap();
    if image.is_none() {
      *image = Some(
        ctx
          .assets()
          .add(Image::solid_rect(6, 4, 255, 255, 255, 255)),
      );
    }
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    if let Some(image) = self.image.lock().unwrap().as_ref() {
      ui.image(image).show();
    }
    SceneAction::Continue
  }
}

#[test]
fn an_image_added_in_update_is_sized_by_the_next_ui_pass() {
  let image = Arc::new(Mutex::new(None));
  let scene = AddsInUpdate {
    image: Arc::clone(&image),
  };
  let Some(mut e) = engine(scene, None) else {
    return;
  };
  let mut frames = 0;
  while image.lock().unwrap().is_none() {
    step(&mut e);
    frames += 1;
    assert!(frames < 10, "update runs");
  }
  let added = image.lock().unwrap().clone().unwrap();
  // The pass after the update that added it
  step(&mut e);
  let rects = drawn(&e, &added);
  assert_eq!(rects.len(), 1, "{rects:?}");
  assert_eq!((rects[0].width, rects[0].height), (6.0, 4.0));
}

/// Spawns a row of an 8 x 8 image and a label on enter; an update after
/// `resize` is set makes the image 24 x 8 through `get_mut`
struct ResizesInUpdate {
  image: Arc<Mutex<Option<Handle<Image>>>>,
  resize: Arc<AtomicBool>,
  resized: Arc<AtomicBool>,
}

impl Scene for ResizesInUpdate {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let image = ctx
      .assets()
      .add(Image::solid_rect(8, 8, 255, 255, 255, 255));
    ctx.spawn_ui(
      UiNode::row()
        .child(UiNode::image(image.clone()))
        .child(UiNode::text("after")),
    );
    *self.image.lock().unwrap() = Some(image);
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    if self.resize.swap(false, Ordering::SeqCst)
      && let Some(image) = self.image.lock().unwrap().as_ref()
      && let Some(mut store) = ctx.world.get_resource_mut::<Assets<Image>>()
      && let Some(shown) = store.get_mut(image)
    {
      *shown = Image::solid_rect(24, 8, 255, 255, 255, 255);
      self.resized.store(true, Ordering::SeqCst);
    }
    SceneAction::Continue
  }
}

#[test]
fn a_resize_through_get_mut_re_lays_out_the_ui() {
  let image = Arc::new(Mutex::new(None));
  let resize = Arc::new(AtomicBool::new(false));
  let resized = Arc::new(AtomicBool::new(false));
  let scene = ResizesInUpdate {
    image: Arc::clone(&image),
    resize: Arc::clone(&resize),
    resized: Arc::clone(&resized),
  };
  let Some(mut e) = engine(scene, None) else {
    return;
  };
  // Long enough for the row's subtree to be kept and replayed
  for _ in 0..4 {
    step(&mut e);
  }
  let image = image.lock().unwrap().clone().unwrap();
  let before = drawn(&e, &image);
  assert_eq!(before.len(), 1, "{before:?}");
  assert_eq!(before[0].width, 8.0);
  let label = text_at(&e, "after").expect("the label draws");

  resize.store(true, Ordering::SeqCst);
  let mut frames = 0;
  while !resized.load(Ordering::SeqCst) {
    step(&mut e);
    frames += 1;
    assert!(frames < 10, "update resizes it");
  }
  step(&mut e);
  let after = drawn(&e, &image);
  assert_eq!(after.len(), 1, "{after:?}");
  assert_eq!((after[0].width, after[0].height), (24.0, 8.0));
  let moved = text_at(&e, "after").expect("the label draws");
  assert_eq!(moved.x, label.x + 16.0, "the label moves over");
}

/// Spawns a UI file that names `icon.png` on its first `ui`, which is where
/// the image first loads, and draws the image itself at its natural size on
/// every pass from then on
struct LoadsInUi {
  file: String,
  image: Arc<Mutex<Option<Handle<Image>>>>,
}

impl Scene for LoadsInUi {
  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) -> SceneAction {
    let mut image = self.image.lock().unwrap();
    if image.is_none() {
      ctx.spawn_ui_file(&self.file).expect("the file spawns");
      // The file loaded it: this finds it and loads nothing
      *image = Some(ctx.assets().load::<Image>("icon.png"));
    }
    if let Some(image) = image.as_ref() {
      ui.image(image).show();
    }
    SceneAction::Continue
  }
}

#[test]
fn an_image_a_ui_file_first_loads_in_ui_is_sized_from_the_next_pass() {
  let dir = tempfile::tempdir().unwrap();
  image::RgbaImage::from_pixel(6, 4, image::Rgba([255, 0, 0, 255]))
    .save(dir.path().join("icon.png"))
    .unwrap();
  std::fs::write(
    dir.path().join("icon.ui.ron"),
    r#"(root: Column(children: [Image("icon.png")]))"#,
  )
  .unwrap();
  let image = Arc::new(Mutex::new(None));
  let scene = LoadsInUi {
    file: "icon.ui.ron".to_string(),
    image: Arc::clone(&image),
  };
  let Some(mut e) = engine(scene, Some(DiskSource::new(dir.path()))) else {
    return;
  };
  let mut frames = 0;
  while image.lock().unwrap().is_none() {
    step(&mut e);
    frames += 1;
    assert!(frames < 10, "ui runs");
  }
  let loaded = image.lock().unwrap().clone().unwrap();
  assert!(
    e.world.resource::<Assets<Image>>().get(&loaded).is_some(),
    "the file loaded it at once"
  );
  // The pass that loaded it sized it as not loaded, so drew nothing of it
  assert_eq!(drawn(&e, &loaded), Vec::<Rect>::new());
  // The next pass draws it at its size, in the tree and in the scene's own UI
  step(&mut e);
  let rects = drawn(&e, &loaded);
  assert_eq!(rects.len(), 2, "the tree's and the scene's: {rects:?}");
  for rect in rects {
    assert_eq!((rect.width, rect.height), (6.0, 4.0));
  }
}
