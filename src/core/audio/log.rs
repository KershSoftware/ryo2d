//! The audio log: what the engine did with each voice, for tests and debug
//! tooling. Off by default, so a game pays nothing for it

use std::time::Duration;

/// How many entries the log keeps: the latest, older ones dropping off
pub(crate) const LOG_CAPACITY: usize = 4096;

/// What happened to a voice
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioLogKind {
  /// It started
  Start,
  /// It was told to stop, fading out over `fade`
  Stop,
  /// It was paused
  Pause,
  /// It was resumed
  Resume,
  /// Its gain changed to `gain_db`
  Gain,
  /// Its pan changed to `pan`
  Pan,
  /// Its playback rate changed to `rate`
  Rate,
  /// A play was refused, so no voice started (`voice` is 0)
  Refused,
  /// A play was culled before it started (out of range)
  Culled,
  /// It was stopped to make room for another voice
  Stolen,
}

/// One thing the engine did with a voice, and the voice as it stood then
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct AudioLogEntry {
  pub kind: AudioLogKind,
  /// The voice's id (from 1; 0 when no voice started)
  pub voice: u32,
  /// The sound definition it plays: its asset path (its id for one made in
  /// code); empty for a clip played directly
  pub sound: String,
  /// What it plays: the clip's asset path (its id for one made in code)
  pub source: String,
  /// Its bus
  pub bus: String,
  /// Its own gain in dB
  pub gain_db: f32,
  /// Its pan, -1 (left) to 1 (right)
  pub pan: f32,
  /// Its playback rate (1 is normal; pitch in semitones `s` is `2^(s/12)`)
  pub rate: f64,
  /// How long the change takes: a stop's fade-out, the smoothing of any
  /// other change, zero for a start without a fade-in
  pub fade: Duration,
}

/// The log: a ring of the latest [`LOG_CAPACITY`] entries, recorded only
/// while it is on. It holds up to twice that and drops the older half at
/// once, so a push is cheap and the latest entries read as one slice
#[derive(Debug, Default)]
pub(crate) struct AudioLog {
  on: bool,
  entries: Vec<AudioLogEntry>,
}

impl AudioLog {
  pub(crate) fn set_on(&mut self, on: bool) {
    self.on = on;
  }

  pub(crate) fn is_on(&self) -> bool {
    self.on
  }

  /// Records `entry` when the log is on
  pub(crate) fn push(&mut self, entry: AudioLogEntry) {
    if !self.on {
      return;
    }
    if self.entries.len() >= 2 * LOG_CAPACITY {
      self.entries.drain(..LOG_CAPACITY);
    }
    self.entries.push(entry);
  }

  /// The latest entries, oldest first
  pub(crate) fn entries(&self) -> &[AudioLogEntry] {
    &self.entries[self.entries.len().saturating_sub(LOG_CAPACITY)..]
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn entry(voice: u32) -> AudioLogEntry {
    AudioLogEntry {
      kind: AudioLogKind::Start,
      voice,
      sound: String::new(),
      source: String::new(),
      bus: "sfx".into(),
      gain_db: 0.0,
      pan: 0.0,
      rate: 1.0,
      fade: Duration::ZERO,
    }
  }

  #[test]
  fn the_log_is_off_until_turned_on() {
    let mut log = AudioLog::default();
    log.push(entry(1));
    assert!(log.entries().is_empty());
    log.set_on(true);
    log.push(entry(2));
    assert_eq!(log.entries(), &[entry(2)]);
  }

  #[test]
  fn the_log_keeps_the_latest_entries() {
    let mut log = AudioLog::default();
    log.set_on(true);
    let pushed = 3 * LOG_CAPACITY as u32 + 7;
    for voice in 1..=pushed {
      log.push(entry(voice));
      assert!(log.entries().len() <= LOG_CAPACITY);
    }
    let entries = log.entries();
    assert_eq!(entries.len(), LOG_CAPACITY);
    assert_eq!(entries[0].voice, pushed - LOG_CAPACITY as u32 + 1);
    assert_eq!(entries[LOG_CAPACITY - 1].voice, pushed);
  }
}
