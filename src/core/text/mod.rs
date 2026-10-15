//! Text: fonts, styles, rich text and markup, layout, glyph rasterization
//! and world-space text. GPU-free: the renderer draws the glyph bitmaps
//! this module produces. `parley`, `swash` and `fdsm` are named only here.

// Until world text and the UI own a text engine, only its tests use it
#[cfg_attr(not(all(test, feature = "default-fonts")), allow(dead_code))]
mod engine;
mod fonts;
#[cfg_attr(not(all(test, feature = "default-fonts")), allow(dead_code))]
mod layout;
mod rich;
mod style;

pub(crate) use fonts::{BitmapFace, FaceSource};
pub use fonts::{FontError, FontId, FontRender, FontRole, Fonts};
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
