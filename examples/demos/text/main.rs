//! Text: fonts and sizes, rich text with icons and markup, the typewriter and
//! glyph effects, world text, and GPU-drawn UI. Loads no files, so it runs on
//! the web too: every icon and panel sprite is drawn in code.

use demo_kit::prelude::*;

fn main() {
  Demo::new("Text")
    .resolution(960, 640)
    .page(FontsPage::default)
    .page(RichPage::default)
    .page(RevealPage::default)
    .page(WorldPage::default)
    .page(DrawingPage::default)
    .run();
}

/// Pages keep their content left of the settings panel.
const RIGHT_EDGE: f32 = 520.0;
const DIM: Color = Color::new(160, 168, 190, 255);
const ACCENT: Color = Color::new(255, 210, 90, 255);
const INK: Color = Color::new(40, 44, 58, 255);

/// A repeatable pseudo-random number in [0, 1) for spawn variety.
fn noise(i: u32) -> f32 {
  ((i as f32 * 12.9898).sin() * 43_758.547).fract().abs()
}

/// A colour from hue, saturation and value (all 0 to 1).
fn hsv(h: f32, s: f32, v: f32) -> Color {
  let i = (h.rem_euclid(1.0) * 6.0).floor();
  let f = h.rem_euclid(1.0) * 6.0 - i;
  let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
  let (r, g, b) = match i as i32 {
    0 => (v, t, p),
    1 => (q, v, p),
    2 => (p, v, t),
    3 => (p, q, v),
    4 => (t, p, v),
    _ => (v, p, q),
  };
  let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
  Color::rgb(byte(r), byte(g), byte(b))
}

/// A small RGBA image drawn pixel by pixel: the demo's icons and panels.
struct Canvas {
  width: u32,
  height: u32,
  pixels: Vec<u8>,
}

impl Canvas {
  fn new(width: u32, height: u32) -> Self {
    Self {
      width,
      height,
      pixels: vec![0; (width * height * 4) as usize],
    }
  }

  fn set(&mut self, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
      return;
    }
    let at = ((y as u32 * self.width + x as u32) * 4) as usize;
    self.pixels[at..at + 4].copy_from_slice(&color.to_array());
  }

  fn fill(&mut self, x: i32, y: i32, width: i32, height: i32, color: Color) {
    for py in y..y + height {
      for px in x..x + width {
        self.set(px, py, color);
      }
    }
  }

  /// Pixel art: `#` marks in `rows`, top-left at (x, y).
  fn glyph(&mut self, x: i32, y: i32, rows: &[&str], color: Color) {
    for (dy, row) in rows.iter().enumerate() {
      for (dx, mark) in row.chars().enumerate() {
        if mark == '#' {
          self.set(x + dx as i32, y + dy as i32, color);
        }
      }
    }
  }

  /// Fill the inside of a rounded rect over the whole canvas: `inside(d)`
  /// gives the colour at signed distance `d` from its edge (negative
  /// inside), or `None` to leave the pixel clear.
  fn rounded(&mut self, radius: f32, inside: impl Fn(f32) -> Option<Color>) {
    let half = Vec2::new(self.width as f32, self.height as f32) / 2.0;
    for y in 0..self.height as i32 {
      for x in 0..self.width as i32 {
        let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - half;
        let q = p.abs() - (half - Vec2::splat(radius));
        let d = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius;
        if let Some(color) = inside(d) {
          // Anti-aliased outer edge
          let cover = (0.5 - d).clamp(0.0, 1.0);
          let alpha = (f32::from(color.a) * cover).round() as u8;
          self.set(x, y, color.with_alpha(alpha));
        }
      }
    }
  }

  fn into_handle(self, name: &str) -> SpriteHandle {
    SpriteHandle::from_sprite(
      Sprite::from_rgba(self.pixels, self.width, self.height),
      name,
    )
  }
}

// --- Fonts ------------------------------------------------------------------

const ROLES: [FontRole; 3] = [FontRole::Ui, FontRole::Mono, FontRole::Pixel];
const ROLE_NAMES: [&str; 3] = ["Ui", "Mono", "Pixel"];
const UI_SCALES: [Option<f32>; 5] = [None, Some(1.0), Some(1.5), Some(2.0), Some(3.0)];
const UI_SCALE_NAMES: [&str; 5] = ["Auto", "1", "1.5", "2", "3"];
const WEIGHTS: [f32; 2] = [400.0, 700.0];
const SIZES: [f32; 7] = [10.0, 12.0, 14.0, 18.0, 24.0, 32.0, 48.0];
const SENTENCE: &str = "Sharp text at every size";
const PANGRAM: &str = "The quick brown fox jumps over the lazy dog";

#[derive(Default)]
struct FontsPage {
  font: usize,
  ui_scale: usize,
  weight: usize,
  /// The UI scale last requested, so it is requested once per change
  applied_scale: Option<usize>,
}

impl DemoPage for FontsPage {
  fn name(&self) -> &str {
    "Fonts and sizes"
  }
  fn description(&self) -> &str {
    "The built-in fonts at real pixel sizes; the UI scale makes the whole UI bigger, and text stays sharp at any display scale.\n\
     m5x7 is a pixel font designed at 16 px: draw it at 16, 32 or 48 px so every pixel is whole."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Font", &mut self.font, &ROLE_NAMES);
    s.choice("UI scale", &mut self.ui_scale, &UI_SCALE_NAMES);
    s.choice("Weight", &mut self.weight, &["400", "700"]);
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.applied_scale != Some(self.ui_scale) {
      self.applied_scale = Some(self.ui_scale);
      ctx.request_ui_scale(UI_SCALES[self.ui_scale]);
    }
  }
  fn exit(&mut self, ctx: &mut SceneContext) {
    // The UI scale outlives the page's scene
    ctx.request_ui_scale(None);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let label = TextStyle::new().size(12.0).color(DIM);
    let label_h = ui.line_height(&label);
    let mut y = area.y + 4.0;
    for size in SIZES {
      let style = TextStyle::new()
        .role(ROLES[self.font])
        .weight(WEIGHTS[self.weight])
        .size(size);
      let line = ui.line_height(&style);
      ui.text(format!("{size} px"))
        .style(&label)
        .at(area.x, y + (line - label_h) / 2.0)
        .draw();
      ui.text(SENTENCE).style(&style).at(area.x + 44.0, y).draw();
      y += line;
    }

    y += 12.0;
    ui.text("The built-in fonts")
      .style(&TextStyle::new().size(14.0).color(ACCENT))
      .at(area.x, y)
      .draw();
    y += 22.0;
    let inter = TextStyle::new().family("Inter").size(18.0);
    let mono = TextStyle::new().role(FontRole::Mono).size(16.0);
    let pixel = TextStyle::new().role(FontRole::Pixel);
    let builtins = [
      ("Inter Regular", inter.clone()),
      ("Inter Bold", inter.bold()),
      ("JetBrains Mono", mono),
      ("m5x7, 16 px", pixel.clone().size(16.0)),
      ("m5x7, 32 px", pixel.size(32.0)),
    ];
    for (name, style) in builtins {
      let line = ui.line_height(&style);
      ui.text(name)
        .style(&label)
        .at(area.x, y + (line - label_h) / 2.0)
        .draw();
      ui.text(PANGRAM)
        .style(&style)
        .at(area.x + 104.0, y)
        .wrap(area.width - 104.0)
        .max_lines(1)
        .ellipsis()
        .draw();
      y += line + 4.0;
    }
  }
}

// --- Rich text --------------------------------------------------------------

const PARAGRAPH: &str = "Markup styles a run of text: [color=#ffd24a]colour[/color], \
  [size=26]size[/size], [font=JetBrains Mono]a font[/font], [b]bold[/b], [i]italic[/i] and \
  [outline=1,#ff3d7f]an outline[/outline]. Tags nest: [b][color=#7fd0ff]bold and blue, \
  [i]then italic too[/i][/color][/b].";
const ICON_LINES: [&str; 3] = [
  "Press [icon=jump] to jump",
  "[icon=attack] attacks",
  "[icon=menu] opens the menu",
];
const ICON_NAMES: [&str; 3] = ["jump", "attack", "menu"];
const MALFORMED: &str = "[color=red]never closed";

/// 5 x 7 pixel letters for the icons
const LETTER_Z: [&str; 7] = [
  "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####",
];
const LETTER_X: [&str; 7] = [
  "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#",
];
const LETTER_A: [&str; 7] = [
  ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
];
const ESC: [&str; 5] = [
  "###.###.###",
  "#...#...#..",
  "##..###.#..",
  "#.....#.#..",
  "###.###.###",
];
const MENU_LINES: [&str; 5] = ["#####", ".....", "#####", ".....", "#####"];

/// A 16 x 16 key cap with a pixel letter on it
fn key_cap(letter: &[&str], name: &str) -> SpriteHandle {
  let mut c = Canvas::new(16, 16);
  c.fill(0, 0, 16, 16, Color::rgb(96, 102, 128));
  for (x, y) in [(0, 0), (15, 0), (0, 15), (15, 15)] {
    c.set(x, y, Color::TRANSPARENT);
  }
  c.fill(1, 1, 14, 11, Color::rgb(226, 230, 240));
  c.fill(1, 12, 14, 3, Color::rgb(160, 166, 188));
  let width = letter.first().map_or(0, |row| row.len()) as i32;
  let top = if letter.len() == 7 { 3 } else { 4 };
  c.glyph((16 - width) / 2, top, letter, INK);
  c.into_handle(name)
}

/// A 16 x 16 round gamepad button with a pixel letter on it
fn pad_button(letter: &[&str], fill: Color, name: &str) -> SpriteHandle {
  let mut c = Canvas::new(16, 16);
  for y in 0..16 {
    for x in 0..16 {
      let d = Vec2::new(x as f32 + 0.5 - 8.0, y as f32 + 0.5 - 8.0).length();
      if d <= 6.6 {
        c.set(x, y, fill);
      } else if d <= 8.0 {
        c.set(x, y, INK);
      }
    }
  }
  let width = letter.first().map_or(0, |row| row.len()) as i32;
  let top = (16 - letter.len() as i32) / 2;
  c.glyph((16 - width) / 2, top, letter, Color::WHITE);
  c.into_handle(name)
}

#[derive(Default)]
struct RichPage {
  icons: usize,
  /// The icon set in `TextIcons`, so a change re-inserts the names once
  applied: Option<usize>,
}

impl DemoPage for RichPage {
  fn name(&self) -> &str {
    "Rich text"
  }
  fn description(&self) -> &str {
    "Rich text from markup: colours, sizes, fonts, bold, italic, outlines and inline icons. Malformed markup draws as written.\n\
     Icons are named sprites in the TextIcons resource.\n\
     Inserting the same names with other sprites swaps the set without touching the text."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Icons", &mut self.icons, &["Keyboard", "Gamepad"]);
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.applied == Some(self.icons) {
      return;
    }
    self.applied = Some(self.icons);
    let set = if self.icons == 0 {
      [
        key_cap(&LETTER_Z, "key-z"),
        key_cap(&LETTER_X, "key-x"),
        key_cap(&ESC, "key-esc"),
      ]
    } else {
      [
        pad_button(&LETTER_A, Color::rgb(60, 176, 84), "pad-a"),
        pad_button(&LETTER_X, Color::rgb(64, 120, 232), "pad-x"),
        pad_button(&MENU_LINES, Color::rgb(112, 118, 138), "pad-menu"),
      ]
    };
    let mut icons = ctx.world.resource_mut::<TextIcons>();
    for (name, sprite) in ICON_NAMES.into_iter().zip(set) {
      icons.insert(name, sprite);
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 4.0;
    let width = RIGHT_EDGE - x;
    let body = TextStyle::new().size(18.0);
    let caption = TextStyle::new().size(12.0).color(DIM);
    let mono = TextStyle::new().role(FontRole::Mono).size(12.0).color(DIM);
    let mut y = area.y + 4.0;

    y += ui
      .text(RichText::markup(PARAGRAPH))
      .style(&body)
      .at(x, y)
      .wrap(width)
      .draw()
      .y;
    y += 8.0;
    // The source, drawn as plain text
    y += ui
      .text(PARAGRAPH)
      .style(&mono)
      .at(x, y)
      .wrap(width)
      .draw()
      .y;

    y += 20.0;
    ui.text("Inline icons, looked up by name when drawn")
      .style(&TextStyle::new().size(14.0).color(ACCENT))
      .at(x, y)
      .draw();
    y += 24.0;
    // An icon is as high as the font size: 16 px draws these texel for texel
    let icon_line = TextStyle::new().size(16.0);
    for line in ICON_LINES {
      let size = ui
        .text(RichText::markup(line))
        .style(&icon_line)
        .at(x, y)
        .draw();
      ui.text(line)
        .style(&mono)
        .at(x + 220.0, y + size.y / 2.0)
        .anchor(Anchor::CenterLeft)
        .draw();
      y += size.y + 8.0;
    }

    y += 20.0;
    ui.text("Malformed markup")
      .style(&TextStyle::new().size(14.0).color(ACCENT))
      .at(x, y)
      .draw();
    y += 24.0;
    y += ui
      .text(RichText::markup(MALFORMED))
      .style(&body)
      .at(x, y)
      .draw()
      .y;
    ui.text("An unclosed tag is drawn as written, and warned about once (see the log).")
      .style(&caption)
      .at(x, y + 4.0)
      .wrap(width)
      .draw();
  }
}

// --- Typewriter and effects -------------------------------------------------

const DIALOGUE: &str = "Welcome to the inn, traveller![pause=0.6] The road ahead is long, \
  and the nights are cold. Rest here, and press Space to hear this again.";
const EFFECT_LINES: [(&str, &str); 4] = [
  ("[wave]", "[wave]Waves roll through these words[/wave]"),
  ("[shake]", "[shake]Something is very wrong[/shake]"),
  (
    "[rainbow]",
    "[rainbow]Every colour of the rainbow[/rainbow]",
  ),
  (
    "[bounce]",
    "[bounce]A custom effect, registered in code[/bounce]",
  ),
];

/// The custom effect: each glyph hops in turn. It is a pure function of the
/// glyph context, as `TextEffects::register` requires.
fn bounce(glyph: &GlyphContext) -> GlyphMod {
  let hop = (glyph.time * 4.0 - glyph.cluster as f32 * 0.35)
    .sin()
    .max(0.0);
  GlyphMod {
    offset: Vec2::new(0.0, -hop * glyph.size * 0.3),
    scale: 1.0 + hop * 0.12,
    ..GlyphMod::default()
  }
}

struct RevealPage {
  /// Characters a second
  speed: f32,
  /// Characters each one takes to fade in
  fade: f32,
  /// Seconds after punctuation
  punctuation: f32,
  reveal: TextReveal,
  blink: f32,
}

impl Default for RevealPage {
  fn default() -> Self {
    Self {
      speed: 30.0,
      fade: 2.0,
      punctuation: 0.15,
      reveal: TextReveal::new(30.0),
      blink: 0.0,
    }
  }
}

impl DemoPage for RevealPage {
  fn name(&self) -> &str {
    "Typewriter and effects"
  }
  fn description(&self) -> &str {
    "A typewriter reveal that holds at [pause] tags and after punctuation, and per-glyph effects: wave, shake, rainbow and a custom bounce.\n\
     The dialogue is ui.text(..).typewriter(&mut reveal), which advances a TextReveal by the UI's real time each frame.\n\
     The bounce effect is registered in TextEffects when the page opens; an effect is a pure function of its glyph."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Speed (chars/s)", &mut self.speed, 10.0..=80.0, 5.0);
    s.float("Fade (chars)", &mut self.fade, 0.0..=4.0, 0.5);
    s.float("Punctuation pause", &mut self.punctuation, 0.0..=0.5, 0.05);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Restart the line")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .world
      .resource_mut::<TextEffects>()
      .register("bounce", bounce);
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    if ui.input().key_pressed(KeyCode::Space) {
      self.reveal.restart();
      self.blink = 0.0;
    }
    self.reveal.per_second = self.speed;
    self.reveal.punctuation_pause = self.punctuation;
    // The fade is set in seconds
    self.reveal.fade = self.fade / self.speed;

    let area = content_area(ui);
    let x = area.x + 4.0;
    let dialog = Rect::new(x, area.y + 22.0, RIGHT_EDGE - x, 108.0);
    ui.draw_rect_bordered(
      dialog,
      Color::new(16, 20, 36, 235),
      10.0,
      2.0,
      Color::rgb(120, 140, 200),
    );
    let plate = Rect::new(dialog.x + 14.0, dialog.y - 16.0, 76.0, 26.0);
    ui.draw_rect_bordered(
      plate,
      Color::rgb(60, 72, 120),
      6.0,
      2.0,
      Color::rgb(120, 140, 200),
    );
    ui.text("Mira")
      .style(&TextStyle::new().bold().size(15.0).color(ACCENT))
      .at(plate.x + plate.width / 2.0, plate.y + plate.height / 2.0)
      .anchor(Anchor::Center)
      .draw();
    ui.text(RichText::markup(DIALOGUE))
      .style(&TextStyle::new().size(18.0))
      .at(dialog.x + 18.0, dialog.y + 20.0)
      .wrap(dialog.width - 36.0)
      .typewriter(&mut self.reveal)
      .draw();
    // A blinking prompt once the line has finished
    if self.reveal.is_done() {
      self.blink = (self.blink + ui.dt()) % 1.0;
    }
    if self.reveal.is_done() && self.blink < 0.6 {
      ui.text("\u{25BC}")
        .style(&TextStyle::new().size(14.0).color(ACCENT))
        .at(dialog.right() - 14.0, dialog.bottom() - 10.0)
        .anchor(Anchor::BottomRight)
        .draw();
    }
    let status = if self.reveal.is_done() {
      "Done: is_done() is true".to_string()
    } else {
      format!("Revealing: {:.1} characters shown", self.reveal.shown())
    };
    ui.text(status)
      .style(&TextStyle::new().size(12.0).color(DIM))
      .at(x, dialog.bottom() + 8.0)
      .draw();

    let mut y = dialog.bottom() + 44.0;
    ui.text("Glyph effects")
      .style(&TextStyle::new().size(14.0).color(ACCENT))
      .at(x, y)
      .draw();
    y += 30.0;
    let tag = TextStyle::new().role(FontRole::Mono).size(13.0).color(DIM);
    let line = TextStyle::new().size(22.0);
    let line_h = ui.line_height(&line);
    for (name, markup) in EFFECT_LINES {
      ui.text(name)
        .style(&tag)
        .at(x, y + line_h / 2.0)
        .anchor(Anchor::CenterLeft)
        .draw();
      ui.text(RichText::markup(markup))
        .style(&line)
        .at(x + 96.0, y)
        .draw();
      y += line_h + 14.0;
    }
  }
}

// --- World text -------------------------------------------------------------

const ZOOMS: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
const ZOOM_NAMES: [&str; 5] = ["0.5", "1", "2", "4", "8"];
const EFFECTS: [&str; 4] = ["None", "Outline", "Shadow", "Glow"];
/// Room in the distance field for glow: the default range (4) holds about
/// 0.06 em of effect, this about 0.375 em
const MSDF: FontRender = FontRender::Msdf {
  base: 32.0,
  range: 24.0,
};
/// The world point that stays put on screen while zooming
const FOCUS: Vec2 = Vec2::new(260.0, 440.0);
/// Seconds a damage number rises and fades
const DAMAGE_LIFE: f32 = 0.8;

/// A damage number's fade: `[expire=born]`, `born` the game time it was
/// spawned. World text has no opacity for a tween to fade, and an effect
/// must be pure, so its start rides in the tag's value.
fn expire(glyph: &GlyphContext) -> GlyphMod {
  let born = glyph.args.parse::<f32>().unwrap_or(glyph.time);
  let t = ((glyph.time - born) / DAMAGE_LIFE).clamp(0.0, 1.0);
  GlyphMod {
    alpha: 1.0 - t * t,
    ..GlyphMod::default()
  }
}

struct WorldPage {
  zoom: usize,
  msdf: bool,
  effects: usize,
  pixel: bool,
  /// The render mode Inter Bold was last given
  applied_msdf: Option<bool>,
  /// Where the camera starts (the middle of the design area)
  home: Vec2,
  walkers: Vec<Entity>,
  tags: Vec<Entity>,
  numbers: Vec<Entity>,
  spawned: u32,
}

impl Default for WorldPage {
  fn default() -> Self {
    Self {
      zoom: 1,
      msdf: false,
      effects: 1,
      pixel: false,
      applied_msdf: None,
      home: Vec2::ZERO,
      walkers: Vec::new(),
      tags: Vec::new(),
      numbers: Vec::new(),
      spawned: 0,
    }
  }
}

impl WorldPage {
  /// The name tags' style for the current settings
  fn tag_style(&self) -> TextStyle {
    let base = if self.pixel {
      TextStyle::new().role(FontRole::Pixel).size(16.0)
    } else {
      TextStyle::new().bold().size(14.0)
    };
    match self.effects {
      1 => base.outline(1.5, Color::rgb(20, 20, 30)),
      2 => base.shadow(Vec2::new(1.5, 1.5), Color::new(0, 0, 0, 200), 1.5),
      3 => base.glow(2.5, Color::new(255, 176, 60, 210)),
      _ => base,
    }
  }

  fn spawn_number(&mut self, ctx: &mut SceneContext) {
    let i = self.spawned;
    self.spawned += 1;
    let Some(&walker) = self
      .walkers
      .get((noise(i + 11) * self.walkers.len() as f32) as usize)
    else {
      return;
    };
    let Some(at) = ctx
      .world
      .get::<GlobalTransform>(walker)
      .map(GlobalTransform::translation)
    else {
      return;
    };
    let value = 5 + (noise(i) * 120.0) as u32;
    let critical = value > 100;
    let born = ctx.time.game.elapsed();
    let style = TextStyle::new()
      .bold()
      .size(if critical { 24.0 } else { 18.0 })
      .color(if critical {
        ACCENT
      } else {
        Color::rgb(255, 255, 255)
      })
      .outline(2.0, Color::rgb(150, 20, 40));
    let text = if critical {
      format!("[expire={born:.3}]{value}![/expire]")
    } else {
      format!("[expire={born:.3}]{value}[/expire]")
    };
    let start = at + Vec2::new((noise(i + 5) - 0.5) * 20.0, -12.0);
    let number = ctx
      .world
      .spawn((
        Transform::from_translation(start),
        Text2d::markup(&text).with_style(style).with_z(10),
        Tween::to_position(start.x, start.y - 40.0, DAMAGE_LIFE).with_easing(Ease::OutCubic),
      ))
      .id();
    self.numbers.push(number);
  }
}

impl DemoPage for WorldPage {
  fn name(&self) -> &str {
    "World text"
  }
  fn description(&self) -> &str {
    "Text on entities: name tags that follow their sprites, a sign with an outline and a shadow, and rising damage numbers.\n\
     Zoom in and the text stays sharp: raster glyphs are made at the size the camera sees.\n\
     MSDF draws Inter Bold from a distance field, which scales freely and adds soft shadows and glow.\n\
     Glow needs an MSDF font with room in its range: this page uses range 24, where the default 4 holds very little."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.choice("Zoom", &mut self.zoom, &ZOOM_NAMES);
    s.toggle("MSDF", &mut self.msdf);
    s.choice("Effects", &mut self.effects, &EFFECTS);
    s.toggle("Pixel font", &mut self.pixel);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::keys(
      "Enter / click",
      &[KeyCode::Enter],
      "Spawn a damage number",
    )]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    self.home = ctx.camera().position();
    ctx
      .world
      .resource_mut::<TextEffects>()
      .register("expire", expire);
    let mut dot = Canvas::new(32, 32);
    dot.rounded(16.0, |_| Some(Color::WHITE));
    let dot = dot.into_handle("walker");
    let walkers = [
      ("Aria", Color::rgb(240, 90, 90), 240.0, (90.0, 430.0), 3.0),
      ("Bram", Color::rgb(90, 200, 120), 305.0, (430.0, 120.0), 4.2),
      (
        "Cleo",
        Color::rgb(100, 150, 255),
        370.0,
        (200.0, 440.0),
        2.6,
      ),
    ];
    let style = self.tag_style();
    for (name, tint, y, (from, to), seconds) in walkers {
      let mut look = Renderable::sprite(dot.clone());
      look.tint = tint;
      let walker = ctx
        .world
        .spawn((
          Transform::from_xy(from, y),
          look,
          Tween::new(seconds)
            .to_x(to)
            .with_easing(Ease::InOutSine)
            .with_loop(TweenLoop::PingPong),
        ))
        .id();
      let tag = ctx.spawn_child(
        walker,
        (
          Transform::from_xy(0.0, -22.0),
          Text2d::new(name)
            .with_style(style.clone())
            .with_layout(TextLayoutOptions::default().anchor(Anchor::BottomCenter)),
        ),
      );
      self.walkers.push(walker);
      self.tags.push(tag);
    }

    // A wooden sign
    let mut board = Canvas::new(300, 60);
    board.fill(0, 0, 300, 60, Color::rgb(70, 42, 22));
    board.fill(4, 4, 292, 52, Color::rgb(128, 82, 46));
    for (y, shade) in [(14, 112u8), (27, 118), (41, 110)] {
      board.fill(4, y, 292, 2, Color::rgb(shade, 72, 40));
    }
    ctx.world.spawn((
      Transform::from_xy(260.0, 450.0),
      Renderable::sprite(board.into_handle("sign")),
    ));
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
  fn exit(&mut self, ctx: &mut SceneContext) {
    // Fonts outlive the page's scene: put Inter Bold back to raster
    if let Some(bold) = ctx.fonts().find_face("Inter", 700.0, false) {
      ctx.fonts().set_render(bold, FontRender::Raster);
    }
  }
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    if self.applied_msdf != Some(self.msdf) {
      self.applied_msdf = Some(self.msdf);
      let render = if self.msdf { MSDF } else { FontRender::Raster };
      if let Some(bold) = ctx.fonts().find_face("Inter", 700.0, false) {
        ctx.fonts().set_render(bold, render);
      }
    }

    // Zoom about FOCUS: it keeps its place on screen
    let zoom = ZOOMS[self.zoom];
    let eye = FOCUS - (FOCUS - self.home) / zoom;
    {
      let mut camera = ctx.camera();
      camera.set_zoom(zoom);
      camera.set_position(eye.x, eye.y);
    }

    let style = self.tag_style();
    for &tag in &self.tags {
      if let Some(mut text) = ctx.world.get_mut::<Text2d>(tag)
        && text.style != style
      {
        text.style = style.clone();
      }
    }

    let clicked = ctx.input.mouse_pressed_and_consume(MouseButton::Left);
    if clicked || ctx.input.key_pressed_and_consume(KeyCode::Enter) {
      self.spawn_number(ctx);
    }
    for done in ctx.messages::<TweenCompleted>() {
      if done.finished
        && let Some(at) = self.numbers.iter().position(|&n| n == done.entity)
      {
        self.numbers.swap_remove(at);
        ctx.world.despawn(done.entity);
      }
    }
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let note = if self.effects == 3 && !self.msdf {
      Some(("Glow needs an MSDF font: turn MSDF on.", ACCENT))
    } else if self.msdf && self.pixel {
      Some((
        "The tags use the pixel font; MSDF applies to the sign.",
        DIM,
      ))
    } else if self.msdf {
      Some((
        "Inter Bold is MSDF (base 32 px, range 24 px): the room in its range is what glow and soft shadows can use.",
        DIM,
      ))
    } else {
      None
    };
    // A backing, so the captions stay readable over the zoomed world
    let height = if note.is_some() { 56.0 } else { 26.0 };
    ui.draw_rounded_rect(
      Rect::new(area.x, area.y, RIGHT_EDGE - area.x, height),
      6.0,
      Color::new(16, 18, 30, 190),
    );
    ui.text(format!(
      "Damage numbers on screen: {}   Zoom: {}x",
      self.numbers.len(),
      ZOOM_NAMES[self.zoom]
    ))
    .at(area.x + 8.0, area.y + 5.0)
    .draw();
    if let Some((note, color)) = note {
      ui.text(note)
        .style(&TextStyle::new().size(12.0).color(color))
        .at(area.x + 8.0, area.y + 25.0)
        .wrap(RIGHT_EDGE - area.x - 16.0)
        .draw();
    }
  }
}

// --- UI drawing -------------------------------------------------------------

const ROW_HEIGHT: f32 = 22.0;
const ITEMS: [&str; 10] = [
  "Iron sword",
  "Healing potion",
  "Rope, 50 ft",
  "Lantern",
  "Silver key",
  "Map of the coast",
  "Bag of coins",
  "Wolf pelt",
  "Torch",
  "Old letter",
];

/// The nine-slice panel: 24 x 24 texels, rounded corners and a rivet inside
/// each 8 px inset, a gold border and a dark fill
fn nine_slice_sprite() -> SpriteHandle {
  let mut c = Canvas::new(24, 24);
  c.rounded(6.0, |d| {
    Some(if d > -2.0 {
      Color::rgb(232, 190, 92)
    } else if d > -3.0 {
      Color::rgb(90, 70, 40)
    } else {
      Color::new(26, 30, 50, 240)
    })
  });
  for (x, y) in [(4, 4), (18, 4), (4, 18), (18, 18)] {
    c.fill(x, y, 2, 2, Color::rgb(255, 236, 170));
  }
  c.into_handle("nine-slice")
}

struct DrawingPage {
  radius: f32,
  border: f32,
  /// Percent
  opacity: f32,
  panel: Option<SpriteHandle>,
  scroll: ScrollViewState,
}

impl Default for DrawingPage {
  fn default() -> Self {
    Self {
      radius: 10.0,
      border: 2.0,
      opacity: 100.0,
      panel: None,
      scroll: ScrollViewState::new(),
    }
  }
}

impl DemoPage for DrawingPage {
  fn name(&self) -> &str {
    "UI drawing"
  }
  fn description(&self) -> &str {
    "The UI drawn on the GPU over a bright world: translucent rounded panels with borders, a nine-slice, a polyline and a clipped scroll view.\n\
     Panel opacity scales the three panels' 25, 50 and 75%; translucent UI blends in sRGB space, as on the web."
  }
  fn settings(&mut self, s: &mut Settings) {
    s.float("Corner radius", &mut self.radius, 0.0..=16.0, 1.0);
    s.float("Border", &mut self.border, 0.0..=4.0, 0.5);
    s.float("Panel opacity (%)", &mut self.opacity, 10.0..=100.0, 5.0);
  }
  fn controls(&self) -> Vec<Control> {
    vec![Control::mouse("Scroll the list (wheel or scrollbar)")]
  }
  fn enter(&mut self, ctx: &mut SceneContext) {
    // A bright background of world sprites for the panels to sit over
    let tile = SpriteHandle::from_sprite(Sprite::solid_rect(60, 60, 255, 255, 255, 255), "tile");
    for row in 0..10 {
      for column in 0..9 {
        let hue = column as f32 / 9.0 + row as f32 * 0.04;
        let value = if (row + column) % 2 == 0 { 1.0 } else { 0.8 };
        let mut look = Renderable::sprite(tile.clone());
        look.tint = hsv(hue, 0.75, value);
        ctx.world.spawn((
          Transform::from_xy(30.0 + column as f32 * 60.0, 60.0 + row as f32 * 60.0),
          look,
        ));
      }
    }
    self.panel = Some(nine_slice_sprite());
  }
  fn ui(&mut self, ui: &mut UiContext, _ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 8.0;
    let caption = TextStyle::new().size(12.0).color(Color::WHITE);
    let label = TextStyle::new().bold().size(18.0);

    // Translucent panels
    let mut y = area.y + 8.0;
    for (k, base) in [0.25, 0.5, 0.75].into_iter().enumerate() {
      let alpha = base * self.opacity / 100.0;
      let rect = Rect::new(x + k as f32 * 166.0, y, 150.0, 80.0);
      ui.draw_rect_bordered(
        rect,
        Color::new(16, 20, 40, (alpha * 255.0).round() as u8),
        self.radius,
        self.border,
        Color::rgb(235, 240, 255),
      );
      let (cx, cy) = rect.center();
      ui.text(format!("{:.0}%", alpha * 100.0))
        .style(&label)
        .at(cx, cy - 8.0)
        .anchor(Anchor::Center)
        .draw();
      ui.text("opacity")
        .style(&caption)
        .at(cx, cy + 14.0)
        .anchor(Anchor::Center)
        .draw();
    }

    // A nine-slice at three sizes: the corners keep their texels
    y += 96.0;
    if let Some(panel) = &self.panel {
      let mut px = x;
      for (w, h) in [(64.0, 48.0), (150.0, 72.0), (260.0, 96.0)] {
        let rect = Rect::new(px, y, w, h);
        ui.draw_nine_slice(panel, rect, [8.0; 4], Color::WHITE);
        let (cx, cy) = rect.center();
        ui.text(format!("{w:.0} x {h:.0}"))
          .style(&caption)
          .at(cx, cy)
          .anchor(Anchor::Center)
          .draw();
        px += w + 14.0;
      }
    }

    // A polyline with sharp corners
    y += 112.0;
    let points: Vec<Vec2> = (0..=10)
      .map(|i| {
        let up = if i % 2 == 0 { 0.0 } else { 40.0 };
        Vec2::new(x + 10.0 + i as f32 * 46.0, y + 44.0 - up)
      })
      .collect();
    ui.draw_line(&points, 6.0, Color::new(20, 24, 40, 230));
    ui.draw_line(&points, 2.0, Color::rgb(255, 255, 255));

    // A scroll view: 30 rows, clipped to the view by a scissor
    y += 66.0;
    let view = Rect::new(x, y, 300.0, (area.bottom() - y - 8.0).max(60.0));
    let dt = ui.dt();
    let row_style = TextStyle::new().size(14.0);
    let line_h = ui.line_height(&row_style);
    ui.scroll_view(
      &mut self.scroll,
      view,
      30.0 * ROW_HEIGHT,
      dt,
      |ui, scroll_y| {
        for i in 0..30 {
          let row_y = view.y + i as f32 * ROW_HEIGHT - scroll_y;
          if i % 2 == 1 {
            ui.draw_rect(
              Rect::new(view.x, row_y, view.width, ROW_HEIGHT),
              Color::new(255, 255, 255, 14),
            );
          }
          let text_y = row_y + (ROW_HEIGHT - line_h) / 2.0;
          ui.text(format!("{:02}", i + 1))
            .style(&row_style)
            .color(DIM)
            .at(view.x + 8.0, text_y)
            .draw();
          ui.text(ITEMS[i % ITEMS.len()])
            .style(&row_style)
            .at(view.x + 40.0, text_y)
            .draw();
          ui.text(format!("x{}", 1 + (noise(i as u32) * 9.0) as u32))
            .style(&row_style)
            .color(ACCENT)
            .at(view.x + 250.0, text_y)
            .anchor(Anchor::TopRight)
            .draw();
        }
      },
    );
    let note_x = view.right() + 16.0;
    let note = Rect::new(note_x - 8.0, view.y, RIGHT_EDGE - note_x + 8.0, 92.0);
    ui.draw_rounded_rect(note, 6.0, Color::new(16, 20, 40, 200));
    ui.text("30 rows in a scroll view: rows wholly outside it are not drawn, and its scissor cuts the rows at its edges.")
      .style(&caption)
      .at(note_x, view.y + 8.0)
      .wrap(note.width - 16.0)
      .draw();
  }
}
