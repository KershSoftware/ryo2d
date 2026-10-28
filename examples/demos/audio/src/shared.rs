//! What the pages share: a start that plays the same way on every run, the
//! sound-started flag the web needs, and the bars and meters each page
//! draws its state with

use std::sync::atomic::{AtomicBool, Ordering};

use demo_kit::prelude::*;
use ryo2d::core::audio::{AudioEngine, MIXER_PATH};

/// The seed for the clips a sound picks and the gains and pitches it draws
const SEED: u64 = 5;

/// Background of a bar
pub(crate) const TRACK: Color = Color::new(255, 255, 255, 26);
/// Labels and headers
pub(crate) const DIM: Color = Color::rgb(150, 150, 165);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something refused or failed
pub(crate) const BAD: Color = Color::rgb(255, 110, 100);

/// Start a page's audio the same way on every run, as the tour compares
/// its frames run to run: seed the picks and draws (the engine seeds them
/// from the system otherwise; its `seed` is a test hook), and load the
/// mixer at once, so its buses are built at the end of the first frame
/// rather than whenever a worker finishes reading it. The handle keeps the
/// mixer for the page to read its snapshots and ducks
pub(crate) fn steady_start(ctx: &mut SceneContext) -> Option<Handle<Mixer>> {
  if let Some(mut engine) = ctx.world.get_resource_mut::<AudioEngine>() {
    engine.seed(SEED);
  }
  ctx.assets().load_now::<Mixer>(MIXER_PATH).ok()
}

/// `path` loaded at once, or `None` if it failed (the registry warns)
pub(crate) fn load<T: Asset>(ctx: &mut SceneContext, path: &str) -> Option<Handle<T>> {
  ctx.assets().load_now(path).ok()
}

/// A one-shot has started since the demo began. On the web the audio
/// device opens at the player's first click, tap or key press, and until
/// then every one-shot is refused: the engine says no more than that, so
/// the Buses page watches its ticks to know when sound has started
static SOUND_STARTED: AtomicBool = AtomicBool::new(false);

/// Note a one-shot's voice: one that started means sound has started
pub(crate) fn note_started(voice: &Voice) {
  if !voice.is_stopped() {
    SOUND_STARTED.store(true, Ordering::Relaxed);
  }
}

/// Whether any one-shot has started
pub(crate) fn sound_started() -> bool {
  SOUND_STARTED.load(Ordering::Relaxed)
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
