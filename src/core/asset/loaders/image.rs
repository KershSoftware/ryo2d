//! Images from files: raster formats decoded by the `image` crate, SVG
//! rasterised by resvg at the size the settings ask for

use std::hash::{Hash, Hasher};

use crate::core::asset::{AssetError, AssetLoader, LoadContext};
use crate::core::error::ImageError;
use crate::core::graphics::Image;

/// The size an SVG is rasterised at; raster images ignore it
#[derive(Debug, Clone, Copy, Default)]
pub enum SvgSize {
  /// The size the SVG gives itself
  #[default]
  Native,
  /// Its own size times this factor: `Scale(2.0)` for a high-DPI icon
  Scale(f32),
  /// As large as fits in the box, keeping its aspect ratio
  Fit {
    /// The box's width in pixels
    width: u32,
    /// The box's height in pixels
    height: u32,
  },
}

// Settings are part of an asset's key, so equality goes by the scale's bits
// as the hash does: a scale equals itself (NaN too), and equal sizes hash alike
impl PartialEq for SvgSize {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (SvgSize::Native, SvgSize::Native) => true,
      (SvgSize::Scale(a), SvgSize::Scale(b)) => a.to_bits() == b.to_bits(),
      (
        SvgSize::Fit { width, height },
        SvgSize::Fit {
          width: other_width,
          height: other_height,
        },
      ) => width == other_width && height == other_height,
      _ => false,
    }
  }
}

impl Eq for SvgSize {}

impl Hash for SvgSize {
  fn hash<H: Hasher>(&self, state: &mut H) {
    std::mem::discriminant(self).hash(state);
    match self {
      SvgSize::Native => {}
      SvgSize::Scale(scale) => scale.to_bits().hash(state),
      SvgSize::Fit { width, height } => (width, height).hash(state),
    }
  }
}

/// How an image file is loaded, given to `load_with` and `load_now_with`:
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn load_icon(ctx: &mut SceneContext<'_>) -> Option<Handle<Image>> {
///   let fit = ImageSettings { svg: SvgSize::Fit { width: 32, height: 32 } };
///   ctx.assets().load_now_with::<Image, _>("icons/heart.svg", fit).ok()
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ImageSettings {
  /// The size an SVG is rasterised at
  pub svg: SvgSize,
}

/// Loads [`Image`]s: PNG, JPEG, BMP, GIF (its first frame), WebP, TGA, TIFF,
/// QOI and ICO through the `image` crate, and SVG through resvg at the
/// settings' [`SvgSize`]. Every registry has it.
///
/// An image too large for the atlas's shared pages (over 256 px on a side)
/// gets its own texture, with mip levels: the loader makes them here, off
/// the main thread
#[derive(Debug, Default, Clone, Copy)]
pub struct ImageLoader;

impl AssetLoader for ImageLoader {
  type Asset = Image;
  type Settings = ImageSettings;

  fn extensions(&self) -> &[&'static str] {
    &[
      "png", "jpg", "jpeg", "bmp", "gif", "webp", "tga", "tif", "tiff", "qoi", "ico", "svg",
    ]
  }

  fn load(
    &self,
    bytes: Vec<u8>,
    settings: &ImageSettings,
    cx: &mut LoadContext<'_>,
  ) -> Result<Image, AssetError> {
    let path = cx.path();
    // The extension chose this loader, so there is one; the last is the format
    let extension = path.extensions().last().unwrap_or_default();
    let mut image = if extension.eq_ignore_ascii_case("svg") {
      rasterise(&bytes, settings.svg).map_err(|reason| AssetError::decode(path, reason))?
    } else {
      decode(&bytes, extension).map_err(|error| AssetError::decode(path, error))?
    };
    image.prepare_mips();
    Ok(image)
  }
}

/// A raster file's pixels. The bytes name their format, except TGA's, which
/// has no signature: then the extension does
fn decode(bytes: &[u8], extension: &str) -> Result<Image, ImageError> {
  match image::ImageFormat::from_extension(extension) {
    Some(format) if image::guess_format(bytes).is_err() => Image::decode_as(bytes, format),
    _ => Image::decode(bytes),
  }
}

/// An SVG's pixels at `size`
fn rasterise(svg: &[u8], size: SvgSize) -> Result<Image, String> {
  match size {
    SvgSize::Native => Image::from_svg_data(svg, 1.0),
    SvgSize::Scale(scale) => Image::from_svg_data(svg, scale),
    SvgSize::Fit { width, height } => Image::from_svg_data_fit(svg, width, height),
  }
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;
  use std::sync::Arc;

  use super::*;
  use crate::core::asset::{
    AssetServer, Assets, AssetsView, Handle, MemorySource, insert_registry,
  };
  use crate::core::ecs::kernel::World;
  use crate::core::graphics::atlas::{BORDER, extrude_border, mip_chain};

  /// `width`x`height` pixels, no two neighbours alike, some translucent
  fn pattern(width: u32, height: u32) -> Vec<u8> {
    (0..width * height)
      .flat_map(|i| {
        let [a, b, ..] = i.to_le_bytes();
        [a, b, a.wrapping_mul(7) ^ b, 255 - (a & 0x7f)]
      })
      .collect()
  }

  /// `rgba` encoded as a `format` file, by the `image` crate
  fn encode(rgba: &[u8], width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let Some(pixels) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
      panic!("{width}x{height} needs {} bytes", width * height * 4);
    };
    let mut file = Cursor::new(Vec::new());
    if let Err(error) = pixels.write_to(&mut file, format) {
      panic!("encoding {format:?}: {error}");
    }
    file.into_inner()
  }

  fn png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    encode(rgba, width, height, image::ImageFormat::Png)
  }

  /// A world whose registry reads `files` and loads images, and nothing else
  fn images_over(files: MemorySource) -> World {
    let mut world = World::new();
    let server = AssetServer::new(files);
    server.register_asset::<Image>(&mut world);
    server.register_loader(ImageLoader);
    world.insert_resource(server);
    world
  }

  fn load(
    world: &mut World,
    path: &str,
    settings: ImageSettings,
  ) -> Result<Handle<Image>, Arc<AssetError>> {
    AssetsView::new(world).load_now_with::<Image, _>(path, settings)
  }

  fn loaded(world: &mut World, path: &str, settings: ImageSettings) -> Image {
    let handle = match load(world, path, settings) {
      Ok(handle) => handle,
      Err(error) => panic!("{path}: {}", error.report()),
    };
    match world.resource::<Assets<Image>>().get(&handle) {
      Some(image) => image.clone(),
      None => panic!("{path} is loaded but not in its store"),
    }
  }

  const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12">
    <rect width="24" height="12" fill="#3080f0"/>
    <circle cx="6" cy="6" r="5" fill="#f0c020" fill-opacity="0.5"/>
  </svg>"##;

  fn svg(size: SvgSize) -> ImageSettings {
    ImageSettings { svg: size }
  }

  #[test]
  fn a_png_decodes_to_its_pixels() {
    let rgba = pattern(5, 3);
    let files = MemorySource::new();
    files.insert("a.png", png(&rgba, 5, 3));
    let mut world = images_over(files);
    let image = loaded(&mut world, "a.png", ImageSettings::default());
    assert_eq!((image.width(), image.height()), (5, 3));
    assert_eq!(image.data(), rgba.as_slice());
    assert!(!image.is_opaque());
  }

  #[test]
  fn a_tga_decodes_by_its_extension() {
    // TGA has no signature: only the extension names it
    let rgba = pattern(4, 2);
    let files = MemorySource::new();
    files.insert("a.tga", encode(&rgba, 4, 2, image::ImageFormat::Tga));
    let mut world = images_over(files);
    let image = loaded(&mut world, "a.tga", ImageSettings::default());
    assert_eq!((image.width(), image.height()), (4, 2));
    assert_eq!(image.data(), rgba.as_slice());
  }

  #[test]
  fn an_svg_is_rasterised_at_the_size_asked_for() {
    let files = MemorySource::new();
    files.insert("icon.svg", SVG);
    let mut world = images_over(files);
    let bytes = SVG.as_bytes();
    let cases = [
      (SvgSize::Native, (24, 12), Image::from_svg_data(bytes, 1.0)),
      (
        SvgSize::Scale(2.0),
        (48, 24),
        Image::from_svg_data(bytes, 2.0),
      ),
      (
        SvgSize::Fit {
          width: 32,
          height: 32,
        },
        (32, 16),
        Image::from_svg_data_fit(bytes, 32, 32),
      ),
    ];
    for (size, expected, before) in cases {
      let before = match before {
        Ok(image) => image,
        Err(error) => panic!("{size:?} before: {error}"),
      };
      let image = loaded(&mut world, "icon.svg", svg(size));
      assert_eq!((image.width(), image.height()), expected, "{size:?}");
      assert_eq!(
        (before.width(), before.height()),
        expected,
        "{size:?}: the size load_svg gives"
      );
      assert!(image.data() == before.data(), "{size:?}: the same pixels");
    }
  }

  #[test]
  fn a_truncated_png_fails_with_decode() {
    let whole = png(&pattern(32, 32), 32, 32);
    let files = MemorySource::new();
    files.insert("cut.png", &whole[..whole.len() / 2]);
    let mut world = images_over(files);
    let error = match load(&mut world, "cut.png", ImageSettings::default()) {
      Ok(_) => panic!("half a PNG loaded"),
      Err(error) => error,
    };
    assert!(
      matches!(*error, AssetError::Decode { .. }),
      "{}",
      error.report()
    );
  }

  #[test]
  fn a_large_image_has_the_mips_the_atlas_would_make() {
    let rgba = pattern(512, 512);
    let files = MemorySource::new();
    files.insert("big.png", png(&rgba, 512, 512));
    files.insert("wide.png", png(&pattern(257, 4), 257, 4));
    files.insert("small.png", png(&pattern(256, 256), 256, 256));
    let mut world = images_over(files);
    // What the atlas makes for a standalone texture: the image with its
    // border, halved down to one texel; the first level is the image itself
    let Some(bordered) = extrude_border(&rgba, 512, 512, BORDER) else {
      panic!("512x512 pixels are 512x512");
    };
    let side = 512 + 2 * BORDER;
    let levels = 32 - side.leading_zeros();
    let expected: Vec<Vec<u8>> = mip_chain(&bordered, side, side, levels)
      .into_iter()
      .skip(1)
      .map(|(_, _, pixels)| pixels)
      .collect();
    let big = loaded(&mut world, "big.png", ImageSettings::default());
    let mips = big.mips().map(|mips| mips.as_slice());
    assert!(
      mips == Some(expected.as_slice()),
      "{:?} levels, not {}",
      mips.map(<[_]>::len),
      expected.len()
    );
    let wide = loaded(&mut world, "wide.png", ImageSettings::default());
    assert!(wide.mips().is_some(), "over 256 px on one side is enough");
    // The atlas makes a shared page's levels with the page
    let small = loaded(&mut world, "small.png", ImageSettings::default());
    assert!(small.mips().is_none());
  }

  #[test]
  fn every_registry_loads_images() {
    let rgba = pattern(3, 2);
    let files = MemorySource::new();
    files.insert("a.png", png(&rgba, 3, 2));
    let mut world = World::new();
    insert_registry(&mut world, AssetServer::new(files));
    let mut assets = AssetsView::new(&mut world);
    let a = match assets.load_now::<Image>("a.png") {
      Ok(a) => a,
      Err(error) => panic!("{}", error.report()),
    };
    assert_eq!(assets.get(&a).map(Image::data), Some(rgba.as_slice()));
    // A test world's registry reads nothing, but knows images. The name is
    // its own: other tests count the log lines that name their files
    let mut world = World::new();
    crate::core::scene::init_world_resources_for_tests(&mut world);
    let absent = "not-in-the-test-world.png";
    let Err(error) = AssetsView::new(&mut world).load_now::<Image>(absent) else {
      panic!("an empty source has no {absent}");
    };
    assert!(
      matches!(*error, AssetError::NotFound { .. }),
      "{}",
      error.report()
    );
  }

  #[test]
  fn svg_sizes_are_equal_as_their_hashes_are() {
    use crate::core::hash::fast_hash;
    let same = |a: SvgSize, b: SvgSize| {
      assert_eq!(a == b, fast_hash(&a) == fast_hash(&b), "{a:?} and {b:?}");
      a == b
    };
    assert!(same(SvgSize::Scale(f32::NAN), SvgSize::Scale(f32::NAN)));
    assert!(!same(SvgSize::Scale(0.0), SvgSize::Scale(-0.0)));
    assert!(same(SvgSize::Scale(2.0), SvgSize::Scale(2.0)));
    assert!(!same(SvgSize::Native, SvgSize::Scale(1.0)));
    assert!(!same(
      SvgSize::Fit {
        width: 1,
        height: 2
      },
      SvgSize::Fit {
        width: 2,
        height: 1
      }
    ));
  }
}
