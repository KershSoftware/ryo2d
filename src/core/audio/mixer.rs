//! [`Mixer`]: the bus tree (`.mixer.ron`), and the tree at runtime: one
//! kira track per bus, with its effects, its sends and its gain layers

use std::fmt;
use std::marker::PhantomData;
use std::time::Duration;

use kira::effect::compressor::CompressorBuilder;
use kira::effect::delay::DelayBuilder;
use kira::effect::distortion::{DistortionBuilder, DistortionKind as KiraDistortion};
use kira::effect::eq_filter::{EqFilterBuilder, EqFilterKind};
use kira::effect::filter::{FilterBuilder, FilterHandle, FilterMode};
use kira::effect::reverb::ReverbBuilder;
use kira::effect::{Effect, EffectBuilder};
use kira::sound::SoundData;
use kira::track::{SendTrackBuilder, SendTrackHandle, SendTrackId, TrackBuilder, TrackHandle};
use kira::{Decibels, PlaySoundError, ResourceLimitReached, Tween};
use ron::extensions::Extensions;
use serde::Deserialize;
use serde::de::{MapAccess, Visitor};

use super::VolumeKind;
use super::mix::OPEN_HZ;
use super::settings::SILENCE_DB;
use crate::core::asset::{Asset, AssetServer, Handle};

/// Where the engine looks for the game's mixer: loaded at start if it
/// exists, the standard layout otherwise. The web build has no asset folder
/// to look in, so there it comes from the game's own source
/// (`EngineBuilder::asset_source`) if that has it, or from code
/// ([`AudioView::set_mixer`](super::AudioView::set_mixer))
pub const MIXER_PATH: &str = "audio/mixer.mixer.ron";

/// The bus tree, loaded from a `.mixer.ron` file (the engine loads
/// [`MIXER_PATH`] if the game has one; see
/// [`MixerLoader`](crate::core::asset::MixerLoader) for the file).
///
/// The standard buses always exist: `master`, and under it `music`, `sfx`,
/// `ui`, `voice` and `ambience`. A mixer adjusts them and adds a game's own
/// buses under them, such as `sfx/footsteps`, each with its volume, its
/// effects, its sends to send buses, whether it pauses with the game and
/// follows time scale, and its voice limit. With no buses it is the
/// standard layout.
///
/// A send's return follows its sender's sliders (the send carries the
/// gains of the buses above the sender), plays at the send bus's own gain
/// and `master`'s, and passes only through the send bus's effects: kira
/// mixes send buses beside `master`'s track, so `master`'s effects do not
/// process returns. A mixer made in code is applied with
/// `ctx.audio().set_mixer(mixer)`.
///
/// A save (or `set_mixer`) builds a new tree: loops move to it where they
/// are, and one-shots play out on the old one, which is kept until its last
/// sound has ended and its effects have rung out (a `Reverb`'s or a
/// `Delay`'s tail, up to 10 s), so tuning a reverb cuts nothing mid-ring.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mixer {
  /// The buses it adjusts or adds, in any order
  pub buses: Vec<BusDef>,
  /// Named presets of bus volumes and low-pass cutoffs, which
  /// `ctx.audio().snapshot(name)` blends to
  pub snapshots: Vec<SnapshotDef>,
  /// Rules that dip one bus while another plays
  pub ducks: Vec<DuckDef>,
  /// What the mix does while the game's window is not focused: `Keep` (the
  /// default), or turn `master` down or off, smoothed over 10 ms
  pub on_focus_loss: FocusLoss,
}

impl Asset for Mixer {}

/// A snapshot of a [`Mixer`]: the volumes and low-pass cutoffs some buses
/// blend to. A bus it does not name blends back to its own. Its volumes
/// are a layer over the mixer's and the player's, in dB; a bus it names a
/// `low_pass` for gets a filter, open at 20 kHz otherwise.
///
/// A snapshot is a full preset: a bus it names without a `low_pass` opens to
/// 20 kHz, like a bus it does not name, when it is blended to
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotDef {
  pub name: String,
  /// Seconds to blend to it, or back from it
  pub blend: f32,
  pub buses: Vec<(String, SnapshotBus)>,
}

/// What a snapshot does to one bus
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SnapshotBus {
  /// Added to the bus's gain, in dB
  pub volume_db: f32,
  /// The cutoff in Hz its low-pass closes to
  pub low_pass: Option<f32>,
}

/// A ducking rule: while any voice plays on `trigger` (or a bus under it),
/// `target` is `by_db` quieter. The engine moves the dip each frame, over
/// `attack` seconds in and `release` seconds out: it is gain, not audio-rate
/// side-chaining. Two rules on one target add their dips in dB
#[derive(Debug, Clone, PartialEq)]
pub struct DuckDef {
  pub trigger: String,
  pub target: String,
  /// The dip in dB (below 0)
  pub by_db: f32,
  /// Seconds to reach the full dip
  pub attack: f32,
  /// Seconds to come back
  pub release: f32,
}

/// One bus of a [`Mixer`]: a standard bus it adjusts, or a game's own
#[derive(Debug, Clone, PartialEq)]
pub struct BusDef {
  /// `master`, a standard bus, or a bus under one (`"sfx/footsteps"`)
  pub name: String,
  /// Its volume in dB (0 plays what reaches it as it is)
  pub volume_db: f32,
  /// Its effects, applied in order to everything on it
  pub effects: Vec<EffectDef>,
  /// The send buses it feeds, each at a level in dB, after its volume (and
  /// with the gains of the buses above it). A bus another sends to is a send
  /// bus: it plays no voices of its own
  pub sends: Vec<(String, f32)>,
  /// Whether its voices pause where they are while the game is paused
  /// (`time.game.pause()`), resuming from there, and while the scene that
  /// played them is not updated (`Scene::holds_audio_below`); `None` keeps
  /// the standard bus's default (sfx, ambience and voice do), or its
  /// parent's. Each bus goes by its own: one that does not pause under one
  /// that does plays on
  pub pauses_with_game: Option<bool>,
  /// Whether its voices follow the game's time scale: they play at their
  /// pitch's rate times the game's speed (up to 8 times), and pause while
  /// time stands still (the game paused, or a speed of 0.01 or less;
  /// hitstop does not hold them); `None` keeps the standard bus's default
  /// (sfx and ambience do), or its parent's
  pub follows_time_scale: Option<bool>,
  /// The most voices playing on it and the buses under it
  pub limit: Option<u16>,
}

impl BusDef {
  /// The bus `name` with nothing changed: 0 dB, no effects or sends, its
  /// defaults and no limit
  pub fn new(name: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      volume_db: 0.0,
      effects: Vec::new(),
      sends: Vec::new(),
      pauses_with_game: None,
      follows_time_scale: None,
      limit: None,
    }
  }
}

/// An effect on a bus, as a `.mixer.ron` file writes it
/// (`LowPass(cutoff: 800.0)`); a parameter left out takes the default shown
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum EffectDef {
  /// Removes frequencies above `cutoff` (Hz, 20 to 20,000); `resonance`
  /// (0 to 1, default 0) rings at the cutoff
  LowPass {
    cutoff: f64,
    #[serde(default)]
    resonance: f64,
  },
  /// Removes frequencies below `cutoff` (Hz, 20 to 20,000); `resonance`
  /// (0 to 1, default 0) rings at the cutoff
  HighPass {
    cutoff: f64,
    #[serde(default)]
    resonance: f64,
  },
  /// Raises or lowers a band (`Bell`) or everything below or above a
  /// frequency (`LowShelf`, `HighShelf`; 20 to 20,000 Hz) by `gain_db` (-24
  /// to +24); a higher `q` is a narrower band (0.01 or more, kira's widest;
  /// default 1)
  Eq {
    kind: EqKind,
    frequency: f64,
    gain_db: f32,
    #[serde(default = "one")]
    q: f64,
  },
  /// A room: `feedback` (0 to 1, default 0.9) is its size, `damping` (0 to
  /// 1, default 0.1) how fast its highs fade, `stereo_width` (0 to 1,
  /// default 1), and `mix` (0 dry to 1 wet, default 0.5)
  Reverb {
    #[serde(default = "reverb_feedback")]
    feedback: f64,
    #[serde(default = "reverb_damping")]
    damping: f64,
    #[serde(default = "one")]
    stereo_width: f64,
    #[serde(default = "half")]
    mix: f32,
  },
  /// Echoes `seconds` apart (0.001 to 10, as kira's delay holds at least a
  /// frame; default 0.5), each `feedback_db` quieter than the last (below 0,
  /// default -6), at `mix` (0 to 1, default 0.5)
  Delay {
    #[serde(default = "half_f64")]
    seconds: f64,
    #[serde(default = "delay_feedback")]
    feedback_db: f32,
    #[serde(default = "half")]
    mix: f32,
  },
  /// Turns down what is louder than `threshold_db` (-60, silence, to +24;
  /// default 0) by `ratio` (1 or more, default 1), reacting over `attack`
  /// (0 to 10 s, default 0.01 s) and recovering over `release` (0 to 10 s,
  /// default 0.1 s), then adds `makeup_db` (-24 to +24, default 0), at
  /// `mix` (0 to 1, default 1)
  Compressor {
    #[serde(default)]
    threshold_db: f64,
    #[serde(default = "one")]
    ratio: f64,
    #[serde(default = "compressor_attack")]
    attack: f64,
    #[serde(default = "compressor_release")]
    release: f64,
    #[serde(default)]
    makeup_db: f32,
    #[serde(default = "one_f32")]
    mix: f32,
  },
  /// Clips the signal (`HardClip` by default, or `SoftClip`) after raising
  /// it by `drive_db` (default 0), at `mix` (default 1)
  Distortion {
    #[serde(default)]
    kind: DistortionKind,
    #[serde(default)]
    drive_db: f32,
    #[serde(default = "one_f32")]
    mix: f32,
  },
}

/// The shape of an [`EffectDef::Eq`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum EqKind {
  /// A band around the frequency
  Bell,
  /// Everything below the frequency
  LowShelf,
  /// Everything above the frequency
  HighShelf,
}

/// How an [`EffectDef::Distortion`] clips
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum DistortionKind {
  /// Cuts the wave off flat
  #[default]
  HardClip,
  /// Rounds the wave off
  SoftClip,
}

/// What the mix does while the game's window is not focused
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
pub enum FocusLoss {
  /// Nothing: the game sounds as it does focused
  #[default]
  Keep,
  /// `master` turns down by this many dB (below 0)
  Duck(f32),
  /// `master` goes silent
  Mute,
}

fn one() -> f64 {
  1.0
}

fn one_f32() -> f32 {
  1.0
}

fn half() -> f32 {
  0.5
}

fn half_f64() -> f64 {
  0.5
}

fn reverb_feedback() -> f64 {
  0.9
}

fn reverb_damping() -> f64 {
  0.1
}

fn delay_feedback() -> f32 {
  -6.0
}

fn compressor_attack() -> f64 {
  0.01
}

fn compressor_release() -> f64 {
  0.1
}

impl Mixer {
  /// A mixer from `.mixer.ron` text, checked
  pub(crate) fn parse(text: &str) -> Result<Mixer, String> {
    let file: MixerFile = ron::Options::default()
      .with_default_extension(Extensions::IMPLICIT_SOME)
      .from_str(text)
      .map_err(|error| error.to_string())?;
    let mixer = Mixer {
      buses: file
        .buses
        .0
        .into_iter()
        .map(|(name, bus)| BusDef {
          name,
          volume_db: bus.volume,
          effects: bus.effects,
          sends: bus.sends.0,
          pauses_with_game: bus.pauses_with_game,
          follows_time_scale: bus.follows_time_scale,
          limit: bus.limit,
        })
        .collect(),
      snapshots: file
        .snapshots
        .0
        .into_iter()
        .map(|(name, snapshot)| SnapshotDef {
          name,
          blend: snapshot.blend,
          buses: snapshot
            .buses
            .0
            .into_iter()
            .map(|(bus, target)| {
              let target = SnapshotBus {
                volume_db: target.volume,
                low_pass: target.low_pass,
              };
              (bus, target)
            })
            .collect(),
        })
        .collect(),
      ducks: file
        .ducks
        .into_iter()
        .map(|duck| DuckDef {
          trigger: duck.trigger,
          target: duck.target,
          by_db: duck.by,
          attack: duck.attack,
          release: duck.release,
        })
        .collect(),
      on_focus_loss: file.on_focus_loss,
    };
    mixer.check()?;
    Ok(mixer)
  }

  /// What RON cannot say is wrong, for a file or a mixer made in code:
  /// every name, parent, volume, send, limit and effect parameter
  pub(crate) fn check(&self) -> Result<(), String> {
    for (index, bus) in self.buses.iter().enumerate() {
      check_bus_name(&bus.name)?;
      if self.buses[..index]
        .iter()
        .any(|other| other.name == bus.name)
      {
        return Err(format!("it names {:?} twice", bus.name));
      }
    }
    let custom = self
      .buses
      .iter()
      .filter(|bus| !is_standard(&bus.name))
      .count();
    if custom > MAX_BUSES {
      return Err(format!(
        "it adds {custom} buses, more than the {MAX_BUSES} a mixer may add"
      ));
    }
    let sends = self.send_buses();
    if sends.len() > MAX_SEND_BUSES {
      return Err(format!(
        "it has {} send buses, more than the {MAX_SEND_BUSES} a mixer may have",
        sends.len()
      ));
    }
    for bus in &self.buses {
      self
        .check_bus(bus, &sends)
        .map_err(|error| format!("the bus {:?}: {error}", bus.name))?;
    }
    self.check_snapshots()?;
    self.check_ducks()?;
    if let FocusLoss::Duck(db) = self.on_focus_loss
      && !(db.is_finite() && db < 0.0)
    {
      return Err(format!(
        "on_focus_loss: Duck({db}) is not a cut in dB (below 0)"
      ));
    }
    Ok(())
  }

  /// Whether `name` is a bus this mixer makes
  fn makes(&self, name: &str) -> bool {
    is_standard(name) || self.has(name)
  }

  fn check_snapshots(&self) -> Result<(), String> {
    for (index, snapshot) in self.snapshots.iter().enumerate() {
      let name = &snapshot.name;
      if self.snapshots[..index]
        .iter()
        .any(|other| other.name == *name)
      {
        return Err(format!("it names the snapshot {name:?} twice"));
      }
      if !(snapshot.blend.is_finite() && (0.0..=MAX_SECONDS).contains(&snapshot.blend)) {
        return Err(format!(
          "the snapshot {name:?}: its blend {} is not a time from 0 to {MAX_SECONDS} seconds",
          snapshot.blend
        ));
      }
      for (at, (bus, target)) in snapshot.buses.iter().enumerate() {
        if !self.makes(bus) {
          return Err(format!(
            "the snapshot {name:?} names the bus {bus:?}, which is not in the mixer"
          ));
        }
        if snapshot.buses[..at].iter().any(|(other, _)| other == bus) {
          return Err(format!("the snapshot {name:?} names the bus {bus:?} twice"));
        }
        if !(target.volume_db.is_finite() && target.volume_db <= MAX_BUS_DB) {
          return Err(format!(
            "the snapshot {name:?}, bus {bus:?}: its volume {} is not a gain in dB of at most +{MAX_BUS_DB}",
            target.volume_db
          ));
        }
        if let Some(hz) = target.low_pass {
          frequency("snapshot", "low_pass", f64::from(hz))
            .map_err(|error| format!("the snapshot {name:?}, bus {bus:?}: {error}"))?;
        }
      }
    }
    Ok(())
  }

  fn check_ducks(&self) -> Result<(), String> {
    let sends = self.send_buses();
    for duck in &self.ducks {
      let (trigger, target) = (&duck.trigger, &duck.target);
      let rule = format!("the duck of {target:?} under {trigger:?}");
      for bus in [trigger, target] {
        if !self.makes(bus) {
          return Err(format!("{rule}: the bus {bus:?} is not in the mixer"));
        }
      }
      if trigger == target {
        return Err(format!("{rule}: a bus cannot duck itself"));
      }
      if sends.contains(&trigger.as_str()) {
        return Err(format!(
          "{rule}: {trigger:?} is a send bus, which plays no voices to trigger it"
        ));
      }
      if !(duck.by_db.is_finite() && duck.by_db < 0.0 && duck.by_db >= SILENCE_DB) {
        return Err(format!(
          "{rule}: its by {} is not a cut in dB (below 0, to {SILENCE_DB})",
          duck.by_db
        ));
      }
      for (field, seconds) in [("attack", duck.attack), ("release", duck.release)] {
        if !(seconds.is_finite() && (0.0..=MAX_SECONDS).contains(&seconds)) {
          return Err(format!(
            "{rule}: its {field} {seconds} is not a time from 0 to {MAX_SECONDS} seconds"
          ));
        }
      }
    }
    Ok(())
  }

  fn check_bus(&self, bus: &BusDef, sends: &[&str]) -> Result<(), String> {
    if let Some((parent, _)) = bus.name.rsplit_once('/') {
      if !is_standard(parent) && !self.has(parent) {
        return Err(format!(
          "its parent {parent:?} is not in the mixer (a bus under a game's bus needs that bus written too)"
        ));
      }
      if sends.contains(&parent) {
        return Err(format!(
          "it is under {parent:?}, a send bus, which has no buses under it"
        ));
      }
    }
    if !(bus.volume_db.is_finite() && bus.volume_db <= MAX_BUS_DB) {
      return Err(format!(
        "its volume {} is not a gain in dB of at most +{MAX_BUS_DB}",
        bus.volume_db
      ));
    }
    if bus.limit == Some(0) {
      return Err("its limit is 0, so nothing would play on it (it is 1 or more)".into());
    }
    if bus.name == "master" && !bus.sends.is_empty() {
      return Err("master sends nowhere: every bus is under it, send buses too".into());
    }
    for (target, level) in &bus.sends {
      if *target == bus.name {
        return Err("it sends to itself".into());
      }
      if is_standard(target) {
        return Err(format!(
          "it sends to {target:?}, a standard bus: sends go to a game's own buses, which become send buses"
        ));
      }
      if !self.has(target) {
        return Err(format!("it sends to {target:?}, which is not in the mixer"));
      }
      if !(level.is_finite() && *level <= MAX_BUS_DB) {
        return Err(format!(
          "its send to {target:?} is at {level}, not a level in dB of at most +{MAX_BUS_DB}"
        ));
      }
    }
    let plays_voices = !bus.sends.is_empty()
      || bus.limit.is_some()
      || bus.pauses_with_game.is_some()
      || bus.follows_time_scale.is_some();
    if sends.contains(&bus.name.as_str()) && plays_voices {
      return Err(
        "it is a send bus (another bus sends to it), so it plays no voices: it sends nowhere and takes no limit, pauses_with_game or follows_time_scale"
          .into(),
      );
    }
    for effect in &bus.effects {
      effect.check()?;
    }
    Ok(())
  }

  fn has(&self, name: &str) -> bool {
    self.buses.iter().any(|bus| bus.name == name)
  }

  fn get(&self, name: &str) -> Option<&BusDef> {
    self.buses.iter().find(|bus| bus.name == name)
  }

  /// The buses another sends to, each once
  fn send_buses(&self) -> Vec<&str> {
    let mut sends: Vec<&str> = Vec::new();
    for (target, _) in self.buses.iter().flat_map(|bus| &bus.sends) {
      if !sends.contains(&target.as_str()) {
        sends.push(target);
      }
    }
    sends
  }
}

impl EffectDef {
  /// Each parameter within the range kira plays well
  fn check(&self) -> Result<(), String> {
    match *self {
      EffectDef::LowPass { cutoff, resonance } | EffectDef::HighPass { cutoff, resonance } => {
        let name = if matches!(self, EffectDef::LowPass { .. }) {
          "LowPass"
        } else {
          "HighPass"
        };
        frequency(name, "cutoff", cutoff)?;
        unit(name, "resonance", resonance)
      }
      EffectDef::Eq {
        frequency: hz,
        gain_db,
        q,
        ..
      } => {
        frequency("Eq", "frequency", hz)?;
        gain("Eq", "gain_db", gain_db)?;
        if q.is_finite() && q >= MIN_Q {
          Ok(())
        } else {
          Err(format!(
            "Eq: its q {q} is not {MIN_Q} or more (kira takes a lower one as {MIN_Q})"
          ))
        }
      }
      EffectDef::Reverb {
        feedback,
        damping,
        stereo_width,
        mix,
      } => {
        unit("Reverb", "feedback", feedback)?;
        unit("Reverb", "damping", damping)?;
        unit("Reverb", "stereo_width", stereo_width)?;
        unit("Reverb", "mix", f64::from(mix))
      }
      EffectDef::Delay {
        seconds,
        feedback_db,
        mix,
      } => {
        if !(seconds.is_finite() && (MIN_DELAY..=MAX_DELAY).contains(&seconds)) {
          return Err(format!(
            "Delay: its seconds {seconds} is not a time from {MIN_DELAY} to {MAX_DELAY} (kira's delay holds at least a frame)"
          ));
        }
        if !(feedback_db.is_finite() && feedback_db < 0.0) {
          return Err(format!(
            "Delay: its feedback_db {feedback_db} is not below 0 (each echo quieter than the last)"
          ));
        }
        unit("Delay", "mix", f64::from(mix))
      }
      EffectDef::Compressor {
        threshold_db,
        ratio,
        attack,
        release,
        makeup_db,
        mix,
      } => {
        let levels = f64::from(SILENCE_DB)..=f64::from(MAX_BUS_DB);
        if !levels.contains(&threshold_db) {
          return Err(format!(
            "Compressor: its threshold_db {threshold_db} is not a level from {SILENCE_DB} (silence) to +{MAX_BUS_DB} dB"
          ));
        }
        if !(ratio.is_finite() && ratio >= 1.0) {
          return Err(format!("Compressor: its ratio {ratio} is not 1 or more"));
        }
        for (field, value) in [("attack", attack), ("release", release)] {
          if !(value.is_finite() && (0.0..=MAX_DELAY).contains(&value)) {
            return Err(format!(
              "Compressor: its {field} {value} is not a time from 0 to {MAX_DELAY} s"
            ));
          }
        }
        gain("Compressor", "makeup_db", makeup_db)?;
        unit("Compressor", "mix", f64::from(mix))
      }
      EffectDef::Distortion { drive_db, mix, .. } => {
        if !(drive_db.is_finite() && drive_db.abs() <= MAX_DRIVE_DB) {
          return Err(format!(
            "Distortion: its drive_db {drive_db} is not a gain from -{MAX_DRIVE_DB} to +{MAX_DRIVE_DB} dB"
          ));
        }
        unit("Distortion", "mix", f64::from(mix))
      }
    }
  }

  /// How long it rings after what reaches it falls silent: the time its
  /// output takes to fall 60 dB, at most [`MAX_TAIL`]. A delay's echoes
  /// fall `feedback_db` each `seconds`, so `seconds × 60 / |feedback_db|`.
  /// A reverb (kira's is Freeverb) falls by its slowest comb filter, which
  /// loses `20·log10(feedback)` dB each pass of its loop, and then its
  /// all-pass filters in series, each losing 6 dB a pass:
  /// `3·REVERB_COMB / -log10(feedback) + 3·REVERB_ALL_PASS / log10(2)`
  /// (a feedback comb filter's 60 dB time; one of 1 never falls). The
  /// filters, the EQ, the compressor and the distortion do not ring
  pub(crate) fn tail(&self) -> Duration {
    let seconds = match *self {
      EffectDef::Delay {
        seconds,
        feedback_db,
        ..
      } => seconds * 60.0 / f64::from(feedback_db.abs()),
      EffectDef::Reverb { feedback, .. } if feedback >= 1.0 => f64::INFINITY,
      EffectDef::Reverb { feedback, .. } => {
        let combs = if feedback > 0.0 {
          3.0 * REVERB_COMB / -feedback.log10()
        } else {
          REVERB_COMB
        };
        combs + 3.0 * REVERB_ALL_PASS / 2f64.log10()
      }
      _ => 0.0,
    };
    if seconds.is_nan() || seconds <= 0.0 {
      Duration::ZERO
    } else {
      Duration::from_secs_f64(seconds.min(MAX_TAIL.as_secs_f64()))
    }
  }

  /// kira's effect, built
  fn build(&self) -> Box<dyn Effect> {
    match *self {
      EffectDef::LowPass { cutoff, resonance } => {
        FilterBuilder::new()
          .mode(FilterMode::LowPass)
          .cutoff(cutoff)
          .resonance(resonance)
          .build()
          .0
      }
      EffectDef::HighPass { cutoff, resonance } => {
        FilterBuilder::new()
          .mode(FilterMode::HighPass)
          .cutoff(cutoff)
          .resonance(resonance)
          .build()
          .0
      }
      EffectDef::Eq {
        kind,
        frequency,
        gain_db,
        q,
      } => {
        let kind = match kind {
          EqKind::Bell => EqFilterKind::Bell,
          EqKind::LowShelf => EqFilterKind::LowShelf,
          EqKind::HighShelf => EqFilterKind::HighShelf,
        };
        EqFilterBuilder::new(kind, frequency, Decibels(gain_db), q)
          .build()
          .0
      }
      EffectDef::Reverb {
        feedback,
        damping,
        stereo_width,
        mix,
      } => {
        ReverbBuilder::new()
          .feedback(feedback)
          .damping(damping)
          .stereo_width(stereo_width)
          .mix(mix)
          .build()
          .0
      }
      EffectDef::Delay {
        seconds,
        feedback_db,
        mix,
      } => {
        DelayBuilder::new()
          .delay_time(Duration::from_secs_f64(seconds))
          .feedback(Decibels(feedback_db))
          .mix(mix)
          .build()
          .0
      }
      EffectDef::Compressor {
        threshold_db,
        ratio,
        attack,
        release,
        makeup_db,
        mix,
      } => {
        CompressorBuilder::new()
          .threshold(threshold_db)
          .ratio(ratio)
          .attack_duration(Duration::from_secs_f64(attack))
          .release_duration(Duration::from_secs_f64(release))
          .makeup_gain(Decibels(makeup_db))
          .mix(mix)
          .build()
          .0
      }
      EffectDef::Distortion {
        kind,
        drive_db,
        mix,
      } => {
        let kind = match kind {
          DistortionKind::HardClip => KiraDistortion::HardClip,
          DistortionKind::SoftClip => KiraDistortion::SoftClip,
        };
        DistortionBuilder::new()
          .kind(kind)
          .drive(Decibels(drive_db))
          .mix(mix)
          .build()
          .0
      }
    }
  }
}

/// A frequency in the audible range
fn frequency(effect: &str, field: &str, hz: f64) -> Result<(), String> {
  if hz.is_finite() && (MIN_HZ..=MAX_HZ).contains(&hz) {
    Ok(())
  } else {
    Err(format!(
      "{effect}: its {field} {hz} is not a frequency from {MIN_HZ} to {MAX_HZ} Hz"
    ))
  }
}

/// A value from 0 to 1
fn unit(effect: &str, field: &str, value: f64) -> Result<(), String> {
  if value.is_finite() && (0.0..=1.0).contains(&value) {
    Ok(())
  } else {
    Err(format!("{effect}: its {field} {value} is not from 0 to 1"))
  }
}

/// A gain within ±[`MAX_BUS_DB`]
fn gain(effect: &str, field: &str, db: f32) -> Result<(), String> {
  if db.is_finite() && db.abs() <= MAX_BUS_DB {
    Ok(())
  } else {
    Err(format!(
      "{effect}: its {field} {db} is not a gain from -{MAX_BUS_DB} to +{MAX_BUS_DB} dB"
    ))
  }
}

/// `master` or one of the five under it
fn is_standard(name: &str) -> bool {
  name == "master" || STANDARD_BUSES.contains(&name)
}

/// `master`, a standard bus, or a bus under one, with no empty part
pub(crate) fn check_bus_name(bus: &str) -> Result<(), String> {
  if bus == "master" {
    return Ok(());
  }
  let mut parts = bus.split('/');
  let root = parts.next().unwrap_or_default();
  if STANDARD_BUSES.contains(&root) && parts.all(|part| !part.is_empty()) {
    Ok(())
  } else {
    Err(format!(
      "the bus {bus:?} is not master, a standard bus ({}) or a bus under one, such as \"sfx/footsteps\"",
      STANDARD_BUSES.join(", ")
    ))
  }
}

/// The game's mixer, loading, if the registry has a file at [`MIXER_PATH`];
/// `None` (the standard layout, and no warning) when it does not
pub(crate) fn default_mixer(server: &AssetServer) -> Option<Handle<Mixer>> {
  server
    .exists(MIXER_PATH)
    .then(|| server.load::<Mixer>(MIXER_PATH))
}

/// A `.mixer.ron` file, as written
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MixerFile {
  #[serde(default)]
  buses: Named<BusFile>,
  #[serde(default)]
  snapshots: Named<SnapshotFile>,
  #[serde(default)]
  ducks: Vec<DuckFile>,
  #[serde(default)]
  on_focus_loss: FocusLoss,
}

/// One of a file's snapshots
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFile {
  #[serde(default = "default_blend")]
  blend: f32,
  #[serde(default)]
  buses: Named<SnapshotBusFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotBusFile {
  #[serde(default)]
  volume: f32,
  #[serde(default)]
  low_pass: Option<f32>,
}

/// One of a file's ducking rules
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DuckFile {
  trigger: String,
  target: String,
  by: f32,
  #[serde(default = "duck_attack")]
  attack: f32,
  #[serde(default = "duck_release")]
  release: f32,
}

fn default_blend() -> f32 {
  0.3
}

fn duck_attack() -> f32 {
  0.1
}

fn duck_release() -> f32 {
  0.6
}

/// One of a file's buses
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusFile {
  #[serde(default)]
  volume: f32,
  #[serde(default)]
  effects: Vec<EffectDef>,
  #[serde(default)]
  sends: Named<f32>,
  #[serde(default)]
  pauses_with_game: Option<bool>,
  #[serde(default)]
  follows_time_scale: Option<bool>,
  #[serde(default)]
  limit: Option<u16>,
}

/// A RON map from names, in the file's order; a name written twice fails
struct Named<T>(Vec<(String, T)>);

impl<T> Default for Named<T> {
  fn default() -> Self {
    Self(Vec::new())
  }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Named<T> {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct Entries<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
      type Value = Named<T>;

      fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a map from names, such as { \"sfx/footsteps\": (..) }")
      }

      fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Named<T>, A::Error> {
        let mut entries: Vec<(String, T)> = Vec::new();
        while let Some((name, value)) = map.next_entry::<String, T>()? {
          if entries.iter().any(|(other, _)| *other == name) {
            return Err(serde::de::Error::custom(format!("it names {name:?} twice")));
          }
          entries.push((name, value));
        }
        Ok(Named(entries))
      }
    }

    deserializer.deserialize_map(Entries(PhantomData))
  }
}

/// The standard buses under `master`, in order
pub(crate) const STANDARD_BUSES: [&str; 5] = ["music", "sfx", "ui", "voice", "ambience"];

/// The standard buses, `master` first, with whether each pauses with the
/// game and follows time scale by default
const STANDARD: [(&str, bool, bool); 6] = [
  ("master", false, false),
  ("music", false, false),
  ("sfx", true, true),
  ("ui", false, false),
  ("voice", true, false),
  ("ambience", true, true),
];

/// The most buses a mixer adds to the standard six
const MAX_BUSES: usize = 64;
/// The most send buses in a mixer (kira's room for send tracks is
/// [`SEND_TRACK_CAPACITY`], so retired trees' can wait out their voices
/// and tails)
const MAX_SEND_BUSES: usize = 16;
/// kira's room for send tracks, set on the manager, each tree's up to
/// [`MAX_SEND_BUSES`]: the live tree's and the one a rebuild builds beside
/// it (the old tree retires only after the new one is built), the four
/// retired trees kept for their voices and tails, and one more: a tree let
/// go and fading out, or one with no tail whose one-shots still play
pub(crate) const SEND_TRACK_CAPACITY: usize = 7 * MAX_SEND_BUSES;
/// The loudest a bus's volume, a send level or a gain layer sum is, in dB
pub(crate) const MAX_BUS_DB: f32 = 24.0;
/// The longest delay or compressor time, in seconds
const MAX_DELAY: f64 = 10.0;
/// The shortest delay, in seconds: kira's delay holds `seconds × rate`
/// whole frames and splits what it processes by that many, which panics on
/// the mixing thread at none (a delay shorter than a frame). A millisecond
/// is a frame at any rate from 1 kHz
const MIN_DELAY: f64 = 0.001;
/// The smallest EQ `q`: kira takes any lower one as this
const MIN_Q: f64 = 0.01;
/// The longest a retired bus tree is kept after its last sound for its
/// effects to ring out (`EffectDef::tail`, `BusTree::tail`)
pub(crate) const MAX_TAIL: Duration = Duration::from_secs(10);
/// The loop of kira's reverb's longest comb filter, in seconds: 1640
/// samples at its reference rate of 44.1 kHz (kira scales its filters'
/// lengths with the sample rate, so the time holds at any rate)
const REVERB_COMB: f64 = 1640.0 / 44_100.0;
/// The loops of kira's reverb's four all-pass filters, in series, in
/// seconds: 1655 samples at 44.1 kHz on its longer side
const REVERB_ALL_PASS: f64 = 1655.0 / 44_100.0;
/// The longest a snapshot's blend or a duck's attack or release is, in
/// seconds
const MAX_SECONDS: f32 = 60.0;
/// The audible range, for cutoffs and EQ frequencies
const MIN_HZ: f64 = 20.0;
const MAX_HZ: f64 = 20_000.0;
/// The most a distortion drives, either way, in dB
const MAX_DRIVE_DB: f32 = 48.0;

/// A bus planned from a mixer, before kira builds it: parents come before
/// their children, the standard six first
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlannedBus<'a> {
  pub(crate) name: &'a str,
  pub(crate) parent: Option<usize>,
  /// What the mixer says of it; `None` for a standard bus it leaves alone
  pub(crate) def: Option<&'a BusDef>,
  /// A send bus: another sends to it
  pub(crate) send: bool,
  /// The player's setting it carries (a standard bus's)
  pub(crate) setting: Option<VolumeKind>,
  pub(crate) pauses_with_game: bool,
  pub(crate) follows_time_scale: bool,
  /// A snapshot closes a low-pass on it, so it gets a filter
  pub(crate) low_pass: bool,
}

/// `mixer`'s buses in build order: the standard six, then the game's by
/// depth (in the file's order within a depth), each with its parent's index
/// and its pause and time-scale settings resolved. `mixer` is checked
pub(crate) fn plan(mixer: &Mixer) -> Vec<PlannedBus<'_>> {
  let sends = mixer.send_buses();
  let mut planned: Vec<PlannedBus<'_>> = Vec::with_capacity(STANDARD.len() + mixer.buses.len());
  for (index, (name, pauses, follows)) in STANDARD.into_iter().enumerate() {
    let def = mixer.get(name);
    planned.push(PlannedBus {
      name,
      parent: (index > 0).then_some(0),
      def,
      send: false,
      setting: Some(VolumeKind::ALL[index]),
      pauses_with_game: def.and_then(|def| def.pauses_with_game).unwrap_or(pauses),
      follows_time_scale: def
        .and_then(|def| def.follows_time_scale)
        .unwrap_or(follows),
      low_pass: filtered(mixer, name),
    });
  }
  let mut custom: Vec<&BusDef> = mixer
    .buses
    .iter()
    .filter(|bus| !is_standard(&bus.name))
    .collect();
  custom.sort_by_key(|bus| bus.name.matches('/').count());
  for def in custom {
    let parent = def
      .name
      .rsplit_once('/')
      .and_then(|(parent, _)| planned.iter().position(|bus| bus.name == parent));
    let Some(parent) = parent else {
      continue;
    };
    let above = planned[parent];
    planned.push(PlannedBus {
      name: &def.name,
      parent: Some(parent),
      def: Some(def),
      send: sends.contains(&def.name.as_str()),
      setting: None,
      pauses_with_game: def.pauses_with_game.unwrap_or(above.pauses_with_game),
      follows_time_scale: def.follows_time_scale.unwrap_or(above.follows_time_scale),
      low_pass: filtered(mixer, &def.name),
    });
  }
  planned
}

/// Whether any snapshot names a `low_pass` for `bus`
fn filtered(mixer: &Mixer, bus: &str) -> bool {
  mixer.snapshots.iter().any(|snapshot| {
    snapshot
      .buses
      .iter()
      .any(|(name, target)| name == bus && target.low_pass.is_some())
  })
}

/// A bus's gain, as layers in dB that add: a snapshot or a duck never
/// overrides the player's setting, and a setting moved under a snapshot is
/// heard at once
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct GainLayers {
  /// The mixer's volume, or the game's (`set_bus_volume`)
  pub(crate) base: f32,
  /// The player's setting on the curve; 0 for a bus without one
  pub(crate) setting: f32,
  /// The active snapshot's
  pub(crate) snapshot: f32,
  /// Ducking's
  pub(crate) duck: f32,
  /// The mixer's `on_focus_loss` while the window is not focused
  /// (`master`'s; 0 on every other bus)
  pub(crate) focus: f32,
}

impl GainLayers {
  /// The layers added, from silence to +[`MAX_BUS_DB`]; silence if any
  /// layer is silent, as a setting of 0 must be whatever the others add
  pub(crate) fn total(&self) -> f32 {
    let layers = [
      self.base,
      self.setting,
      self.snapshot,
      self.duck,
      self.focus,
    ];
    if layers.iter().any(|&layer| layer <= SILENCE_DB) {
      return SILENCE_DB;
    }
    layers.iter().sum::<f32>().clamp(SILENCE_DB, MAX_BUS_DB)
  }
}

/// The filter a snapshot closes: a low-pass, open
fn snapshot_filter() -> FilterBuilder {
  FilterBuilder::new()
    .mode(FilterMode::LowPass)
    .cutoff(f64::from(OPEN_HZ))
}

/// Warn that kira had no room for `bus`'s track: the capacities the engine
/// sets and a mixer's limits on buses leave room, so this is only a
/// safeguard
fn warn_no_room(bus: &str) {
  crate::engine_warn!(
    module: "Audio",
    "kira has no room for the {bus:?} bus's track: plays and sends to it are dropped"
  );
}

/// Two gains in dB added; silence if either is silent
fn add_db(a: f32, b: f32) -> f32 {
  if a <= SILENCE_DB || b <= SILENCE_DB {
    SILENCE_DB
  } else {
    (a + b).max(SILENCE_DB)
  }
}

/// What kira plays a bus's voices or sends through. There is one per bus,
/// built with the tree, and every play goes through a track: boxing it
/// would add a pointer chase to each play for no saving worth having
#[allow(clippy::large_enum_variant)]
pub(crate) enum Output {
  /// A track, under its parent's (`master`'s under kira's main track).
  /// `None` if kira had no room for it: plays on it are refused
  Track(Option<TrackHandle>),
  /// A send track, which kira mixes straight into its main track
  Send(Option<SendTrackHandle>),
}

/// One bus at runtime
pub(crate) struct Bus {
  /// Its full name, such as `sfx` or `sfx/footsteps`
  pub(crate) name: String,
  pub(crate) parent: Option<usize>,
  pub(crate) output: Output,
  /// How many effects its track has
  pub(crate) effects: usize,
  /// How long its effects ring after what reaches them falls silent, each
  /// effect's tail added (`EffectDef::tail`)
  pub(crate) tail: Duration,
  /// The send buses it feeds
  pub(crate) sends: Vec<Route>,
  pub(crate) layers: GainLayers,
  /// The low-pass snapshots close, if any names one for it
  pub(crate) cutoff: Option<Cutoff>,
  /// The volume its kira track was last given, in dB (`master`'s goes to
  /// kira's main track)
  pub(crate) applied: f32,
  pub(crate) pauses_with_game: bool,
  pub(crate) follows_time_scale: bool,
  /// Its voices are held, paused where they are: the game is paused and it
  /// pauses with the game, or time stands still and it follows time scale
  pub(crate) held: bool,
  /// The most voices playing on it and the buses under it
  pub(crate) limit: Option<u16>,
  /// A play was refused for want of kira's room on its track: warned once
  pub(crate) full_warned: bool,
}

/// A bus's snapshot filter: a low-pass kira holds on its track, open at
/// [`OPEN_HZ`] until a snapshot closes it
pub(crate) struct Cutoff {
  /// `None` while kira has no track for the bus
  pub(crate) handle: Option<FilterHandle>,
  /// The cutoff kira's filter was last given, in Hz
  pub(crate) applied: f32,
}

impl Cutoff {
  fn open(handle: Option<FilterHandle>) -> Self {
    Self {
      handle,
      applied: OPEN_HZ,
    }
  }
}

/// One of a bus's sends: to send bus `to`, at the mixer's `level` in dB
pub(crate) struct Route {
  pub(crate) to: usize,
  pub(crate) level: f32,
  /// The volume kira's route was last given, in dB: `level` plus the
  /// gains of the buses above the sender (see [`BusTree::route_volume`])
  pub(crate) applied: f32,
}

/// Where a tree's tracks are made: kira's manager
pub(crate) trait Tracks {
  fn add_sub_track(&mut self, builder: TrackBuilder) -> Result<TrackHandle, ResourceLimitReached>;
  fn add_send_track(
    &mut self,
    builder: SendTrackBuilder,
  ) -> Result<SendTrackHandle, ResourceLimitReached>;
}

/// The buses, `master` first and parents before children: the standard
/// layout, and the mixer's buses
pub(crate) struct BusTree {
  pub(crate) buses: Vec<Bus>,
}

impl BusTree {
  /// The tree `plan` describes, each bus's gain from `layers`, its tracks
  /// made through `tracks` (none without: every play is refused). Every
  /// track persists until its sounds finish once its handle drops, so a
  /// retired tree's voices play out. `master`'s gain is the engine's to put
  /// on kira's main track (its own track stays at 0 dB, so send buses'
  /// returns, which kira mixes into the main track, are under it too)
  pub(crate) fn build(
    tracks: Option<&mut dyn Tracks>,
    plan: &[PlannedBus<'_>],
    layers: impl Fn(&PlannedBus<'_>) -> GainLayers,
  ) -> BusTree {
    let mut buses: Vec<Bus> = plan
      .iter()
      .map(|planned| {
        let def = planned.def;
        Bus {
          name: planned.name.to_string(),
          parent: planned.parent,
          output: if planned.send {
            Output::Send(None)
          } else {
            Output::Track(None)
          },
          effects: def.map_or(0, |def| def.effects.len()),
          tail: def.map_or(Duration::ZERO, |def| {
            def.effects.iter().map(EffectDef::tail).sum()
          }),
          sends: Vec::new(),
          layers: layers(planned),
          cutoff: planned.low_pass.then(|| Cutoff::open(None)),
          applied: 0.0,
          pauses_with_game: planned.pauses_with_game,
          follows_time_scale: planned.follows_time_scale,
          held: false,
          limit: def.and_then(|def| def.limit),
          full_warned: false,
        }
      })
      .collect();
    for (index, planned) in plan.iter().enumerate() {
      let Some(def) = planned.def else {
        continue;
      };
      buses[index].sends = def
        .sends
        .iter()
        .filter_map(|(target, level)| {
          let to = plan.iter().position(|bus| bus.name == target)?;
          plan[to].send.then_some(Route {
            to,
            level: *level,
            applied: *level,
          })
        })
        .collect();
    }
    let mut tree = BusTree { buses };
    for index in 0..tree.buses.len() {
      for route in 0..tree.buses[index].sends.len() {
        let level = tree.buses[index].sends[route].level;
        tree.buses[index].sends[route].applied = tree.route_volume(index, level);
      }
    }
    let Some(tracks) = tracks else {
      return tree;
    };
    // Send tracks first: the tracks that feed them name them when built
    for (index, planned) in plan.iter().enumerate() {
      if !planned.send {
        continue;
      }
      let volume = tree.kira_volume(index);
      let mut builder = SendTrackBuilder::new().volume(Decibels(volume));
      for effect in planned.def.map_or(&[][..], |def| &def.effects) {
        builder.add_built_effect(effect.build());
      }
      let filter = planned
        .low_pass
        .then(|| builder.add_effect(snapshot_filter()));
      let send = tracks.add_send_track(builder).ok();
      if let Some(cutoff) = &mut tree.buses[index].cutoff {
        cutoff.handle = filter;
      }
      if send.is_none() {
        warn_no_room(planned.name);
      }
      tree.buses[index].output = Output::Send(send);
      tree.buses[index].applied = volume;
    }
    for (index, planned) in plan.iter().enumerate() {
      if planned.send {
        continue;
      }
      let volume = if index == 0 {
        0.0
      } else {
        tree.kira_volume(index)
      };
      let mut builder = TrackBuilder::new()
        .volume(Decibels(volume))
        .persist_until_sounds_finish(true);
      for effect in planned.def.map_or(&[][..], |def| &def.effects) {
        builder.add_built_effect(effect.build());
      }
      let filter = planned
        .low_pass
        .then(|| builder.add_effect(snapshot_filter()));
      if let Some(cutoff) = &mut tree.buses[index].cutoff {
        cutoff.handle = filter;
      }
      for route in &tree.buses[index].sends {
        if let Some(to) = tree.send_id(route.to) {
          builder = builder.with_send(to, Decibels(route.applied));
        }
      }
      let track = match planned.parent {
        None => tracks.add_sub_track(builder).ok(),
        Some(parent) => match tree.buses.get_mut(parent).map(|bus| &mut bus.output) {
          Some(Output::Track(Some(parent))) => parent.add_sub_track(builder).ok(),
          _ => None,
        },
      };
      if track.is_none() {
        warn_no_room(planned.name);
      }
      tree.buses[index].output = Output::Track(track);
      if index > 0 {
        tree.buses[index].applied = volume;
      }
    }
    tree
  }

  /// The standard layout
  pub(crate) fn standard(
    tracks: Option<&mut dyn Tracks>,
    layers: impl Fn(&PlannedBus<'_>) -> GainLayers,
  ) -> BusTree {
    BusTree::build(tracks, &plan(&Mixer::default()), layers)
  }

  pub(crate) fn find(&self, name: &str) -> Option<usize> {
    self.buses.iter().position(|bus| bus.name == name)
  }

  pub(crate) fn name(&self, bus: usize) -> &str {
    self.buses.get(bus).map_or("", |bus| bus.name.as_str())
  }

  /// Whether `bus` is a send bus, which plays no voices
  pub(crate) fn is_send(&self, bus: usize) -> bool {
    self
      .buses
      .get(bus)
      .is_some_and(|bus| matches!(bus.output, Output::Send(_)))
  }

  /// The bus voices meant for `name` play on: `name` itself unless it is
  /// a send bus or missing, else the nearest bus above it that plays
  /// voices, else `sfx`
  pub(crate) fn playable(&self, name: &str) -> usize {
    let mut at = name;
    loop {
      if let Some(bus) = self.find(at)
        && !self.is_send(bus)
      {
        return bus;
      }
      match at.rsplit_once('/') {
        Some((parent, _)) => at = parent,
        None => return self.find("sfx").unwrap_or(0),
      }
    }
  }

  /// Whether `bus` is `ancestor` or under it
  pub(crate) fn within(&self, bus: usize, ancestor: usize) -> bool {
    let mut at = Some(bus);
    while let Some(index) = at {
      if index == ancestor {
        return true;
      }
      at = self.buses.get(index).and_then(|bus| bus.parent);
    }
    false
  }

  /// The track of `bus`, if it plays voices and kira made it
  pub(crate) fn track(&self, bus: usize) -> Option<&TrackHandle> {
    match self.buses.get(bus).map(|bus| &bus.output) {
      Some(Output::Track(Some(track))) => Some(track),
      _ => None,
    }
  }

  /// Whether kira has room for one more sound on `bus`'s track. Counting
  /// kira's sounds, not the voices, takes in the ones it has yet to free
  pub(crate) fn has_room(&self, bus: usize) -> bool {
    self
      .track(bus)
      .is_some_and(|track| track.num_sounds() < track.sound_capacity())
  }

  /// Start `data` on `bus`. A bus without a track has no room
  pub(crate) fn play<D: SoundData>(
    &mut self,
    bus: usize,
    data: D,
  ) -> Result<D::Handle, PlaySoundError<D::Error>> {
    match self.buses.get_mut(bus).map(|bus| &mut bus.output) {
      Some(Output::Track(Some(track))) => track.play(data),
      _ => Err(PlaySoundError::SoundLimitReached),
    }
  }

  /// What a voice on `bus` plays at: its gain and every bus's above it. A
  /// send bus's return is at its own gain and `master`'s (kira mixes it into
  /// its main track), after its senders' routes
  pub(crate) fn effective_db(&self, bus: usize) -> f32 {
    if self.is_send(bus) {
      let own = self.buses.get(bus).map_or(0.0, |bus| bus.layers.total());
      let master = self
        .buses
        .first()
        .map_or(0.0, |master| master.layers.total());
      return add_db(own, master);
    }
    let mut total = 0.0;
    let mut at = Some(bus);
    while let Some(index) = at {
      let Some(this) = self.buses.get(index) else {
        break;
      };
      total = add_db(total, this.layers.total());
      at = this.parent;
    }
    total
  }

  /// The volume kira's track for `bus` takes: its gain (`master`'s goes on
  /// kira's main track). A send bus's track takes only its own: what is
  /// above its senders rides on their routes
  pub(crate) fn kira_volume(&self, bus: usize) -> f32 {
    self.buses.get(bus).map_or(0.0, |bus| bus.layers.total())
  }

  /// The volume of a send route from `sender` at `level` dB. kira takes a
  /// send after the sender's own volume and before its parent's, so the
  /// route carries the gains of the buses above the sender, but `master`'s,
  /// which is on kira's main track and so over every return. A return thus
  /// follows its sender's sliders, whichever send bus it reaches
  pub(crate) fn route_volume(&self, sender: usize, level: f32) -> f32 {
    let mut volume = if level <= SILENCE_DB {
      SILENCE_DB
    } else {
      level
    };
    let mut at = self.buses.get(sender).and_then(|bus| bus.parent);
    while let Some(index) = at.filter(|&index| index > 0) {
      let Some(above) = self.buses.get(index) else {
        break;
      };
      volume = add_db(volume, above.layers.total());
      at = above.parent;
    }
    volume
  }

  /// The id of `bus`'s send track, if it is a send bus kira made
  pub(crate) fn send_id(&self, bus: usize) -> Option<SendTrackId> {
    match self.buses.get(bus).map(|bus| &bus.output) {
      Some(Output::Send(Some(send))) => Some(send.id()),
      _ => None,
    }
  }

  /// How long the tree rings after its last sound: its longest chain of
  /// effects, each effect's tail added (`EffectDef::tail`), at most
  /// [`MAX_TAIL`]. A chain runs from a bus voices play on through each bus
  /// above it to `master`, and from any of those into a send bus (kira
  /// mixes a send bus's return beside `master`). Only the tracks kira made
  /// ring
  pub(crate) fn tail(&self) -> Duration {
    let ringing = |bus: &Bus| match bus.output {
      Output::Track(Some(_)) | Output::Send(Some(_)) => bus.tail,
      _ => Duration::ZERO,
    };
    let mut longest = Duration::ZERO;
    for start in 0..self.buses.len() {
      if self.is_send(start) {
        continue;
      }
      // Up from `start`: the effects below and on each bus, then at each
      // bus its sends' returns
      let mut below = Duration::ZERO;
      let mut at = Some(start);
      while let Some(index) = at {
        let Some(bus) = self.buses.get(index) else {
          break;
        };
        let through = below + ringing(bus);
        let send = bus
          .sends
          .iter()
          .filter_map(|route| self.buses.get(route.to))
          .map(ringing)
          .max()
          .unwrap_or_default();
        longest = longest.max(through + send);
        below = through;
        at = bus.parent;
      }
    }
    longest.min(MAX_TAIL)
  }

  /// Fade what the tree gives kira's main track to silence with `tween`:
  /// `master`'s track and each send track (kira mixes send tracks' returns
  /// beside `master`, so `master`'s fade does not reach them)
  pub(crate) fn fade_out(&mut self, tween: Tween) {
    for bus in &mut self.buses {
      match &mut bus.output {
        Output::Track(Some(track)) if bus.parent.is_none() => {
          track.set_volume(Decibels::SILENCE, tween);
        }
        Output::Send(Some(send)) => send.set_volume(Decibels::SILENCE, tween),
        _ => {}
      }
    }
  }

  /// The send tracks, taken out: a retired tree's are kept while its
  /// voices play, so their sends still reach them
  pub(crate) fn take_send_tracks(&mut self) -> Vec<SendTrackHandle> {
    self
      .buses
      .iter_mut()
      .filter_map(|bus| match &mut bus.output {
        Output::Send(send) => send.take(),
        Output::Track(_) => None,
      })
      .collect()
  }

  /// Each bus as it stands
  pub(crate) fn report(&self) -> Vec<BusReport> {
    self
      .buses
      .iter()
      .enumerate()
      .map(|(index, bus)| BusReport {
        name: bus.name.clone(),
        parent: bus.parent.map(|parent| self.name(parent).to_string()),
        send: matches!(bus.output, Output::Send(_)),
        effects: bus.effects,
        sends: bus
          .sends
          .iter()
          .map(|route| (self.name(route.to).to_string(), route.level))
          .collect(),
        send_gains: bus
          .sends
          .iter()
          .map(|route| (self.name(route.to).to_string(), route.applied))
          .collect(),
        base_db: bus.layers.base,
        setting_db: bus.layers.setting,
        snapshot_db: bus.layers.snapshot,
        duck_db: bus.layers.duck,
        focus_db: bus.layers.focus,
        cutoff_hz: bus.cutoff.as_ref().map(|cutoff| cutoff.applied),
        gain_db: bus.layers.total(),
        effective_db: self.effective_db(index),
        pauses_with_game: bus.pauses_with_game,
        follows_time_scale: bus.follows_time_scale,
        held: bus.held,
        limit: bus.limit,
      })
      .collect()
  }
}

/// A bus as it stands, for tests and debug tooling: what the mixer built
/// and the gain it plays at
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct BusReport {
  pub name: String,
  pub parent: Option<String>,
  /// A send bus: a kira send track, fed by other buses' sends, playing no
  /// voices of its own
  pub send: bool,
  /// How many effects its track has
  pub effects: usize,
  /// The send buses it feeds, and at what level in dB
  pub sends: Vec<(String, f32)>,
  /// Each send's route gain in dB, as kira was given it: its level plus the
  /// gains of the buses above this one, but `master`'s
  pub send_gains: Vec<(String, f32)>,
  /// Its gain layers in dB: the mixer's volume (or the game's,
  /// `set_bus_volume`), the player's setting, the snapshot's, the duck's
  /// and, on `master`, the focus loss's
  pub base_db: f32,
  pub setting_db: f32,
  pub snapshot_db: f32,
  pub duck_db: f32,
  pub focus_db: f32,
  /// The cutoff in Hz of the low-pass snapshots close on it (20000 is
  /// open), for a bus a snapshot names a `low_pass` for
  pub cutoff_hz: Option<f32>,
  /// The layers added (silence if any is silent)
  pub gain_db: f32,
  /// What a voice on it plays at: its gain and every bus's above it
  pub effective_db: f32,
  pub pauses_with_game: bool,
  pub follows_time_scale: bool,
  /// Its voices are held where they are: by the game's pause, or by time
  /// standing still (a time scale of 0.01 or less)
  pub held: bool,
  pub limit: Option<u16>,
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use kira::sound::PlaybackState;

  use super::*;
  use crate::core::asset::{
    AssetServer, Assets, AssetsView, AudioClipSettings, ClipMode, MemorySource,
  };
  use crate::core::audio::engine::MIN_FADE;
  use crate::core::audio::{
    AudioClip, AudioEngine, AudioLogEntry, AudioLogKind, AudioView, Sound, SoundClip, Voice,
    VolumeKind, volume_to_db,
  };
  use crate::core::audio::{ClipData, EncodedClip};
  use crate::core::ecs::World;
  use crate::core::testing::{audio_step, audio_world, audio_world_over};

  const FRAME: Duration = Duration::from_nanos(16_666_667);

  /// A mixer with an effect on master, a quieter, limited sfx, a custom bus
  /// with two effects sending to a reverb send bus, and a bus that pauses
  const MIXER: &str = r#"(
    buses: {
      "master": (effects: [Compressor(threshold_db: -12.0, ratio: 4.0)]),
      "sfx": (volume: -6.0, limit: 32),
      "sfx/footsteps": (
        volume: -3.0,
        effects: [HighPass(cutoff: 120.0), Eq(kind: Bell, frequency: 2000.0, gain_db: 3.0)],
        sends: { "sfx/reverb": -12.0 },
        limit: 8,
      ),
      "sfx/reverb": (effects: [Reverb(feedback: 0.8, mix: 1.0)]),
      "music/stingers": (pauses_with_game: true),
    },
    on_focus_loss: Duck(-12.0),
  )"#;

  fn buses(world: &World) -> Vec<BusReport> {
    world.resource::<AudioEngine>().buses()
  }

  /// A mixer made in code needs only the prelude, its snapshots and ducks
  /// included
  #[test]
  fn a_mixer_s_snapshots_and_ducks_are_made_from_the_prelude() {
    let paused = crate::prelude::SnapshotBus {
      volume_db: -12.0,
      low_pass: Some(800.0),
    };
    let mixer = crate::prelude::Mixer {
      snapshots: vec![crate::prelude::SnapshotDef {
        name: "paused".into(),
        blend: 0.3,
        buses: vec![("sfx".into(), paused)],
      }],
      ducks: vec![crate::prelude::DuckDef {
        trigger: "voice".into(),
        target: "music".into(),
        by_db: -9.0,
        attack: 0.1,
        release: 0.5,
      }],
      ..Default::default()
    };
    assert!(mixer.check().is_ok(), "{:?}", mixer.check());
  }

  fn bus(world: &World, name: &str) -> BusReport {
    let buses = buses(world);
    match buses.iter().find(|bus| bus.name == name) {
      Some(bus) => bus.clone(),
      None => panic!("no bus {name:?} in {buses:#?}"),
    }
  }

  fn log(world: &World) -> Vec<AudioLogEntry> {
    world.resource::<AudioEngine>().log().to_vec()
  }

  /// The bus entries the log holds for `name`: gain changes on the bus
  /// itself, not on a voice
  fn bus_gains(world: &World, name: &str) -> Vec<AudioLogEntry> {
    log(world)
      .into_iter()
      .filter(|entry| entry.kind == AudioLogKind::Gain && entry.voice == 0 && entry.bus == name)
      .collect()
  }

  fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 0.01
  }

  /// An audio world whose registry holds `text` at [`MIXER_PATH`], the
  /// mixer loaded and applied
  fn world_with_mixer(text: &str) -> (World, MemorySource) {
    let files = MemorySource::new();
    files.insert(MIXER_PATH, text);
    let mut world = audio_world_over(files.clone());
    let mixer = match AssetsView::new(&mut world).load_now::<Mixer>(MIXER_PATH) {
      Ok(mixer) => mixer,
      Err(error) => panic!("the mixer loads: {}", error.report()),
    };
    world.resource_mut::<AudioEngine>().set_mixer(Some(mixer));
    audio_step(&mut world, FRAME);
    (world, files)
  }

  fn clip(world: &mut World, seconds: f32) -> Handle<AudioClip> {
    let frames = vec![[0.1, 0.1]; (48_000.0 * seconds) as usize];
    world
      .resource_mut::<Assets<AudioClip>>()
      .add(AudioClip::from_frames(48_000, &frames).unwrap())
  }

  fn looping(world: &mut World, clip: Handle<AudioClip>, bus: &str) -> Handle<Sound> {
    world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      bus: bus.to_string(),
      looping: true,
      ..Default::default()
    })
  }

  /// A mixer loaded at once is built before what is asked of the engine
  /// next, in the same frame: a one-shot played as a scene enters plays on
  /// the bus the mixer adds (it could never move there later), `voices_on`
  /// counts it, a snapshot and a bus volume reach it, and a mixer set in
  /// code is built as it is set
  #[test]
  fn a_one_shot_played_as_the_mixer_loads_plays_on_its_bus() {
    let files = MemorySource::new();
    files.insert(
      MIXER_PATH,
      r#"(
        buses: { "sfx/steps-at-once": (volume: -3.0) },
        snapshots: { "hushed-at-once": (blend: 0.0, buses: { "sfx": (volume: -12.0) }) },
      )"#,
    );
    let mut world = audio_world_over(files);
    // As the engine starts: the mixer queued to load
    let queued = AssetsView::new(&mut world).load::<Mixer>(MIXER_PATH);
    world.resource_mut::<AudioEngine>().set_mixer(Some(queued));
    let step_clip = clip(&mut world, 1.0);
    let step = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(step_clip)],
      bus: "sfx/steps-at-once".to_string(),
      ..Default::default()
    });
    // A scene's `on_enter` loads it at once and plays on its bus
    if let Err(error) = AssetsView::new(&mut world).load_now::<Mixer>(MIXER_PATH) {
      panic!("the mixer loads: {}", error.report());
    }
    let voice = AudioView::new(&mut world).play(&step);
    let start = log(&world)
      .into_iter()
      .find(|entry| entry.kind == AudioLogKind::Start && entry.voice == voice.id());
    assert_eq!(
      start.map(|entry| entry.bus).as_deref(),
      Some("sfx/steps-at-once")
    );
    assert_eq!(AudioView::new(&mut world).voices_on("sfx/steps-at-once"), 1);
    AudioView::new(&mut world).set_bus_volume("sfx/steps-at-once", -9.0);
    assert_eq!(bus(&world, "sfx/steps-at-once").base_db, -9.0);
    AudioView::new(&mut world).snapshot("hushed-at-once");
    assert_eq!(bus(&world, "sfx").snapshot_db, -12.0);
    // A mixer made in code is built as it is set
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![BusDef::new("ui/menu-at-once")],
      ..Default::default()
    });
    assert!(
      buses(&world)
        .iter()
        .any(|bus| bus.name == "ui/menu-at-once"),
      "{:#?}",
      buses(&world)
    );
  }

  #[test]
  fn the_standard_layout_exists_without_a_mixer_file() {
    let files = MemorySource::new();
    let mut world = audio_world_over(files.clone());
    let server = world.resource::<AssetServer>().clone();
    assert!(
      default_mixer(&server).is_none(),
      "no file: nothing loads, so nothing warns"
    );
    audio_step(&mut world, FRAME);
    let names: Vec<String> = buses(&world).into_iter().map(|bus| bus.name).collect();
    assert_eq!(names, ["master", "music", "sfx", "ui", "voice", "ambience"]);
    for name in ["music", "sfx", "ui", "voice", "ambience"] {
      let bus = bus(&world, name);
      assert_eq!(bus.parent.as_deref(), Some("master"), "{name}");
      assert!(
        !bus.send && bus.effects == 0 && bus.sends.is_empty(),
        "{bus:?}"
      );
      assert_eq!(bus.limit, None);
    }
    let pauses: Vec<bool> = buses(&world).iter().map(|b| b.pauses_with_game).collect();
    assert_eq!(pauses, [false, false, true, false, true, true]);
    let follows: Vec<bool> = buses(&world).iter().map(|b| b.follows_time_scale).collect();
    assert_eq!(follows, [false, false, true, false, false, true]);
    // With the file, the engine loads it
    files.insert(MIXER_PATH, "()");
    assert!(default_mixer(&server).is_some());
  }

  #[test]
  fn a_custom_bus_takes_its_parents_setting() {
    let (mut world, _files) = world_with_mixer(MIXER);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.5);
    let footsteps = bus(&world, "sfx/footsteps");
    assert_eq!(footsteps.parent.as_deref(), Some("sfx"));
    assert_eq!(footsteps.setting_db, 0.0, "a game's bus has no setting");
    assert_eq!(footsteps.gain_db, -3.0);
    let sfx = -6.0 + volume_to_db(0.5);
    let master = volume_to_db(VolumeKind::Master.default_value());
    assert!(
      close(footsteps.effective_db, -3.0 + sfx + master),
      "{footsteps:?}"
    );
    // Its send to the reverb is taken after its own volume, so the route
    // carries the gains above it: sfx's (master's is on the main track)
    assert_eq!(footsteps.send_gains.len(), 1);
    assert!(
      close(footsteps.send_gains[0].1, -12.0 + sfx),
      "{footsteps:?}"
    );
    // And a sound on the custom bus plays there
    let clip = clip(&mut world, 1.0);
    let sound = looping(&mut world, clip, "sfx/footsteps");
    let voice = AudioView::new(&mut world).play(&sound);
    let start = log(&world)
      .into_iter()
      .find(|entry| entry.kind == AudioLogKind::Start && entry.voice == voice.id());
    assert_eq!(
      start.map(|entry| entry.bus).as_deref(),
      Some("sfx/footsteps")
    );
  }

  #[test]
  fn a_setting_changes_a_playing_voices_bus() {
    let mut world = audio_world();
    let clip = clip(&mut world, 1.0);
    let sound = looping(&mut world, clip, "sfx");
    let voice = AudioView::new(&mut world).play(&sound);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.5);
    audio_step(&mut world, FRAME);
    let gains = bus_gains(&world, "sfx");
    let Some(gain) = gains.last() else {
      panic!("no gain entry for sfx in {:#?}", log(&world));
    };
    assert!(close(gain.gain_db, -12.04), "{gain:?}");
    assert_eq!(gain.fade, MIN_FADE);
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(
      world
        .resource::<AudioEngine>()
        .volume_setting(VolumeKind::Sfx),
      0.5
    );
  }

  /// Review focus 4: settings at their ends, and corrupted ones
  #[test]
  fn volume_settings_at_their_ends_and_out_of_range() {
    let mut world = audio_world();
    let cases = [
      (0.0, 0.0, -60.0),
      (1.0, 1.0, 0.0),
      (f32::NAN, 1.0, 0.0),
      (-1.0, 0.0, -60.0),
      (5.0, 1.0, 0.0),
      (f32::INFINITY, 1.0, 0.0),
      (0.5, 0.5, -12.04),
    ];
    for (value, setting, db) in cases {
      let mut engine = world.resource_mut::<AudioEngine>();
      engine.set_volume_setting(VolumeKind::Sfx, 0.25);
      engine.set_volume_setting(VolumeKind::Sfx, value);
      assert_eq!(engine.volume_setting(VolumeKind::Sfx), setting, "{value}");
      let sfx = bus(&world, "sfx");
      assert!(close(sfx.gain_db, db), "{value}: {sfx:?}");
      let logged = bus_gains(&world, "sfx").pop().map(|entry| entry.gain_db);
      assert!(logged.is_some_and(|gain| close(gain, db)), "{value}");
    }
    // 0 is kira's silence exactly
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.0);
    assert_eq!(bus(&world, "sfx").gain_db, -60.0);
    // NaN is each kind's own default: master's is 1, the mix as designed
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Master, 0.5);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Master, f32::NAN);
    assert_eq!(
      world
        .resource::<AudioEngine>()
        .volume_setting(VolumeKind::Master),
      1.0
    );
    assert!(close(bus(&world, "master").gain_db, 0.0));
  }

  #[test]
  fn gain_layers_add_in_db() {
    let (mut world, _files) = world_with_mixer(MIXER);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.5);
    let sfx = bus(&world, "sfx");
    assert_eq!(
      (sfx.base_db, sfx.snapshot_db, sfx.duck_db),
      (-6.0, 0.0, 0.0)
    );
    assert!(close(sfx.setting_db, -12.04));
    assert!(close(sfx.gain_db, -18.04), "{sfx:?}");
    world
      .resource_mut::<AudioEngine>()
      .set_bus_layers("sfx", -3.0, -9.0);
    assert!(close(bus(&world, "sfx").gain_db, -30.04));
    let logged = bus_gains(&world, "sfx").pop().map(|entry| entry.gain_db);
    assert!(logged.is_some_and(|gain| close(gain, -30.04)));
    // The game's volume replaces the mixer's, under the other layers
    AudioView::new(&mut world).set_bus_volume("sfx", 6.0);
    assert!(close(bus(&world, "sfx").gain_db, 6.0 - 12.04 - 3.0 - 9.0));
    // A silent layer silences the bus, whatever the others add
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.0);
    assert_eq!(bus(&world, "sfx").gain_db, -60.0);
    // A custom bus's own volume
    AudioView::new(&mut world).set_bus_volume("sfx/footsteps", -10.0);
    assert_eq!(bus(&world, "sfx/footsteps").gain_db, -10.0);
  }

  #[test]
  fn effects_and_sends_are_built_from_the_file() {
    let (mut world, _files) = world_with_mixer(MIXER);
    assert_eq!(bus(&world, "master").effects, 1);
    let footsteps = bus(&world, "sfx/footsteps");
    assert_eq!(footsteps.effects, 2);
    assert_eq!(footsteps.sends, [("sfx/reverb".to_string(), -12.0)]);
    assert_eq!(footsteps.limit, Some(8));
    let reverb = bus(&world, "sfx/reverb");
    assert!(reverb.send && reverb.effects == 1, "{reverb:?}");
    assert_eq!(bus(&world, "sfx").limit, Some(32));
    let stingers = bus(&world, "music/stingers");
    assert!(stingers.pauses_with_game, "set in the file");
    assert!(!stingers.follows_time_scale, "music's default");
    let footsteps_follow = bus(&world, "sfx/footsteps");
    assert!(footsteps_follow.pauses_with_game && footsteps_follow.follows_time_scale);
    // In kira: one master under the main track (the standard tree's went
    // once it was empty) and one send track
    audio_step(&mut world, FRAME);
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (1, 1));
  }

  #[test]
  fn a_mixer_reload_restarts_loops_on_the_new_tree_and_lets_one_shots_finish() {
    let (mut world, files) = world_with_mixer(MIXER);
    let long = clip(&mut world, 2.0);
    let looped_sound = looping(&mut world, long, "sfx");
    // Long enough to outlast the reload however slowly the machine runs
    let short = clip(&mut world, 2.0);
    let (looped, shot) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&looped_sound), audio.play(&short))
    };
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    let before = log(&world).len();
    let position = |world: &World| {
      world
        .resource::<AudioEngine>()
        .voice_position(looped.id())
        .unwrap_or_default()
    };
    let at_reload = position(&world);
    assert!(at_reload > 0.04, "it has played: {at_reload}");
    files.insert(MIXER_PATH, MIXER.replace("volume: -6.0", "volume: -9.0"));
    world.resource::<AssetServer>().reload(MIXER_PATH);
    let start = Instant::now();
    let mut stepped = Duration::ZERO;
    while bus(&world, "sfx").base_db != -9.0 {
      assert!(start.elapsed() < Duration::from_secs(5), "the reload lands");
      audio_step(&mut world, Duration::from_millis(1));
      stepped += Duration::from_millis(1);
      std::thread::sleep(Duration::from_millis(1));
    }
    // The loop carries on from where it was, not from its start (to within
    // the time stepped while the reload loaded, and a chunk)
    let restarted = position(&world);
    let chunk = 128.0 / 48_000.0;
    assert!(
      restarted >= at_reload - chunk && restarted <= at_reload + stepped.as_secs_f64() + chunk,
      "{at_reload} then {restarted}"
    );
    let after: Vec<AudioLogEntry> = log(&world).split_off(before);
    let of = |voice: &Voice| -> Vec<(AudioLogKind, Duration)> {
      after
        .iter()
        .filter(|entry| entry.voice == voice.id())
        .map(|entry| (entry.kind, entry.fade))
        .collect()
    };
    assert_eq!(
      of(&looped),
      [
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ],
      "the loop crossfades onto the new tree"
    );
    assert_eq!(of(&shot), [], "the one-shot carries on where it was");
    assert!(looped.is_playing() && shot.is_playing());
    let engine = world.resource::<AudioEngine>();
    assert_eq!(
      engine.sounds_on("sfx"),
      1,
      "the new sfx track holds the loop"
    );
    assert_eq!(engine.voices_on("sfx"), 2);
    // The one-shot plays out on the retired tree, which rings out its
    // reverb (fed by footsteps' send: 1.525 s) and then goes
    for _ in 0..200 {
      if shot.is_stopped() {
        break;
      }
      audio_step(&mut world, FRAME);
    }
    assert!(shot.is_stopped(), "{shot:?}");
    audio_step(&mut world, FRAME);
    assert!(looped.is_playing(), "{looped:?}");
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (2, 2));
    for _ in 0..95 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(world.resource::<AudioEngine>().kira_tracks(), (1, 1));
  }

  #[test]
  fn a_paused_loop_stays_paused_across_a_mixer_reload() {
    let (mut world, files) = world_with_mixer(MIXER);
    let long = clip(&mut world, 2.0);
    let sound = looping(&mut world, long, "sfx/footsteps");
    let voice = AudioView::new(&mut world).play(&sound);
    voice.pause();
    audio_step(&mut world, FRAME);
    files.insert(MIXER_PATH, MIXER.replace("volume: -3.0", "volume: -4.0"));
    world.resource::<AssetServer>().reload(MIXER_PATH);
    let start = Instant::now();
    while bus(&world, "sfx/footsteps").base_db != -4.0 {
      assert!(start.elapsed() < Duration::from_secs(5), "the reload lands");
      audio_step(&mut world, Duration::from_millis(1));
      std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    assert!(!voice.is_playing() && !voice.is_stopped(), "{voice:?}");
    let engine = world.resource::<AudioEngine>();
    assert_eq!(engine.sounds_on("sfx/footsteps"), 1);
    assert_eq!(
      engine.voice_playback(voice.id()),
      Some(PlaybackState::Paused),
      "kira's new sound is paused too"
    );
    let restarted = engine
      .log()
      .iter()
      .rev()
      .find(|entry| entry.voice == voice.id() && entry.kind == AudioLogKind::Start)
      .map(|entry| entry.fade);
    assert_eq!(
      restarted,
      Some(Duration::ZERO),
      "it starts silent and paused"
    );
    voice.resume();
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing(), "{voice:?}");
  }

  /// The game's pause holds each voice, so it reaches a one-shot still
  /// finishing on the buses a mixer reload retired
  #[test]
  fn a_game_pause_reaches_a_one_shot_finishing_on_a_retired_tree() {
    let (mut world, files) = world_with_mixer(MIXER);
    let long = clip(&mut world, 2.0);
    let shot = AudioView::new(&mut world).play(&long);
    audio_step(&mut world, FRAME);
    files.insert(MIXER_PATH, MIXER.replace("volume: -6.0", "volume: -9.0"));
    world.resource::<AssetServer>().reload(MIXER_PATH);
    let start = Instant::now();
    while bus(&world, "sfx").base_db != -9.0 {
      assert!(start.elapsed() < Duration::from_secs(5), "the reload lands");
      audio_step(&mut world, Duration::from_millis(1));
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
      world.resource::<AudioEngine>().sounds_on("sfx"),
      0,
      "it plays on the retired tree"
    );
    world.resource_mut::<crate::core::time::Time>().game.pause();
    audio_step(&mut world, FRAME);
    let engine = world.resource::<AudioEngine>();
    assert_eq!(
      engine.voice_playback(shot.id()),
      Some(PlaybackState::Paused)
    );
    let at_pause = engine.voice_position(shot.id()).unwrap_or_default();
    for _ in 0..5 {
      audio_step(&mut world, FRAME);
    }
    let held = world
      .resource::<AudioEngine>()
      .voice_position(shot.id())
      .unwrap_or_default();
    assert_eq!(held, at_pause);
  }

  #[test]
  fn a_sound_on_a_send_bus_plays_on_its_parent_with_one_warning() {
    let text = r#"(buses: {
      "ambience/caves": (sends: { "ambience/cave-echo-unique": -6.0 }),
      "ambience/cave-echo-unique": (effects: [Delay(seconds: 0.3)]),
    })"#;
    let (mut world, _files) = world_with_mixer(text);
    let clip = clip(&mut world, 1.0);
    let sound = looping(&mut world, clip, "ambience/cave-echo-unique");
    for _ in 0..2 {
      let voice = AudioView::new(&mut world).play(&sound);
      let start = log(&world)
        .into_iter()
        .find(|entry| entry.kind == AudioLogKind::Start && entry.voice == voice.id());
      assert_eq!(start.map(|entry| entry.bus).as_deref(), Some("ambience"));
      audio_step(&mut world, FRAME);
    }
    let warned =
      warnings(|message| message.contains("\"ambience/cave-echo-unique\" is a send bus"));
    assert_eq!(warned, 1);
  }

  #[test]
  fn a_bus_the_mixer_adds_is_not_warned_about_before_it_lands() {
    let files = MemorySource::new();
    files.insert(
      MIXER_PATH,
      r#"(buses: { "sfx/landing-unique": (volume: -2.0) })"#,
    );
    let mut world = audio_world_over(files);
    let mixer = AssetsView::new(&mut world).load::<Mixer>(MIXER_PATH);
    world.resource_mut::<AudioEngine>().set_mixer(Some(mixer));
    let clip = clip(&mut world, 1.0);
    let sound = looping(&mut world, clip, "sfx/landing-unique");
    // Before it lands: the nearest bus, a game volume kept for later, and
    // no warning
    let early = AudioView::new(&mut world).play(&sound);
    AudioView::new(&mut world).set_bus_volume("sfx/landing-unique", -9.0);
    let start = Instant::now();
    while !buses(&world)
      .iter()
      .any(|bus| bus.name == "sfx/landing-unique")
    {
      assert!(start.elapsed() < Duration::from_secs(5), "the mixer lands");
      audio_step(&mut world, Duration::from_millis(1));
      std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(bus(&world, "sfx/landing-unique").base_db, -9.0);
    let late = AudioView::new(&mut world).play(&sound);
    let bus_of = |voice: &Voice| {
      log(&world)
        .into_iter()
        .filter(|entry| entry.kind == AudioLogKind::Start && entry.voice == voice.id())
        .map(|entry| entry.bus)
        .collect::<Vec<_>>()
    };
    assert_eq!(
      bus_of(&early),
      ["sfx", "sfx/landing-unique"],
      "it restarted on the bus its definition names"
    );
    assert_eq!(bus_of(&late), ["sfx/landing-unique"]);
    assert_eq!(
      warnings(|message| message.contains("landing-unique")),
      0,
      "nothing was warned about"
    );
  }

  /// Review I1: kira takes a send after the sender's own volume, before its
  /// parent's, so a return follows the sender's buses, not the send bus's
  #[test]
  fn a_send_return_follows_its_senders_buses() {
    let text = r#"(buses: {
      "voice/npc": (sends: { "sfx/reverb": -6.0 }),
      "sfx/steps": (sends: { "sfx/reverb": -3.0 }),
      "sfx/reverb": (volume: -2.0, effects: [Reverb(mix: 1.0)]),
    })"#;
    let (mut world, _files) = world_with_mixer(text);
    let send_gain = |world: &World, from: &str| {
      let sender = bus(world, from);
      match sender.send_gains.as_slice() {
        [(to, gain)] if to == "sfx/reverb" => *gain,
        other => panic!("{from}: {other:?}"),
      }
    };
    // The voice slider reaches the NPC's reverb
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Voice, 0.5);
    assert!(close(send_gain(&world, "voice/npc"), -6.0 - 12.04));
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Voice, 0.0);
    assert_eq!(send_gain(&world, "voice/npc"), -60.0, "silent");
    // The sfx slider does not scale it: the reverb's track has only its
    // own gain
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Voice, 1.0);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Sfx, 0.5);
    assert!(close(send_gain(&world, "voice/npc"), -6.0));
    let reverb = bus(&world, "sfx/reverb");
    assert_eq!(reverb.gain_db, -2.0);
    let master = volume_to_db(VolumeKind::Master.default_value());
    assert!(close(reverb.effective_db, -2.0 + master), "{reverb:?}");
    // A sibling's send still follows the parent they share
    assert!(close(send_gain(&world, "sfx/steps"), -3.0 - 12.04));
    // The game's volume on a bus above the sender reaches its route too
    AudioView::new(&mut world).set_bus_volume("sfx", -4.0);
    assert!(close(send_gain(&world, "sfx/steps"), -3.0 - 4.0 - 12.04));
    // A bus may send to a send bus under itself: its own volume is in the
    // signal once, and nothing above it is added twice
    let text = r#"(buses: {
      "sfx": (volume: -5.0, sends: { "sfx/room": -6.0 }),
      "sfx/room": (),
    })"#;
    let (world, _files) = world_with_mixer(text);
    let sfx = bus(&world, "sfx");
    assert_eq!(sfx.send_gains, [("sfx/room".to_string(), -6.0)], "{sfx:?}");
  }

  /// Review M5: a mixer made in code, for a game with no file (the web)
  #[test]
  fn a_mixer_made_in_code_rebuilds_the_buses_as_a_reload_would() {
    let mut world = audio_world();
    let long = clip(&mut world, 2.0);
    let sound = looping(&mut world, long, "sfx");
    let short = clip(&mut world, 2.0);
    let (looped, shot) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&sound), audio.play(&short))
    };
    audio_step(&mut world, FRAME);
    let before = log(&world).len();
    let sfx = BusDef {
      volume_db: -2.0,
      ..BusDef::new("sfx")
    };
    let coded = BusDef {
      volume_db: -4.0,
      ..BusDef::new("sfx/coded")
    };
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![sfx, coded],
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    assert_eq!(bus(&world, "sfx").base_db, -2.0);
    assert_eq!(bus(&world, "sfx/coded").base_db, -4.0);
    let after = log(&world).split_off(before);
    let of = |voice: &Voice| -> Vec<(AudioLogKind, Duration)> {
      after
        .iter()
        .filter(|entry| entry.voice == voice.id())
        .map(|entry| (entry.kind, entry.fade))
        .collect()
    };
    assert_eq!(
      of(&looped),
      [
        (AudioLogKind::Stop, MIN_FADE),
        (AudioLogKind::Start, MIN_FADE)
      ]
    );
    assert_eq!(of(&shot), []);
    // One that fails its check is warned about, and changes nothing
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![
        BusDef::new("sfx/coded-twice-unique"),
        BusDef::new("sfx/coded-twice-unique"),
      ],
      ..Default::default()
    });
    audio_step(&mut world, FRAME);
    assert_eq!(bus(&world, "sfx/coded").base_db, -4.0);
    assert_eq!(
      warnings(|message| message.contains("\"sfx/coded-twice-unique\" twice")),
      1
    );
  }

  /// Review M3, with streams that open on their decoder threads: a loop
  /// whose clip's bytes go bad after it started is switched to them, as a
  /// changed clip's loops are, fails there and ends, warned about once. The
  /// rebuilds that follow have nothing left to move, and say nothing more
  #[test]
  fn a_loop_whose_clip_goes_bad_ends_warned_once_and_rebuilds_leave_it() {
    let tone = match std::fs::read(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/fixtures/audio/tone.wav"
    )) {
      Ok(bytes) => bytes,
      Err(error) => panic!("tone.wav: {error}"),
    };
    let (mut world, files) = world_with_mixer(MIXER);
    files.insert("stuck-loop-unique.wav", tone);
    let settings = AudioClipSettings {
      mode: ClipMode::Stream,
    };
    let clip = match AssetsView::new(&mut world)
      .load_now_with::<AudioClip, _>("stuck-loop-unique.wav", settings)
    {
      Ok(clip) => clip,
      Err(error) => panic!("{}", error.report()),
    };
    let sound = looping(&mut world, clip.clone(), "sfx");
    let voice = AudioView::new(&mut world).play(&sound);
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing(), "{voice:?}");
    if let Some(clip) = world.resource_mut::<Assets<AudioClip>>().get_mut(&clip) {
      let garbage = crate::core::audio::ClipBytes::from(&b"not audio at all"[..]);
      let encoded = EncodedClip {
        bytes: garbage,
        frames: 22_050,
      };
      *clip = AudioClip::new(ClipData::Encoded(encoded), Duration::from_secs(1), 22_050);
    }
    let start = Instant::now();
    while !voice.is_stopped() {
      assert!(start.elapsed() < Duration::from_secs(5), "{voice:?} ends");
      audio_step(&mut world, Duration::from_millis(1));
      std::thread::sleep(Duration::from_millis(1));
    }
    for volume in ["-9.0", "-10.0"] {
      let text = MIXER.replace("volume: -6.0", &format!("volume: {volume}"));
      files.insert(MIXER_PATH, text);
      world.resource::<AssetServer>().reload(MIXER_PATH);
      let wanted: f32 = volume.parse().unwrap_or_default();
      let start = Instant::now();
      while bus(&world, "sfx").base_db != wanted {
        assert!(start.elapsed() < Duration::from_secs(5), "the reload lands");
        audio_step(&mut world, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(1));
      }
    }
    assert!(voice.is_stopped(), "{voice:?}");
    let named = |what: &str| {
      warnings(|message| message.starts_with("stuck-loop-unique.wav:") && message.contains(what))
    };
    assert_eq!(named("could not be decoded"), 1);
    assert_eq!(named("rebuilt buses"), 0);
  }

  /// Snapshots and a duck over the standard buses, and a send
  const SNAPSHOTS: &str = r#"(
    buses: {
      "sfx/footsteps": (sends: { "sfx/reverb": -12.0 }),
      "sfx/reverb": (effects: [Reverb(feedback: 0.8, mix: 1.0)]),
    },
    snapshots: {
      "pause": (blend: 0.3, buses: {
        "music": (volume: -6.0, low_pass: 800.0),
        "sfx": (volume: -6.0),
      }),
      "dive": (blend: 0.2, buses: { "music": (volume: -12.0, low_pass: 400.0) }),
    },
    ducks: [(trigger: "voice", target: "music", by: -9.0, attack: 0.1, release: 0.6)],
  )"#;

  /// Step `seconds` of frames
  fn run(world: &mut World, seconds: f32) {
    let frames = (seconds / FRAME.as_secs_f32()).round() as usize;
    for _ in 0..frames {
      audio_step(world, FRAME);
    }
  }

  #[test]
  fn a_snapshot_blends_to_its_targets_over_its_blend() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    assert_eq!(bus(&world, "music").cutoff_hz, Some(20_000.0));
    assert_eq!(bus(&world, "ui").cutoff_hz, None, "no snapshot filters ui");
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.15);
    let half = bus(&world, "music");
    assert!(
      half.snapshot_db < -1.0 && half.snapshot_db > -5.0,
      "{half:?}"
    );
    assert!(half.cutoff_hz.is_some_and(|hz| hz < 20_000.0 && hz > 800.0));
    run(&mut world, 0.2);
    let music = bus(&world, "music");
    assert!((music.snapshot_db + 6.0).abs() < 0.1, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(800.0));
    assert!((bus(&world, "sfx").snapshot_db + 6.0).abs() < 0.1);
    assert_eq!(bus(&world, "voice").snapshot_db, 0.0);
  }

  #[test]
  fn clear_snapshot_blends_back() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.4);
    AudioView::new(&mut world).clear_snapshot();
    run(&mut world, 0.15);
    let half = bus(&world, "music");
    assert!(
      half.snapshot_db < -1.0 && half.snapshot_db > -5.0,
      "{half:?}"
    );
    run(&mut world, 0.2);
    let music = bus(&world, "music");
    assert!(music.snapshot_db.abs() < 0.1, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(20_000.0));
  }

  #[test]
  fn a_new_snapshot_blends_from_where_the_mix_is() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.15);
    let before = bus(&world, "music").snapshot_db;
    AudioView::new(&mut world).snapshot("dive");
    audio_step(&mut world, FRAME);
    let after = bus(&world, "music").snapshot_db;
    assert!(
      (after - before).abs() < 1.5 && before < -1.0,
      "no jump: {before} then {after}"
    );
    run(&mut world, 0.3);
    let music = bus(&world, "music");
    assert!((music.snapshot_db + 12.0).abs() < 0.1, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(400.0));
    // sfx is not in "dive": it blended back to 0
    assert!(bus(&world, "sfx").snapshot_db.abs() < 0.1);
  }

  #[test]
  fn a_slider_moved_under_a_snapshot_is_heard_at_once() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.4);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Music, 0.5);
    let music = bus(&world, "music");
    assert!(
      close(music.gain_db, -6.0 + volume_to_db(0.5)),
      "snapshot and slider add: {music:?}"
    );
  }

  #[test]
  fn a_snapshot_reaches_a_send_route_through_the_buses_above_the_sender() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.4);
    let footsteps = bus(&world, "sfx/footsteps");
    assert!(
      (footsteps.send_gains[0].1 + 18.0).abs() < 0.1,
      "-12 plus sfx's -6: {footsteps:?}"
    );
  }

  #[test]
  fn music_ducks_while_a_voice_plays_and_releases_after() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    let clip = clip(&mut world, 0.5);
    let line = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(clip)],
      bus: "voice".to_string(),
      ..Default::default()
    });
    run(&mut world, 0.1);
    assert_eq!(bus(&world, "music").duck_db, 0.0);
    let voice = AudioView::new(&mut world).play(&line);
    run(&mut world, 0.05);
    let half = bus(&world, "music").duck_db;
    assert!(half < -2.0 && half > -8.0, "mid attack: {half}");
    run(&mut world, 0.07);
    let music = bus(&world, "music");
    assert!((music.duck_db + 9.0).abs() < 0.01, "{music:?}");
    assert!(close(music.gain_db, -9.0), "{music:?}");
    // While it plays it stays down; after it ends, it releases over 0.6 s
    run(&mut world, 0.3);
    assert!(voice.is_playing());
    assert!((bus(&world, "music").duck_db + 9.0).abs() < 0.01);
    run(&mut world, 0.2);
    assert!(!voice.is_playing());
    run(&mut world, 0.2);
    let mid = bus(&world, "music").duck_db;
    assert!(mid < -1.0 && mid > -8.9, "mid release: {mid}");
    run(&mut world, 0.8);
    assert_eq!(bus(&world, "music").duck_db, 0.0);
  }

  /// A duck on a bus above a sender reaches its send route, as a snapshot
  /// does: the reverb of the footsteps dips with them
  #[test]
  fn a_duck_reaches_a_send_route_through_the_buses_above_the_sender() {
    let text = r#"(
      buses: {
        "sfx/footsteps": (sends: { "sfx/reverb": -12.0 }),
        "sfx/reverb": (effects: [Reverb(mix: 1.0)]),
      },
      ducks: [(trigger: "voice", target: "sfx", by: -9.0, attack: 0.1, release: 0.6)],
    )"#;
    let (mut world, _files) = world_with_mixer(text);
    let clip = clip(&mut world, 2.0);
    let line = looping(&mut world, clip, "voice");
    let route = |world: &World| bus(world, "sfx/footsteps").send_gains[0].1;
    assert!(close(route(&world), -12.0));
    AudioView::new(&mut world).play(&line);
    run(&mut world, 0.2);
    assert!((bus(&world, "sfx").duck_db + 9.0).abs() < 0.01);
    assert!(
      close(route(&world), -21.0),
      "-12 plus sfx's -9 dip: {:?}",
      bus(&world, "sfx/footsteps")
    );
  }

  /// A snapshot with a blend of 0 lands in the call that asks for it, its
  /// change smoothed over the 10 ms every change takes, and clearing it the
  /// same; the next update changes nothing more
  #[test]
  fn a_blend_of_zero_lands_in_its_call() {
    let text = r#"(snapshots: {
      "cut": (blend: 0.0, buses: { "music": (volume: -12.0, low_pass: 500.0) }),
    })"#;
    let (mut world, _files) = world_with_mixer(text);
    AudioView::new(&mut world).snapshot("cut");
    let music = bus(&world, "music");
    assert_eq!(music.snapshot_db, -12.0, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(500.0), "{music:?}");
    let fades = |world: &World| -> Vec<Duration> {
      bus_gains(world, "music")
        .iter()
        .map(|entry| entry.fade)
        .collect()
    };
    assert_eq!(fades(&world), [MIN_FADE]);
    audio_step(&mut world, FRAME);
    assert_eq!(fades(&world), [MIN_FADE], "nothing more to step");
    AudioView::new(&mut world).clear_snapshot();
    let music = bus(&world, "music");
    assert_eq!(music.snapshot_db, 0.0, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(20_000.0), "{music:?}");
    assert_eq!(fades(&world), [MIN_FADE, MIN_FADE]);
  }

  /// Two rules ducking one bus add their dips while both trigger, and the
  /// bus comes back as each lets go
  #[test]
  fn two_ducks_on_one_target_add_their_dips() {
    let text = r#"(ducks: [
      (trigger: "voice", target: "music", by: -9.0, attack: 0.1, release: 0.2),
      (trigger: "sfx", target: "music", by: -6.0, attack: 0.1, release: 0.2),
    ])"#;
    let (mut world, _files) = world_with_mixer(text);
    let line = clip(&mut world, 2.0);
    let line = looping(&mut world, line, "voice");
    let shot = clip(&mut world, 2.0);
    let shot = looping(&mut world, shot, "sfx");
    let (line, shot) = {
      let mut audio = AudioView::new(&mut world);
      (audio.play(&line), audio.play(&shot))
    };
    run(&mut world, 0.2);
    let music = bus(&world, "music");
    assert!((music.duck_db + 15.0).abs() < 0.01, "-9 and -6: {music:?}");
    assert!(close(music.gain_db, -15.0), "{music:?}");
    line.stop(Duration::ZERO);
    run(&mut world, 0.4);
    assert!(
      (bus(&world, "music").duck_db + 6.0).abs() < 0.01,
      "the sfx rule's"
    );
    shot.stop(Duration::ZERO);
    run(&mut world, 0.4);
    assert_eq!(bus(&world, "music").duck_db, 0.0);
  }

  /// A bus that sends to a send bus under itself is heard with its own
  /// volume in the return once (kira takes a send after the sender's own
  /// volume) and the buses above it once (on the route, not again on the
  /// return): measured on kira's main track, the return adds the send's
  /// level to what the bus plays directly
  #[test]
  fn a_send_into_the_sender_s_own_subtree_is_heard_once() {
    let heard = |text: &str| {
      let (mut world, meter) = crate::core::testing::metered_audio_world();
      let mixer = match Mixer::parse(text) {
        Ok(mixer) => mixer,
        Err(error) => panic!("{error}"),
      };
      AudioView::new(&mut world).set_mixer(mixer);
      let clip = clip(&mut world, 2.0);
      let sound = looping(&mut world, clip, "sfx/steps");
      AudioView::new(&mut world).play(&sound);
      run(&mut world, 0.2);
      meter.last().left
    };
    let direct = heard(
      r#"(buses: {
        "sfx": (volume: -4.0),
        "sfx/steps": (volume: -5.0),
      })"#,
    );
    let with_room = heard(
      r#"(buses: {
        "sfx": (volume: -4.0),
        "sfx/steps": (volume: -5.0, sends: { "sfx/steps/room": -6.0 }),
        "sfx/steps/room": (),
      })"#,
    );
    assert!(direct > 0.01, "heard: {direct}");
    // The direct path is -9 dB; the return -5 (the sender's own), then -6
    // and sfx's -4 on the route: -15 dB, 6 dB under the direct path
    let ratio = with_room / direct;
    let expected = 1.0 + 10f32.powf(-6.0 / 20.0);
    assert!(
      (ratio - expected).abs() < 0.01,
      "{with_room} against {direct}: {ratio}, not {expected}"
    );
  }

  #[test]
  fn the_snapshot_and_a_duck_survive_a_rebuild() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    let clip = clip(&mut world, 5.0);
    let line = looping(&mut world, clip, "voice");
    AudioView::new(&mut world).snapshot("pause");
    AudioView::new(&mut world).play(&line);
    run(&mut world, 0.5);
    let before = bus(&world, "music");
    assert!((before.duck_db + 9.0).abs() < 0.01 && before.cutoff_hz == Some(800.0));
    let mixer = Mixer::parse(SNAPSHOTS).unwrap_or_default();
    AudioView::new(&mut world).set_mixer(mixer);
    audio_step(&mut world, FRAME);
    let music = bus(&world, "music");
    assert!((music.snapshot_db + 6.0).abs() < 0.1, "{music:?}");
    assert!((music.duck_db + 9.0).abs() < 0.01, "{music:?}");
    assert!(
      close(music.gain_db, -15.0),
      "the layers are on the new tree at once: {music:?}"
    );
    run(&mut world, 0.1);
    assert_eq!(bus(&world, "music").cutoff_hz, Some(800.0));
  }

  #[test]
  fn a_snapshot_the_reload_drops_blends_back_to_open() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.4);
    let text = SNAPSHOTS.replace("\"pause\"", "\"vanishing-unique\"");
    let renamed =
      Mixer::parse(&text.replace("\"vanishing-unique\"", "\"other\"")).unwrap_or_default();
    // The active snapshot is "pause": the new mixer has only "other"
    AudioView::new(&mut world).set_mixer(renamed);
    audio_step(&mut world, FRAME);
    run(&mut world, 0.5);
    let music = bus(&world, "music");
    assert!(music.snapshot_db.abs() < 0.01, "{music:?}");
    assert_eq!(music.cutoff_hz, Some(20_000.0));
    assert!(bus(&world, "sfx").snapshot_db.abs() < 0.01);
    assert_eq!(
      warnings(|message| message.contains("\"pause\" is not in the reloaded mixer")),
      1
    );
  }

  #[test]
  fn a_rebuild_mid_blend_keeps_the_blends_remaining_time() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    run(&mut world, 0.15);
    let mixer = Mixer::parse(SNAPSHOTS).unwrap_or_default();
    AudioView::new(&mut world).set_mixer(mixer);
    audio_step(&mut world, FRAME);
    run(&mut world, 0.17);
    let music = bus(&world, "music");
    assert!((music.snapshot_db + 6.0).abs() < 0.1, "{music:?}");
    // At its target, to within the change a cutoff is sent for (0.01 Hz)
    assert!(
      music.cutoff_hz.is_some_and(|hz| (hz - 800.0).abs() < 0.01),
      "{music:?}"
    );
  }

  #[test]
  fn a_blend_glides_over_a_slow_frame() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    AudioView::new(&mut world).snapshot("pause");
    audio_step(&mut world, Duration::from_millis(100));
    let gains = bus_gains(&world, "music");
    let last = gains.last().map(|entry| entry.fade);
    assert_eq!(last, Some(Duration::from_millis(100)), "{gains:?}");
    // Back on fast frames the smoothing is the minimum again
    run(&mut world, 0.4);
    world
      .resource_mut::<AudioEngine>()
      .set_volume_setting(VolumeKind::Music, 0.5);
    assert_eq!(
      bus_gains(&world, "music").last().map(|e| e.fade),
      Some(MIN_FADE)
    );
  }

  /// A long frame (a 2 s hitch) glides what the frame steps over at most
  /// 100 ms, so the mix does not trail the hitch by its length: the
  /// snapshot's blend on the bus gains, a voice's rate following time
  /// scale, and a placed voice's gain following its place
  #[test]
  fn a_long_frame_glides_over_at_most_100_ms() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    let long = clip(&mut world, 30.0);
    let hum = looping(&mut world, long.clone(), "sfx");
    let placed = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(long)],
      looping: true,
      spatial: Some(crate::core::audio::Spatial {
        min_distance: 10.0,
        max_distance: 1000.0,
        curve: crate::core::audio::Falloff::Linear,
        pan_width: 1000.0,
        low_pass_at_max: None,
      }),
      ..Default::default()
    });
    let listener = world
      .spawn((
        crate::core::audio::AudioListener,
        crate::core::ecs::Transform::default(),
      ))
      .id();
    let (hummed, near) = {
      let mut audio = AudioView::new(&mut world);
      (
        audio.play(&hum),
        audio.play_at(&placed, crate::core::ecs::Vec2::new(100.0, 0.0)),
      )
    };
    audio_step(&mut world, FRAME);
    AudioView::new(&mut world).snapshot("pause");
    world
      .resource_mut::<crate::core::time::Time>()
      .game
      .set_speed(0.5);
    world
      .entity_mut(listener)
      .insert(crate::core::ecs::Transform::from_xy(400.0, 0.0));
    audio_step(&mut world, Duration::from_secs(2));
    let cap = Duration::from_millis(100);
    let fades = |voice: u32, kind: AudioLogKind| -> Vec<Duration> {
      log(&world)
        .iter()
        .filter(|entry| entry.voice == voice && entry.kind == kind)
        .map(|entry| entry.fade)
        .collect()
    };
    assert_eq!(
      bus_gains(&world, "music").last().map(|entry| entry.fade),
      Some(cap)
    );
    assert_eq!(fades(hummed.id(), AudioLogKind::Rate).last(), Some(&cap));
    assert_eq!(fades(near.id(), AudioLogKind::Gain).last(), Some(&cap));
  }

  /// A blend keeps exact time: a 1 s blend stepped in frames of 1/60 s is
  /// at its target after 60 of them, not 61
  #[test]
  fn a_blend_lands_on_the_frame_its_time_runs_out() {
    let mut world = audio_world();
    AudioView::new(&mut world).set_mixer(Mixer {
      snapshots: vec![SnapshotDef {
        name: "slow".into(),
        blend: 1.0,
        buses: vec![(
          "music".into(),
          SnapshotBus {
            volume_db: -12.0,
            low_pass: None,
          },
        )],
      }],
      ..Default::default()
    });
    AudioView::new(&mut world).snapshot("slow");
    for _ in 0..59 {
      audio_step(&mut world, FRAME);
    }
    assert_ne!(bus(&world, "music").snapshot_db, -12.0, "still blending");
    audio_step(&mut world, FRAME);
    assert_eq!(bus(&world, "music").snapshot_db, -12.0);
  }

  /// A snapshot whose target the reloaded mixer lacks, mid-blend, blends
  /// back to open over the whole blend, not what was left of the blend
  /// toward it
  #[test]
  fn a_snapshot_dropped_mid_blend_blends_back_over_its_whole_blend() {
    let named = SNAPSHOTS.replace("\"pause\"", "\"dropped-mid-blend\"");
    let (mut world, _files) = world_with_mixer(&named);
    AudioView::new(&mut world).snapshot("dropped-mid-blend");
    run(&mut world, 0.15);
    let renamed = Mixer::parse(&SNAPSHOTS.replace("\"pause\"", "\"other\"")).unwrap_or_default();
    AudioView::new(&mut world).set_mixer(renamed);
    audio_step(&mut world, FRAME);
    // 0.17 s of the 0.3 s back: a third of the way left
    run(&mut world, 0.17);
    let music = bus(&world, "music");
    assert!(music.snapshot_db < -0.5, "still blending back: {music:?}");
    run(&mut world, 0.15);
    assert!(bus(&world, "music").snapshot_db.abs() < 0.01);
  }

  /// A snapshot asked for before the mixer lands waits for it, the last
  /// call winning
  #[test]
  fn the_last_snapshot_asked_for_before_the_mixer_lands_applies() {
    let files = MemorySource::new();
    files.insert(MIXER_PATH, SNAPSHOTS);
    let mut world = audio_world_over(files);
    let mixer = AssetsView::new(&mut world).load::<Mixer>(MIXER_PATH);
    world
      .resource_mut::<AudioEngine>()
      .set_mixer(Some(mixer.clone()));
    {
      let mut audio = AudioView::new(&mut world);
      audio.snapshot("pause");
      audio.snapshot("dive");
    }
    let started = Instant::now();
    while world.resource::<Assets<Mixer>>().get(&mixer).is_none() {
      assert!(
        started.elapsed() < Duration::from_secs(5),
        "the mixer loads"
      );
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    run(&mut world, 0.4);
    let music = bus(&world, "music");
    assert!((music.snapshot_db + 12.0).abs() < 0.01, "dive: {music:?}");
    assert!(bus(&world, "sfx").snapshot_db.abs() < 0.01, "not pause");
  }

  /// A snapshot asked for before a mixer that then fails to load is
  /// warned about once, by name: it can never apply
  #[test]
  fn a_snapshot_waiting_for_a_mixer_that_fails_warns_once() {
    let files = MemorySource::new();
    files.insert(MIXER_PATH, "(buses: { \"sfx\": (volume: oops) })");
    let mut world = audio_world_over(files);
    let mixer = AssetsView::new(&mut world).load::<Mixer>(MIXER_PATH);
    world
      .resource_mut::<AudioEngine>()
      .set_mixer(Some(mixer.clone()));
    AudioView::new(&mut world).snapshot("stranded-unique");
    let started = Instant::now();
    while !world.resource::<AssetServer>().state(&mixer).is_failed() {
      assert!(
        started.elapsed() < Duration::from_secs(5),
        "the mixer fails"
      );
      audio_step(&mut world, FRAME);
      std::thread::sleep(Duration::from_millis(1));
    }
    run(&mut world, 0.1);
    assert_eq!(
      warnings(|message| message.contains("\"stranded-unique\"")),
      1
    );
  }

  #[test]
  fn an_unknown_snapshot_warns_once_and_changes_nothing() {
    let (mut world, _files) = world_with_mixer(SNAPSHOTS);
    for _ in 0..3 {
      AudioView::new(&mut world).snapshot("no-such-snapshot-unique");
    }
    run(&mut world, 0.1);
    assert_eq!(bus(&world, "music").snapshot_db, 0.0);
    assert_eq!(
      warnings(|message| message.contains("no-such-snapshot-unique")),
      1
    );
  }

  #[test]
  fn snapshots_and_ducks_that_name_what_is_not_there_fail_the_check() {
    let bad = |text: &str| match Mixer::parse(text) {
      Ok(mixer) => panic!("{text} parsed: {mixer:?}"),
      Err(error) => error,
    };
    let error = bad(r#"(snapshots: { "a": (buses: { "sfx/nope": (volume: -1.0) }) })"#);
    assert!(error.contains("sfx/nope"), "{error}");
    let error = bad(r#"(snapshots: { "a": (buses: { "music": (low_pass: 5.0) }) })"#);
    assert!(error.contains("low_pass"), "{error}");
    let error = bad(r#"(snapshots: { "a": (blend: -1.0) })"#);
    assert!(error.contains("blend"), "{error}");
    let error =
      bad(r#"(ducks: [(trigger: "voice", target: "nope", by: -9.0, attack: 0.1, release: 0.1)])"#);
    assert!(error.contains("nope"), "{error}");
    let error =
      bad(r#"(ducks: [(trigger: "voice", target: "music", by: 3.0, attack: 0.1, release: 0.1)])"#);
    assert!(error.contains("by"), "{error}");
    let error =
      bad(r#"(ducks: [(trigger: "music", target: "music", by: -3.0, attack: 0.1, release: 0.1)])"#);
    assert!(error.contains("itself"), "{error}");
  }

  /// A rebuild's move logs the loop's old copy stopping on the bus it
  /// played on, at the gain it played at, as a reload's switch does, and
  /// the new copy starting on its new bus: a bus the new mixer drops, and
  /// one it adds
  #[test]
  fn a_rebuild_s_stop_names_the_old_copy_s_bus() {
    let (mut world, _files) = world_with_mixer(r#"(buses: { "sfx/steps": (volume: -3.0) })"#);
    let long = clip(&mut world, 2.0);
    let sound = world.resource_mut::<Assets<Sound>>().add(Sound {
      clips: vec![SoundClip::new(long)],
      bus: "sfx/steps".to_string(),
      volume_db: -6.0,
      looping: true,
      ..Default::default()
    });
    let voice = AudioView::new(&mut world).play(&sound);
    audio_step(&mut world, FRAME);
    let moves = |world: &World| -> Vec<(AudioLogKind, String, f32)> {
      log(world)
        .into_iter()
        .filter(|entry| entry.voice == voice.id())
        .skip(1)
        .map(|entry| (entry.kind, entry.bus, entry.gain_db))
        .collect()
    };
    // The bus goes: the loop moves to the nearest above it
    set_mixer(&mut world, "()");
    assert_eq!(
      moves(&world),
      [
        (AudioLogKind::Stop, "sfx/steps".to_string(), -6.0),
        (AudioLogKind::Start, "sfx".to_string(), -6.0),
      ]
    );
    // The bus comes back: the loop moves onto it
    audio_step(&mut world, FRAME);
    set_mixer(&mut world, r#"(buses: { "sfx/steps": (volume: -3.0) })"#);
    assert_eq!(
      moves(&world)[2..],
      [
        (AudioLogKind::Stop, "sfx".to_string(), -6.0),
        (AudioLogKind::Start, "sfx/steps".to_string(), -6.0),
      ]
    );
  }

  /// `text` parsed, set in code and built at once
  fn set_mixer(world: &mut World, text: &str) {
    match Mixer::parse(text) {
      Ok(mixer) => AudioView::new(world).set_mixer(mixer),
      Err(error) => panic!("{text}: {error}"),
    }
  }

  /// kira's tracks under its main track: one `master` for the live tree,
  /// and one for each retired tree kira still keeps
  fn masters(world: &World) -> usize {
    world.resource::<AudioEngine>().kira_tracks().0
  }

  fn secs_close(actual: Duration, expected: f64) -> bool {
    (actual.as_secs_f64() - expected).abs() < 0.005
  }

  /// Each effect rings for its tail after its input falls silent: a delay
  /// for its echoes to fall 60 dB, a reverb by its feedback, the rest not
  /// at all; at most 10 s
  #[test]
  fn each_effect_rings_for_its_tail() {
    let delay = |seconds: f64, feedback_db: f32| EffectDef::Delay {
      seconds,
      feedback_db,
      mix: 0.5,
    };
    assert!(secs_close(delay(0.5, -12.0).tail(), 2.5));
    // 2 s apart at -6 dB is 20 s of echoes: capped
    assert_eq!(delay(2.0, -6.0).tail(), MAX_TAIL);
    let reverb = |feedback: f64| EffectDef::Reverb {
      feedback,
      damping: 0.1,
      stereo_width: 1.0,
      mix: 0.5,
    };
    // Its slowest comb filter's 60 dB fall (1.15 s at 0.8, 2.44 s at
    // 0.9), and its all-pass filters' (0.37 s)
    assert!(
      secs_close(reverb(0.8).tail(), 1.525),
      "{:?}",
      reverb(0.8).tail()
    );
    assert!(
      secs_close(reverb(0.9).tail(), 2.814),
      "{:?}",
      reverb(0.9).tail()
    );
    assert_eq!(reverb(1.0).tail(), MAX_TAIL, "it never falls silent");
    let filter = EffectDef::LowPass {
      cutoff: 800.0,
      resonance: 0.0,
    };
    assert_eq!(filter.tail(), Duration::ZERO);
  }

  /// A tree rings for its longest chain of effects: a voice's bus, the
  /// buses above it, and from any of them a send bus, each effect's tail
  /// added
  #[test]
  fn a_tree_rings_for_its_longest_chain_of_effects() {
    let (world, _files) = world_with_mixer(
      r#"(
        buses: {
          "sfx": (effects: [Delay(seconds: 0.5, feedback_db: -12.0)]),
          "sfx/steps": (effects: [Reverb(feedback: 0.8)], sends: { "sfx/room": -6.0 }),
          "sfx/room": (effects: [Reverb(feedback: 0.9)]),
          "ui": (effects: [LowPass(cutoff: 900.0)]),
        },
      )"#,
    );
    // sfx/steps (1.525 s), then the room (2.814 s) beats sfx (2.5 s)
    let tail = world.resource::<AudioEngine>().tree_tail();
    assert!(secs_close(tail, 1.525 + 2.814), "{tail:?}");
    let (plain, _files) =
      world_with_mixer(r#"(buses: { "ui": (effects: [LowPass(cutoff: 900.0)]) })"#);
    assert_eq!(plain.resource::<AudioEngine>().tree_tail(), Duration::ZERO);
  }

  /// 2 s echoes, each 6 dB down: 20 s of them, held for 10
  const ECHOES: &str =
    r#"(buses: { "sfx": (effects: [Delay(seconds: 2.0, feedback_db: -6.0, mix: 0.5)]) })"#;

  /// A one-shot on a tree that a save retires: the tree is kept after its
  /// last sound while its delay rings (its first echo is heard), for the
  /// tail of 10 s, and then goes
  #[test]
  fn a_retired_tree_rings_out_its_tail_after_its_last_sound() {
    let (mut world, meter) = crate::core::testing::metered_audio_world();
    set_mixer(&mut world, ECHOES);
    let shot = clip(&mut world, 0.5);
    let voice = AudioView::new(&mut world).play(&shot);
    let mut clock = Duration::ZERO;
    let step = |world: &mut World, clock: &mut Duration| {
      audio_step(world, FRAME);
      *clock += FRAME;
    };
    for _ in 0..6 {
      step(&mut world, &mut clock);
    }
    set_mixer(
      &mut world,
      &ECHOES.replace("(effects", "(volume: -3.0, effects"),
    );
    while !voice.is_stopped() {
      assert!(clock < Duration::from_secs(1), "the one-shot ends");
      step(&mut world, &mut clock);
    }
    let ended = clock;
    assert_eq!(masters(&world), 2, "kept after its last sound");
    // Its first echo, 2 s after the one-shot's start, from the old tree:
    // kira's delay feeds its echoes back at -6 dB (an amplitude of half)
    // and mixes them in at the square root of its mix
    while clock < Duration::from_millis(2_250) {
      step(&mut world, &mut clock);
    }
    let echo = meter.last().left;
    let expected = 0.1 * kira::Decibels(-6.0).as_amplitude() * 0.5f32.sqrt();
    assert!((echo - expected).abs() < 0.001, "the echo is heard: {echo}");
    while clock < ended + Duration::from_millis(9_900) {
      step(&mut world, &mut clock);
    }
    assert_eq!(masters(&world), 2, "ringing until 10 s after");
    while clock < ended + Duration::from_millis(10_100) {
      step(&mut world, &mut clock);
    }
    step(&mut world, &mut clock);
    assert_eq!(masters(&world), 1, "then dropped");
  }

  /// A loop that moves to the new tree leaves its old copy's tail ringing
  /// on the retired one, though no voice plays there
  #[test]
  fn a_moved_loop_s_old_tree_rings_out_its_tail() {
    let reverb = r#"(buses: { "sfx": (effects: [Reverb(feedback: 0.8, mix: 0.5)]) })"#;
    let (mut world, _files) = world_with_mixer(reverb);
    let long = clip(&mut world, 2.0);
    let sound = looping(&mut world, long, "sfx");
    let voice = AudioView::new(&mut world).play(&sound);
    audio_step(&mut world, FRAME);
    set_mixer(
      &mut world,
      &reverb.replace("(effects", "(volume: -3.0, effects"),
    );
    audio_step(&mut world, FRAME);
    assert!(voice.is_playing(), "{voice:?}");
    assert_eq!(masters(&world), 2, "the old copy's reverb rings on");
    // 1.525 s of reverb after the old copy's 10 ms fade
    for _ in 0..85 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(masters(&world), 2, "ringing {:?} on", FRAME * 86);
    // Then it fades out over 10 ms, and kira's tracks go 50 ms after
    for _ in 0..15 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(masters(&world), 1, "then it goes");
  }

  /// A tree with no tail (no effects, or only filters) goes with its last
  /// sound, as kira keeps a dropped track only while it plays
  #[test]
  fn a_tree_without_a_tail_goes_with_its_last_sound() {
    for text in [
      "()",
      r#"(buses: { "sfx": (effects: [LowPass(cutoff: 900.0)]) })"#,
    ] {
      let (mut world, _files) = world_with_mixer(text);
      let shot = clip(&mut world, 0.2);
      let voice = AudioView::new(&mut world).play(&shot);
      audio_step(&mut world, FRAME);
      set_mixer(&mut world, r#"(buses: { "sfx": (volume: -3.0) })"#);
      for _ in 0..60 {
        if voice.is_stopped() {
          break;
        }
        assert_eq!(masters(&world), 2, "{text}: kept while it plays");
        audio_step(&mut world, FRAME);
      }
      assert!(voice.is_stopped(), "{voice:?}");
      audio_step(&mut world, FRAME);
      assert_eq!(masters(&world), 1, "{text}: gone with its sound");
    }
  }

  /// At most four retired trees wait for their tails: a fifth lets the
  /// oldest go, which fades out and goes
  #[test]
  fn a_fifth_retired_tree_lets_the_oldest_go() {
    let reverb = r#"(buses: { "sfx": (volume: -1.0, effects: [Reverb(feedback: 0.8)]) })"#;
    let (mut world, _files) = world_with_mixer(reverb);
    for volume in 2..=6 {
      set_mixer(&mut world, &reverb.replace("-1.0", &format!("-{volume}.0")));
      audio_step(&mut world, FRAME);
    }
    // The oldest fades out, kept 50 ms
    assert_eq!(masters(&world), 6);
    for _ in 0..4 {
      audio_step(&mut world, FRAME);
    }
    // The live tree and the four latest retired, each with its master
    assert_eq!(masters(&world), 5);
    let ringing = world.resource::<AudioEngine>().ringing_generations();
    let live = world.resource::<AudioEngine>().generation();
    assert_eq!(ringing, (live - 4..live).collect::<Vec<u32>>());
  }

  /// A delay that hardly falls (0.1 dB an echo), on `sfx` and on a send
  /// bus `sfx` feeds: a one-shot as long as its delay leaves it ringing a
  /// steady level
  const STEADY: [&str; 2] = [
    r#"(buses: { "sfx": (volume: -1.0, effects: [Delay(seconds: 0.1, feedback_db: -0.1, mix: 1.0)]) })"#,
    r#"(buses: {
      "sfx": (volume: -1.0, sends: { "sfx/steady-room": 0.0 }),
      "sfx/steady-room": (effects: [Delay(seconds: 0.1, feedback_db: -0.1, mix: 1.0)]),
    })"#,
  ];

  /// The largest change from one rendered frame to the next
  fn largest_step(frames: &[kira::Frame]) -> f32 {
    frames
      .windows(2)
      .map(|pair| (pair[1].left - pair[0].left).abs())
      .fold(0.0, f32::max)
  }

  /// An audio world on `text`, a one-shot played into it and ended, its
  /// echoes ringing: the meter, and the level they ring at
  fn ringing_world(text: &str) -> (World, crate::core::audio::Meter, f32) {
    let (mut world, meter) = crate::core::testing::metered_audio_world();
    set_mixer(&mut world, text);
    let shot = clip(&mut world, 0.1);
    let voice = AudioView::new(&mut world).play(&shot);
    for _ in 0..15 {
      audio_step(&mut world, FRAME);
    }
    assert!(voice.is_stopped(), "{voice:?}");
    let level = meter.last().left;
    assert!(level > 0.05, "{text}: it rings: {level}");
    (world, meter, level)
  }

  /// A ringing tree let go past the four kept fades out over at least
  /// [`MIN_FADE`] before kira removes its tracks: what rings on `master`
  /// and on its send buses (whose returns kira mixes beside `master`) never
  /// falls to silence in one step
  #[test]
  fn a_ringing_tree_let_go_fades_out_before_it_goes() {
    for text in STEADY {
      let (mut world, meter, level) = ringing_world(text);
      let from = meter.rendered_count();
      // Four saves keep it ringing; the fifth lets it go
      for volume in 2..=6 {
        set_mixer(&mut world, &text.replace("-1.0", &format!("-{volume}.0")));
        audio_step(&mut world, FRAME);
      }
      for _ in 0..10 {
        audio_step(&mut world, FRAME);
      }
      assert_eq!(masters(&world), 5, "{text}: the live tree and four kept");
      let after = meter.rendered(meter.rendered_count() - from);
      let last = after.last().map_or(1.0, |frame| frame.left);
      assert!(last.abs() < 1e-4, "{text}: silent once it has gone: {last}");
      // A 10 ms fade from 0 to -60 dB falls at most 1.5% a frame
      let step = largest_step(&after);
      assert!(step < 0.05 * level, "{text}: a step of {step} from {level}");
    }
  }

  /// A ringing tree whose tail is capped at 10 s fades out as it goes,
  /// though it still rings
  #[test]
  fn a_ringing_tree_capped_at_its_tail_fades_out() {
    let (mut world, meter, _) = ringing_world(STEADY[0]);
    set_mixer(&mut world, &STEADY[0].replace("-1.0", "-2.0"));
    for _ in 0..590 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(masters(&world), 2, "ringing near its 10 s");
    let from = meter.rendered_count();
    for _ in 0..30 {
      audio_step(&mut world, FRAME);
    }
    assert_eq!(masters(&world), 1, "gone after its 10 s");
    let after = meter.rendered(meter.rendered_count() - from);
    // 10 echoes a second, each 0.1 dB down: 10 dB down by then
    let level = after.first().map_or(0.0, |frame| frame.left);
    assert!(level > 0.02, "still ringing at 10 s: {level}");
    let step = largest_step(&after);
    assert!(step < 0.05 * level, "a step of {step} from {level}");
  }

  /// A mixer made in code with a delay shorter than a frame, which kira's
  /// delay would panic on as it renders, is refused by the check: the
  /// buses stay as they are and what plays renders
  #[test]
  fn a_delay_shorter_than_a_frame_is_refused_and_nothing_panics() {
    let mut world = audio_world();
    let mut sfx = BusDef::new("sfx");
    sfx.effects.push(EffectDef::Delay {
      seconds: 0.000_01,
      feedback_db: -6.0,
      mix: 0.5,
    });
    AudioView::new(&mut world).set_mixer(Mixer {
      buses: vec![sfx],
      ..Default::default()
    });
    let shot = clip(&mut world, 0.2);
    let voice = AudioView::new(&mut world).play(&shot);
    for _ in 0..3 {
      audio_step(&mut world, FRAME);
    }
    assert!(voice.is_playing(), "{voice:?}");
    let effects = world
      .resource::<AudioEngine>()
      .buses()
      .into_iter()
      .find(|bus| bus.name == "sfx")
      .map(|bus| bus.effects);
    assert_eq!(effects, Some(0), "the buses stay as they were");
  }

  /// A mixer with the most send buses a mixer may have, each with a delay
  /// that rings, at `volume` dB on `sfx` (each save changes it)
  fn sixteen_sends(volume: usize) -> String {
    let routes: String = (0..16)
      .map(|send| format!(r#""sfx/send-room-{send}": -6.0, "#))
      .collect();
    let rooms: String = (0..16)
      .map(|send| {
        format!(r#""sfx/send-room-{send}": (effects: [Delay(seconds: 0.5, feedback_db: -6.0)]), "#)
      })
      .collect();
    format!(r#"(buses: {{ "sfx": (volume: -{volume}.0, sends: {{ {routes} }}), {rooms} }})"#)
  }

  /// kira's send-track room holds every rebuild's send tracks: with the
  /// most send buses a mixer may have, a long one-shot holding its tree
  /// past the four kept, saves past them, and then two saves in one frame
  /// (the tree let go before still fading out), every tree is built with
  /// a track for each send bus. The one-shot on the tree let go stops with
  /// the tree's fade
  #[test]
  fn every_rebuild_has_room_for_its_send_tracks() {
    let room_warnings =
      || warnings(|message| message.contains("kira has no room for the \"sfx/send-room-"));
    let warned = room_warnings();
    let (mut world, _files) = world_with_mixer(&sixteen_sends(1));
    let long = clip(&mut world, 20.0);
    let voice_over = AudioView::new(&mut world).play(&long);
    let made = |world: &World| world.resource::<AudioEngine>().send_tracks_made();
    assert_eq!(made(&world), (16, 16));
    for save in 2..=9 {
      for _ in 0..12 {
        audio_step(&mut world, FRAME);
      }
      set_mixer(&mut world, &sixteen_sends(save));
      assert_eq!(made(&world), (16, 16), "save {save}");
    }
    // The fifth save let the voice-over's tree go, and the voice-over
    // with it
    assert!(voice_over.is_stopped(), "{voice_over:?}");
    let stops: Vec<Duration> = world
      .resource::<AudioEngine>()
      .log()
      .iter()
      .filter(|entry| entry.voice == voice_over.id() && entry.kind == AudioLogKind::Stop)
      .map(|entry| entry.fade)
      .collect();
    assert_eq!(stops, [MIN_FADE]);
    for _ in 0..12 {
      audio_step(&mut world, FRAME);
    }
    set_mixer(&mut world, &sixteen_sends(10));
    set_mixer(&mut world, &sixteen_sends(11));
    assert_eq!(made(&world), (16, 16), "the second save in a frame");
    assert_eq!(room_warnings(), warned, "no send bus without a track");
  }

  fn warnings(matching: impl Fn(&str) -> bool) -> usize {
    crate::core::dev_log::get_entries()
      .iter()
      .filter(|entry| {
        entry.level == crate::core::dev_log::LogLevel::Warn && matching(&entry.message)
      })
      .count()
  }
}
