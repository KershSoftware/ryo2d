//! Text: fonts, styles, rich text and markup, layout, glyph rasterization
//! and world-space text. GPU-free: the renderer draws the glyph bitmaps
//! this module produces. `parley`, `swash` and `fdsm` are named only here.

mod cpu;
mod effects;
mod engine;
mod fonts;
mod icons;
mod layout;
mod markup;
mod msdf;
mod raster;
mod rich;
mod style;
mod world;

pub use effects::{GlyphContext, GlyphMod, TextEffects};
pub(crate) use fonts::{BitmapFace, FaceSource};
pub use fonts::{FontError, FontId, FontRender, FontRole, Fonts};
pub use icons::{IconSource, TextIcons};
pub use rich::{Icon, RichText, Span};
pub use style::{
  Glow, Outline, Shadow, TextAlign, TextFont, TextLayoutOptions, TextOverflow, TextStyle,
};
pub use world::{Text2d, TextBounds, TextReveal, TextRevealed};

pub(crate) use cpu::CpuText;
pub(crate) use effects::reveal_alpha;
pub(crate) use engine::{RevealClock, TextEngine};
pub(crate) use layout::{GlyphStyle, PlacedGlyph, PlacedIcon, TextLayout};
pub(crate) use raster::{
  GlyphFormat, GlyphImage, GlyphKey, GlyphRasterizer, glyph_id, raster_size_step,
};
pub(crate) use world::{TextLayoutState, advance_text_reveal, clean_opacity};

#[cfg(test)]
mod guard {
  /// parley, swash and fdsm stay inside src/core/text/
  #[test]
  fn the_text_crates_are_named_only_in_the_text_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![root.clone()];
    let mut scanned = 0;
    while let Some(dir) = stack.pop() {
      for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
          stack.push(path);
        } else if path.extension().is_some_and(|e| e == "rs")
          && !path.starts_with(root.join("core/text"))
        {
          scanned += 1;
          let source = std::fs::read_to_string(&path).unwrap();
          for name in ["parley::", "swash::", "fdsm::"] {
            if source.contains(name) {
              offenders.push(format!("{} names {name}", path.display()));
            }
          }
        }
      }
    }
    assert!(
      scanned > 20,
      "scanned only {scanned} files; is the root right?"
    );
    assert!(offenders.is_empty(), "{offenders:#?}");
  }
}
