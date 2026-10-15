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

## Markup, icons, effects, reveal

- Markup: `[color=..]`, `[size=..]`, `[font=..]`, `[b]`, `[i]`,
  `[outline=width,color]`, `[icon=name]`, `[pause=seconds]`, `[[` for a
  bracket. Any other tag is an effect (`[wave]`, `[sparkle=3]`). Nested tags
  flatten into non-overlapping spans. Malformed markup (unclosed, unmatched,
  a bad value) is drawn as written and warned once per string; nothing
  panics.
- `TextIcons` maps names to sprites or regions; `TextEffects` maps names to
  `Fn(&GlyphContext) -> GlyphMod` (built in: wave, shake, rainbow). An
  unknown effect name is warned once when drawn and ignored.
- Reveal counts character clusters. `RevealClock` advances them, holding
  at `[pause]` (a leading one delays the first cluster) and after punctuation;
  effects and reveal never relayout.
  Pauses live in the `TextLayout` (and in `RichText::hash_into`), so a
  different pause is a different cached layout.

## Glyph images

- `GlyphKey` names one image: font, glyph id, size (1/64 px fixed point),
  a 4-bin subpixel position (raster fills and strokes only), kind (fill,
  stroke band, pixel, MSDF), variable-font coordinates, synthesized italic
  (a 14 degree skew) and bold.
- Raster glyphs are hinted up to 48 px, unhinted above; pixel glyphs are
  thresholded to on/off; strokes are a band centred on the outline (drawn
  under the fill). MSDF glyphs are made once per glyph at the font's base
  size (default 32 px, range 4 px) with error correction, about a third of
  a millisecond each: `Fonts::prewarm` builds known text ahead.
- Glyphs above 1024 px are refused. Bitmap fonts have no glyph images:
  the renderer draws their cells from the sprite sheet.
- One `GlyphRasterizer` serves one `Fonts`: swash caches parsed fonts
  under `[FontId, face index]`. The engine owns one of each.
- The built-in fonts are subset with `--notdef-outline`, so a character no
  font has draws a box (`hb-subset` empties `.notdef` by default).

## World text

- `Text2d` lays out (cached) when its text, style or options change or a
  font changes (`TextLayoutState::key`; the anchor is in it too, since it
  places the bounds); `TextBounds` holds the block's size and its top-left
  offset from the position (unscaled).
- Glyphs are emitted per camera (`only_camera`): raster glyphs are made at
  the size that camera sees (snapped to `raster_size_step`) and land on
  whole pixels when nothing is rotated; MSDF glyphs scale. Sorted with
  sprites (`DrawKind::Text`, after entities at equal z); unlit by default.
  The emitter (`render/text.rs`) is shared with the UI.
- Shadows, outlines, fills are three passes over the block. Raster outlines
  are stroke bands (twice the width, centred); MSDF outline/shadow/glow are
  shader lanes, limited to half the font's range at its base size. Pixel
  and bitmap outlines are four copies a whole number of render pixels away.
- MSDF reach: the field and the image padding hold `range / 2` base pixels
  around the glyph. Outlines are clamped to that less half a render pixel,
  and a soft layer's spread and softness shrink together to fit, so an
  over-wide effect degrades instead of clipping at the quad's edge. A wider
  effect needs a bigger range (`FontRender::Msdf { range, .. }`).
- MSDF fonts have no synthesized italic or bold: glyph quads cannot shear,
  so MSDF keys leave skew and embolden out (one image per glyph) and the
  text draws upright and regular, warned once per font. Add the real
  italic or bold face to the registry.
- Effects run per glyph per frame with one `GlyphContext` per block (its
  `args` buffer reused, so a glyph allocates nothing); `TextEffects::apply`
  clamps alpha to 0..=1 and scale to 0..=8.
- `TextReveal` advances on game time (`PostUpdateSet::Extract`) from the
  previous frame's layout and writes `TextRevealed` once. It starts over
  when the text changes (not its style or a font). A finished reveal draws
  every glyph at full alpha: its fade would leave the last ones partial.
- The glyph atlas remembers keys with no image (spaces), so they are not
  rasterized again every frame.
