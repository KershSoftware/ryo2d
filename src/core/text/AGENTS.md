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
- Every real change to `Fonts` bumps `generation`; the text engine re-syncs
  and drops cached layouts when it changes. Registering is idempotent: the
  same bytes and face index (a hash of the bytes, then a byte compare), the
  same canonical path (`load`) or the same bitmap family, sheet, cell and
  characters return the existing id, and a `set_role` or `set_render` to
  the current value bumps nothing, so a game calling them every frame costs
  no relayout.
- `Fonts` has no public constructor (`new` is crate-internal, no
  `Default`): `FontId`s are registry indices that the glyph atlas and
  swash's cache key on, so a replaced registry would alias them. Its
  `Debug` (and `FaceSource`'s) leaves the font bytes out.
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
- Fallback: a style's font, its fallbacks, then the UI font, then the mono font.
  A character no font has draws glyph 0 (the font's box) and is warned once
  per family, naming the first five such characters and how many more.
- Bitmap fonts (`Fonts::add_bitmap`) are laid out by a monospace grid
  layouter; a text uses bitmap or outline fonts, not both.
- Clusters: each glyph and icon carries its character cluster in logical
  order; reveal and per-glyph effects count clusters. An icon shares the
  cluster after it; an icon with no character after it gets a cluster of
  its own, so a reveal reaches it.
- `max_lines` keeps the first lines, and the block measures only those;
  `max_lines(0)` gives an empty block of size zero.
- Pixel faces (`FontRender::Pixel`, bitmap fonts included) lay out at a
  snapped size: the whole multiple of the design size, in layout pixels,
  nearest the size asked for (at least one), so a glyph's image (made at
  that size, `pixel_raster_px`) maps texel for texel onto its advance.
  `TextEngine::snapped` applies it to the style before layout, line height
  and the caret queries; `span_runs` to spans that set a size or a pixel
  font. The cache key keeps the size as asked.
- `TextFont::Id` lays out that exact face: Parley picks faces by family,
  weight and slant, so the face's own weight and slant are pushed. A word
  wider than the wrap width breaks inside it (`OverflowWrap::Anywhere`).

## Markup, icons, effects, reveal

- Markup: `[color=..]`, `[size=..]`, `[font=..]`, `[b]`, `[i]`,
  `[outline=width,color]`, `[icon=name]`, `[pause=seconds]`, `[[` for a
  bracket (`RichText::escape` doubles every `[`, for untrusted text). Any
  other tag is an effect (`[wave]`, `[sparkle=3]`). Nested tags
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
  size (default 32 px, range 4 px) with error correction, about half a
  millisecond each: `Fonts::prewarm` builds known text ahead.
- Glyph images are capped (`render/text.rs`): raster and pixel images at
  256 render pixels for world text (`WORLD_GLYPH_PX`) and 512 for the UI
  (`UI_GLYPH_PX`), MSDF bases at 256 (`MSDF_MAX_BASE`). Larger raster text
  draws the capped image scaled up, off the pixel grid; a pixel glyph past
  the cap is made at the largest multiple of its design size within the
  cap that divides its own, drawn a whole number of render pixels per
  texel. A capped raster font and a capped MSDF base are warned once per
  font. The rasterizer itself refuses sizes above 1024 px. Bitmap fonts
  have no glyph images: the renderer draws their cells from the sprite
  sheet.
- One `GlyphRasterizer` serves one `Fonts`: swash caches parsed fonts
  under `[FontId, face index]`. The engine owns one of each.
- The built-in fonts are subset with `--notdef-outline`, so a character no
  font has draws a box (`hb-subset` empties `.notdef` by default).

## World text

- `Text2d` lays out (cached) when its text, style or options change or a
  font changes (`TextLayoutState::key`; the anchor is in it too, since it
  places the bounds); `TextBounds` holds the block's size and its top-left
  offset from the position (unscaled).
- A `Text2d` scaled to zero is skipped without a warning (pop-in tweens
  start there); a non-finite transform is warned once per entity.
- `Text2d::opacity` fades the whole block when it is drawn
  (`TextPlacement::opacity`, clamped by `clean_opacity`); it is not in the
  layout key, so a fade never lays out again. A `Tween` alpha target drives
  it (`systems/tween.rs`), as it drives a sprite's alpha.
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
  over-wide effect degrades instead of clipping at the quad's edge, and is
  warned once per font. A wider effect needs a bigger range
  (`FontRender::Msdf { range, .. }`).
- MSDF fonts have no synthesized italic or bold: glyph quads cannot shear,
  so MSDF keys leave skew and embolden out (one image per glyph) and the
  text draws upright and regular, warned once per font. Add the real
  italic or bold face to the registry.
- Effects run per glyph per frame with one `GlyphContext` per block (its
  `args` buffer reused, so a glyph allocates nothing); `TextEffects::apply`
  clamps alpha to 0..=1 and scale to 0..=8. An effect runs once per pass
  (shadow, outline, fill) and once per camera, so it must be a pure function
  of its `GlyphContext` (documented on `TextEffects::register`). An unknown
  name is looked up once per block, then skipped.
- `TextReveal` advances on game time (`PostUpdateSet::Extract`) from the
  previous frame's layout and writes `TextRevealed` once, when every
  character has started showing (`is_done`). With a fade, the clock then
  runs on (`RevealClock::run_on`) until the last character has faded in, so
  the end never pops; once settled the emitter draws without a reveal. It
  starts over when the text changes (not its style or a font).
- The glyph atlas remembers keys with no image (spaces) and keys whose
  image is bigger than a page, so they are not rasterized again every
  frame; the set is cleared past 4096 keys.
- Icon regions are inset half a texel on each side, so linear filtering
  never reads their neighbours on the sheet.

## CpuText

`CpuText` (`cpu.rs`) is the dev windows' text: monospace glyphs from the
built-in mono font, rasterized by swash into the window's CPU pixel buffer and
cached per character. The dev windows are separate OS windows with pixel
buffers of their own, so it owns a `Fonts` and a `GlyphRasterizer` apart from
the engine's registry. It never lays out or wraps: callers measure in
characters. Without `default-fonts` there is no mono font and nothing is
drawn (`CpuText::mono` is `None`). `src/core/windows/AGENTS.md` has the layout
constants.
