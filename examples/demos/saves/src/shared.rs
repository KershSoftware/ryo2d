//! What the pages share: the saved types, the game's settings section, the
//! colours, and the small drawing and formatting helpers

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use demo_kit::prelude::*;
use serde::{Deserialize, Serialize};

/// Labels and headers
pub(crate) const DIM: Color = Color::rgb(150, 150, 165);
/// A value that matters now
pub(crate) const HOT: Color = Color::XP_GOLD;
/// Something refused, failed or damaged
pub(crate) const BAD: Color = Color::rgb(255, 110, 100);
/// Something that worked
pub(crate) const GOOD: Color = Color::rgb(120, 220, 130);

/// The three slots of the Slots page
pub const SLOTS: [&str; 3] = ["slot-a", "slot-b", "slot-c"];
/// The slot the Upgrades page seeds with a version 1 save
pub const OLD_SLOT: &str = "hero-old";

/// A hero as the first build saved it
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroV1 {
  pub name: String,
  pub hp: u32,
  pub gold: u32,
}

/// The second build renamed `hp` and added a class
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroV2 {
  pub name: String,
  pub max_hp: u32,
  pub gold: u32,
  pub class: String,
}

/// The current build: stats on top
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeroV3 {
  pub name: String,
  pub max_hp: u32,
  pub gold: u32,
  pub class: String,
  pub strength: u32,
  pub agility: u32,
  pub wits: u32,
}

impl HeroV3 {
  /// One line with every field
  pub(crate) fn summary(&self) -> String {
    format!(
      "{} the {}: {} hp, {} gold, str {} agi {} wits {}",
      self.name, self.class, self.max_hp, self.gold, self.strength, self.agility, self.wits
    )
  }
}

impl HeroV1 {
  /// What an older build left in `hero-old`
  pub fn rowan() -> Self {
    HeroV1 {
      name: "Rowan".to_string(),
      hp: 30,
      gold: 120,
    }
  }
}

pub fn v1_to_v2(old: HeroV1) -> HeroV2 {
  HeroV2 {
    name: old.name,
    max_hp: old.hp,
    gold: old.gold,
    class: "Wanderer".to_string(),
  }
}

pub fn v2_to_v3(old: HeroV2) -> HeroV3 {
  HeroV3 {
    name: old.name,
    max_hp: old.max_hp,
    gold: old.gold,
    class: old.class,
    strength: 5,
    agility: 5,
    wits: 5,
  }
}

/// The demo's own settings section, kept in `system.json` under `demo`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DemoOptions {
  pub subtitles: bool,
  pub difficulty: String,
}

impl Default for DemoOptions {
  fn default() -> Self {
    DemoOptions {
      subtitles: false,
      difficulty: "normal".to_string(),
    }
  }
}

pub(crate) const DIFFICULTIES: [&str; 3] = ["easy", "normal", "hard"];

const NAMES: [&str; 6] = ["Ash", "Briar", "Cinder", "Dune", "Ember", "Flint"];
const CLASSES: [&str; 4] = ["Knight", "Ranger", "Mage", "Rogue"];

/// A hero with random stats; each call differs from the last
pub(crate) fn random_hero() -> HeroV3 {
  static SEED: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
  let mut x = SEED.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
  let mut next = |range: u64| {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x % range
  };
  HeroV3 {
    name: NAMES[next(NAMES.len() as u64) as usize].to_string(),
    max_hp: 20 + next(41) as u32,
    gold: next(500) as u32,
    class: CLASSES[next(CLASSES.len() as u64) as usize].to_string(),
    strength: 1 + next(10) as u32,
    agility: 1 + next(10) as u32,
    wits: 1 + next(10) as u32,
  }
}

/// The save folder, where the engine keeps `*.ryo-save` and `system.json`.
/// The web has none: its files are in the browser's storage
pub(crate) fn save_dir() -> Option<PathBuf> {
  if cfg!(target_arch = "wasm32") {
    return None;
  }
  Workspace::discover().ok().map(|w| w.save_location())
}

/// Seconds since the epoch as a time of day in UTC
pub(crate) fn time_of_day(secs: u64) -> String {
  let day = secs % 86_400;
  format!(
    "{:02}:{:02}:{:02} UTC",
    day / 3600,
    day % 3600 / 60,
    day % 60
  )
}

/// A play time in seconds as `m:ss`
pub(crate) fn clock(secs: u64) -> String {
  format!("{}:{:02}", secs / 60, secs % 60)
}

/// The last lines of what happened, newest at the bottom
#[derive(Default)]
pub(crate) struct Log(Vec<(String, Color)>);

impl Log {
  pub(crate) fn push(&mut self, line: impl Into<String>, color: Color) {
    self.0.push((line.into(), color));
    if self.0.len() > 5 {
      self.0.remove(0);
    }
  }

  pub(crate) fn draw(&self, ui: &mut UiContext, x: f32, y: f32) {
    ui.draw_text("Log", x, y, DIM);
    for (i, (line, color)) in self.0.iter().enumerate() {
      ui.draw_text(line, x, y + 22.0 * (i as f32 + 1.0), *color);
    }
  }
}

/// The report of each finished save or delete, as a log line
pub(crate) fn finished_line(done: &SaveFinished) -> (String, Color) {
  let what = match done.op {
    SaveOp::Save => "save",
    SaveOp::Delete => "delete",
  };
  match &done.result {
    Ok(()) => (format!("{what} {}: finished", done.slot), GOOD),
    Err(error) => (format!("{what} {}: {error}", done.slot), BAD),
  }
}
