//! Applying the settings: what changed since the engine last applied them.

use super::EngineValues;
use crate::core::audio::VolumeKind;
use crate::core::config::DisplayMode;
use crate::core::ecs::kernel::Tick;

/// The engine's settings as the engine last applied them, and the world's
/// tick at the last check of `Settings` for a change
#[derive(Debug, Clone)]
pub(crate) struct AppliedSettings {
  pub(crate) values: EngineValues,
  checked: Option<Tick>,
}

impl AppliedSettings {
  /// Settings applied at `values`, never checked
  pub(crate) fn new(values: EngineValues) -> AppliedSettings {
    AppliedSettings {
      values,
      checked: None,
    }
  }

  /// Whether `Settings`, last changed at `changed`, changed since the last
  /// check, as of the world's tick `now`. The caller moves the world's tick
  /// on after it, so a change made after the check is newer than `now`.
  pub(crate) fn check(&mut self, changed: Tick, now: Tick) -> bool {
    let changed = self
      .checked
      .is_none_or(|checked| changed.is_newer_than(checked, now));
    self.checked = Some(now);
    changed
  }
}

/// What differs between the applied settings and the current ones
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct SettingsChanges {
  pub(crate) volumes: Vec<(VolumeKind, f32)>,
  pub(crate) display_mode: Option<DisplayMode>,
  pub(crate) window_size: Option<(u32, u32)>,
  pub(crate) vsync: Option<bool>,
  pub(crate) show_fps: Option<bool>,
}

/// The settings in `now` that differ from `applied`, with their new values
pub(crate) fn diff(applied: &EngineValues, now: &EngineValues) -> SettingsChanges {
  SettingsChanges {
    volumes: VolumeKind::ALL
      .into_iter()
      .filter(|kind| applied.volumes[kind.index()] != now.volumes[kind.index()])
      .map(|kind| (kind, now.volumes[kind.index()]))
      .collect(),
    display_mode: differs(applied.display_mode, now.display_mode),
    window_size: differs(applied.window_size, now.window_size),
    vsync: differs(applied.vsync, now.vsync),
    show_fps: differs(applied.show_fps, now.show_fps),
  }
}

fn differs<T: PartialEq>(applied: T, now: T) -> Option<T> {
  (applied != now).then_some(now)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_diff_holds_each_changed_setting_and_nothing_else() {
    let applied = EngineValues::default();
    assert_eq!(diff(&applied, &applied), SettingsChanges::default());

    let mut now = applied;
    now.volumes[VolumeKind::Music.index()] = 0.3;
    now.volumes[VolumeKind::Voice.index()] = 0.0;
    assert_eq!(
      diff(&applied, &now),
      SettingsChanges {
        volumes: vec![(VolumeKind::Music, 0.3), (VolumeKind::Voice, 0.0)],
        ..SettingsChanges::default()
      }
    );

    let now = EngineValues {
      window_size: (800, 600),
      display_mode: DisplayMode::BorderlessFullscreen,
      vsync: false,
      show_fps: true,
      ..applied
    };
    assert_eq!(
      diff(&applied, &now),
      SettingsChanges {
        volumes: Vec::new(),
        display_mode: Some(DisplayMode::BorderlessFullscreen),
        window_size: Some((800, 600)),
        vsync: Some(false),
        show_fps: Some(true),
      }
    );
  }

  #[test]
  fn a_change_is_seen_once_and_a_later_one_again() {
    let tick = Tick::new;
    let mut applied = AppliedSettings::new(EngineValues::default());
    assert!(applied.check(tick(5), tick(5)), "the first check applies");
    assert!(!applied.check(tick(5), tick(6)), "nothing changed since");
    assert!(
      applied.check(tick(7), tick(7)),
      "changed after the last check"
    );
    assert!(!applied.check(tick(7), tick(9)));
  }
}
