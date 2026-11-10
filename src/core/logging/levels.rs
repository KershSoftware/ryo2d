//! The levels: a default per origin, and overrides per target prefix.

use log::LevelFilter;

use super::record::{Origin, under};

/// The level each target logs at.
#[derive(Debug, Clone)]
pub(crate) struct Levels {
  game: LevelFilter,
  engine: LevelFilter,
  library: LevelFilter,
  /// `(prefix, level)`: a target takes the level of the longest prefix it is
  /// under, matched by whole path segments
  overrides: Vec<(String, LevelFilter)>,
}

const LEVEL_NAMES: &str = "off, error, warn, info, debug, trace";

impl Levels {
  /// `info` for the game and the engine, `warn` for libraries.
  pub(crate) fn defaults() -> Self {
    Self {
      game: LevelFilter::Info,
      engine: LevelFilter::Info,
      library: LevelFilter::Warn,
      overrides: Vec::new(),
    }
  }

  /// Reads `RYO_LOG`'s text over the defaults: comma-separated entries, each
  /// a level (for every origin) or `target=level`. A bad entry is skipped and
  /// gives one line in the second value.
  pub(crate) fn parse(text: &str) -> (Self, Vec<String>) {
    let mut levels = Self::defaults();
    let mut problems = Vec::new();
    for entry in text.split(',').map(str::trim).filter(|e| !e.is_empty()) {
      match entry.split_once('=') {
        None => match parse_level(entry) {
          Some(level) => {
            levels.game = level;
            levels.engine = level;
            levels.library = level;
          }
          None => problems.push(format!(
            "RYO_LOG: `{entry}` is not a level ({LEVEL_NAMES}); skipped"
          )),
        },
        Some((target, level)) => {
          let target = target.trim();
          if target.is_empty() {
            problems.push(format!("RYO_LOG: `{entry}` names no target; skipped"));
            continue;
          }
          match parse_level(level.trim()) {
            Some(level) => levels.set(target, level),
            None => problems.push(format!(
              "RYO_LOG: `{entry}`: `{}` is not a level ({LEVEL_NAMES}); skipped",
              level.trim()
            )),
          }
        }
      }
    }
    (levels, problems)
  }

  /// The level of the longest override `target` is under, else its origin's
  /// default.
  pub(crate) fn of(&self, target: &str) -> LevelFilter {
    let mut best: Option<(usize, LevelFilter)> = None;
    for (prefix, level) in &self.overrides {
      if under(target, prefix) && best.is_none_or(|(len, _)| prefix.len() > len) {
        best = Some((prefix.len(), *level));
      }
    }
    match best {
      Some((_, level)) => level,
      None => match Origin::of(target) {
        Origin::Game => self.game,
        Origin::Engine => self.engine,
        Origin::Library => self.library,
      },
    }
  }

  /// Sets the level of `target` and everything under it, replacing an
  /// earlier override of the same target.
  pub(crate) fn set(&mut self, target: &str, level: LevelFilter) {
    match self
      .overrides
      .iter_mut()
      .find(|(prefix, _)| prefix == target)
    {
      Some((_, old)) => *old = level,
      None => self.overrides.push((target.to_owned(), level)),
    }
  }

  /// The defaults (`game`, `engine`, `library`), then each target's own
  /// level, in the order they were set
  #[cfg(dev_tools)]
  pub(crate) fn report(&self) -> Vec<(String, LevelFilter)> {
    [
      ("game", self.game),
      ("engine", self.engine),
      ("library", self.library),
    ]
    .into_iter()
    .map(|(name, level)| (name.to_owned(), level))
    .chain(self.overrides.iter().cloned())
    .collect()
  }

  /// The most verbose level any target logs at: `log`'s global maximum.
  pub(crate) fn most_verbose(&self) -> LevelFilter {
    self
      .overrides
      .iter()
      .map(|(_, level)| *level)
      .fold(self.game.max(self.engine).max(self.library), Ord::max)
  }
}

/// A level by its name, in any case (`off` to `trace`)
pub(crate) fn parse_level(text: &str) -> Option<LevelFilter> {
  [
    LevelFilter::Off,
    LevelFilter::Error,
    LevelFilter::Warn,
    LevelFilter::Info,
    LevelFilter::Debug,
    LevelFilter::Trace,
  ]
  .into_iter()
  .find(|level| level.as_str().eq_ignore_ascii_case(text))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_defaults_are_info_for_game_and_engine_and_warn_for_libraries() {
    let levels = Levels::defaults();
    assert_eq!(levels.of("game"), LevelFilter::Info);
    assert_eq!(levels.of("game::menu"), LevelFilter::Info);
    assert_eq!(levels.of("ryo2d::core::audio"), LevelFilter::Info);
    assert_eq!(levels.of("ryo2d_procgen::noise"), LevelFilter::Info);
    assert_eq!(levels.of("wgpu_core::device"), LevelFilter::Warn);
    assert_eq!(levels.of("gamepad"), LevelFilter::Warn);
    assert_eq!(levels.most_verbose(), LevelFilter::Info);
  }

  #[test]
  fn the_longest_prefix_wins() {
    let mut levels = Levels::defaults();
    levels.set("ryo2d", LevelFilter::Warn);
    levels.set("ryo2d::core::audio", LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d::core::audio::engine"), LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d::core::audio"), LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d::core::ui"), LevelFilter::Warn);
    assert_eq!(levels.most_verbose(), LevelFilter::Debug);
  }

  #[test]
  fn a_prefix_matches_whole_path_segments() {
    let mut levels = Levels::defaults();
    levels.set("game", LevelFilter::Debug);
    levels.set("ryo2d::core::audio", LevelFilter::Trace);
    assert_eq!(levels.of("game::x"), LevelFilter::Debug);
    assert_eq!(levels.of("gamepad"), LevelFilter::Warn);
    assert_eq!(levels.of("ryo2d::core::audiox"), LevelFilter::Info);
  }

  #[test]
  fn setting_a_target_again_replaces_its_level() {
    let mut levels = Levels::defaults();
    levels.set("game::menu", LevelFilter::Trace);
    levels.set("game::menu", LevelFilter::Info);
    assert_eq!(levels.of("game::menu"), LevelFilter::Info);
    assert_eq!(levels.most_verbose(), LevelFilter::Info);
  }

  #[test]
  fn ryo_log_reads_levels_and_pairs() {
    let (levels, problems) = Levels::parse(" Debug, wgpu=WARN ,game::menu=trace");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(levels.of("game::x"), LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d::core"), LevelFilter::Debug);
    assert_eq!(levels.of("naga::front"), LevelFilter::Debug);
    assert_eq!(levels.of("wgpu"), LevelFilter::Warn);
    assert_eq!(levels.of("wgpu::device"), LevelFilter::Warn);
    assert_eq!(levels.of("game::menu::options"), LevelFilter::Trace);
    assert_eq!(levels.most_verbose(), LevelFilter::Trace);

    let (empty, problems) = Levels::parse("");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(empty.most_verbose(), LevelFilter::Info);
  }

  #[test]
  fn a_bad_ryo_log_entry_is_reported_and_skipped() {
    let (levels, problems) = Levels::parse("loud,wgpu=7");
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].contains("loud"), "{problems:?}");
    assert!(problems[1].contains("wgpu=7"), "{problems:?}");
    assert_eq!(levels.of("wgpu"), LevelFilter::Warn);
    assert_eq!(levels.of("game"), LevelFilter::Info);
    assert_eq!(levels.most_verbose(), LevelFilter::Info);

    let (levels, problems) = Levels::parse("=debug,warn");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(levels.of("game"), LevelFilter::Warn);
  }
}
