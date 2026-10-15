//! The font registry games add fonts to (`ctx.fonts()`), the built-in
//! fonts, and each font's render mode. The text engine picks up changes at
//! the start of the next frame (it compares `generation`).

use std::path::Path;
use std::sync::Arc;

use swash::{FontRef, StringId};
use thiserror::Error;

use crate::core::asset::SpriteHandle;
use crate::core::ecs::Resource;
use crate::core::error::{AssetError, AssetResult};

/// One registered font face: a font file, or one face of a collection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub(crate) u32);

/// How a font's glyphs are drawn
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum FontRender {
  /// Hinted and anti-aliased, rasterized at the exact pixel size
  #[default]
  Raster,
  /// No anti-aliasing: sizes snap to whole multiples of `design_size` and
  /// glyphs land on whole pixels
  Pixel { design_size: f32 },
  /// A multi-channel signed distance field made once at `base` pixels with
  /// a distance `range` (pixels at that size): scales freely, and the
  /// shader draws outlines, soft shadows and glow
  Msdf { base: f32, range: f32 },
}

impl FontRender {
  /// A pixel font designed at `design_size` pixels
  pub const fn pixel(design_size: f32) -> Self {
    Self::Pixel { design_size }
  }

  /// MSDF at the default base (32 px) and range (4 px)
  pub const fn msdf() -> Self {
    Self::Msdf {
      base: 32.0,
      range: 4.0,
    }
  }
}

/// The built-in roles a style can name instead of a family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
  /// The UI and world font (Inter)
  #[default]
  Ui,
  /// The monospace font (JetBrains Mono): console, overlays, dev windows
  Mono,
  /// The pixel font (m5x7)
  Pixel,
}

impl FontRole {
  pub(crate) fn index(self) -> usize {
    match self {
      Self::Ui => 0,
      Self::Mono => 1,
      Self::Pixel => 2,
    }
  }
}

/// Why a font was refused
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FontError {
  #[error("not a TrueType or OpenType font")]
  Invalid,
  #[error("the font file has no face {0}")]
  NoFace(u32),
  #[error("a bitmap font needs a non-empty cell and at least one character")]
  EmptyBitmap,
}

/// A grid of glyphs on a sprite sheet: `chars[i]` is cell `i`, row-major
#[derive(Debug, Clone)]
pub(crate) struct BitmapFace {
  pub(crate) sprite: SpriteHandle,
  pub(crate) cell: (u32, u32),
  pub(crate) columns: u32,
  pub(crate) chars: Vec<char>,
}

/// Where a face's glyphs come from
#[derive(Debug, Clone)]
pub(crate) enum FaceSource {
  Outline { data: Arc<[u8]>, index: u32 },
  Bitmap(BitmapFace),
}

/// One registered face
#[derive(Debug, Clone)]
pub(crate) struct Face {
  pub(crate) family: String,
  pub(crate) weight: f32,
  pub(crate) italic: bool,
  pub(crate) source: FaceSource,
  pub(crate) render: FontRender,
}

/// A request to build glyphs ahead of their first use
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prewarm {
  pub(crate) font: FontId,
  pub(crate) size: f32,
  pub(crate) chars: String,
}

/// The fonts text can use: built-in (feature `default-fonts`) and added by
/// the game. A resource: `ctx.fonts()` in scenes, `ResMut<Fonts>` in systems.
#[derive(Resource, Debug, Default)]
pub struct Fonts {
  faces: Vec<Face>,
  roles: [Option<FontId>; 3],
  generation: u64,
  prewarm: Vec<Prewarm>,
}

impl Fonts {
  /// An empty registry (no built-in fonts)
  pub fn new() -> Self {
    Self::default()
  }

  /// The registry the engine starts with: the built-in fonts when the
  /// `default-fonts` feature is on, otherwise empty
  pub(crate) fn with_builtins() -> Self {
    #[cfg_attr(not(feature = "default-fonts"), allow(unused_mut))]
    let mut fonts = Self::new();
    #[cfg(feature = "default-fonts")]
    builtins::register(&mut fonts);
    fonts
  }

  /// Add a TrueType or OpenType font (face 0 of a collection)
  pub fn add_bytes(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<FontId, FontError> {
    self.add_face(bytes.into(), 0)
  }

  /// Add face `index` of a font collection
  pub fn add_face(&mut self, data: Arc<[u8]>, index: u32) -> Result<FontId, FontError> {
    let font = FontRef::from_index(&data, index as usize).ok_or(if index == 0 {
      FontError::Invalid
    } else {
      FontError::NoFace(index)
    })?;
    let family = family_name(&font).ok_or(FontError::Invalid)?;
    let attributes = font.attributes();
    let face = Face {
      family,
      weight: f32::from(attributes.weight().0),
      italic: !matches!(attributes.style(), swash::Style::Normal),
      source: FaceSource::Outline { data, index },
      render: FontRender::Raster,
    };
    Ok(self.push(face))
  }

  /// Read a font file (relative paths are relative to the working
  /// directory, as sprites load by default). There is no file system on
  /// the web: embed fonts with `add_bytes(include_bytes!(..))` there.
  pub fn load(&mut self, path: impl AsRef<Path>) -> AssetResult<FontId> {
    let path = path.as_ref();
    #[cfg(target_arch = "wasm32")]
    {
      Err(AssetError::LoadError {
        path: path.to_path_buf(),
        reason: "the web has no file system: embed the font with Fonts::add_bytes".into(),
      })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
      let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
          AssetError::NotFound(path.to_path_buf())
        } else {
          AssetError::LoadError {
            path: path.to_path_buf(),
            reason: error.to_string(),
          }
        }
      })?;
      self
        .add_bytes(bytes)
        .map_err(|error| AssetError::LoadError {
          path: path.to_path_buf(),
          reason: error.to_string(),
        })
    }
  }

  /// A pixel font from a grid of `cell`-sized glyphs on a sprite sheet:
  /// `chars` names the cells in order, left to right, then top to bottom
  pub fn add_bitmap(
    &mut self,
    family: &str,
    sprite: SpriteHandle,
    cell: (u32, u32),
    chars: &str,
  ) -> Result<FontId, FontError> {
    if cell.0 == 0 || cell.1 == 0 || chars.is_empty() {
      return Err(FontError::EmptyBitmap);
    }
    let columns = (sprite.width() / cell.0).max(1);
    let face = Face {
      family: family.to_string(),
      weight: 400.0,
      italic: false,
      source: FaceSource::Bitmap(BitmapFace {
        sprite,
        cell,
        columns,
        chars: chars.chars().collect(),
      }),
      render: FontRender::pixel(cell.1 as f32),
    };
    Ok(self.push(face))
  }

  /// Change how a font draws; text using it lays out again
  pub fn set_render(&mut self, font: FontId, render: FontRender) {
    if let Some(face) = self.faces.get_mut(font.0 as usize) {
      face.render = render;
      self.generation += 1;
    }
  }

  /// How a font draws (`Raster` for an unknown id)
  pub fn render(&self, font: FontId) -> FontRender {
    self
      .face(font)
      .map_or(FontRender::Raster, |face| face.render)
  }

  /// The family a font belongs to
  pub fn family(&self, font: FontId) -> Option<&str> {
    self.face(font).map(|face| face.family.as_str())
  }

  /// A face of `family`, the regular (weight 400, upright) one if there
  /// is one
  pub fn find(&self, family: &str) -> Option<FontId> {
    let mut best: Option<(FontId, f32)> = None;
    for (id, face) in self.faces() {
      if face.family != family {
        continue;
      }
      let distance = (face.weight - 400.0).abs() + if face.italic { 1000.0 } else { 0.0 };
      if best.is_none_or(|(_, d)| distance < d) {
        best = Some((id, distance));
      }
    }
    best.map(|(id, _)| id)
  }

  /// The font a role uses
  pub fn role(&self, role: FontRole) -> Option<FontId> {
    self.roles[role.index()]
  }

  /// Use `font` for a role (e.g. a game's own UI font)
  pub fn set_role(&mut self, role: FontRole, font: FontId) {
    if self.face(font).is_some() {
      self.roles[role.index()] = Some(font);
      self.generation += 1;
    }
  }

  /// Build these glyphs ahead of their first use (MSDF glyphs cost about a
  /// third of a millisecond each to make): call it in a loading scene
  pub fn prewarm(&mut self, font: FontId, size: f32, chars: &str) {
    self.prewarm.push(Prewarm {
      font,
      size,
      chars: chars.to_string(),
    });
  }

  /// Changes whenever a font is added or changed
  pub fn generation(&self) -> u64 {
    self.generation
  }

  pub(crate) fn face(&self, font: FontId) -> Option<&Face> {
    self.faces.get(font.0 as usize)
  }

  pub(crate) fn faces(&self) -> impl Iterator<Item = (FontId, &Face)> {
    self
      .faces
      .iter()
      .enumerate()
      .map(|(i, face)| (FontId(i as u32), face))
  }

  /// The glyphs asked for ahead, once (the renderer builds them)
  pub(crate) fn take_prewarm(&mut self) -> Vec<Prewarm> {
    std::mem::take(&mut self.prewarm)
  }

  fn push(&mut self, face: Face) -> FontId {
    let id = FontId(self.faces.len() as u32);
    self.faces.push(face);
    self.generation += 1;
    id
  }
}

/// The typographic family (name 16) if the font has one, else its family
/// (name 1)
fn family_name(font: &FontRef<'_>) -> Option<String> {
  let strings = font.localized_strings();
  strings
    .find_by_id(StringId::TypographicFamily, None)
    .or_else(|| strings.find_by_id(StringId::Family, None))
    .map(|name| name.chars().collect::<String>())
    .filter(|name| !name.is_empty())
}

#[cfg(feature = "default-fonts")]
mod builtins {
  use std::sync::Arc;

  use super::{FontRender, FontRole, Fonts};

  const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
  const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");
  const JETBRAINS_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
  const M5X7: &[u8] = include_bytes!("../../assets/fonts/m5x7.ttf");

  pub(super) fn register(fonts: &mut Fonts) {
    if let Some(ui) = add(fonts, INTER_REGULAR) {
      fonts.set_role(FontRole::Ui, ui);
    }
    let _ = add(fonts, INTER_BOLD);
    if let Some(mono) = add(fonts, JETBRAINS_MONO) {
      fonts.set_role(FontRole::Mono, mono);
    }
    if let Some(pixel) = add(fonts, M5X7) {
      fonts.set_render(pixel, FontRender::pixel(16.0));
      fonts.set_role(FontRole::Pixel, pixel);
    }
  }

  /// A built-in font that fails to parse is left out (its role stays empty
  /// and text using it falls back), never a panic
  fn add(fonts: &mut Fonts, bytes: &'static [u8]) -> Option<crate::core::text::FontId> {
    fonts.add_bytes(Arc::<[u8]>::from(bytes)).ok()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(feature = "default-fonts")]
  #[test]
  fn the_builtin_fonts_fill_every_role() {
    let fonts = Fonts::with_builtins();
    for role in [FontRole::Ui, FontRole::Mono, FontRole::Pixel] {
      assert!(fonts.role(role).is_some(), "{role:?} has a font");
    }
    let ui = fonts.role(FontRole::Ui).unwrap();
    assert_eq!(fonts.family(ui), Some("Inter"));
    let mono = fonts.role(FontRole::Mono).unwrap();
    assert_eq!(fonts.family(mono), Some("JetBrains Mono"));
    let pixel = fonts.role(FontRole::Pixel).unwrap();
    assert!(matches!(fonts.render(pixel), FontRender::Pixel { .. }));
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn inter_bold_is_a_heavier_face_of_the_same_family() {
    let fonts = Fonts::with_builtins();
    let mut weights: Vec<f32> = fonts
      .faces()
      .filter(|(_, face)| face.family == "Inter")
      .map(|(_, face)| face.weight)
      .collect();
    weights.sort_by(f32::total_cmp);
    assert_eq!(weights, vec![400.0, 700.0]);
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn find_prefers_the_regular_face() {
    let fonts = Fonts::with_builtins();
    let id = fonts.find("Inter").unwrap();
    assert_eq!(fonts.face(id).unwrap().weight, 400.0);
    assert!(fonts.find("No Such Family").is_none());
  }

  #[test]
  fn bytes_that_are_not_a_font_are_refused() {
    let mut fonts = Fonts::new();
    assert!(matches!(
      fonts.add_bytes(vec![1u8, 2, 3]),
      Err(FontError::Invalid)
    ));
    assert!(fonts.faces().next().is_none());
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn every_change_bumps_the_generation() {
    let mut fonts = Fonts::with_builtins();
    let before = fonts.generation();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.set_render(ui, FontRender::msdf());
    assert!(fonts.generation() > before);
    assert_eq!(fonts.render(ui), FontRender::msdf());
  }

  #[test]
  fn a_bitmap_font_needs_a_cell_and_characters() {
    let mut fonts = Fonts::new();
    let sheet = crate::core::asset::SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(64, 16, 255, 255, 255, 255),
      "sheet",
    );
    assert!(matches!(
      fonts.add_bitmap("Grid", sheet.clone(), (0, 8), "AB"),
      Err(FontError::EmptyBitmap)
    ));
    assert!(matches!(
      fonts.add_bitmap("Grid", sheet.clone(), (8, 8), ""),
      Err(FontError::EmptyBitmap)
    ));
    let id = fonts
      .add_bitmap("Grid", sheet.clone(), (8, 8), "ABCDEFGH")
      .unwrap();
    assert!(matches!(fonts.render(id), FontRender::Pixel { design_size } if design_size == 8.0));
    let Some(Face {
      source: FaceSource::Bitmap(grid),
      ..
    }) = fonts.face(id)
    else {
      panic!("a bitmap face");
    };
    assert_eq!(grid.columns, 8, "a 64 wide sheet of 8 wide cells");
    assert_eq!(grid.sprite.serial(), sheet.serial());
  }

  #[cfg(not(target_arch = "wasm32"))]
  #[test]
  fn loading_a_missing_file_says_not_found() {
    let mut fonts = Fonts::new();
    let err = fonts.load("does/not/exist.ttf").unwrap_err();
    assert!(matches!(err, crate::core::error::AssetError::NotFound(_)));
  }

  #[cfg(feature = "default-fonts")]
  #[test]
  fn prewarm_requests_are_queued_for_the_engine() {
    let mut fonts = Fonts::with_builtins();
    let ui = fonts.role(FontRole::Ui).unwrap();
    fonts.prewarm(ui, 32.0, "ABC");
    let queued = fonts.take_prewarm();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].chars, "ABC");
    assert!(fonts.take_prewarm().is_empty());
  }
}
