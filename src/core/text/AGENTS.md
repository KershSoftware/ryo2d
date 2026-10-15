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
