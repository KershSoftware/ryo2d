//! Bitmap fonts from `.bitmap.ron` descriptors

use serde::Deserialize;

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::graphics::Image;
use crate::core::text::{BitmapFont, FontError};

/// A `.bitmap.ron` file as written
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
  sheet: String,
  cell: (u32, u32),
  chars: String,
  family: String,
}

/// Loads [`BitmapFont`]s from `.bitmap.ron` descriptors, which name the
/// sheet image (an asset path, loaded as a dependency, so the font is ready
/// once its sheet is), the cell size, the cells' characters and the family:
///
/// ```text
/// (sheet: "fonts/pixel.png", cell: (8, 8), chars: " !\"#$%&'()*+,-./", family: "Pixel")
/// ```
///
/// Every registry has it
#[derive(Debug, Default, Clone, Copy)]
pub struct BitmapFontLoader;

impl AssetLoader for BitmapFontLoader {
  type Asset = BitmapFont;
  type Settings = ();

  fn extensions(&self) -> &[&'static str] {
    &["bitmap.ron"]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    _: &(),
    cx: &mut LoadContext<'_>,
  ) -> Result<BitmapFont, AssetError> {
    let path = cx.path().clone();
    let text = String::from_utf8(bytes).map_err(|error| AssetError::decode(&path, error))?;
    let descriptor: Descriptor =
      ron::from_str(&text).map_err(|error| AssetError::decode(&path, error))?;
    let font = BitmapFont {
      family: descriptor.family,
      sheet: cx.load::<Image>(&descriptor.sheet)?,
      cell: descriptor.cell,
      chars: descriptor.chars,
    };
    if !font.is_drawable() {
      return Err(AssetError::decode(&path, FontError::EmptyBitmap));
    }
    Ok(font)
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
  use crate::core::text::BitmapFont;

  const DESCRIPTOR: &str =
    r##"(sheet: "fonts/pixel.png", cell: (8, 8), chars: " !\"#", family: "Pixel")"##;

  /// A PNG of `width` x `height` white pixels
  fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255; 4]));
    let mut bytes = Vec::new();
    if let Err(error) = image.write_to(
      &mut std::io::Cursor::new(&mut bytes),
      image::ImageFormat::Png,
    ) {
      panic!("encoding a png: {error}");
    }
    bytes
  }

  /// The descriptor and its 32 x 8 sheet: four 8 x 8 cells
  fn files() -> MemorySource {
    let files = MemorySource::new();
    files.insert("fonts/pixel.bitmap.ron", DESCRIPTOR);
    files.insert("fonts/pixel.png", png(32, 8));
    files
  }

  /// An engine's registry over `source`
  fn registry_over(source: impl AssetSource) -> World {
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(source));
    world
  }

  #[test]
  fn a_descriptor_loads_with_its_sheet() {
    let mut world = registry_over(files());
    let font = match AssetsView::new(&mut world).load_now::<BitmapFont>("fonts/pixel.bitmap.ron") {
      Ok(font) => font,
      Err(error) => panic!("pixel.bitmap.ron: {}", error.report()),
    };
    let Some(loaded) = world.resource::<Assets<BitmapFont>>().get(&font).cloned() else {
      panic!("the font is loaded but not in its store");
    };
    assert_eq!(loaded.family, "Pixel");
    assert_eq!(loaded.cell, (8, 8));
    assert_eq!(loaded.chars, " !\"#");
    let sheet = world.resource::<Assets<Image>>().get(&loaded.sheet);
    assert_eq!(
      sheet.map(|image| (image.width(), image.height())),
      Some((32, 8))
    );
    let server = world.resource::<AssetServer>();
    assert_eq!(
      server.path(&loaded.sheet).as_ref().map(AssetPath::as_str),
      Some("fonts/pixel.png"),
      "the sheet's path is an asset path"
    );
  }

  /// Files whose sheet is not read until the test lets it go
  struct HeldSheet {
    files: MemorySource,
    release: crossbeam_channel::Receiver<()>,
  }

  impl AssetSource for HeldSheet {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, AssetError> {
      if path.as_str().ends_with(".png") {
        // Returns when the test sends, or drops its sender
        let _ = self.release.recv();
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
  fn a_bitmap_font_is_ready_only_when_its_sheet_is() {
    let (release, held) = crossbeam_channel::unbounded();
    let mut world = registry_over(HeldSheet {
      files: files(),
      release: held,
    });
    let server = world.resource::<AssetServer>().clone();
    let font = server.load::<BitmapFont>("fonts/pixel.bitmap.ron");
    // The descriptor is read, and waits for its sheet
    pump_until(&mut world, |_| server.waiting_count() == 1);
    assert!(!server.is_ready(&font));
    assert!(world.resource::<Assets<BitmapFont>>().get(&font).is_none());
    let _ = release.send(());
    pump_until(&mut world, |_| server.is_ready(&font));
    assert!(world.resource::<Assets<BitmapFont>>().get(&font).is_some());
  }

  #[test]
  fn a_missing_sheet_fails_the_font() {
    let files = MemorySource::new();
    files.insert("fonts/pixel.bitmap.ron", DESCRIPTOR);
    let mut world = registry_over(files);
    let error = match AssetsView::new(&mut world).load_now::<BitmapFont>("fonts/pixel.bitmap.ron") {
      Ok(_) => panic!("a font without its sheet loads"),
      Err(error) => error,
    };
    assert!(
      matches!(&*error, AssetError::Dependency { dependency, .. } if dependency.as_str() == "fonts/pixel.png"),
      "{}",
      error.report()
    );
  }

  #[test]
  fn a_descriptor_without_a_cell_or_characters_does_not_decode() {
    for descriptor in [
      r#"(sheet: "fonts/pixel.png", cell: (0, 8), chars: "AB", family: "Pixel")"#,
      r#"(sheet: "fonts/pixel.png", cell: (8, 8), chars: "", family: "Pixel")"#,
      r#"(sheet: "fonts/pixel.png", cell: (8, 8))"#,
    ] {
      let files = files();
      files.insert("fonts/bad.bitmap.ron", descriptor);
      let mut world = registry_over(files);
      let loaded = AssetsView::new(&mut world).load_now::<BitmapFont>("fonts/bad.bitmap.ron");
      assert!(
        matches!(
          loaded.as_ref().map_err(|error| &**error),
          Err(AssetError::Decode { .. })
        ),
        "{descriptor}: {loaded:?}"
      );
    }
  }
}
