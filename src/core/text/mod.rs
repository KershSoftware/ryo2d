//! Text: fonts, styles, rich text and markup, layout, glyph rasterization
//! and world-space text. GPU-free: the renderer draws the glyph bitmaps
//! this module produces. `parley`, `swash` and `fdsm` are named only here.

// The text engine (later tasks) reads the crate-internal parts
#[allow(dead_code)]
mod fonts;

#[allow(unused_imports)]
pub(crate) use fonts::{BitmapFace, Face, FaceSource, Prewarm};
pub use fonts::{FontError, FontId, FontRender, FontRole, Fonts};
