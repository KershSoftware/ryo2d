//! The Voice limits page: one row per limit rule, each a bell played five
//! times a second, past its limit of three voices. Each row plays on a bus
//! of its own (the mixer's), so `voices_on` counts it. The quietest row
//! plays each bell at another volume, the farthest row at another distance
//! from the listener, and the priority row rotates three bells of different
//! priorities, which share their bus's limit

use std::collections::VecDeque;

use demo_kit::prelude::*;

use crate::shared::{BAD, DIM, bus_color, load, steady_start, voice_dots};

/// The voices each row's limit allows
pub const MAX_VOICES: usize = 3;

/// Each row's bus, in the page's order
pub const ROW_BUSES: [&str; 5] = [
  "sfx/prevent",
  "sfx/oldest",
  "sfx/quietest",
  "sfx/farthest",
  "sfx/priority",
];

/// The plays a row shows
const SHOWN: usize = 8;
/// The quietest row's volumes, in dB, in turn
const VOLUMES: [f32; 7] = [0.0, -18.0, -6.0, -12.0, -3.0, -24.0, -9.0];
/// The farthest row's distances from the listener, in turn (alternate ones
/// to its left)
const DISTANCES: [f32; 7] = [60.0, 480.0, 180.0, 360.0, 30.0, 270.0, 420.0];
/// The priority row's bells, in turn: indexes into its sounds (low, mid,
/// high)
const PRIORITY_TURNS: [usize; 7] = [1, 0, 2, 0, 1, 0, 2];
/// The first frame a row plays on: the mixer's buses are built at the end
/// of the first
const FIRST_PLAY: u32 = 10;

/// A row's rule
#[derive(Clone, Copy, PartialEq)]
enum Rule {
  PreventNew,
  StopOldest,
  StopQuietest,
  StopFarthest,
  StopLowestPriority,
}

impl Rule {
  const ALL: [Rule; 5] = [
    Rule::PreventNew,
    Rule::StopOldest,
    Rule::StopQuietest,
    Rule::StopFarthest,
    Rule::StopLowestPriority,
  ];

  fn name(self) -> &'static str {
    match self {
      Rule::PreventNew => "PreventNew",
      Rule::StopOldest => "StopOldest",
      Rule::StopQuietest => "StopQuietest",
      Rule::StopFarthest => "StopFarthest",
      Rule::StopLowestPriority => "StopLowestPriority",
    }
  }

  fn says(self) -> &'static str {
    match self {
      Rule::PreventNew => "a fourth play is refused",
      Rule::StopOldest => "a fourth play stops the oldest",
      Rule::StopQuietest => "a fourth play stops the quietest",
      Rule::StopFarthest => "a fourth play stops the farthest",
      Rule::StopLowestPriority => "a fourth stops the least important",
    }
  }

  fn files(self) -> &'static [&'static str] {
    match self {
      Rule::PreventNew => &["limits/prevent.sound.ron"],
      Rule::StopOldest => &["limits/oldest.sound.ron"],
      Rule::StopQuietest => &["limits/quietest.sound.ron"],
      Rule::StopFarthest => &["limits/farthest.sound.ron"],
      Rule::StopLowestPriority => &[
        "limits/priority-low.sound.ron",
        "limits/priority-mid.sound.ron",
        "limits/priority-high.sound.ron",
      ],
    }
  }
}

/// One play of a row's
struct Play {
  voice: Voice,
  /// What sets it apart: its volume, distance or priority
  label: String,
  /// The frame it played in
  started: u32,
  /// The frame it was first seen not playing
  ended: Option<u32>,
  /// A limit refused it
  refused: bool,
  /// How many frames its clip lasts at its pitch
  frames: u32,
}

/// What became of a play, as its cell shows it
#[derive(Clone, Copy)]
enum State {
  Playing,
  /// Stopped by its row's rule, before its end
  Stopped,
  Refused,
  /// Played to its end
  Ended,
}

impl State {
  /// The cell's fill and its text's colour
  fn colors(self) -> (Color, Color) {
    match self {
      State::Playing => (Color::rgb(110, 210, 120), Color::BLACK),
      State::Stopped => (
        Color::rgb(255, 170, 90).with_alpha(80),
        Color::rgb(255, 190, 120),
      ),
      State::Refused => (BAD.with_alpha(70), BAD),
      State::Ended => (Color::new(255, 255, 255, 22), DIM),
    }
  }
}

impl Play {
  fn state(&self) -> State {
    if self.refused {
      State::Refused
    } else if self.voice.is_playing() {
      State::Playing
    } else if self
      .ended
      .is_some_and(|ended| ended + 3 < self.started + self.frames)
    {
      State::Stopped
    } else {
      State::Ended
    }
  }
}

struct Row {
  rule: Rule,
  bus: &'static str,
  sounds: Vec<Handle<Sound>>,
  plays: VecDeque<Play>,
  turn: usize,
}

pub(crate) struct LimitsPage {
  rows: Vec<Row>,
  plays_per_second: i32,
  playing: bool,
  frame: u32,
  since_play: u32,
  _mixer: Option<Handle<Mixer>>,
}

impl LimitsPage {
  pub(crate) fn new() -> Self {
    Self {
      rows: Vec::new(),
      plays_per_second: 5,
      playing: true,
      frame: 0,
      since_play: 0,
      _mixer: None,
    }
  }

  /// Play each row's next bell
  fn play_all(&mut self, ctx: &mut SceneContext) {
    let frame = self.frame;
    for row in &mut self.rows {
      let turn = row.turn;
      row.turn += 1;
      let at = |list: &[f32]| list[turn % list.len()];
      let sound = match row.rule {
        Rule::StopLowestPriority => &row.sounds[PRIORITY_TURNS[turn % PRIORITY_TURNS.len()]],
        _ => &row.sounds[0],
      };
      let (voice, label) = match row.rule {
        Rule::StopQuietest => {
          let volume_db = at(&VOLUMES);
          let options = PlayOptions {
            volume_db,
            ..Default::default()
          };
          (
            ctx.audio().play_with(sound, options),
            format!("{volume_db}"),
          )
        }
        Rule::StopFarthest => {
          let distance = at(&DISTANCES);
          let side = if turn % 2 == 0 { 1.0 } else { -1.0 };
          let voice = ctx.audio().play_at(sound, Vec2::new(side * distance, 0.0));
          (voice, format!("{distance}"))
        }
        Rule::StopLowestPriority => {
          let priority = ctx.assets().get(sound).map_or(0, |sound| sound.priority);
          (ctx.audio().play(sound), format!("p{priority}"))
        }
        Rule::PreventNew | Rule::StopOldest => (ctx.audio().play(sound), format!("#{}", turn + 1)),
      };
      let frames = clip_frames(ctx, sound);
      let refused = voice.is_stopped();
      row.plays.push_back(Play {
        voice,
        label,
        started: frame,
        ended: refused.then_some(frame),
        refused,
        frames,
      });
      if row.plays.len() > SHOWN {
        row.plays.pop_front();
      }
    }
  }
}

/// How many fixed steps `sound`'s first clip lasts at its pitch
fn clip_frames(ctx: &mut SceneContext, sound: &Handle<Sound>) -> u32 {
  let assets = ctx.assets();
  let Some(sound) = assets.get(sound) else {
    return 0;
  };
  let seconds = sound
    .clips
    .first()
    .and_then(|take| assets.get(&take.clip))
    .map_or(0.0, |clip| clip.duration().as_secs_f32());
  (seconds / 2f32.powf(sound.pitch / 12.0) * 60.0) as u32
}

impl DemoPage for LimitsPage {
  fn name(&self) -> &str {
    "Voice limits"
  }

  fn description(&self) -> &str {
    "A sound's limit caps its voices; its rule picks what a play past the limit does"
  }

  fn settings(&mut self, s: &mut Settings) {
    s.int("Plays a second", &mut self.plays_per_second, 1..=10);
    s.toggle("Playing", &mut self.playing);
  }

  fn controls(&self) -> Vec<Control> {
    vec![Control::key(KeyCode::Space, "Play each row once")]
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    self._mixer = steady_start(ctx);
    // The farthest row is heard from here
    ctx
      .world
      .spawn((AudioListener, Transform::from_xy(0.0, 0.0)));
    self.rows = Rule::ALL
      .into_iter()
      .zip(ROW_BUSES)
      .map(|(rule, bus)| Row {
        rule,
        bus,
        sounds: rule
          .files()
          .iter()
          .filter_map(|file| load(ctx, file))
          .collect(),
        plays: VecDeque::new(),
        turn: 0,
      })
      .filter(|row| row.sounds.len() == row.rule.files().len())
      .collect();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    self.since_play += 1;
    let every = 60 / self.plays_per_second.max(1) as u32;
    let due = self.playing && self.frame >= FIRST_PLAY && self.since_play >= every;
    if due || ctx.input.key_pressed(KeyCode::Space) {
      self.since_play = 0;
      self.play_all(ctx);
    }
    for row in &mut self.rows {
      for play in &mut row.plays {
        if play.ended.is_none() && !play.voice.is_playing() {
          play.ended = Some(self.frame);
        }
      }
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    let cells_x = x + 220.0;
    let (cell_w, cell_h) = (46.0, 26.0);
    ui.draw_text(
      "Each bell rings for about a second, so five a second pass the limit of three.",
      x,
      y,
      Color::LIGHT_GRAY,
    );
    y += 24.0;
    // What a play's cell shows
    let mut key_x = cells_x;
    for (state, label) in [
      (State::Playing, "playing"),
      (State::Stopped, "stopped by the rule"),
      (State::Refused, "refused"),
      (State::Ended, "ended"),
    ] {
      let (fill, _) = state.colors();
      ui.draw_rect(Rect::new(key_x, y + 3.0, 12.0, 12.0), fill);
      ui.draw_text(label, key_x + 18.0, y, Color::LIGHT_GRAY);
      key_x += 30.0 + ui.text_size(label).0 as f32;
    }
    y += 30.0;

    for row in &self.rows {
      let color = bus_color(row.bus);
      let voices = ctx.audio().voices_on(row.bus);
      ui.draw_text(row.rule.name(), x, y, color);
      voice_dots(ui, x + 150.0, y, voices, MAX_VOICES, color);
      ui.draw_text(row.rule.says(), x, y + 20.0, DIM);
      ui.draw_text(
        &format!("voices_on(\"{}\") = {voices}", row.bus),
        x,
        y + 40.0,
        DIM,
      );

      for (i, play) in row.plays.iter().enumerate() {
        let cell = Rect::new(cells_x + i as f32 * (cell_w + 4.0), y + 4.0, cell_w, cell_h);
        let (fill, text) = play.state().colors();
        ui.draw_rect(cell, fill);
        let label = if play.refused { "no" } else { &play.label };
        ui.draw_text(label, cell.x + 5.0, cell.y + 5.0, text);
      }
      y += 76.0;
    }

    let sfx = ctx.audio().voices_on("sfx");
    let max = self.rows.len() * MAX_VOICES;
    ui.draw_text(
      &format!("voices_on(\"sfx\") = {sfx}, at most {max}: sfx counts the buses under it"),
      x,
      y,
      Color::LIGHT_GRAY,
    );
  }
}
