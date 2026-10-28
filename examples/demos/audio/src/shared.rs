//! What the pages share: the mixer loaded at once, the web's "click or tap
//! to start sound" line on every page, and the bars and meters each page
//! draws its state with

use demo_kit::prelude::*;
use ryo2d::core::audio::MIXER_PATH;

/// Background of a bar
pub(crate) const TRACK: Color = Color::new(255, 255, 255, 26);
/// Labels and headers
pub(crate) const DIM: Color = Color::rgb(150, 150, 165);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something refused or failed
pub(crate) const BAD: Color = Color::rgb(255, 110, 100);

/// The game's mixer, loaded at once. The engine loads it by itself, on a
/// worker; a scene that plays on the mixer's own buses (the limits page's
/// rows) or reads its snapshots from its first frame loads it at once, so
/// its buses are built at the end of that frame. Every page here does, so
/// the tour's captures do not wait on a worker. The handle keeps it for the
/// page to read
pub(crate) fn load_mixer(ctx: &mut SceneContext) -> Option<Handle<Mixer>> {
  ctx.assets().load_now::<Mixer>(MIXER_PATH).ok()
}

/// `path` loaded at once, or `None` if it failed (the registry warns)
pub(crate) fn load<T: Asset>(ctx: &mut SceneContext, path: &str) -> Option<Handle<T>> {
  ctx.assets().load_now(path).ok()
}

/// A page with the web's "click or tap to start sound" line: on the web
/// the audio device opens at the player's first click, tap or key press,
/// and until then nothing sounds. Natively it never shows
pub(crate) struct WebHint<P>(pub(crate) P);

impl<P: DemoPage> DemoPage for WebHint<P> {
  fn name(&self) -> &str {
    self.0.name()
  }

  fn description(&self) -> &str {
    self.0.description()
  }

  fn settings(&mut self, s: &mut Settings) {
    self.0.settings(s);
  }

  fn controls(&self) -> Vec<Control> {
    self.0.controls()
  }

  fn wants_escape(&self) -> bool {
    self.0.wants_escape()
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.0.enter(ctx);
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    self.0.exit(ctx);
  }

  fn update(&mut self, ctx: &mut SceneContext, dt: f32) {
    self.0.update(ctx, dt);
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    self.0.ui(ui, ctx);
    if ctx.audio().waiting_for_gesture() {
      // Over the foot of the page's content, as a banner
      let area = content_area(ui);
      let text = "Click or tap to start sound (a gamepad cannot start it)";
      let (w, h) = ui.text_size(text);
      let banner = Rect::new(
        area.x,
        area.y + area.height - h as f32 - 12.0,
        w as f32 + 20.0,
        h as f32 + 12.0,
      );
      ui.draw_rect(banner, Color::new(20, 20, 30, 230));
      ui.draw_text(text, banner.x + 10.0, banner.y + 6.0, HOT);
    }
  }

  fn render(&mut self, gpu: &mut GpuRenderContext) {
    self.0.render(gpu);
  }
}

/// Each bus's colour, by the standard bus it is under
pub(crate) fn bus_color(bus: &str) -> Color {
  match bus.split('/').next().unwrap_or(bus) {
    "music" => Color::rgb(170, 130, 255),
    "sfx" => Color::rgb(255, 160, 70),
    "ui" => Color::rgb(90, 210, 240),
    "voice" => Color::rgb(255, 120, 190),
    "ambience" => Color::rgb(110, 210, 120),
    _ => Color::WHITE,
  }
}

/// A bar `width` wide, filled `fraction` of the way
pub(crate) fn bar(ui: &mut UiContext, rect: Rect, fraction: f32, color: Color) {
  ui.draw_rect(rect, TRACK);
  let fraction = fraction.clamp(0.0, 1.0);
  if fraction > 0.0 {
    ui.draw_rect(
      Rect::new(rect.x, rect.y, rect.width * fraction, rect.height),
      color,
    );
  }
}

/// A gain in dB as a bar's fill: silence (-60 dB) empty, 0 dB full
pub(crate) fn db_fraction(db: f32) -> f32 {
  (db + 60.0) / 60.0
}

/// A gain in dB as text: "silent" at -60 dB and below
pub(crate) fn db_text(db: f32) -> String {
  if db <= -60.0 {
    "silent".to_string()
  } else {
    format!("{db:+.1} dB")
  }
}

/// `count` voices as filled dots in `slots` places from `x` (more than
/// `slots` show as the last dot filled), `y` the row's top
pub(crate) fn voice_dots(
  ui: &mut UiContext,
  x: f32,
  y: f32,
  count: usize,
  slots: usize,
  color: Color,
) {
  for slot in 0..slots {
    let dot = Rect::new(x + slot as f32 * 13.0, y + 4.0, 10.0, 10.0);
    if slot < count {
      ui.draw_rounded_rect(dot, 5.0, color);
    } else {
      ui.draw_rounded_rect(dot, 5.0, Color::new(255, 255, 255, 30));
    }
  }
}

/// A meter of each of `buses`: its name, its voices (`voices_on`, which
/// counts the buses under it too) as dots and a number. Returns the height
/// drawn
pub(crate) fn bus_meters(
  ui: &mut UiContext,
  ctx: &mut SceneContext,
  x: f32,
  y: f32,
  buses: &[&str],
) -> f32 {
  const ROW: f32 = 22.0;
  ui.draw_text("voices_on", x, y, DIM);
  let audio = ctx.audio();
  for (i, &bus) in buses.iter().enumerate() {
    let row_y = y + ROW * (i as f32 + 1.0);
    let voices = audio.voices_on(bus);
    let color = bus_color(bus);
    ui.draw_text(bus, x, row_y, color);
    voice_dots(ui, x + 110.0, row_y, voices, 6, color);
    ui.draw_text(&voices.to_string(), x + 194.0, row_y, Color::WHITE);
  }
  ROW * (buses.len() as f32 + 1.0)
}
