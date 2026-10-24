//! UI themes from `.theme.ron` text

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::graphics::Image;
use crate::core::ui::Theme;

/// Loads [`Theme`]s from `.theme.ron` files. A file states only what it
/// changes: it is merged over the dark theme map by map (a nested struct
/// keeps dark's other fields; a file's `variants` add to dark's), and a
/// variant's `base`, a text role's `font` and a skin's `filter` are strings
/// with the variant's name (`"Button"`, `"Pixel"`, `"Nearest"`). A mistake
/// fails the load with `AssetError::Decode`, naming the path.
///
/// The images its skins name (asset paths) are its dependencies, so a theme
/// is ready once they are, one that does not load fails it
/// (`AssetError::Dependency`), and a theme whose image reloads is `Modified`
/// too. A `UiTheme` following the theme (`UiTheme::follow`) uses it again
/// whenever it changes.
///
/// Its `sounds` are not asset paths: audio is not in the registry yet, so
/// they are relative to the working directory, as `Audio::load_sound` takes
/// them (`"assets/ui/click.wav"` beside a skin's `"ui/wood.png"`).
///
/// Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct ThemeLoader;

impl AssetLoader for ThemeLoader {
  type Asset = Theme;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["theme.ron"]
  }

  fn load(&self, bytes: Vec<u8>, _: &(), cx: &mut LoadContext<'_>) -> Result<Theme, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let mut theme = Theme::parse(&text).map_err(|error| AssetError::decode(&path, error))?;
    for (name, skin) in &mut theme.skins {
      let Some(image) = skin.path.as_deref() else {
        continue;
      };
      let loaded = cx.load::<Image>(image).map_err(|error| match error {
        AssetError::Dependency { .. } => error,
        // A path the registry refused (one that leaves the asset root, or
        // no loader for it) is the theme's mistake
        refused => AssetError::decode(&path, format!("the skin {name:?}: {}", refused.report())),
      })?;
      skin.image = Some(loaded);
    }
    Ok(theme)
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use crate::core::asset::{
    AssetError, AssetPath, AssetServer, AssetSource, Assets, AssetsView, MemorySource,
    insert_registry, pump,
  };
  use crate::core::ecs::kernel::World;
  use crate::core::graphics::Image;
  use crate::core::ui::{Color, Theme};

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

  fn registry_over(source: impl AssetSource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  /// The theme `text` at `path`, loaded at once
  fn load(path: &str, text: &str) -> Result<Theme, String> {
    let files = MemorySource::new();
    files.insert(path, text);
    let mut world = registry_over(files);
    let mut assets = AssetsView::new(&mut world);
    let theme = assets.load_now::<Theme>(path).map_err(|e| e.report())?;
    assets
      .get(&theme)
      .cloned()
      .ok_or_else(|| "not in the store".to_string())
  }

  /// A theme with one skin, drawn from `image`
  fn skinned(image: &str) -> String {
    format!(
      r#"(skins: {{"frame": (path: "{image}", insets: (2.0, 2.0, 2.0, 2.0), filter: "Nearest")}})"#
    )
  }

  #[test]
  fn a_theme_round_trips_through_ron() {
    let Ok(dir) = tempfile::tempdir() else {
      panic!("a temp dir");
    };
    let theme = Theme::light();
    let saved = theme.save(dir.path().join("light.theme.ron"));
    assert!(saved.is_ok(), "{saved:?}");
    let mut world = registry_over(crate::core::asset::DiskSource::new(dir.path()));
    let mut assets = AssetsView::new(&mut world);
    let loaded = assets.load_now::<Theme>("light.theme.ron");
    let Some(loaded) = loaded.ok().and_then(|handle| assets.get(&handle).cloned()) else {
      panic!("light.theme.ron loads");
    };
    assert_eq!(loaded.palette, theme.palette);
    assert_eq!(loaded.widgets, theme.widgets);
  }

  #[test]
  fn a_partial_file_fills_the_rest_from_dark() {
    let theme = load(
      "mine.theme.ron",
      "(name: \"mine\", palette: (accent: (r: 1, g: 2, b: 3, a: 255)))",
    );
    let Ok(theme) = theme else {
      panic!("mine.theme.ron: {theme:?}");
    };
    assert_eq!(theme.name, "mine");
    assert_eq!(theme.palette.accent, Color::new(1, 2, 3, 255));
    assert_eq!(theme.palette.text, Theme::dark().palette.text);
    assert_eq!(theme.widgets, Theme::dark().widgets);
  }

  #[test]
  fn a_file_changing_one_button_colour_keeps_dark_s_other_looks() {
    let text = "(widgets: (button: (normal: (fill: (r: 1, g: 2, b: 3, a: 255)))), \
      variants: {\"gold\": (base: \"Button\", normal: (fill: (r: 9, g: 9, b: 9, a: 255)))})";
    let theme = load("gold.theme.ron", text);
    let Ok(theme) = theme else {
      panic!("gold.theme.ron: {theme:?}");
    };
    let dark = Theme::dark();
    let (got, want) = (&theme.widgets.button, &dark.widgets.button);
    assert_eq!(got.normal.fill, Color::new(1, 2, 3, 255));
    assert_eq!(got.normal.border, want.normal.border);
    assert_eq!(got.normal.radius, want.normal.radius);
    assert_eq!(got.hovered, want.hovered);
    assert_eq!(got.pressed, want.pressed);
    assert!(theme.variants.contains_key("gold"));
    assert_eq!(theme.variants["danger"], dark.variants["danger"]);
  }

  #[test]
  fn an_unknown_enum_name_in_a_file_is_an_error_naming_it() {
    let loaded = load(
      "bad-enum.theme.ron",
      "(variants: {\"x\": (base: \"Buton\")})",
    );
    let Err(error) = loaded else {
      panic!("a variant of an unknown kind loads");
    };
    assert!(
      error.contains("bad-enum.theme.ron")
        && error.contains("Buton")
        && error.contains("WidgetKind"),
      "{error}"
    );
  }

  #[test]
  fn a_bad_file_is_an_error_naming_the_path() {
    let loaded = load("broken.theme.ron", "(palette: (accent: nonsense");
    let Err(error) = loaded else {
      panic!("a broken file loads");
    };
    assert!(error.contains("broken.theme.ron"), "{error}");
  }

  #[test]
  fn a_skin_s_image_loads_with_its_theme_loaded_at_once() {
    let files = MemorySource::new();
    files.insert("ui/skinned.theme.ron", skinned("ui/frame.png"));
    files.insert("ui/frame.png", png());
    let mut world = registry_over(files);
    let mut assets = AssetsView::new(&mut world);
    let theme = assets.load_now::<Theme>("ui/skinned.theme.ron");
    let Ok(theme) = theme else {
      panic!(
        "skinned.theme.ron: {}",
        theme.err().map(|e| e.report()).unwrap_or_default()
      );
    };
    let frame = assets.load::<Image>("ui/frame.png");
    assert!(assets.is_ready(&frame), "loaded with the theme");
    let skin = assets.get(&theme).map(|theme| theme.skins["frame"].clone());
    let Some(skin) = skin else {
      panic!("the theme is in its store");
    };
    assert_eq!(skin.image, Some(frame));
    assert_eq!(skin.path.as_deref(), Some("ui/frame.png"));
    assert_eq!(skin.insets, [2.0; 4]);
  }

  #[test]
  fn a_skin_whose_image_does_not_load_fails_its_theme() {
    let loaded = load("ui/gone.theme.ron", &skinned("ui/gone.png"));
    let Err(error) = loaded else {
      panic!("a theme whose skin has no image loads");
    };
    assert_eq!(
      error,
      "\"ui/gone.theme.ron\" needs \"ui/gone.png\", which did not load: no asset at \"ui/gone.png\""
    );
  }

  #[test]
  fn a_skin_path_outside_the_asset_root_is_the_theme_s_mistake() {
    let loaded = load("ui/outside.theme.ron", &skinned("../outside.png"));
    let Err(error) = loaded else {
      panic!("a theme whose skin leaves the asset root loads");
    };
    assert!(
      error.starts_with(
        "could not load \"ui/outside.theme.ron\": the skin \"frame\": \"../outside.png\" is not an asset path"
      ),
      "{error}"
    );
  }

  /// Files whose skin image is not read until the test lets it go
  struct Held {
    files: MemorySource,
    image: crossbeam_channel::Receiver<()>,
  }

  impl AssetSource for Held {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      // Returns when the test sends, or drops its sender
      if path.as_str() == "ui/frame.png" {
        let _ = self.image.recv();
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
  fn a_theme_is_ready_only_once_its_skin_s_image_is() {
    let (image, held) = crossbeam_channel::unbounded();
    let files = MemorySource::new();
    files.insert("ui/skinned.theme.ron", skinned("ui/frame.png"));
    files.insert("ui/frame.png", png());
    let mut world = registry_over(Held { files, image: held });
    let server = world.resource::<AssetServer>().clone();
    // One worker: the theme is read, then its image
    server.limit_workers(1);
    let theme = server.load::<Theme>("ui/skinned.theme.ron");
    pump_until(&mut world, |_| server.waiting_count() == 1);
    assert!(!server.is_ready(&theme), "its image is still loading");
    assert!(world.resource::<Assets<Theme>>().get(&theme).is_none());
    let _ = image.send(());
    pump_until(&mut world, |_| server.is_ready(&theme));
    let frame = server.load::<Image>("ui/frame.png");
    assert!(server.is_ready(&frame));
    let skin = world
      .resource::<Assets<Theme>>()
      .get(&theme)
      .map(|t| t.skins["frame"].clone());
    assert_eq!(skin.and_then(|skin| skin.image), Some(frame));
  }
}
