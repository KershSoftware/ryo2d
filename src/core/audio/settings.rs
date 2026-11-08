//! The player's volume settings: [`VolumeKind`], one per standard bus, and
//! the curve that turns a setting (0 to 1) into bus gain in dB

use crate::core::config::resolve::{
  DEFAULT_AMBIENCE_VOLUME, DEFAULT_MASTER_VOLUME, DEFAULT_MUSIC_VOLUME, DEFAULT_SFX_VOLUME,
  DEFAULT_UI_VOLUME, DEFAULT_VOICE_VOLUME,
};

/// kira's silence: a gain at or below it plays nothing
pub(crate) const SILENCE_DB: f32 = -60.0;

/// One of the player's six volume settings, each on its standard bus (a
/// game's own buses take the setting of the standard bus they are under).
/// A settings menu sets them with `ctx.settings().set_volume(kind, value)`,
/// from 0 (silent) to 1 (the mix as designed); the engine plays the change and
/// saves it in `system.json`:
///
/// ```
/// use ryo2d::prelude::*;
///
/// fn music_slider(ctx: &mut SceneContext<'_>, value: f32) {
///   ctx.settings().set_volume(VolumeKind::Music, value);
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeKind {
  /// Everything: the `master` bus
  Master,
  /// The `music` bus
  Music,
  /// The `sfx` bus: sound effects
  Sfx,
  /// The `ui` bus: the interface's sounds
  Ui,
  /// The `voice` bus: dialogue
  Voice,
  /// The `ambience` bus: environment beds and loops
  Ambience,
}

impl VolumeKind {
  /// The six, in the order of the standard buses
  pub const ALL: [VolumeKind; 6] = [
    VolumeKind::Master,
    VolumeKind::Music,
    VolumeKind::Sfx,
    VolumeKind::Ui,
    VolumeKind::Voice,
    VolumeKind::Ambience,
  ];

  /// The bus it sets
  pub fn bus(self) -> &'static str {
    match self {
      VolumeKind::Master => "master",
      VolumeKind::Music => "music",
      VolumeKind::Sfx => "sfx",
      VolumeKind::Ui => "ui",
      VolumeKind::Voice => "voice",
      VolumeKind::Ambience => "ambience",
    }
  }

  /// The setting a game starts with when nothing sets it: 1 for every
  /// kind, the mix as designed. A setting that is not a number becomes this
  pub fn default_value(self) -> f32 {
    match self {
      VolumeKind::Master => DEFAULT_MASTER_VOLUME,
      VolumeKind::Music => DEFAULT_MUSIC_VOLUME,
      VolumeKind::Sfx => DEFAULT_SFX_VOLUME,
      VolumeKind::Ui => DEFAULT_UI_VOLUME,
      VolumeKind::Voice => DEFAULT_VOICE_VOLUME,
      VolumeKind::Ambience => DEFAULT_AMBIENCE_VOLUME,
    }
  }

  /// Its place in [`VolumeKind::ALL`], which is its bus's place in the tree
  pub(crate) fn index(self) -> usize {
    self as usize
  }

  /// `value` as a setting: NaN is the default, the rest clamped to 0..=1
  pub(crate) fn sanitize(self, value: f32) -> f32 {
    if value.is_nan() {
      self.default_value()
    } else {
      value.clamp(0.0, 1.0)
    }
  }
}

/// A volume setting's gain in dB, on the perceptual curve: amplitude is the
/// setting squared, so dB is `40·log10(v)`. 1 is 0 dB, 0.5 about -12 dB,
/// and 0 (or less) is exact silence (-60 dB, where kira plays nothing); a
/// setting above 1 counts as 1. A setting that is not a number is 0 dB:
/// callers turn NaN into the kind's default first
pub fn volume_to_db(v: f32) -> f32 {
  if v.is_nan() {
    return 0.0;
  }
  if v <= 0.0 {
    return SILENCE_DB;
  }
  (40.0 * v.min(1.0).log10()).max(SILENCE_DB)
}

/// The six settings, sanitized, in [`VolumeKind::ALL`] order
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VolumeSettings([f32; 6]);

impl Default for VolumeSettings {
  fn default() -> Self {
    Self(VolumeKind::ALL.map(VolumeKind::default_value))
  }
}

impl VolumeSettings {
  pub(crate) fn get(&self, kind: VolumeKind) -> f32 {
    self.0[kind.index()]
  }

  /// Sets `kind` to `value`, sanitized, and returns what it is now
  pub(crate) fn set(&mut self, kind: VolumeKind, value: f32) -> f32 {
    let value = kind.sanitize(value);
    self.0[kind.index()] = value;
    value
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_curve_is_the_square_of_the_setting() {
    assert_eq!(volume_to_db(1.0), 0.0);
    assert!((volume_to_db(0.5) - -12.0412).abs() < 1e-3);
    assert!((volume_to_db(0.1) - -40.0).abs() < 1e-4);
    assert_eq!(volume_to_db(0.0), SILENCE_DB, "0 is exact silence");
    assert_eq!(volume_to_db(0.01), SILENCE_DB, "below -60 dB is silence");
    assert_eq!(volume_to_db(-1.0), SILENCE_DB);
    assert_eq!(volume_to_db(5.0), 0.0);
    assert_eq!(volume_to_db(f32::INFINITY), 0.0);
  }

  #[test]
  fn a_setting_is_clamped_and_nan_is_its_default() {
    assert_eq!(VolumeKind::Sfx.sanitize(f32::NAN), 1.0);
    assert_eq!(VolumeKind::Master.sanitize(f32::NAN), 1.0);
    assert_eq!(VolumeKind::Music.sanitize(-1.0), 0.0);
    assert_eq!(VolumeKind::Ui.sanitize(5.0), 1.0);
    assert_eq!(VolumeKind::Voice.sanitize(f32::NEG_INFINITY), 0.0);
    let mut settings = VolumeSettings::default();
    assert_eq!(settings.get(VolumeKind::Master), 1.0);
    assert_eq!(settings.set(VolumeKind::Ambience, 0.25), 0.25);
    assert_eq!(settings.get(VolumeKind::Ambience), 0.25);
  }

  #[test]
  fn each_kind_names_its_standard_bus_in_order() {
    let buses: Vec<&str> = VolumeKind::ALL.iter().map(|kind| kind.bus()).collect();
    assert_eq!(buses, ["master", "music", "sfx", "ui", "voice", "ambience"]);
    for (index, kind) in VolumeKind::ALL.iter().enumerate() {
      assert_eq!(kind.index(), index);
    }
  }
}
