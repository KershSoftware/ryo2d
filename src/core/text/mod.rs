//! Text: fonts, styles, rich text and markup, layout, glyph rasterization
//! and world-space text. GPU-free: the renderer draws the glyph bitmaps
//! this module produces. `parley`, `swash` and `fdsm` are named only here.

// Until world text and the UI own a text engine, only its tests use it (and
// the effects and reveal they will drive)
#[cfg_attr(not(all(test, feature = "default-fonts")), allow(dead_code))]
mod effects;
#[cfg_attr(not(all(test, feature = "default-fonts")), allow(dead_code))]
mod engine;
mod fonts;
mod icons;
#[cfg_attr(not(all(test, feature = "default-fonts")), allow(dead_code))]
mod layout;
mod markup;
#[cfg_attr(not(test), allow(dead_code))]
mod msdf;
#[cfg_attr(not(test), allow(dead_code))]
mod raster;
mod rich;
mod style;

pub use effects::{GlyphContext, GlyphMod, TextEffects};
pub(crate) use fonts::{BitmapFace, FaceSource};
pub use fonts::{FontError, FontId, FontRender, FontRole, Fonts};
pub use icons::{IconSource, TextIcons};
pub use rich::{Icon, RichText, Span};
pub use style::{
  Glow, Outline, Shadow, TextAlign, TextFont, TextLayoutOptions, TextOverflow, TextStyle,
};

// For world text, the glyph cache and the UI (later tasks)
#[allow(unused_imports)]
pub(crate) use engine::{CacheStats, TextEngine};
#[allow(unused_imports)]
pub(crate) use fonts::{Face, Prewarm};
#[allow(unused_imports)]
pub(crate) use layout::{GlyphStyle, LineInfo, PlacedGlyph, PlacedIcon, TextLayout};

#[allow(unused_imports)]
pub(crate) use raster::{
  GlyphFormat, GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer, MAX_GLYPH_PX, glyph_id,
  raster_size_step,
};

// The reveal belongs to the UI and world text (later tasks)
#[allow(unused_imports)]
pub(crate) use effects::reveal_alpha;
#[allow(unused_imports)]
pub(crate) use engine::RevealClock;
#[cfg(test)]
mod guard {
  /// parley, swash and fdsm stay inside src/core/text/
  #[test]
  fn the_text_crates_are_named_only_in_the_text_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
      for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
          stack.push(path);
        } else if path.extension().is_some_and(|e| e == "rs")
          && !path.starts_with(root.join("core/text"))
        {
          let source = std::fs::read_to_string(&path).unwrap();
          for name in ["parley::", "swash::", "fdsm::"] {
            if source.contains(name) {
              offenders.push(format!("{} names {name}", path.display()));
            }
          }
        }
      }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
  }
}
