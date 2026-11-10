//! UI themes from `.theme.ron` text

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::audio::{AudioClip, Sound};
use crate::core::graphics::Image;
use crate::core::ui::{Theme, ThemeSound, ThemeSoundSource};

/// Loads [`Theme`]s from `.theme.ron` files. A file states only what it
/// changes: it is merged over the dark theme map by map (a nested struct
/// keeps dark's other fields; a file's `variants` add to dark's), and a
/// variant's `base`, a text role's `font` and a skin's `filter` are strings
/// with the variant's name (`"Button"`, `"Pixel"`, `"Nearest"`). A mistake
/// fails the load with `AssetError::Decode`, naming the path, and a syntax
/// mistake gives the line and column RON gives. A `)` left out shows only
/// at the end of the file: the text after it reads as more fields of the
/// struct it leaves open, one level deeper, and is well formed until then.
///
/// The images its skins name are its dependencies, so a theme is ready once
/// they are, one that does not load fails it (`AssetError::Dependency`), and
/// a theme whose image reloads is `Modified` too. The files its UI sounds
/// name (asset paths: a `.sound.ron` loads as a [`Sound`], any other as an
/// [`AudioClip`]) load with it, at once under `load_now`, but are not
/// dependencies: one that does not load is warned about once and leaves
/// its sound silent, as a theme made in code does, and one still loading
/// plays nothing. A saved sound reloads in place. A `UiTheme` following the
/// theme (`UiTheme::follow`) uses it again whenever it changes.
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
    // A missing UI sound is that sound's silence, not the theme's failure
    for sound in theme.sounds.each_mut() {
      let Some(file) = sound.path.as_deref() else {
        continue;
      };
      sound.sound = Some(if ThemeSound::is_definition(file) {
        ThemeSoundSource::Sound(cx.load_optional::<Sound>(file))
      } else {
        ThemeSoundSource::Clip(cx.load_optional::<AudioClip>(file))
      });
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
  fn a_broken_theme_is_named_once_with_the_place_ron_gives() {
    // A comma missing between two fields: RON stops at the second
    assert_eq!(
      load("ui/a.theme.ron", "(\n  name: \"a\"\n  palette: (),\n)\n").err(),
      Some("could not load \"ui/a.theme.ron\": 3:3: Expected comma".to_string())
    );
    // A `)` missing: the fields after it read as fields of the open
    // struct, one level deeper, and are well formed, so the text is RON up
    // to its end, where the struct is still open
    let unclosed = "(\n  palette: (\n    accent: (r: 1, g: 2, b: 3, a: 255,\n  ),\n)\n";
    assert_eq!(
      load("ui/b.theme.ron", unclosed).err(),
      Some("could not load \"ui/b.theme.ron\": 6:1: Expected comma".to_string())
    );
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

  #[test]
  fn a_theme_s_sounds_load_with_it_as_definitions_or_clips() {
    use crate::core::audio::{AudioClip, Sound};
    use crate::core::ui::ThemeSoundSource;
    let files = MemorySource::new();
    files.insert(
      "ui/sounds.theme.ron",
      r#"(sounds: (confirm: (path: "ui/click.wav"), back: (path: "ui/back.sound.ron")))"#,
    );
    files.insert(
      "ui/click.wav",
      crate::core::testing::wav(22_050, 220, 1_000),
    );
    files.insert(
      "ui/back.sound.ron",
      r#"(clips: [(path: "ui/click.wav")], bus: "ui")"#,
    );
    let mut world = registry_over(files);
    let mut assets = AssetsView::new(&mut world);
    let theme = assets.load_now::<Theme>("ui/sounds.theme.ron");
    let Some(sounds) = theme
      .ok()
      .and_then(|theme| assets.get(&theme).map(|theme| theme.sounds.clone()))
    else {
      panic!("ui/sounds.theme.ron loads");
    };
    let click = assets.load::<AudioClip>("ui/click.wav");
    let back = assets.load::<Sound>("ui/back.sound.ron");
    assert!(
      assets.is_ready(&click) && assets.is_ready(&back),
      "loaded with the theme"
    );
    assert_eq!(sounds.confirm.sound, Some(ThemeSoundSource::Clip(click)));
    assert_eq!(sounds.back.sound, Some(ThemeSoundSource::Sound(back)));
    assert_eq!(
      sounds.focus.sound, None,
      "an event naming no file is silent"
    );
  }

  /// The theme's UI sound volume is a dB offset, as a designer writes every
  /// level; the amplitude it replaced is refused, naming the field to use
  #[test]
  fn a_theme_s_ui_sound_volume_is_in_db() {
    let theme = load("ui/db.theme.ron", "(sounds: (volume_db: -6.0))");
    assert_eq!(theme.map(|theme| theme.sounds.volume_db), Ok(-6.0));
    assert_eq!(Theme::dark().sounds.volume_db, 0.0, "as the sound is");
    let old = load("ui/amplitude.theme.ron", "(sounds: (volume: 0.5))");
    assert!(
      old.as_ref().is_err_and(|error| error.contains("volume_db")),
      "{old:?}"
    );
  }

  /// How many records `log` captured name `path`
  fn warnings_naming(log: &crate::core::testing::LogCapture, path: &str) -> usize {
    log.containing(path)
  }

  /// A UI sound that does not load leaves its theme loaded and that sound
  /// silent, with one warning naming it, as a theme made in code does; the
  /// theme's other sounds load with it. Loaded at once or queued alike
  #[test]
  fn a_ui_sound_that_does_not_load_is_silent_with_one_warning() {
    use crate::core::audio::AudioClip;
    use crate::core::ui::ThemeSoundSource;
    let log = crate::core::testing::capture_log();
    let files = MemorySource::new();
    files.insert(
      "ui/quiet.theme.ron",
      r#"(sounds: (tick: (path: "ui/gone-now.wav"), confirm: (path: "ui/here.wav")))"#,
    );
    files.insert(
      "ui/queued.theme.ron",
      r#"(sounds: (tick: (path: "ui/gone-queued.wav")))"#,
    );
    files.insert("ui/here.wav", crate::core::testing::wav(22_050, 220, 1_000));
    let mut world = registry_over(files);
    let clip = |sound: &crate::core::ui::ThemeSound| match &sound.sound {
      Some(ThemeSoundSource::Clip(clip)) => clip.clone(),
      other => panic!("a clip: {other:?}"),
    };
    let mut assets = AssetsView::new(&mut world);
    let theme = assets.load_now::<Theme>("ui/quiet.theme.ron");
    let Some(sounds) = theme
      .ok()
      .and_then(|theme| assets.get(&theme).map(|theme| theme.sounds.clone()))
    else {
      panic!("ui/quiet.theme.ron loads");
    };
    assert!(assets.state(&clip(&sounds.tick)).is_failed());
    let here = assets.load::<AudioClip>("ui/here.wav");
    assert!(assets.is_ready(&here), "loaded with the theme");
    assert_eq!(clip(&sounds.confirm), here);
    let server = world.resource::<AssetServer>().clone();
    let queued = server.load::<Theme>("ui/queued.theme.ron");
    pump_until(&mut world, |world| {
      let tick = world
        .resource::<Assets<Theme>>()
        .get(&queued)
        .and_then(|theme| theme.sounds.tick.sound.clone());
      match tick {
        Some(ThemeSoundSource::Clip(tick)) => server.state(&tick).is_failed(),
        _ => false,
      }
    });
    assert!(server.is_ready(&queued));
    assert_eq!(warnings_naming(&log, "ui/gone-now.wav"), 1);
    assert_eq!(warnings_naming(&log, "ui/gone-queued.wav"), 1);
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
