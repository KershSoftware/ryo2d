//! The Mix page: what the game's clock and the mixer do to the buses.
//! Pausing holds the voices on the buses that pause with the game (sfx,
//! ambience and voice) where they are, while music and ui play on; a time
//! scale plays sfx and ambience faster or slower; a snapshot blends the
//! mix to one of the mixer's presets; and a line of dialogue ducks the
//! music. The page's own updates follow the game clock (its ticks slow with
//! the time scale and stop while paused), so the panel applies in its UI

use demo_kit::prelude::*;

use crate::shared::{DIM, HOT, bus_color, bus_meters, load, load_mixer};

const SNAPSHOTS: [&str; 3] = ["none", "muffled", "quiet"];
/// The buses the page shows, and whether each pauses with the game and
/// follows the time scale by default: what a mixer that leaves them says
const BUSES: [(&str, bool, bool); 5] = [
  ("music", false, false),
  ("sfx", true, true),
  ("ui", false, false),
  ("voice", true, false),
  ("ambience", true, true),
];

pub(crate) struct MixPage {
  paused: bool,
  time_scale: f32,
  snapshot: usize,
  /// The snapshot last applied
  applied: usize,
  frame: u32,
  sounds: Option<Sounds>,
  mixer: Option<Handle<Mixer>>,
}

struct Sounds {
  theme: Handle<Music>,
  wind: Handle<Sound>,
  tick: Handle<Sound>,
  click: Handle<Sound>,
  voice: Handle<Sound>,
}

impl MixPage {
  pub(crate) fn new() -> Self {
    Self {
      paused: false,
      time_scale: 1.0,
      snapshot: 0,
      applied: 0,
      frame: 0,
      sounds: None,
      mixer: None,
    }
  }
}

impl DemoPage for MixPage {
  fn name(&self) -> &str {
    "The mix"
  }

  fn description(&self) -> &str {
    "Pause and time scale hold or speed buses; a snapshot blends the mix; a voice ducks the music"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.toggle("Game paused", &mut self.paused);
    s.float("Time scale", &mut self.time_scale, 0.0..=2.0, 0.25);
    s.choice("Snapshot", &mut self.snapshot, &SNAPSHOTS);
  }

  fn controls(&self) -> Vec<Control> {
    vec![
      Control::key(KeyCode::Space, "A line of dialogue"),
      Control::key(KeyCode::Digit1, "Click (ui)"),
    ]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self.mixer = load_mixer(ctx);
    self.sounds = (|| {
      Some(Sounds {
        theme: load(ctx, "music/theme.music.ron")?,
        wind: load(ctx, "sounds/wind.sound.ron")?,
        tick: load(ctx, "sounds/tick.sound.ron")?,
        click: load(ctx, "sounds/click.sound.ron")?,
        voice: load(ctx, "sounds/voice.sound.ron")?,
      })
    })();
    if let Some(sounds) = &self.sounds {
      let mut audio = ctx.audio();
      audio.play_music(&sounds.theme);
      audio.play(&sounds.wind);
    }
  }

  fn exit(&mut self, ctx: &mut SceneContext) {
    // The page's cleanup resets the clock, not the mix
    ctx.audio().clear_snapshot();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    let Some(sounds) = &self.sounds else {
      return;
    };
    let frame = self.frame;
    let mut audio = ctx.audio();
    if frame % 20 == 5 {
      audio.play(&sounds.tick);
    }
    if frame % 300 == 60 {
      audio.play(&sounds.voice);
    }
    // Keys that should work while paused are read in `ui`
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    // The panel applies here: `update` does not run while the game is paused
    let game = &mut ctx.time.game;
    if self.paused != game.is_paused() {
      if self.paused {
        game.pause();
      } else {
        game.resume();
      }
    }
    if game.speed() != self.time_scale {
      game.set_speed(self.time_scale);
    }
    if self.snapshot != self.applied {
      self.applied = self.snapshot;
      match SNAPSHOTS[self.snapshot] {
        "none" => ctx.audio().clear_snapshot(),
        name => ctx.audio().snapshot(name),
      }
    }
    if let Some(sounds) = &self.sounds {
      if ui.input().key_pressed(KeyCode::Space) {
        ctx.audio().play(&sounds.voice);
      }
      if ui.input().key_pressed(KeyCode::Digit1) {
        ctx.audio().play(&sounds.click);
      }
    }

    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    let clock = if self.paused { "paused" } else { "running" };
    ui.draw_text(
      &format!("Game clock: {clock}, time scale {:.2}", self.time_scale),
      x,
      y,
      Color::WHITE,
    );
    y += 30.0;

    let (pause_x, scale_x, state_x) = (x + 90.0, x + 200.0, x + 310.0);
    ui.draw_text("Bus", x, y, DIM);
    ui.draw_text("Pauses", pause_x, y, DIM);
    ui.draw_text("Time scale", scale_x, y, DIM);
    ui.draw_text("Now", state_x, y, DIM);
    y += 24.0;
    let still = self.paused || self.time_scale <= 0.01;
    // What the mixer says of each bus, or the standard bus's default
    let buses = {
      let assets = ctx.assets();
      let mixer = self.mixer.as_ref().and_then(|mixer| assets.get(mixer));
      BUSES.map(|(bus, pauses, follows)| {
        let def = mixer.and_then(|mixer| mixer.buses.iter().find(|def| def.name == bus));
        (
          bus,
          def.and_then(|def| def.pauses_with_game).unwrap_or(pauses),
          def
            .and_then(|def| def.follows_time_scale)
            .unwrap_or(follows),
        )
      })
    };
    for (bus, pauses, follows) in buses {
      let held = (pauses && self.paused) || (follows && still);
      let state = if held {
        "held".to_string()
      } else if follows && self.time_scale != 1.0 {
        format!("{:.2}x rate", self.time_scale.min(8.0))
      } else {
        "plays".to_string()
      };
      ui.draw_text(bus, x, y, bus_color(bus));
      ui.draw_text(if pauses { "yes" } else { "no" }, pause_x, y, Color::WHITE);
      ui.draw_text(
        if follows { "follows" } else { "no" },
        scale_x,
        y,
        Color::WHITE,
      );
      let color = if held { HOT } else { Color::WHITE };
      ui.draw_text(&state, state_x, y, color);
      y += 22.0;
    }
    y += 16.0;

    let assets = ctx.assets();
    let mixer = self.mixer.as_ref().and_then(|mixer| assets.get(mixer));
    let active = SNAPSHOTS[self.snapshot];
    let snapshot = mixer.and_then(|mixer| mixer.snapshots.iter().find(|s| s.name == active));
    let line = match snapshot {
      Some(snapshot) => {
        let buses: Vec<String> = snapshot
          .buses
          .iter()
          .map(|(bus, change)| {
            let mut text = bus.clone();
            if change.volume_db != 0.0 {
              text += &format!(" {:+.0} dB", change.volume_db);
            }
            if let Some(cutoff) = change.low_pass {
              text += &format!(" low-pass {cutoff:.0} Hz");
            }
            text
          })
          .collect();
        format!(
          "Snapshot \"{active}\" (blends over {} s): {}",
          snapshot.blend,
          buses.join(", ")
        )
      }
      None => "No snapshot: the mix as the mixer and the settings make it".to_string(),
    };
    ui.text(line)
      .at(x, y)
      .wrap(560.0)
      .color(if snapshot.is_some() {
        HOT
      } else {
        Color::LIGHT_GRAY
      })
      .draw();
    y += 48.0;

    let duck = mixer.and_then(|mixer| mixer.ducks.first()).cloned();
    drop(assets);
    if let Some(duck) = duck {
      let ducking = ctx.audio().voices_on(&duck.trigger) > 0;
      let rule = format!(
        "Duck: while {} plays, {} is {:+.0} dB (in over {} s, back over {} s)",
        duck.trigger, duck.target, duck.by_db, duck.attack, duck.release
      );
      ui.draw_text(&rule, x, y, Color::LIGHT_GRAY);
      y += 22.0;
      let (now, color) = if ducking {
        ("ducking the music now", HOT)
      } else {
        ("not ducking", DIM)
      };
      ui.draw_text(now, x, y, color);
      y += 34.0;
    }

    bus_meters(ui, ctx, x, y, &["music", "sfx", "ui", "voice", "ambience"]);
  }
}
