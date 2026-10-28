//! The mixer's controllers: the active snapshot's blend and the ducking
//! rules' dips. Each moves a gain layer (and a cutoff) over time; the engine
//! puts what they say on the buses

use std::time::Duration;

use super::mixer::DuckDef;
use crate::core::hash::FastMap;

/// A low-pass cutoff that lets everything through, in Hz
pub(crate) const OPEN_HZ: f32 = 20_000.0;

/// What a snapshot does to one bus at one moment: its gain layer and its
/// low-pass cutoff
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Level {
  pub(crate) db: f32,
  pub(crate) hz: f32,
}

impl Level {
  /// A bus no snapshot touches
  pub(crate) const OPEN: Level = Level {
    db: 0.0,
    hz: OPEN_HZ,
  };

  /// Between `a` and `b`: dB in a line, Hz in a line of octaves, which is
  /// how a sweep sounds even
  fn lerp(a: Level, b: Level, t: f32) -> Level {
    if t >= 1.0 {
      return b;
    }
    Level {
      db: a.db + (b.db - a.db) * t,
      hz: a.hz * (b.hz / a.hz).powf(t),
    }
  }
}

/// The active snapshot and the blend toward it (or back from it): the level
/// each bus is at now, found by name so it outlives a rebuilt bus tree. Its
/// time is kept exactly (a sum of frames' `Duration`s), so a blend lands
/// on the frame its time runs out
#[derive(Default)]
pub(crate) struct Blend {
  /// The snapshot blended to; `None` once cleared
  pub(crate) active: Option<String>,
  /// How long the snapshot named in `active` (or last active) blends
  blend: Duration,
  /// Each bus's level now: the buses a snapshot has moved, and not yet
  /// back to open
  current: FastMap<String, Level>,
  /// Where the running blend started and where it goes, over the same buses
  from: FastMap<String, Level>,
  to: FastMap<String, Level>,
  elapsed: Duration,
  duration: Duration,
  running: bool,
}

impl Blend {
  /// `bus`'s level now
  pub(crate) fn level(&self, bus: &str) -> Level {
    self.current.get(bus).copied().unwrap_or(Level::OPEN)
  }

  /// The levels now, by bus name
  pub(crate) fn levels(&self) -> impl Iterator<Item = (&str, Level)> {
    self
      .current
      .iter()
      .map(|(bus, level)| (bus.as_str(), *level))
  }

  /// How long the snapshot last active blends
  pub(crate) fn blend(&self) -> Duration {
    self.blend
  }

  /// Blend from the levels now to `targets` (every other bus to open) over
  /// `blend`, and make `active` the active snapshot
  pub(crate) fn begin<'a>(
    &mut self,
    active: Option<&str>,
    blend: Duration,
    targets: impl Iterator<Item = (&'a str, Level)>,
  ) {
    self.active = active.map(str::to_string);
    self.blend = blend;
    self.from.clear();
    self.to.clear();
    for (bus, level) in &self.current {
      self.from.insert(bus.clone(), *level);
      self.to.insert(bus.clone(), Level::OPEN);
    }
    for (bus, level) in targets {
      if !self.from.contains_key(bus) {
        self.from.insert(bus.to_string(), Level::OPEN);
      }
      self.to.insert(bus.to_string(), level);
    }
    self.elapsed = Duration::ZERO;
    self.duration = blend;
    self.running = true;
  }

  pub(crate) fn running(&self) -> bool {
    self.running
  }

  /// The time the running blend has left (none when none runs)
  pub(crate) fn remaining(&self) -> Duration {
    if self.running {
      self.duration.saturating_sub(self.elapsed)
    } else {
      Duration::ZERO
    }
  }

  /// Make the blend just begun take `time` instead of its full blend
  pub(crate) fn shorten_to(&mut self, time: Duration) {
    self.duration = time;
  }

  /// Move the blend on by `delta`; whether it did anything (the engine
  /// then puts the levels on the buses)
  pub(crate) fn step(&mut self, delta: Duration) -> bool {
    if !self.running {
      return false;
    }
    self.elapsed = self.elapsed.saturating_add(delta);
    let t = if self.elapsed >= self.duration {
      1.0
    } else {
      (self.elapsed.as_secs_f64() / self.duration.as_secs_f64()) as f32
    };
    for (bus, from) in &self.from {
      let to = self.to.get(bus).copied().unwrap_or(Level::OPEN);
      let level = Level::lerp(*from, to, t);
      match self.current.get_mut(bus) {
        Some(current) => *current = level,
        None => {
          self.current.insert(bus.clone(), level);
        }
      }
    }
    if t >= 1.0 {
      self.running = false;
    }
    true
  }

  /// Forget the buses a finished blend left open
  pub(crate) fn settle(&mut self) {
    if !self.running {
      self.current.retain(|_, level| *level != Level::OPEN);
    }
  }
}

/// One ducking rule at runtime: the rule, the buses it reads and dips as
/// indices into the current tree, and how far down it is now
pub(crate) struct DuckState {
  pub(crate) def: DuckDef,
  pub(crate) trigger: usize,
  pub(crate) target: usize,
  /// The dip now, in dB: 0 to `def.by_db`
  pub(crate) depth: f32,
}

impl DuckState {
  /// Move the dip toward its goal for `seconds`: all the way down over the
  /// attack while the trigger plays, back to 0 over the release after.
  /// Whether it moved
  pub(crate) fn step(&mut self, playing: bool, seconds: f32) -> bool {
    let goal = if playing { self.def.by_db } else { 0.0 };
    if self.depth == goal {
      return false;
    }
    let time = if playing {
      self.def.attack
    } else {
      self.def.release
    };
    let travel = if time > 0.0 {
      seconds * self.def.by_db.abs() / time
    } else {
      f32::INFINITY
    };
    self.depth = if goal < self.depth {
      (self.depth - travel).max(goal)
    } else {
      (self.depth + travel).min(goal)
    };
    true
  }
}
