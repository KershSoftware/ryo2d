# AGENTS.md - Text

Fonts, styles, rich text and markup, layout, glyph rasterization and world
text. GPU-free: the renderer draws the bitmaps this module makes.

- `parley` (layout), `swash` (raster glyphs, font tables) and `fdsm` (MSDF)
  are named only in this module. Everything else sees `FontId`, `TextStyle`,
  `TextLayout` and `GlyphImage`.
- `Fonts` is the registry (a resource, `ctx.fonts()`). Built-in fonts come
  from `src/assets/fonts/` behind the `default-fonts` feature (on by
  default): Inter (UI and world), JetBrains Mono (console, overlays, dev
  windows), m5x7 (pixel, ASCII only). Sources, licences and the subset
  command are in `src/assets/fonts/README.md`.
- Every change to `Fonts` bumps `generation`; the text engine re-syncs and
  drops cached layouts when it changes.
- There is no file system on the web: `Fonts::load` fails there, so games
  embed fonts with `add_bytes(include_bytes!(..))`.

## Layout

- `TextEngine` (engine-owned, crate-internal) holds one Parley
  `FontContext` (no system fonts) and `LayoutContext`, rebuilt when
  `Fonts::generation` changes. Layouts are cached by text, style, options,
  scale and font generation, and dropped after 120 unused frames, so an
  immediate-mode call every frame costs a hash lookup.
- `scale` is layout pixels per text unit (the UI's render scale; 1 for
  world text). Layouts are in layout pixels, y down; `measure` returns
  text units.
- A block's width is its widest line, except that with a wrap width a
  block aligned `Center`, `End` or `Justify` is the wrap width wide: Parley
  aligns lines within the wrap width, so that is the frame the glyphs sit in
  and the anchor must place.
- Styled spans never overlap (`RichText` appends); Parley's brush is the
  span index, and `TextLayout::styles` resolves each to colour, outline,
  shadow, glow and effect.
- Fallback: a style's font, its fallbacks, then the UI font, then the mono font. A character no
  font has draws glyph 0 (the font's box) and is warned once per family and
  character.
- Bitmap fonts (`Fonts::add_bitmap`) are laid out by a monospace grid
  layouter; a text uses bitmap or outline fonts, not both.
- Clusters: each glyph and icon carries its character cluster in logical
  order; reveal and per-glyph effects count clusters. An icon shares the
  cluster after it; an icon with no character after it gets a cluster of
  its own, so a reveal reaches it.
- `max_lines` keeps the first lines, and the block measures only those;
  `max_lines(0)` gives an empty block of size zero.
- `TextFont::Id` lays out that exact face: Parley picks faces by family,
  weight and slant, so the face's own weight and slant are pushed. A word
  wider than the wrap width breaks inside it (`OverflowWrap::Anywhere`).
