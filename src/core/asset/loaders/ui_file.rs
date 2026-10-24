//! UI files from `.ui.ron` text

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::ecs::kernel::World;
use crate::core::ui::UiFile;

/// Loads [`UiFile`]s from `.ui.ron` files. The files a file imports and the
/// images it names are its dependencies (asset paths, loaded through the
/// registry), so a file is ready once they are, and an import that leads
/// back to the file fails it with `AssetError::Cycle`; one that does not
/// load fails it at the position the file names it. Once they load, the
/// file's root and every template are built once against them (`check`),
/// so a mistake anywhere fails the load with every issue's line and column
/// (`AssetError::Decode` over a `UiFileError`); a file importing one that
/// reloads is checked again.
///
/// Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct UiFileLoader;

impl AssetLoader for UiFileLoader {
  type Asset = UiFile;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["ui.ron"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<UiFile, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    UiFile::load(&path, &text, cx)
  }

  fn check(&self, file: &UiFile, world: &World) -> Result<(), AssetError> {
    file.check(world)
  }

  /// An import or image that did not load, at the position the file names
  /// it, as `load_now` reports it
  fn dependency_failed(&self, file: &UiFile, error: &AssetError) -> Option<AssetError> {
    let AssetError::Dependency {
      path,
      dependency,
      error,
    } = error
    else {
      return None;
    };
    let issues = file.dependency_issues(dependency, error)?;
    Some(AssetError::decode(path, issues))
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use std::sync::Arc;

  use super::UiFileLoader;
  use crate::core::asset::AssetEvent;
  use crate::core::asset::{
    AssetError, AssetLoader, AssetPath, AssetServer, AssetSource, Assets, AssetsView, LoadState,
    MemorySource, insert_registry, pump,
  };
  use crate::core::ecs::kernel::{MessageCursor, Messages, World};
  use crate::core::graphics::Image;
  use crate::core::ui::{UiFile, UiFileError};

  /// A 2 x 2 PNG
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

  /// A menu importing a file of templates and naming an icon
  fn menu_files() -> MemorySource {
    let files = MemorySource::new();
    files.insert(
      "ui/menu.ui.ron",
      r#"(imports: ["ui/common.ui.ron"], root: Column(children: [Label(text: "Play"), Image("icons/play.png")]))"#,
    );
    files.insert(
      "ui/common.ui.ron",
      r#"(templates: {"Label": (params: ["text"], node: Text(text))})"#,
    );
    files.insert("icons/play.png", png());
    files
  }

  fn registry_over(source: impl AssetSource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  /// Files whose import and image are not read until the test lets each go
  struct Held {
    files: MemorySource,
    import: crossbeam_channel::Receiver<()>,
    image: crossbeam_channel::Receiver<()>,
  }

  impl AssetSource for Held {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      // Each returns when the test sends, or drops its sender
      match path.as_str() {
        "ui/common.ui.ron" => {
          let _ = self.import.recv();
        }
        "icons/play.png" => {
          let _ = self.image.recv();
        }
        _ => {}
      }
      self.files.read(path)
    }
  }

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
  fn a_file_is_ready_only_once_its_import_and_its_image_are() {
    let (import, held_import) = crossbeam_channel::unbounded();
    let (image, held_image) = crossbeam_channel::unbounded();
    let mut world = registry_over(Held {
      files: menu_files(),
      import: held_import,
      image: held_image,
    });
    let server = world.resource::<AssetServer>().clone();
    // One worker, as a small machine has: the import is read first, then the image
    server.limit_workers(1);
    let menu = server.load::<UiFile>("ui/menu.ui.ron");
    // The menu is read, and waits for both
    pump_until(&mut world, |_| server.waiting_count() == 1);
    assert!(!server.is_ready(&menu));
    let _ = import.send(());
    let common = server.load::<UiFile>("ui/common.ui.ron");
    pump_until(&mut world, |_| server.is_ready(&common));
    assert!(!server.is_ready(&menu), "the image is still loading");
    assert!(world.resource::<Assets<UiFile>>().get(&menu).is_none());
    let _ = image.send(());
    pump_until(&mut world, |_| server.is_ready(&menu));
    let icon = server.load::<Image>("icons/play.png");
    assert!(server.is_ready(&icon));
    let built = menu.root(&mut AssetsView::new(&mut world));
    assert!(built.is_ok(), "{built:?}");
  }

  /// Two files importing each other
  fn cycle() -> MemorySource {
    let files = MemorySource::new();
    files.insert(
      "ui/a.ui.ron",
      r#"(imports: ["ui/b.ui.ron"], templates: {"A": (node: Column)})"#,
    );
    files.insert(
      "ui/b.ui.ron",
      r#"(imports: ["ui/a.ui.ron"], templates: {"B": (node: Column)})"#,
    );
    files
  }

  #[test]
  fn an_import_cycle_fails_with_cycle_loaded_now() {
    let mut world = registry_over(cycle());
    let loaded = AssetsView::new(&mut world).load_now::<UiFile>("ui/a.ui.ron");
    let Err(error) = loaded else {
      panic!("a file importing itself loads");
    };
    assert!(
      matches!(&*error, AssetError::Cycle { chain, .. } if chain == "ui/a.ui.ron → ui/b.ui.ron → ui/a.ui.ron"),
      "{}",
      error.report()
    );
  }

  #[test]
  fn an_import_cycle_fails_with_cycle_loaded_on_the_workers() {
    let mut world = registry_over(cycle());
    let server = world.resource::<AssetServer>().clone();
    let a = server.load::<UiFile>("ui/a.ui.ron");
    pump_until(&mut world, |_| {
      !matches!(server.state(&a), LoadState::Loading)
    });
    let LoadState::Failed(error) = server.state(&a) else {
      panic!("a file importing itself loads: {:?}", server.state(&a));
    };
    assert!(
      matches!(&*error, AssetError::Cycle { .. }),
      "{}",
      error.report()
    );
  }

  #[test]
  fn a_bad_template_reference_fails_with_its_line_and_column() {
    let files = MemorySource::new();
    files.insert(
      "ui/menu.ui.ron",
      "(templates: {\"Item\": (node: Text(\"a\"))},\n  root: Column(children: [\n    Itme,\n  ]))",
    );
    let mut world = registry_over(files);
    let Err(error) = AssetsView::new(&mut world).load_now::<UiFile>("ui/menu.ui.ron") else {
      panic!("a file using a template it lacks loads");
    };
    let AssetError::Decode { source, .. } = &*error else {
      panic!("not a decode error: {}", error.report());
    };
    let Some(file) = source.downcast_ref::<UiFileError>() else {
      panic!("not the file's mistakes: {}", error.report());
    };
    let issue = &file.issues[0];
    assert_eq!(
      (issue.path.as_str(), issue.line, issue.column),
      ("ui/menu.ui.ron", 3, 5)
    );
    assert_eq!(
      issue.message,
      "unknown node kind \"Itme\"; did you mean \"Item\"?"
    );
  }

  #[test]
  fn a_file_loaded_at_once_is_checked_again_once_an_import_changes() {
    let mut world = registry_over(menu_files());
    let menu = match AssetsView::new(&mut world).load_now::<UiFile>("ui/menu.ui.ron") {
      Ok(menu) => menu,
      Err(error) => panic!("menu.ui.ron: {}", error.report()),
    };
    let checks = |world: &World| {
      let files = world.resource::<Assets<UiFile>>();
      files
        .get(&menu)
        .map(|file| UiFileLoader.check(file, world).is_ok())
    };
    assert_eq!(checks(&world), Some(true));
    // The import loses the template the menu uses
    let common = AssetsView::new(&mut world)
      .load_now::<UiFile>("ui/common.ui.ron")
      .ok();
    let Some(common) = common else {
      panic!("common.ui.ron is loaded");
    };
    let Ok(emptied) = UiFile::parse("ui/common.ui.ron", "(templates: {})") else {
      panic!("an empty file of templates parses");
    };
    if let Some(file) = world.resource_mut::<Assets<UiFile>>().get_mut(&common) {
      *file = emptied;
    }
    assert_eq!(checks(&world), Some(false), "built again against it");
  }

  /// The issues of a `Decode` error over a UI file's mistakes, shown
  fn shown(error: &AssetError) -> Vec<String> {
    let AssetError::Decode { source, .. } = error else {
      panic!("not a decode error: {}", error.report());
    };
    let Some(file) = source.downcast_ref::<UiFileError>() else {
      panic!("not the file's mistakes: {}", error.report());
    };
    file.issues.iter().map(ToString::to_string).collect()
  }

  #[test]
  fn on_the_workers_a_missing_import_fails_at_its_position() {
    let files = MemorySource::new();
    files.insert(
      "ui/menu.ui.ron",
      "(imports: [\"ui/gone.ui.ron\"], root: Column)",
    );
    let mut world = registry_over(files);
    let server = world.resource::<AssetServer>().clone();
    let menu = server.load::<UiFile>("ui/menu.ui.ron");
    pump_until(&mut world, |_| server.state(&menu).is_failed());
    let LoadState::Failed(error) = server.state(&menu) else {
      panic!("the menu loads");
    };
    assert_eq!(
      shown(&error),
      [
        "ui/menu.ui.ron:1:12: cannot import \"ui/gone.ui.ron\"",
        "ui/gone.ui.ron: no asset at \"ui/gone.ui.ron\"",
      ]
    );
  }

  #[test]
  fn a_reload_naming_a_missing_image_fails_at_its_position() {
    let files = MemorySource::new();
    files.insert("ui/hud.ui.ron", "(root: Column)");
    let mut world = registry_over(files.clone());
    let server = world.resource::<AssetServer>().clone();
    server.start_watching();
    let hud = match AssetsView::new(&mut world).load_now::<UiFile>("ui/hud.ui.ron") {
      Ok(hud) => hud,
      Err(error) => panic!("hud.ui.ron: {}", error.report()),
    };
    let mut events = world
      .resource::<Messages<AssetEvent<UiFile>>>()
      .get_cursor_current();
    files.insert(
      "ui/hud.ui.ron",
      "(root: Column(children: [\n  Image(\"gone.png\"),\n]))",
    );
    let failed = |world: &World, events: &mut MessageCursor<AssetEvent<UiFile>>| {
      events
        .read(world.resource::<Messages<AssetEvent<UiFile>>>())
        .find_map(|event| match event {
          AssetEvent::ReloadFailed { id, error } if *id == hud.id() => Some(Arc::clone(error)),
          _ => None,
        })
    };
    let start = Instant::now();
    let error = loop {
      pump(&mut world);
      if let Some(error) = failed(&world, &mut events) {
        break error;
      }
      assert!(start.elapsed() < Duration::from_secs(5), "timed out");
      std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(
      shown(&error),
      ["ui/hud.ui.ron:2:9: cannot load the image \"gone.png\": no asset at \"gone.png\""]
    );
  }
}
