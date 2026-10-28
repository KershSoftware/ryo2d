//! The Variations page: one footstep sound with four takes. Each play picks
//! a take by weight, avoiding its latest picks (`no_repeat`), and draws its
//! gain and pitch from their ranges. The panel's changes play a copy of the
//! file's definition made in code; with none, the file itself plays, so a
//! save of it is heard

use std::collections::VecDeque;

use demo_kit::prelude::*;
use ryo2d::core::audio::{AudioEngine, AudioLogKind};

use crate::shared::{BAD, DIM, bar, bus_meters, load, steady_start};

const FOOTSTEP: &str = "sounds/footstep.sound.ron";
/// How many picks the strip shows
const HISTORY: usize = 24;
/// Each take's colour, in the definition's order
const TAKE_COLORS: [Color; 4] = [
  Color::rgb(255, 160, 70),
  Color::rgb(110, 210, 120),
  Color::rgb(90, 170, 255),
  Color::rgb(230, 110, 200),
];

pub(crate) struct VariationsPage {
  /// The definition as its file says
  file: Option<Handle<Sound>>,
  /// The file's definition with the panel's changes, made in code
  changed: Option<Handle<Sound>>,
  /// The `no_repeat` and pitch range `changed` was made with
  made_with: (i32, f32),
  /// Each take's clip path and weight
  takes: Vec<(String, f32)>,
  no_repeat: i32,
  pitch_range: f32,
  steps_per_second: i32,
  walking: bool,
  frame: u32,
  /// Fixed steps since the last step played
  since_step: u32,
  /// The latest picks, oldest first, by take
  picks: VecDeque<usize>,
  counts: [u32; 4],
  /// Times a take was picked twice in a row
  repeats: u32,
  /// The take the last step picked
  last: Option<usize>,
  /// The last step's gain in dB and pitch in semitones, as drawn
  drawn: Option<(f32, f32)>,
  /// The definition's gain and its range, in dB
  volume: (f32, f32),
  _mixer: Option<Handle<Mixer>>,
}

impl VariationsPage {
  pub(crate) fn new() -> Self {
    Self {
      file: None,
      changed: None,
      made_with: (0, 0.0),
      takes: Vec::new(),
      no_repeat: 1,
      pitch_range: 1.0,
      steps_per_second: 4,
      walking: true,
      frame: 0,
      since_step: 0,
      picks: VecDeque::new(),
      counts: [0; 4],
      repeats: 0,
      last: None,
      drawn: None,
      volume: (0.0, 0.0),
      _mixer: None,
    }
  }

  /// The definition a step plays: the file's, or a copy with the panel's
  /// changes (made again when they change)
  fn definition(&mut self, ctx: &mut SceneContext) -> Option<Handle<Sound>> {
    let file = self.file.clone()?;
    let original = ctx.assets().get(&file).cloned()?;
    let wanted = (self.no_repeat, self.pitch_range);
    if wanted == (i32::from(original.no_repeat), original.pitch_range) {
      self.changed = None;
      return Some(file);
    }
    if self.changed.is_none() || self.made_with != wanted {
      let sound = Sound {
        no_repeat: u8::try_from(self.no_repeat).unwrap_or(0),
        pitch_range: self.pitch_range,
        ..original
      };
      self.changed = Some(ctx.assets().add(sound));
      self.made_with = wanted;
    }
    self.changed.clone()
  }

  /// Play a step and record the take it picked. The engine says which clip
  /// a voice plays only in its audio log (a debugging hook, turned on as
  /// the page opens)
  fn step(&mut self, ctx: &mut SceneContext) {
    let Some(sound) = self.definition(ctx) else {
      return;
    };
    let voice = ctx.audio().play(&sound);
    let Some(engine) = ctx.world.get_resource::<AudioEngine>() else {
      return;
    };
    let Some(start) = engine
      .log()
      .iter()
      .rev()
      .find(|entry| entry.kind == AudioLogKind::Start && entry.voice == voice.id())
    else {
      return;
    };
    let Some(take) = self
      .takes
      .iter()
      .position(|(path, _)| *path == start.source)
    else {
      return;
    };
    self.drawn = Some((start.gain_db, 12.0 * start.rate.log2() as f32));
    if self.last == Some(take) {
      self.repeats += 1;
    }
    self.last = Some(take);
    self.counts[take] += 1;
    self.picks.push_back(take);
    if self.picks.len() > HISTORY {
      self.picks.pop_front();
    }
  }
}

impl DemoPage for VariationsPage {
  fn name(&self) -> &str {
    "Variations"
  }

  fn description(&self) -> &str {
    "Each play picks a take by weight, avoiding the latest picks, and varies its gain and pitch"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("No repeat", &mut self.no_repeat, 0..=3);
    s.float("Pitch range", &mut self.pitch_range, 0.0..=4.0, 0.5);
    s.int("Steps a second", &mut self.steps_per_second, 1..=8);
    s.toggle("Walking", &mut self.walking);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "One step")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = steady_start(ctx);
    // Left on as the page closes: the engine cannot say whether it was on
    // before (a headless test turns it on), and it keeps at most 4096 entries
    if let Some(mut engine) = ctx.world.get_resource_mut::<AudioEngine>() {
      engine.set_logging(true);
    }
    self.file = load(ctx, FOOTSTEP);
    let assets = ctx.assets();
    let Some(sound) = self.file.as_ref().and_then(|file| assets.get(file)) else {
      return;
    };
    self.no_repeat = i32::from(sound.no_repeat);
    self.pitch_range = sound.pitch_range;
    self.volume = (sound.volume_db, sound.volume_range_db);
    self.takes = sound
      .clips
      .iter()
      .map(|take| {
        let path = assets
          .server()
          .path(take.clip.id())
          .map_or_else(String::new, |path| path.to_string());
        (path, take.weight)
      })
      .collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    self.since_step += 1;
    let every = 60 / self.steps_per_second.max(1) as u32;
    let walk = self.walking && self.since_step >= every;
    if walk || ctx.input.key_pressed(KeyCode::Space) {
      self.since_step = 0;
      self.step(ctx);
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;

    let weights: Vec<String> = self.takes.iter().map(|(_, w)| format!("{w}")).collect();
    ui.draw_text(
      &format!(
        "{FOOTSTEP}: {} takes, weights {}",
        self.takes.len(),
        weights.join(", ")
      ),
      x,
      y,
      Color::WHITE,
    );
    y += 22.0;
    let playing = if self.changed.is_some() {
      "playing a copy made in code with the panel's no_repeat and pitch range"
    } else {
      "playing the file as it is"
    };
    ui.draw_text(playing, x, y, DIM);
    y += 34.0;

    // The latest picks, newest on the right
    ui.draw_text("The latest picks", x, y, DIM);
    y += 22.0;
    let cell = 22.0;
    for (i, &take) in self.picks.iter().enumerate() {
      let cx = x + i as f32 * (cell + 2.0);
      ui.draw_rect(Rect::new(cx, y, cell, cell), TAKE_COLORS[take]);
      let letter = ["A", "B", "C", "D"][take];
      ui.draw_text(letter, cx + 6.0, y + 3.0, Color::BLACK);
      let repeat = i > 0 && self.picks[i - 1] == take;
      if repeat {
        ui.draw_rect(Rect::new(cx, y + cell + 2.0, cell, 3.0), BAD);
      }
    }
    for i in self.picks.len()..HISTORY {
      let cx = x + i as f32 * (cell + 2.0);
      ui.draw_rect(Rect::new(cx, y, cell, cell), Color::new(255, 255, 255, 18));
    }
    y += cell + 18.0;
    let repeats = format!("Picked twice in a row: {}", self.repeats);
    let color = if self.repeats > 0 { BAD } else { DIM };
    ui.draw_text(&repeats, x, y, color);
    y += 22.0;
    if let Some((gain, pitch)) = self.drawn {
      let (volume, range) = self.volume;
      ui.draw_text(
        &format!(
          "The last step drew {gain:+.1} dB (of {volume:+.1} \u{b1} {range:.1}) and pitch {pitch:+.2} (of 0 \u{b1} {:.1})",
          self.pitch_range
        ),
        x,
        y,
        Color::LIGHT_GRAY,
      );
    }
    y += 36.0;

    // How often each take came up, against its share of the weights
    ui.draw_text("Take", x, y, DIM);
    ui.draw_text("Picks", x + 170.0, y, DIM);
    ui.draw_text("Weight share", x + 420.0, y, DIM);
    y += 24.0;
    let total: u32 = self.counts.iter().sum();
    let weight_total: f32 = self.takes.iter().map(|(_, w)| w).sum();
    for (take, (path, weight)) in self.takes.iter().enumerate().take(4) {
      let name = path.rsplit('/').next().unwrap_or(path);
      let letter = ["A", "B", "C", "D"][take];
      ui.draw_text(&format!("{letter}  {name}"), x, y, TAKE_COLORS[take]);
      let share = if total > 0 {
        self.counts[take] as f32 / total as f32
      } else {
        0.0
      };
      bar(
        ui,
        Rect::new(x + 170.0, y + 3.0, 160.0, 12.0),
        share,
        TAKE_COLORS[take],
      );
      ui.draw_text(&self.counts[take].to_string(), x + 340.0, y, Color::WHITE);
      let expected = if weight_total > 0.0 {
        weight / weight_total
      } else {
        0.0
      };
      ui.draw_text(
        &format!("{:.0}%", expected * 100.0),
        x + 420.0,
        y,
        Color::WHITE,
      );
      y += 24.0;
    }

    y += 20.0;
    bus_meters(ui, ctx, x, y, &["sfx"]);
  }
}
