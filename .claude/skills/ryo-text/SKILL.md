---
name: ryo-text
description: "Text: fonts, styles, rich text and markup, inline icons, typewriter reveal and glyph effects, world text (Text2d) and the damage-number recipe."
metadata:
  version: 1.0.0
---

# Ryo2D Text Specialist

Text is drawn on the GPU, in the UI and in the world, with one text engine behind both. Everything below is in the prelude. Complete signatures are in `ryo-api`; UI widgets are in `ryo-ui`.

## 1. When to Use What

| Need | Use |
|------|-----|
| A label, a score, a quick line in the default style | `ui.draw_text(text, x, y, color)` and its family (`draw_text_anchored`, `draw_text_centered`, `draw_text_wrapped`, `draw_text_wrapped_centered`, `label`) |
| Any style, markup, wrapping, alignment, ellipsis, reveal, fade or opacity in the UI | `ui.text(..)...draw()` (`UiText`), or `ui.draw_text_styled(text, x, y, &style)` for a styled line |
| Text that lives in the world (name tags, signs, damage numbers) | a `Text2d` component on an entity |

```rust
fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
  ui.draw_text("Score: 100", 10.0, 10.0, Color::WHITE);          // default style: Ui font, 14 px
  ui.text(RichText::markup("Deal [color=#f44]20[/color] damage"))
    .at(10.0, 40.0)
    .size(20.0)
    .wrap(300.0)
    .draw();                                                      // returns the size in UI units
  SceneAction::Continue
}
```

- UI text sizes and positions are UI units (screen units with the automatic UI scale). World text sizes are world units.
- `draw_text` and the other shorthands carry the colour's alpha as opacity, so a fading colour reuses one cached layout. `draw_text_styled` and `ui.text(..).style(..)` use the style's colour as given.
- **Fade through opacity, never through the style colour.** The colour is part of the layout cache key, so changing it (or its alpha) every frame lays the text out again every frame. `ui.text(..).opacity(a)` and `Text2d::opacity` fade without a relayout.

## 2. Fonts

`Fonts` is a resource, reached with `ctx.fonts()` in scenes (`ResMut<Fonts>` in systems). Changes take effect at the start of the next frame; text using a changed font lays out again.

### The built-in fonts (feature `default-fonts`, on by default)

| Role (`FontRole`) | Font | Used for |
|-------------------|------|----------|
| `Ui` (the default) | Inter Regular and Bold | UI and world text |
| `Mono` | JetBrains Mono Regular | console, overlays, dev windows |
| `Pixel` | m5x7 (16 px design, `FontRender::Pixel`) | pixel-art text |

They cover Latin, Latin Extended, Greek and Cyrillic (m5x7: ASCII only).

### Adding fonts

```rust
let id: FontId = ctx.fonts().load("assets/fonts/Game.ttf")?;              // native: AssetResult<FontId>
let id: FontId = ctx.fonts().add_bytes(include_bytes!("../assets/Game.ttf").as_slice())?;   // anywhere, the web included
let id: FontId = ctx.fonts().add_face(bytes_arc, 2)?;                    // face 2 of a .ttc collection
```

- `load` reads a file (relative to the working directory). **The web has no file system: `load` fails there, so embed fonts with `add_bytes(include_bytes!(..))`.**
- `add_bytes(impl Into<Arc<[u8]>>)` and `add_face(Arc<[u8]>, index)` return `Result<FontId, FontError>` (`Invalid`, `NoFace(index)`, `EmptyBitmap`). Fonts are TrueType or OpenType.
- A face is described by its family name, weight and slant, read from the file.

### Finding fonts and roles

| Call | What |
|------|------|
| `fonts.find("Inter") -> Option<FontId>` | the family's regular (weight 400, upright) face |
| `fonts.find_face("Inter", 700.0, false) -> Option<FontId>` | the face nearest that weight, preferring the slant asked for; at equal distance the face added first wins |
| `fonts.family(id) -> Option<&str>` | the family a face belongs to |
| `fonts.role(FontRole::Ui) -> Option<FontId>` | the face a role uses |
| `fonts.set_role(FontRole::Ui, id)` | point a role at your font (all default-style text, the console and the overlays follow) |

### Render modes

```rust
ctx.fonts().set_render(id, FontRender::Raster);
ctx.fonts().set_render(id, FontRender::pixel(16.0));                       // Pixel { design_size: 16.0 }
ctx.fonts().set_render(id, FontRender::Msdf { base: 32.0, range: 8.0 });   // FontRender::msdf() is base 32, range 4
let mode = ctx.fonts().render(id);
```

| Mode | Glyphs | Choose it for |
|------|--------|---------------|
| `Raster` (default) | hinted, anti-aliased, made at the exact pixel size the text is drawn at (per camera in the world), up to 512 px in the UI and 256 px in the world; larger text draws that image scaled up | almost everything; sharpest text at any size up to the cap |
| `Pixel { design_size }` | no anti-aliasing, on whole pixels | pixel fonts. Sizes snap to whole multiples of the design size at the layout's scale: `n = max(1, round(size * scale / design))`, drawn at `n * design`. A 16 px design draws sizes under 24 px at 16 and 24 to 39 px at 32. Layout, measuring and drawing all use the snapped size |
| `Msdf { base, range }` | a multi-channel distance field made once per glyph at `base` px: scales freely, soft shadows and glow | text that scales a lot, or glow and soft shadow |

- MSDF glyphs cost about half a millisecond each to build (0.46 ms measured in a release build). `ctx.fonts().prewarm(id, size, "0123456789 abc")` builds known glyphs ahead of their first use; call it in a loading scene.
- **MSDF effect reach is limited to `range / 2` base pixels.** The default `FontRender::msdf()` (base 32, range 4) holds about 0.06 em, so a glow or a wide outline clips; use `Msdf { base: 32.0, range: 8.0 }` or more (the text demo uses 24 for glow). An over-wide effect degrades instead of clipping at the quad edge.
- **MSDF fonts get no synthesized italic or bold** (glyph quads cannot shear). The text draws upright and regular and the engine warns once per font. Add the real italic or bold face as its own font.
- Spans and `set_render` apply per face: Inter Bold can be MSDF while Inter Regular stays raster. Find the face with `find_face`.

### Bitmap fonts

```rust
let sheet = ctx.assets.load_sprite("fonts/hud_font.png")?;                  // a grid of equal cells
let font = ctx.fonts().add_bitmap("HudFont", sheet, (8, 8), "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ")?;
ctx.fonts().set_role(FontRole::Ui, font);                                   // or use TextStyle::family("HudFont")
```

`chars` names the cells row-major (left to right, then top to bottom). A bitmap face lays out on a monospace grid, draws as a pixel font (design size = the cell height), and a text uses bitmap or outline fonts, not both.

### Fallback

A style's font, then its `fallbacks`, then the UI font, then the mono font. A character no font has draws the font's box and is warned once per family and character.

## 3. `TextStyle` and `TextLayoutOptions`

```rust
let style = TextStyle::new()
  .role(FontRole::Ui)           // or .family("Inter"), .font(TextFont::Id(id))
  .size(18.0)
  .bold()                       // .weight(650.0); .italic()
  .color(Color::WHITE)
  .line_height(1.2)
  .letter_spacing(0.5)
  .outline(2.0, Color::BLACK)
  .shadow(Vec2::new(1.5, 1.5), Color::new(0, 0, 0, 160), 1.0)
  .glow(3.0, Color::new(255, 180, 60, 200))     // MSDF fonts only
  .fallback(TextFont::Family("Noto Sans JP".into()));
```

| Field | Default | Notes |
|-------|---------|-------|
| `font: TextFont` | `Role(FontRole::Ui)` | `Role(FontRole)`, `Family(String)`, or `Id(FontId)` (one exact face; `weight` and `italic` do not apply) |
| `fallbacks: Vec<TextFont>` | empty | tried in order when the font lacks a character |
| `size: f32` | `14.0` | text units: pixels in the UI, world units on entities |
| `weight: f32` | `400.0` | 100 to 900; `bold()` is 700 |
| `italic: bool` | `false` | a real italic face if the family has one, else a 14 degree skew (not on MSDF fonts) |
| `color: Color` | `Color::WHITE` | sRGB, straight alpha |
| `line_height: f32` | `1.2` | a multiple of the size |
| `letter_spacing: f32` | `0.0` | extra space between characters, text units |
| `outline: Option<Outline>` | none | `Outline { width, color }`; raster: a stroke band, MSDF: the shader draws it, pixel: copies whole render pixels away |
| `shadow: Option<Shadow>` | none | `Shadow { offset, color, softness }`; `softness` only matters for MSDF fonts, raster shadows are hard |
| `glow: Option<Glow>` | none | `Glow { radius, color }`; **MSDF fonts only** |

`TextStyle` is `#[non_exhaustive]`: build it with the builders above (`new()`, then chain). A style with a non-positive or non-finite size is refused and warned once.

```rust
let options = TextLayoutOptions::default()
  .wrap(240.0)                  // wrap width in text units; None keeps lines whole
  .align(TextAlign::Center)     // Start (default), Center, End, Justify
  .max_lines(3)                 // extra lines are dropped
  .overflow(TextOverflow::Ellipsis)   // Clip (default) or Ellipsis ("…" on the last kept line)
  .anchor(Anchor::TopLeft);     // the point of the block that sits on the text's position
```

- A block is as wide as its widest line, except that with a wrap width and `Center`, `End` or `Justify` it is the wrap width wide.
- `max_lines(0)` lays out an empty block of size zero. A word wider than the wrap width breaks inside it.
- Text input widgets and the console wrap and edit any script.

## 4. `RichText` and Markup

`RichText` is a string with styled spans, inline icons and typewriter pauses. Anything taking `impl Into<RichText>` accepts a `&str` or `String` (plain text, no markup).

```rust
let rich = RichText::new("Press ")
  .icon_named("jump")                                            // from TextIcons
  .plain(" to ")
  .span("jump", Span::bold().with_color(Color::YELLOW))
  .plain(".")
  .pause(0.5);                                                   // the typewriter holds here

let parsed = RichText::markup("Deal [b][color=#f44]20[/color][/b] damage[pause=0.4]!");
```

- Builders: `new`, `plain(&str)`, `span(&str, Span)`, `icon(SpriteHandle)`, `icon_sized(SpriteHandle, Vec2)`, `icon_named(name)`, `pause(seconds)`, `markup(&str)`; read with `as_str()` (characters only) and `is_empty()`.
- `Span` overrides (all optional, unset keeps the base style): `color`, `size`, `font`, `weight`, `italic`, `outline`, `effect`. Constructors `Span::color(c)`, `size(s)`, `font(TextFont)`, `bold()`, `italic()`, `outline(width, color)`, `effect(name)`; chain with `with_color`, `with_size`, `with_font`, `with_weight`, `with_italic`, `with_outline`, `with_effect`.
- Spans never overlap (`RichText` appends); a text uses a span's size and pixel face with the same snapping as a style.

### Markup tags

| Tag | Value | Meaning |
|-----|-------|---------|
| `[color=..]..[/color]` | `#rgb`, `#rrggbb`, `#rrggbbaa`, or `white black red green blue yellow cyan magenta gray grey orange purple pink` | text colour |
| `[size=..]..[/size]` | positive number, text units | font size |
| `[font=..]..[/font]` | a family name | font family |
| `[b]..[/b]` | none | bold (weight 700) |
| `[i]..[/i]` | none | italic |
| `[outline=width,color]..[/outline]` | positive width, then a colour as above | outline |
| `[icon=name]` | a `TextIcons` name | an inline icon; no closing tag |
| `[pause=seconds]` | positive number | holds the typewriter; no closing tag |
| `[name]..[/name]` or `[name=args]..[/name]` | anything | any other tag is an effect (`wave`, `shake`, `rainbow`, or one you registered) |
| `[[` | | a literal `[` |

- Tags nest and flatten into non-overlapping spans. Tag names are letters, digits, `_` and `-`. A lone `]` is plain text.
- **Malformed markup is drawn as written and warned once per string; nothing panics.** That covers an unclosed tag, a close with no open, a bad or missing value (`[color=banana]`, `[size]`, `[b=1]`), a bad tag name, and a `[` with no `]`. Mis-nested tags still apply but warn.
- Effect arguments are passed whole: `[wave=3,2]` gives the effect `args == "3,2"`.

## 5. Icons

`TextIcons` is a resource mapping names to sprites. Markup (`[icon=jump]`) and `RichText::icon_named` look the name up when the text is drawn.

```rust
let mut icons = ctx.world.resource_mut::<TextIcons>();
icons.insert("jump", key_z_sprite);                                              // a whole sprite
icons.insert_region("coin", icon_sheet, Rect::new(16.0, 0.0, 16.0, 16.0));      // a pixel region of a sheet
let has_jump = icons.get("jump").is_some();
icons.remove("coin");
```

- An icon is a square as high as the font size by default; `icon_sized` and `Icon::Sprite { sprite, size }` set a size in text units.
- **Swap a set by inserting the same names again** (keyboard keys for gamepad buttons): the text does not change, and the icons update on the next draw.
- An unknown name leaves its space empty and is warned once. Icon regions are inset half a texel so linear filtering never reads the sheet's neighbours.

## 6. Effects

Per-glyph effects animate characters without laying the text out again. Name one in markup (`[wave]..[/wave]`) or a `Span::effect`.

| Built in | Tag | Defaults |
|----------|-----|----------|
| wave | `[wave=amplitude,frequency]` | amplitude 0.15 x size, frequency 2 per second |
| shake | `[shake=intensity]` | 0.06 x size |
| rainbow | `[rainbow=speed]` | 0.5 hue cycles per second |

Register your own on the `TextEffects` resource:

```rust
fn bounce(glyph: &GlyphContext) -> GlyphMod {
  let hop = (glyph.time * 4.0 - glyph.cluster as f32 * 0.35).sin().max(0.0);
  GlyphMod {
    offset: Vec2::new(0.0, -hop * glyph.size * 0.3),   // text units, +y down
    scale: 1.0 + hop * 0.12,                           // about the glyph's centre (clamped to 0..=8)
    ..GlyphMod::default()                               // color: None, alpha: 1.0 (clamped to 0..=1)
  }
}

ctx.world.resource_mut::<TextEffects>().register("bounce", bounce);
// "[bounce]hello[/bounce]"
```

- `GlyphContext` fields: `index` (the glyph in its text), `cluster` (the character, the reveal's unit), `line`, `time`, `size`, `seed` (stable per text, for noise) and `args` (the tag's value, empty without one).
- `GlyphMod`: `offset: Vec2`, `scale: f32`, `color: Option<Color>` (replaces the glyph's colour), `alpha: f32` (multiplies opacity). A non-finite result is replaced by no change.
- **An effect must be a pure function of its `GlyphContext`.** It runs per glyph, once for each pass (shadow, outline, fill) and once for each camera that sees the text, every frame, and every run must agree. Animate with `time`, vary with `seed`, `cluster` and `index`, keep no state.
- **`time` is real time in the UI and game time on world text**: UI effects keep playing through pause, world effects stop with the game clock.
- An unknown effect name is warned once and the text draws without it.

## 7. Reveal (the Typewriter)

Reveal counts character clusters and holds at `[pause=..]` tags and after punctuation. Neither reveal nor effects relayout the text.

### In the UI

`ui.text(..).typewriter(&mut TextReveal)` is the UI typewriter. When the text is drawn, the reveal advances by the UI's real time (`ui.dt()`) over the text's own layout, so `[pause]` and the punctuation pause hold it and a fade ends at full opacity.

```rust
// field: reveal: TextReveal = TextReveal::new(30.0).with_punctuation_pause(0.15).with_fade(0.1)
ui.text(RichText::markup("Hello.[pause=0.5] Welcome!"))
  .at(40.0, 420.0)
  .wrap(400.0)
  .typewriter(&mut self.reveal)
  .draw();
if self.reveal.is_done() { /* show the "next" prompt */ }
```

- Call it **once per frame per reveal**: every call advances it.
- It sends no message: check `reveal.is_done()`. It does not notice a new text: call `reveal.restart()` when the line changes.
- It replaces `.reveal(..)` and `.fade(..)` on the same text.

### Manual control in the UI

`.reveal(shown: f32)` shows the first `shown` clusters (fractional while fading); `.fade(clusters)` makes revealed characters fade in over that many clusters. With a fade, the last characters are still partly transparent when `shown` reaches the cluster count, so keep counting past the end: pass `reveal.drawn_clusters()` (the run-on value that lets a fade finish) or at least the cluster count plus the fade.

### In the world

Add `TextReveal` to a `Text2d` entity. It runs on game time (so it stops with the game clock), starts over when the text changes, and writes `TextRevealed { entity }` once, when every character has started showing.

```rust
ctx.world.spawn((
  Transform::from_xy(0.0, -80.0),
  Text2d::markup("The door creaks...[pause=0.6] open."),
  TextReveal::new(20.0).with_punctuation_pause(0.2).with_fade(0.15),
));

for done in ctx.messages::<TextRevealed>() {
  game_info!("{:?} finished", done.entity);
}
```

`TextReveal`: `new(per_second)`, `with_punctuation_pause(seconds)`, `with_fade(seconds)`, `restart()`, `is_done()`, `shown()`, `drawn_clusters()`; fields `per_second`, `punctuation_pause`, `fade` are public and can be changed live. With a fade, `is_done()` is true as soon as the last character starts showing; the clock runs on until it has faded in.

## 8. World Text: `Text2d`

```rust
ctx.world.spawn((
  Transform::from_xy(100.0, 40.0),
  Text2d::markup("[color=#ffd700]Gold[/color] x3")
    .with_style(TextStyle::new().bold().size(16.0).outline(1.5, Color::BLACK))
    .with_layout(TextLayoutOptions::default().wrap(120.0).align(TextAlign::Center).anchor(Anchor::Center))
    .with_z(10)
    .with_lighting(false)
    .with_blend(BlendMode::Alpha)
    .with_opacity(1.0),
));
```

| Builder | |
|---------|--|
| `Text2d::new(impl Into<RichText>)`, `Text2d::markup(&str)` | the text, centred on the entity |
| `with_style(TextStyle)`, `with_layout(TextLayoutOptions)` | style and layout (the anchor defaults to `Center`) |
| `with_lighting(bool)` | `true`: darkened and coloured by lights like sprites. Default `false`, so labels stay readable in the dark |
| `with_z(i32)` | sorts with sprites by `z_order`; at equal z it draws after entities |
| `with_blend(BlendMode)` | default `Alpha` |
| `with_opacity(f32)` | fades the whole text, 0 to 1 (clamped; a non-number is opaque) |
| `set_text(impl Into<RichText>)` | replace the text (a reveal starts over) |

- Fields are public (`text`, `style`, `layout`, `lit`, `z_order`, `blend`, `opacity`); the type is `#[non_exhaustive]`, so build it with the builders.
- The component requires `Transform` and `TextBounds`. The entity's transform moves, turns and scales the text; make it a child (`ctx.spawn_child(parent, bundle)`) to follow a sprite.
- **`TextBounds { size, offset }`** is the laid-out block in world units before the entity's scale and rotation; `offset` runs from the entity's position to the block's top-left. It is updated when the text is drawn, so read it a frame after spawning.
- **Opacity is applied when the text is drawn and is not part of the layout.** Changing it every frame lays nothing out again. A tween's alpha target drives it: `Tween::fade_out(0.4)` or `.to_alpha(0.0)` on the entity, exactly as for a `Renderable`. Never fade through `style.color`.
- **Lighting:** lit text is darkened and coloured by the scene's lights like sprites; unlit text (the default) is drawn at full brightness.
- **Interpolation:** the text is positioned like entities, so an `Interpolated` entity (or a child of one) moves smoothly between fixed steps. `RenderLayers` works as on sprites.
- **Per-camera sharpness:** each camera that sees a block gets glyphs made at the size it sees. Zoom in on raster text and it stays sharp; MSDF glyphs scale; pixel fonts snap per camera.
- Layout is cached and redone only when the text, style, options or a font changes.

## 9. Recipes

### A HUD with a score

```rust
struct Hud {
  score: u32,
}

impl Scene for Hud {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    self.score += 1;
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (width, _) = ui.screen_size();
    let score = TextStyle::new()
      .bold()
      .size(22.0)
      .color(Color::YELLOW)
      .outline(1.5, Color::BLACK);
    ui.text(format!("Score {:06}", self.score))
      .style(&score)
      .at(width as f32 - 12.0, 10.0)
      .anchor(Anchor::TopRight)
      .draw();
    ui.draw_text("HP", 12.0, 10.0, Color::WHITE);
    SceneAction::Continue
  }
}
```

### A dialogue box with the typewriter

```rust
struct Dialogue {
  lines: Vec<&'static str>,
  line: usize,
  reveal: TextReveal,
}

impl Dialogue {
  fn new() -> Self {
    Self {
      lines: vec![
        "Welcome, traveller.[pause=0.5] The road ahead is [color=orange]dangerous[/color].",
        "Take this [icon=key].",
      ],
      line: 0,
      reveal: TextReveal::new(30.0).with_punctuation_pause(0.15).with_fade(0.1),
    }
  }
}

impl Scene for Dialogue {
  fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    SceneAction::Continue
  }

  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) -> SceneAction {
    let (width, height) = ui.screen_size();
    let panel = Rect::new(20.0, height as f32 - 130.0, width as f32 - 40.0, 110.0);
    ui.draw_rect_bordered(panel, Color::new(16, 20, 36, 235), 10.0, 2.0, Color::rgb(120, 140, 200));
    ui.text(RichText::markup(self.lines[self.line]))
      .style(&TextStyle::new().size(18.0))
      .at(panel.x + 16.0, panel.y + 16.0)
      .wrap(panel.width - 32.0)
      .typewriter(&mut self.reveal)
      .draw();

    if self.reveal.is_done() {
      ui.text("\u{25BC}")
        .at(panel.right() - 14.0, panel.bottom() - 10.0)
        .anchor(Anchor::BottomRight)
        .draw();
      if ui.input().key_pressed(KeyCode::Space) {
        ui.consume_key(KeyCode::Space);
        if self.line + 1 < self.lines.len() {
          self.line += 1;
          self.reveal.restart();
        } else {
          return SceneAction::Pop;
        }
      }
    }
    SceneAction::Continue
  }
}
```

### A sign with an outline

```rust
fn spawn_sign(ctx: &mut SceneContext) {
  ctx.world.spawn((
    Transform::from_xy(260.0, 450.0),
    Text2d::new("THE RUSTY ANCHOR").with_style(
      TextStyle::new()
        .bold()
        .size(26.0)
        .color(Color::rgb(255, 222, 140))
        .outline(2.0, Color::rgb(40, 20, 8))
        .shadow(Vec2::new(3.0, 3.0), Color::new(0, 0, 0, 170), 2.0),
    ),
  ));
}
```

### Damage numbers

A `Text2d` that rises and fades, then despawns. The fade goes through the tween's alpha target (the text's opacity), so nothing lays out again.

```rust
fn spawn_damage(ctx: &mut SceneContext, at: Vec2, amount: u32, critical: bool) -> Entity {
  let style = TextStyle::new()
    .bold()
    .size(if critical { 24.0 } else { 18.0 })
    .color(if critical { Color::YELLOW } else { Color::WHITE })
    .outline(2.0, Color::rgb(150, 20, 40));
  ctx.world.spawn((
    Transform::from_translation(at),
    Text2d::new(amount.to_string()).with_style(style).with_z(10),
    TweenSequence::new()
      .then(Tween::to_position(at.x, at.y - 30.0, 0.35).with_easing(Ease::OutCubic))
      .then(Tween::to_position(at.x, at.y - 40.0, 0.45).to_alpha(0.0)),
  )).id()
}

// In update: despawn each number when its sequence finishes
fn clean_up(ctx: &mut SceneContext, numbers: &mut Vec<Entity>) {
  for done in ctx.messages::<TweenCompleted>() {
    if done.finished
      && let Some(at) = numbers.iter().position(|&n| n == done.entity)
    {
      numbers.swap_remove(at);
      ctx.world.despawn(done.entity);
    }
  }
}
```

### Pixel-art text

```rust
// Pixel-art game: the UI is drawn at the low-res target, so text is pixel art too
EngineBuilder::new().pixel_art(320, 180).start_scene(Level::default())

// The pixel font snaps to whole multiples of its 16 px design size
let style = TextStyle::new().role(FontRole::Pixel).size(16.0).color(Color::WHITE).outline(1.0, Color::BLACK);
ui.text("SCORE 0042").style(&style).at(8.0, 8.0).draw();
```

With `UiResolution::Window` the UI is drawn at the window's resolution instead (smooth text over pixel art). m5x7 is ASCII-only and draws capitals with lowercase shapes; that is the font file, not the engine.

### A custom effect

See section 6 (`bounce`). Register it once (on a scene's `on_enter`), then use `[bounce]..[/bounce]` in any markup.

### A game font: loaded on native, embedded on the web

```rust
fn install_game_font(ctx: &mut SceneContext) {
  // Native reads the file; the web has no file system, so it embeds the bytes
  #[cfg(not(target_arch = "wasm32"))]
  let font = ctx.fonts().load("assets/fonts/Game.ttf").ok();
  #[cfg(target_arch = "wasm32")]
  let font = ctx
    .fonts()
    .add_bytes(include_bytes!("../assets/fonts/Game.ttf").as_slice())
    .ok();

  if let Some(font) = font {
    ctx.fonts().set_role(FontRole::Ui, font);
  }
}
```

Embedding on every target (`add_bytes` only) is simpler and always works; `load` is there for fonts a player or mod can replace.

## 10. Limits and Gotchas

- **Glow needs MSDF.** A raster or pixel font draws no glow (warned once). Turn a face to MSDF with enough range (`Msdf { base: 32.0, range: 8.0 }` or more).
- **MSDF outline, glow and shadow reach is limited to `range / 2` base pixels.** The default range 4 is about 0.06 em.
- **MSDF fonts have no synthesized italic or bold.** Ship a real italic or bold face.
- **Very large text: use MSDF.** Raster and pixel glyph images are capped at 512 render pixels in the UI and 256 in the world (a zooming camera reaches huge sizes fast). Larger raster text draws the capped image scaled up, so it softens, and the engine warns once per font. Pixel fonts stay crisp: past the cap their image is a smaller multiple of the design size, scaled by a whole number. An MSDF font scales freely; MSDF bases above 256 px are made at 256 (warned once).
- **m5x7 is ASCII-only** and draws capitals with lowercase shapes; other characters fall back to Inter.
- **CJK, emoji and other scripts need a font that has them.** The built-in fonts cover Latin, Latin Extended, Greek and Cyrillic. Add a font and put it in `TextStyle::fallback(..)` or `set_role`.
- **Binary size.** The built-in fonts are about 0.58 MB in the binary. A game that ships its own fonts can turn the `default-fonts` cargo feature off (`ryo2d = { default-features = false }`); the registry then starts empty: add a font and assign it to each role you use (`set_role`), or no text draws.
- **The web has no file system:** `Fonts::load` fails there; embed fonts with `add_bytes(include_bytes!(..))`.
- **Fades go through opacity**, never the style colour (a colour fade relays out every frame).
- **Effects are pure functions of their `GlyphContext`.**
- Invalid input warns once (a bad style size, markup, an unknown effect, a missing icon, a missing character) and never panics.
