//! The Buses page: the standard bus tree (master, then music, sfx, ui,
//! voice and ambience under it) and the player's six volume settings, one
//! slider each. Music and a wind loop play throughout, and a pattern of
//! one-shots on sfx, ui and voice, so each slider is heard as it moves

use demo_kit::prelude::*;

use crate::shared::{DIM, bar, bus_color, db_fraction, db_text, load, load_mixer, voice_dots};

/// The rows' labels, in `VolumeKind::ALL` order
const LABELS: [&str; 6] = ["Master", "Music", "Sfx", "UI", "Voice", "Ambience"];

pub(crate) struct BusesPage {
  /// The six settings, in `VolumeKind::ALL` order
  levels: [f32; 6],
  /// The pattern of one-shots plays by itself
  pattern: bool,
  /// Fixed steps since the page opened
  frame: u32,
  sounds: Option<Sounds>,
  _mixer: Option<Handle<Mixer>>,
}

/// What the page plays
struct Sounds {
  theme: Handle<Music>,
  wind: Handle<Sound>,
  tick: Handle<Sound>,
  click: Handle<Sound>,
  voice: Handle<Sound>,
}

impl BusesPage {
  pub(crate) fn new() -> Self {
    Self {
      levels: VolumeKind::ALL.map(VolumeKind::default_value),
      pattern: true,
      frame: 0,
      sounds: None,
      _mixer: None,
    }
  }
}

impl DemoPage for BusesPage {
  fn name(&self) -> &str {
    "Buses"
  }

  fn description(&self) -> &str {
    "Every sound plays on a bus; the player's six volume settings set the standard buses"
  }

  fn settings(&mut self, s: &mut Settings) {
    for (label, level) in LABELS.iter().zip(&mut self.levels) {
      s.float(label, level, 0.0..=1.0, 0.05);
    }
    s.toggle("Pattern", &mut self.pattern);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Digit1, "Tick (sfx)"),
      Control::key(KeyCode::Digit2, "Click (ui)"),
      Control::key(KeyCode::Digit3, "Line (voice)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = load_mixer(ctx);
    self.sounds = (|| {
      Some(Sounds {
        theme: load(ctx, "music/theme.music.ron")?,
        wind: load(ctx, "sounds/wind.sound.ron")?,
        tick: load(ctx, "sounds/tick.sound.ron")?,
        click: load(ctx, "sounds/click.sound.ron")?,
        voice: load(ctx, "sounds/voice.sound.ron")?,
      })
    })();
    let mut audio = ctx.audio();
    for (kind, level) in VolumeKind::ALL.into_iter().zip(self.levels) {
      audio.set_volume_setting(kind, level);
    }
    if let Some(sounds) = &self.sounds {
      audio.play_music(&sounds.theme);
      // A loop: it stops, fading out, as the page closes
      audio.play(&sounds.wind);
    }
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    // The other pages start from the default mix
    let mut audio = ctx.audio();
    for kind in VolumeKind::ALL {
      audio.set_volume_setting(kind, kind.default_value());
    }
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    let mut audio = ctx.audio();
    for (kind, level) in VolumeKind::ALL.into_iter().zip(self.levels) {
      if audio.volume_setting(kind) != level {
        audio.set_volume_setting(kind, level);
      }
    }
    let Some(sounds) = &self.sounds else {
      return;
    };
    let frame = self.frame;
    let input = &*ctx.input;
    let tick = input.key_pressed(KeyCode::Digit1) || (self.pattern && frame % 30 == 10);
    let click = input.key_pressed(KeyCode::Digit2) || (self.pattern && frame % 45 == 25);
    let line = input.key_pressed(KeyCode::Digit3) || (self.pattern && frame % 240 == 60);
    if tick {
      ctx.audio().play(&sounds.tick);
    }
    if click {
      ctx.audio().play(&sounds.click);
    }
    if line {
      ctx.audio().play(&sounds.voice);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;

    let (bar_x, value_x, gain_x, heard_x, voices_x) =
      (x + 100.0, x + 260.0, x + 310.0, x + 400.0, x + 490.0);
    ui.draw_text("Bus", x, y, DIM);
    ui.draw_text("Setting", bar_x, y, DIM);
    ui.draw_text("Gain", gain_x, y, DIM);
    ui.draw_text("Heard at", heard_x, y, DIM);
    ui.draw_text("voices_on", voices_x, y, DIM);
    y += 28.0;

    let audio = ctx.audio();
    let master_db = volume_to_db(audio.volume_setting(VolumeKind::Master));
    for kind in VolumeKind::ALL {
      let setting = audio.volume_setting(kind);
      let gain = volume_to_db(setting);
      // A bus under master adds its gain to master's
      let heard = if kind == VolumeKind::Master {
        gain
      } else {
        (gain + master_db).max(-60.0)
      };
      let bus = kind.bus();
      let color = bus_color(bus);
      let indent = if kind == VolumeKind::Master {
        0.0
      } else {
        16.0
      };
      ui.draw_text(bus, x + indent, y, color);
      bar(ui, Rect::new(bar_x, y + 3.0, 150.0, 12.0), setting, color);
      ui.draw_text(&format!("{setting:.2}"), value_x, y, Color::WHITE);
      ui.draw_text(&db_text(gain), gain_x, y, Color::WHITE);
      bar(
        ui,
        Rect::new(heard_x, y + 5.0, 80.0, 8.0),
        db_fraction(heard),
        color,
      );
      let voices = audio.voices_on(bus);
      voice_dots(ui, voices_x, y, voices, 6, color);
      ui.draw_text(&voices.to_string(), voices_x + 84.0, y, Color::WHITE);
      y += 26.0;
    }

    y += 14.0;
    for line in [
      "A setting runs from 0 to 1 on a perceptual curve, dB = 40 log10(v):",
      "1 is the mix as designed, 0.5 about -12 dB, and 0 silence.",
      "A bus under master is heard at its gain plus master's, and master",
      "counts every voice. The music and the wind loop play throughout;",
      "the pattern plays a tick, a click and a line of dialogue.",
    ] {
      ui.draw_text(line, x, y, Color::LIGHT_GRAY);
      y += 20.0;
    }
  }
}
