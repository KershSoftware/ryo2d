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

// The reveal belongs to the UI and world text (later tasks)
#[allow(unused_imports)]
pub(crate) use effects::reveal_alpha;
#[allow(unused_imports)]
pub(crate) use engine::RevealClock;
