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

/// The levels as `log:level` lists them
#[cfg(dev_tools)]
#[derive(Debug)]
pub(crate) struct LevelReport {
  /// `game`, `engine` and `library`: each origin's default
  pub(crate) defaults: [(&'static str, LevelFilter); 3],
  /// `(target prefix, level)`, in the order set
  pub(crate) overrides: Vec<(String, LevelFilter)>,
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
  /// a level (for every origin) or `target=level` (`game`, `engine` and
  /// `library` naming the defaults, as in [`set`](Self::set)). Order does
  /// not matter: the bare level applies first (the last one, if there are
  /// several), then every `target=level`. A bad entry is skipped and gives
  /// one line in the second value.
  pub(crate) fn parse(text: &str) -> (Self, Vec<String>) {
    let mut levels = Self::defaults();
    let mut problems = Vec::new();
    let entries: Vec<&str> = text
      .split(',')
      .map(str::trim)
      .filter(|e| !e.is_empty())
      .collect();
    let mut bare = None;
    for entry in entries.iter().filter(|entry| !entry.contains('=')) {
      match parse_level(entry) {
        Some(level) => bare = Some(level),
        None => problems.push(format!(
          "RYO_LOG: `{entry}` is not a level ({LEVEL_NAMES}); skipped"
        )),
      }
    }
    if let Some(level) = bare {
      levels.game = level;
      levels.engine = level;
      levels.library = level;
    }
    for entry in entries {
      match entry.split_once('=') {
        None => {}
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

  /// The level `target` is read as: for `game`, `engine` and `library`,
  /// that origin's default; for any other target, [`of`](Self::of).
  pub(crate) fn read(&self, target: &str) -> LevelFilter {
    match reserved(target) {
      Some(origin) => self.default(origin),
      None => self.of(target),
    }
  }

  /// Sets the level of `target` and everything under it, replacing an
  /// earlier override of the same target. `game`, `engine` and `library`
  /// set that origin's default instead (every `game::…` target; every
  /// `ryo2d…` and `ryo2d_…` one; every other one), for each target of it
  /// with no nearer override. So a crate whose target is exactly `engine`
  /// or `library` cannot be set at its root, though its sub-targets can.
  pub(crate) fn set(&mut self, target: &str, level: LevelFilter) {
    if let Some(origin) = reserved(target) {
      self.set_default(origin, level);
      return;
    }
    match self
      .overrides
      .iter_mut()
      .find(|(prefix, _)| prefix == target)
    {
      Some((_, old)) => *old = level,
      None => self.overrides.push((target.to_owned(), level)),
    }
  }

  /// The default level of `origin`'s targets: those under no override
  pub(crate) fn default(&self, origin: Origin) -> LevelFilter {
    match origin {
      Origin::Game => self.game,
      Origin::Engine => self.engine,
      Origin::Library => self.library,
    }
  }

  /// Sets the default level of `origin`'s targets
  pub(crate) fn set_default(&mut self, origin: Origin, level: LevelFilter) {
    match origin {
      Origin::Game => self.game = level,
      Origin::Engine => self.engine = level,
      Origin::Library => self.library = level,
    }
  }

  /// The three defaults and each target's own level, in the order set
  #[cfg(dev_tools)]
  pub(crate) fn report(&self) -> LevelReport {
    LevelReport {
      defaults: [
        ("game", self.game),
        ("engine", self.engine),
        ("library", self.library),
      ],
      overrides: self.overrides.clone(),
    }
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

/// The origin whose default a reserved word names: `game`, `engine` or
/// `library`
fn reserved(target: &str) -> Option<Origin> {
  match target {
    "game" => Some(Origin::Game),
    "engine" => Some(Origin::Engine),
    "library" => Some(Origin::Library),
    _ => None,
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
  fn engine_and_library_set_their_defaults_in_ryo_log() {
    let (levels, problems) = Levels::parse("engine=debug");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(levels.of("ryo2d::core::audio"), LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d_procgen::tilemap"), LevelFilter::Debug);
    assert_eq!(levels.of("wgpu_core"), LevelFilter::Warn);
    assert_eq!(levels.read("engine"), LevelFilter::Debug);

    let (levels, _) = Levels::parse("library=error");
    assert_eq!(levels.of("wgpu_core"), LevelFilter::Error);
    assert_eq!(levels.read("library"), LevelFilter::Error);
    assert_eq!(levels.of("ryo2d::core::audio"), LevelFilter::Info);

    // A nearer override still wins over the engine's default
    let (levels, _) = Levels::parse("engine=debug,ryo2d::core::audio=warn");
    assert_eq!(levels.of("ryo2d::core::audio::voice"), LevelFilter::Warn);
    assert_eq!(levels.of("ryo2d::core::render"), LevelFilter::Debug);
  }

  #[test]
  fn the_order_of_ryo_log_s_entries_does_not_matter() {
    let read = |text: &str| {
      let (levels, problems) = Levels::parse(text);
      assert!(problems.is_empty(), "{problems:?}");
      (
        levels.read("game"),
        levels.read("engine"),
        levels.read("library"),
      )
    };
    let expected = (LevelFilter::Debug, LevelFilter::Warn, LevelFilter::Warn);
    assert_eq!(read("game=debug,warn"), expected);
    assert_eq!(read("warn,game=debug"), expected);
    // The last bare level wins, still under the named entries
    assert_eq!(read("trace,game=debug,warn"), expected);
  }

  #[test]
  fn the_three_words_set_the_defaults_through_set() {
    let mut levels = Levels::defaults();
    levels.set("engine", LevelFilter::Debug);
    levels.set("library", LevelFilter::Error);
    levels.set("game", LevelFilter::Trace);
    assert_eq!(levels.of("ryo2d::core::console"), LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d_procgen"), LevelFilter::Debug);
    assert_eq!(levels.of("wgpu_core"), LevelFilter::Error);
    assert_eq!(levels.of("game::menu"), LevelFilter::Trace);
    assert!(levels.overrides.is_empty(), "{:?}", levels.overrides);
    // A crate whose target is exactly `engine` cannot be set at its root,
    // but its sub-targets can
    levels.set("engine::physics", LevelFilter::Off);
    assert_eq!(levels.of("engine::physics::step"), LevelFilter::Off);
  }

  #[cfg(dev_tools)]
  #[test]
  fn the_three_defaults_are_set_by_origin_and_game_is_its_default() {
    let mut levels = Levels::defaults();
    levels.set_default(Origin::Engine, LevelFilter::Debug);
    assert_eq!(levels.of("ryo2d::core::console"), LevelFilter::Debug);
    assert_eq!(levels.default(Origin::Engine), LevelFilter::Debug);
    levels.set_default(Origin::Library, LevelFilter::Error);
    assert_eq!(levels.of("wgpu_core"), LevelFilter::Error);
    // The prefix `game` is the game's default, not an override beside it
    levels.set("game", LevelFilter::Trace);
    assert_eq!(levels.default(Origin::Game), LevelFilter::Trace);
    assert_eq!(levels.of("game::menu"), LevelFilter::Trace);
    let report = levels.report();
    assert!(report.overrides.is_empty(), "{:?}", report.overrides);
    assert_eq!(
      report.defaults,
      [
        ("game", LevelFilter::Trace),
        ("engine", LevelFilter::Debug),
        ("library", LevelFilter::Error)
      ]
    );
  }

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
