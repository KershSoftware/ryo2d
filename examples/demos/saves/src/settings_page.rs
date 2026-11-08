//! The Settings page: the player's settings are live. A slider, a toggle or
//! the display mode changes the setting at once, and it is kept a second
//! after the last change. The demo's own section, `DemoOptions`, is kept in
//! the same file. Natively the text of `system.json` refreshes each second;
//! the web keeps the values in the browser's storage and shows them.

use std::path::PathBuf;

use demo_kit::prelude::*;

use crate::shared::{DIFFICULTIES, DIM, DemoOptions, HOT, save_dir};

const MODES: [&str; 3] = ["windowed", "exclusive", "borderless"];

/// The values the panel edits
#[derive(Clone, Copy, PartialEq)]
struct Values {
  master: f32,
  music: f32,
  sfx: f32,
  mode: usize,
  vsync: bool,
  fps: bool,
  subtitles: bool,
  difficulty: usize,
}

fn read(ctx: &mut SceneContext) -> Values {
  let settings = ctx.settings();
  let options = settings.game::<DemoOptions>();
  Values {
    master: settings.volume(VolumeKind::Master),
    music: settings.volume(VolumeKind::Music),
    sfx: settings.volume(VolumeKind::Sfx),
    mode: match settings.display_mode() {
      DisplayMode::Windowed => 0,
      DisplayMode::ExclusiveFullscreen => 1,
      DisplayMode::BorderlessFullscreen => 2,
    },
    vsync: settings.vsync(),
    fps: settings.show_fps(),
    subtitles: options.subtitles,
    difficulty: DIFFICULTIES
      .iter()
      .position(|d| *d == options.difficulty)
      .unwrap_or(1),
  }
}

fn write(ctx: &mut SceneContext, v: Values) {
  let mut settings = ctx.settings();
  settings.set_volume(VolumeKind::Master, v.master);
  settings.set_volume(VolumeKind::Music, v.music);
  settings.set_volume(VolumeKind::Sfx, v.sfx);
  settings.set_display_mode(match v.mode {
    0 => DisplayMode::Windowed,
    1 => DisplayMode::ExclusiveFullscreen,
    _ => DisplayMode::BorderlessFullscreen,
  });
  settings.set_vsync(v.vsync);
  settings.set_show_fps(v.fps);
  let options = settings.game_mut::<DemoOptions>();
  options.subtitles = v.subtitles;
  options.difficulty = DIFFICULTIES[v.difficulty].to_string();
}

pub(crate) struct SettingsPage {
  values: Option<Values>,
  /// What the panel showed last frame: a difference from `values` is the
  /// player's edit
  seen: Option<Values>,
  dir: Option<PathBuf>,
  frame: u32,
  file: Vec<String>,
}

impl SettingsPage {
  pub(crate) fn new() -> Self {
    Self {
      values: None,
      seen: None,
      dir: None,
      frame: 0,
      file: Vec::new(),
    }
  }

  fn look(&mut self) {
    let Some(dir) = &self.dir else {
      return;
    };
    self.file = match std::fs::read_to_string(dir.join("system.json")) {
      Ok(text) => text.lines().take(18).map(str::to_string).collect(),
      Err(_) => vec!["(no system.json yet: nothing the player changed)".to_string()],
    };
  }
}

impl DemoPage for SettingsPage {
  fn name(&self) -> &str {
    "Settings"
  }

  fn description(&self) -> &str {
    "ctx.settings() is live and saves itself a second after the last change"
  }

  fn settings(&mut self, s: &mut Settings) {
    let Some(v) = &mut self.values else {
      return;
    };
    s.float("Master", &mut v.master, 0.0..=1.0, 0.05);
    s.float("Music", &mut v.music, 0.0..=1.0, 0.05);
    s.float("Sfx", &mut v.sfx, 0.0..=1.0, 0.05);
    s.choice("Display", &mut v.mode, &MODES);
    s.toggle("Vsync", &mut v.vsync);
    s.toggle("Show FPS", &mut v.fps);
    s.toggle("Subtitles", &mut v.subtitles);
    s.choice("Difficulty", &mut v.difficulty, &DIFFICULTIES);
  }

  fn enter(&mut self, ctx: &mut SceneContext) {
    let now = read(ctx);
    self.values = Some(now);
    self.seen = Some(now);
    self.dir = save_dir();
    self.look();
  }

  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) {
    self.frame += 1;
    let (Some(values), Some(seen)) = (&mut self.values, &mut self.seen) else {
      return;
    };
    // An edit in the panel goes to the settings; otherwise a change made
    // elsewhere (a fullscreen key) shows in the panel
    if *values != *seen {
      write(ctx, *values);
      *seen = *values;
    } else {
      let now = read(ctx);
      *values = now;
      *seen = now;
    }
    if self.frame.is_multiple_of(60) {
      self.look();
    }
  }

  fn ui(&mut self, ui: &mut UiContext, ctx: &mut SceneContext) {
    let area = content_area(ui);
    let x = area.x + 16.0;
    let mut y = area.y + 10.0;
    if self.dir.is_some() {
      ui.draw_text("system.json, as the player's file holds it now", x, y, DIM);
      y += 24.0;
      for line in &self.file {
        ui.draw_text(line, x + 8.0, y, Color::WHITE);
        y += 19.0;
      }
    } else {
      ui.draw_text("Kept in the browser's storage:", x, y, DIM);
      y += 24.0;
      let v = read(ctx);
      let lines = [
        format!(
          "master volume {:.2}, music {:.2}, sfx {:.2}",
          v.master, v.music, v.sfx
        ),
        format!(
          "display {}, vsync {}, show fps {}",
          MODES[v.mode], v.vsync, v.fps
        ),
        format!(
          "demo: subtitles {}, difficulty {}",
          v.subtitles, DIFFICULTIES[v.difficulty]
        ),
      ];
      for line in lines {
        ui.draw_text(&line, x + 8.0, y, Color::WHITE);
        y += 22.0;
      }
    }
    y += 12.0;
    ui.draw_text(
      "Only values that differ from the game's defaults are kept.",
      x,
      y,
      HOT,
    );
  }
}
