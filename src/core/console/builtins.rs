//! The engine's own commands. Each runs with the world and the engine's
//! hooks ([`EngineHooks`]): the debug flags, the overlay mode F1 cycles,
//! the frame capture and the quit request. Names use a `category:command`
//! form (`debug:fps`); `help`, `clear`, `quit` and `exit` have none.

use std::path::PathBuf;
use std::sync::Arc;

use log::LevelFilter;

use super::Console;
use super::colors;
use super::command::{Command, CommandSpec};
use super::registry::{ConsoleCommands, EngineHooks, Handler};
use crate::core::asset::AssetServer;
use crate::core::capture::CaptureTarget;
use crate::core::diagnostics::{DebugConfig, OverlayMode};
use crate::core::ecs::World;
use crate::core::engine::SceneHost;
use crate::core::settings::Settings;
use crate::core::time::{GameClock, Time};

/// Add the built-in commands to `commands`.
pub(crate) fn add_builtins(commands: &mut ConsoleCommands) {
  let mut add =
    |spec: CommandSpec,
     run: fn(&mut World, &mut EngineHooks<'_>, &mut Command<'_>) -> Result<(), String>| {
      commands.add(spec, Handler::Builtin(Arc::new(run)));
    };

  add(
    CommandSpec::new("help", "List the commands, or show one's usage and help")
      .usage("help [command]"),
    help,
  );
  add(
    CommandSpec::new("clear", "Clear the console's output"),
    |world, _, _| {
      if let Some(mut console) = world.get_resource_mut::<Console>() {
        console.clear();
      }
      Ok(())
    },
  );
  add(CommandSpec::new("quit", "Exit the game"), quit);
  add(CommandSpec::new("exit", "Exit the game (as quit)"), quit);

  add(
    CommandSpec::new("debug:fps", "Toggle the FPS counter (F1)"),
    fps,
  );
  add(
    CommandSpec::new("debug:stats", "Toggle the statistics overlay (F1)"),
    |world, hooks, cmd| toggle_overlay(world, hooks, cmd, OverlayMode::FullStats),
  );
  add(
    CommandSpec::new("debug:timing", "Toggle the frame timing overlay (F1)"),
    |world, hooks, cmd| toggle_overlay(world, hooks, cmd, OverlayMode::FrameTiming),
  );
  add(
    CommandSpec::new("debug:colliders", "Toggle collision boundaries (F2)"),
    |_, hooks, cmd| {
      flip(
        cmd,
        &mut hooks.debug.show_colliders,
        "Collision boundaries",
        ("visible", "hidden"),
      );
      Ok(())
    },
  );
  add(
    CommandSpec::new("debug:velocities", "Toggle velocity vectors (F3)"),
    |_, hooks, cmd| {
      flip(
        cmd,
        &mut hooks.debug.show_velocities,
        "Velocity vectors",
        ("visible", "hidden"),
      );
      Ok(())
    },
  );
  add(
    CommandSpec::new("debug:origins", "Toggle entity origin points (F4)"),
    |_, hooks, cmd| {
      flip(
        cmd,
        &mut hooks.debug.show_origins,
        "Entity origins",
        ("visible", "hidden"),
      );
      Ok(())
    },
  );
  add(
    CommandSpec::new(
      "debug:ui",
      "Toggle the UI debug overlay (widget rects and ids)",
    ),
    |_, hooks, cmd| {
      flip(
        cmd,
        &mut hooks.debug.show_ui_debug,
        "UI debug overlay",
        ("enabled", "disabled"),
      );
      Ok(())
    },
  );
  add(
    CommandSpec::new(
      "debug:ui-rebuilds",
      "Toggle the overlay outlining UI nodes rebuilt this frame",
    ),
    |_, hooks, cmd| {
      flip(
        cmd,
        &mut hooks.debug.show_ui_rebuilds,
        "UI rebuilds overlay",
        ("enabled", "disabled"),
      );
      Ok(())
    },
  );
  add(
    CommandSpec::new("debug:capture", "Save the next frame as a PNG (F12)")
      .usage("debug:capture [path]"),
    |_, hooks, cmd| {
      let path = cmd.rest(0);
      *hooks.capture = Some(if path.is_empty() {
        CaptureTarget::Default
      } else {
        CaptureTarget::Path(PathBuf::from(path))
      });
      cmd.print("Capturing next frame...");
      Ok(())
    },
  );
  add(
    CommandSpec::new("ui:cache", "Show or set UI node tree caching").usage("ui:cache [on|off]"),
    |_, hooks, cmd| {
      let on = switch(cmd, &mut hooks.debug.ui_cache)?;
      cmd.print(format!("UI caching is {}", on_off(on)));
      Ok(())
    },
  );
  add(
    CommandSpec::new("ui:fragments", "Show or set UI fragments").usage("ui:fragments [on|off]"),
    |_, hooks, cmd| {
      let on = switch(cmd, &mut hooks.debug.ui_fragments)?;
      cmd.print(format!("UI fragments are {}", on_off(on)));
      Ok(())
    },
  );
  add(
    CommandSpec::new("ui:retain", "Show or set UI render retention").usage("ui:retain [on|off]"),
    |_, hooks, cmd| {
      let on = switch(cmd, &mut hooks.debug.ui_retain)?;
      cmd.print(format!("UI render retention is {}", on_off(on)));
      Ok(())
    },
  );

  add(
    CommandSpec::new(
      "system:timescale",
      "Show or set the game clock's speed (1 = normal)",
    )
    .usage("system:timescale [0..100]"),
    timescale,
  );
  add(
    CommandSpec::new(
      "system:version",
      "Show the game's and the engine's versions",
    ),
    version,
  );
  add(
    CommandSpec::new("system:scenes", "Show the scene stack, top first"),
    scenes,
  );
  add(
    CommandSpec::new("system:entities", "Count the game's entities"),
    |world, _, cmd| {
      cmd.print(format!(
        "Entities: {}",
        crate::core::engine::game_entity_count(world)
      ));
      Ok(())
    },
  );
  add(
    CommandSpec::new("system:assets", "Show the asset registry's counts per type"),
    assets,
  );
  add(
    CommandSpec::new("log:level", "Show or set the log's levels")
      .usage(LOG_LEVEL_USAGE)
      .help(
        "With no argument, the three defaults and each target's own level. `game`, \
         `engine` and `library` name the defaults: the level of every game (`game::...`), \
         engine (`ryo2d...`) or other target with no level of its own. Any other word is a \
         target, which covers the targets under it. With a level too (off, error, warn, \
         info, debug, trace), sets it.",
      ),
    log_level,
  );
  add(
    CommandSpec::new(
      "log:show",
      "Show or set the lowest log level the console shows",
    )
    .usage("log:show [level|off]"),
    log_show,
  );
}

fn quit(_: &mut World, _: &mut EngineHooks<'_>, cmd: &mut Command<'_>) -> Result<(), String> {
  cmd.print_colored("Shutting down...", colors::SYSTEM);
  cmd.quit();
  Ok(())
}

/// `help`: every command grouped by the text before its first `:` (those
/// without one first), or one command's description, usage and help
fn help(world: &mut World, _: &mut EngineHooks<'_>, cmd: &mut Command<'_>) -> Result<(), String> {
  let Some(commands) = world.get_resource::<ConsoleCommands>() else {
    return Err("No commands".to_string());
  };
  if let Some(name) = cmd.args().first().cloned() {
    let Some((spec, _)) = commands.find(&name) else {
      return Err(format!("Unknown command: '{name}'. Type 'help'."));
    };
    cmd.print_colored(format!("Command: {}", spec.name), colors::SYSTEM);
    cmd.print(format!("  {}", spec.description));
    if let Some(usage) = &spec.usage {
      cmd.print(format!("  Usage: {usage}"));
    }
    if let Some(help) = &spec.help {
      for line in help.lines() {
        cmd.print(format!("  {line}"));
      }
    }
    return Ok(());
  }

  let list = commands.list();
  let group = |name: &str| name.split_once(':').map(|(group, _)| group.to_string());
  let width = list
    .iter()
    .map(|spec| spec.name.len())
    .max()
    .unwrap_or(0)
    .max(16);
  cmd.print_colored("Available commands:", colors::SYSTEM);
  // Sorted by name, so each group's commands are together; no group first
  let (plain, grouped): (Vec<_>, Vec<_>) =
    list.iter().partition(|spec| group(&spec.name).is_none());
  let mut last_group = None;
  for spec in plain.into_iter().chain(grouped) {
    let this = group(&spec.name);
    if this.is_some() && this != last_group {
      cmd.print_colored("", colors::SYSTEM);
      cmd.print_colored(
        format!("{}:", this.as_deref().unwrap_or_default()),
        colors::SYSTEM,
      );
    }
    cmd.print(format!("  {:<width$} - {}", spec.name, spec.description));
    last_group = this;
  }
  cmd.print_colored("", colors::SYSTEM);
  cmd.print_colored("Type 'help <command>' for its usage", colors::SYSTEM);
  cmd.print_colored(
    "Press ` to toggle the console, Escape or X to close it",
    colors::SYSTEM,
  );
  cmd.print_colored(
    "Use the mouse wheel or Page Up/Down to scroll",
    colors::SYSTEM,
  );
  Ok(())
}

/// `debug:fps`: the FPS counter is the player's setting, so it goes into
/// `Settings` too, and the overlay mode F1 cycles follows it
fn fps(
  world: &mut World,
  hooks: &mut EngineHooks<'_>,
  cmd: &mut Command<'_>,
) -> Result<(), String> {
  let show = !hooks.debug.show_fps;
  hooks.debug.show_fps = show;
  if let Some(mut settings) = world.get_resource_mut::<Settings>()
    && settings.show_fps() != show
  {
    settings.set_show_fps(show);
  }
  let debug = &hooks.debug;
  *hooks.overlay = OverlayMode::from_flags(show, debug.show_frame_timing, debug.show_stats);
  cmd.print(if show {
    "FPS display enabled"
  } else {
    "FPS display disabled"
  });
  Ok(())
}

/// `debug:stats` and `debug:timing`: the overlay mode F1 would reach for
/// `mode`, or no overlay when it is shown already, so F1 carries on from it
fn toggle_overlay(
  world: &mut World,
  hooks: &mut EngineHooks<'_>,
  cmd: &mut Command<'_>,
  mode: OverlayMode,
) -> Result<(), String> {
  let next = if *hooks.overlay == mode {
    OverlayMode::Off
  } else {
    mode
  };
  set_overlay(world, hooks.debug, hooks.overlay, next);
  let what = match mode {
    OverlayMode::FullStats => "Debug statistics",
    _ => "Frame timing breakdown",
  };
  cmd.print(format!(
    "{what} {}",
    if next == mode { "visible" } else { "hidden" }
  ));
  Ok(())
}

/// The overlay mode and its flags, as F1 sets them, with the FPS counter in
/// the player's settings
fn set_overlay(
  world: &mut World,
  debug: &mut DebugConfig,
  overlay: &mut OverlayMode,
  mode: OverlayMode,
) {
  *overlay = mode;
  let (show_fps, show_frame_timing, show_stats) = mode.to_flags();
  debug.show_fps = show_fps;
  debug.show_frame_timing = show_frame_timing;
  debug.show_stats = show_stats;
  if let Some(mut settings) = world.get_resource_mut::<Settings>()
    && settings.show_fps() != show_fps
  {
    settings.set_show_fps(show_fps);
  }
}

/// Flip `flag` and say so: `<what> visible` or `<what> hidden`
fn flip(cmd: &mut Command<'_>, flag: &mut bool, what: &str, (on, off): (&str, &str)) {
  *flag = !*flag;
  cmd.print(format!("{what} {}", if *flag { on } else { off }));
}

/// Set `flag` from an `on` or `off` argument (none leaves it); its value
fn switch(cmd: &Command<'_>, flag: &mut bool) -> Result<bool, String> {
  match cmd
    .args()
    .first()
    .map(|arg| arg.to_ascii_lowercase())
    .as_deref()
  {
    None => {}
    Some("on") => *flag = true,
    Some("off") => *flag = false,
    Some(_) => return Err(format!("Usage: {} [on|off]", cmd.name())),
  }
  Ok(*flag)
}

fn on_off(on: bool) -> &'static str {
  if on { "on" } else { "off" }
}

/// `system:timescale`: the game clock's speed, within the clock's own range
fn timescale(
  world: &mut World,
  _: &mut EngineHooks<'_>,
  cmd: &mut Command<'_>,
) -> Result<(), String> {
  let Some(mut time) = world.get_resource_mut::<Time>() else {
    return Err("No game clock".to_string());
  };
  if cmd.args().is_empty() {
    cmd.print(format!("Current time scale: {:.2}", time.game.speed()));
    return Ok(());
  }
  let scale: f32 = cmd.arg(0)?;
  // NaN is in no range
  if !(0.0..=GameClock::MAX_SPEED).contains(&scale) {
    return Err(format!(
      "Time scale must be from 0 to {}, not {scale}",
      GameClock::MAX_SPEED
    ));
  }
  time.game.set_speed(scale);
  cmd.print(format!("Time scale set to {scale:.2}"));
  Ok(())
}

fn version(
  _: &mut World,
  hooks: &mut EngineHooks<'_>,
  cmd: &mut Command<'_>,
) -> Result<(), String> {
  if let Some(name) = hooks.game_name {
    cmd.print(name);
    if let Some(version) = hooks.game_version {
      cmd.print(format!("  Version: {version}"));
    }
    cmd.print("");
  }
  cmd.print("Ryo2D Game Engine");
  cmd.print(format!("  Engine: {}", env!("CARGO_PKG_VERSION")));
  cmd.print(format!(
    "  Build:  {}",
    if cfg!(debug_assertions) {
      "Dev"
    } else {
      "Release"
    }
  ));
  Ok(())
}

fn scenes(world: &mut World, _: &mut EngineHooks<'_>, cmd: &mut Command<'_>) -> Result<(), String> {
  let Some(host) = world.get_non_send::<SceneHost>() else {
    return Err("No scene stack".to_string());
  };
  let stack = host.scenes.stack_report();
  cmd.print_colored("Scenes, top first:", colors::SYSTEM);
  for (name, updates) in stack {
    let state = if updates { "updates" } else { "stopped" };
    cmd.print(format!("  {name} ({state})"));
  }
  Ok(())
}

fn assets(world: &mut World, _: &mut EngineHooks<'_>, cmd: &mut Command<'_>) -> Result<(), String> {
  let Some(server) = world.get_resource::<AssetServer>() else {
    return Err("No asset registry".to_string());
  };
  let stats = server.stats();
  cmd.print_colored("Assets (loaded by path):", colors::SYSTEM);
  for kind in stats.types {
    cmd.print(format!(
      "  {}: {} loaded, {} loading, {} failed",
      kind.name, kind.loaded, kind.loading, kind.failed
    ));
  }
  Ok(())
}

const LOG_LEVEL_USAGE: &str = "log:level [target] [level]";
const LOG_SHOW_USAGE: &str = "log:show [level|off]";

/// Argument `index` as a level's name, as `RYO_LOG` takes it; else the
/// valid names and `usage`
fn parse_level(cmd: &Command<'_>, index: usize, usage: &str) -> Result<LevelFilter, String> {
  let text = cmd
    .args()
    .get(index)
    .map(String::as_str)
    .unwrap_or_default();
  crate::core::logging::parse_level(text).ok_or_else(|| {
    format!("{text:?} is not a level (off, error, warn, info, debug, trace); usage: {usage}")
  })
}

fn level_name(level: LevelFilter) -> String {
  level.as_str().to_ascii_lowercase()
}

/// `log:level`: `game`, `engine` and `library` name the three defaults (as
/// everywhere a level is set); any other word is a target prefix with a
/// level of its own
fn log_level(_: &mut World, _: &mut EngineHooks<'_>, cmd: &mut Command<'_>) -> Result<(), String> {
  use crate::core::logging::{level, set_level};
  let target = cmd.args().first().cloned();
  let new_level = match cmd.args().len() {
    0 | 1 => None,
    _ => Some(parse_level(cmd, 1, LOG_LEVEL_USAGE)?),
  };
  let Some(target) = target else {
    let Some(report) = crate::core::logging::level_report() else {
      return Err("The engine's log is not installed".to_string());
    };
    cmd.print_colored("Log levels:", colors::SYSTEM);
    cmd.print_colored("  defaults:", colors::SYSTEM);
    for (origin, level) in report.defaults {
      cmd.print(format!("    {origin}: {}", level_name(level)));
    }
    cmd.print_colored("  overrides:", colors::SYSTEM);
    if report.overrides.is_empty() {
      cmd.print("    (none)");
    }
    for (prefix, level) in report.overrides {
      cmd.print(format!("    {prefix}: {}", level_name(level)));
    }
    return Ok(());
  };
  if let Some(new) = new_level {
    set_level(&target, new);
  }
  cmd.print(format!("{target}: {}", level_name(level(&target))));
  Ok(())
}

fn log_show(
  world: &mut World,
  _: &mut EngineHooks<'_>,
  cmd: &mut Command<'_>,
) -> Result<(), String> {
  let level = match cmd.args().is_empty() {
    true => None,
    false => Some(parse_level(cmd, 0, LOG_SHOW_USAGE)?),
  };
  let Some(mut console) = world.get_resource_mut::<Console>() else {
    return Err("No console".to_string());
  };
  if let Some(level) = level {
    console.log_show = level;
  }
  let shown = console.log_show;
  cmd.print(if shown == LevelFilter::Off {
    "The console shows no log records".to_string()
  } else {
    format!(
      "The console shows log records at {} and above",
      level_name(shown)
    )
  });
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::console::dispatch::run_line;

  /// A world with the console, its commands and a clock, the hooks' state,
  /// and what the last line asked to capture
  struct Rig {
    world: World,
    debug: DebugConfig,
    overlay: OverlayMode,
    capture: Option<CaptureTarget>,
    quit: bool,
  }

  impl Rig {
    fn new() -> Self {
      let mut world = World::new();
      let mut commands = ConsoleCommands::default();
      add_builtins(&mut commands);
      world.insert_resource(commands);
      world.insert_resource(Console::new());
      world.insert_resource(Time::new(60));
      Self {
        world,
        debug: DebugConfig::default(),
        overlay: OverlayMode::Off,
        capture: None,
        quit: false,
      }
    }

    fn run(&mut self, line: &str) -> Vec<String> {
      self.world.resource_mut::<Console>().capture();
      let mut hooks = EngineHooks {
        debug: &mut self.debug,
        overlay: &mut self.overlay,
        capture: &mut self.capture,
        quit: &mut self.quit,
        game_name: Some("Test Game"),
        game_version: Some("1.2.3"),
      };
      run_line(&mut self.world, &mut hooks, line);
      self.world.resource_mut::<Console>().take_captured()
    }
  }

  #[test]
  fn capture_takes_the_default_target_or_a_path_with_spaces() {
    let mut rig = Rig::new();
    rig.run("debug:capture");
    assert_eq!(rig.capture.take(), Some(CaptureTarget::Default));
    rig.run("debug:capture shots/my shot.png");
    assert_eq!(
      rig.capture,
      Some(CaptureTarget::Path(PathBuf::from("shots/my shot.png")))
    );
  }

  #[test]
  fn the_debug_overlays_toggle() {
    let mut rig = Rig::new();
    assert_eq!(rig.run("debug:ui"), ["UI debug overlay enabled"]);
    assert!(rig.debug.show_ui_debug);
    rig.run("debug:ui");
    assert!(!rig.debug.show_ui_debug);
    rig.run("debug:ui-rebuilds");
    assert!(rig.debug.show_ui_rebuilds);
    assert_eq!(rig.run("debug:colliders"), ["Collision boundaries visible"]);
    assert!(rig.debug.show_colliders);
    rig.run("debug:velocities");
    rig.run("debug:origins");
    assert!(rig.debug.show_velocities && rig.debug.show_origins);
  }

  #[test]
  fn the_ui_switches_turn_off_and_on() {
    let mut rig = Rig::new();
    for (name, flag) in [
      (
        "ui:cache",
        (|d: &DebugConfig| d.ui_cache) as fn(&DebugConfig) -> bool,
      ),
      ("ui:fragments", |d: &DebugConfig| d.ui_fragments),
      ("ui:retain", |d: &DebugConfig| d.ui_retain),
    ] {
      assert!(flag(&rig.debug), "{name} on by default");
      rig.run(&format!("{name} off"));
      assert!(!flag(&rig.debug));
      rig.run(name);
      assert!(!flag(&rig.debug), "{name} alone only prints");
      rig.run(&format!("{name} ON"));
      assert!(flag(&rig.debug));
      let said = rig.run(&format!("{name} maybe"));
      assert_eq!(said, [format!("Usage: {name} [on|off]")]);
      assert!(flag(&rig.debug));
    }
  }

  #[test]
  fn version_names_the_game_and_the_engine() {
    let mut rig = Rig::new();
    let said = rig.run("system:version");
    assert_eq!(said[0], "Test Game");
    assert_eq!(said[1], "  Version: 1.2.3");
    assert!(said.contains(&"Ryo2D Game Engine".to_string()));
  }

  #[test]
  fn log_show_keeps_the_console_s_level() {
    let mut rig = Rig::new();
    assert_eq!(rig.world.resource::<Console>().log_show, LevelFilter::Warn);
    assert_eq!(
      rig.run("log:show"),
      ["The console shows log records at warn and above"]
    );
    rig.run("log:show error");
    assert_eq!(rig.world.resource::<Console>().log_show, LevelFilter::Error);
    assert_eq!(
      rig.run("log:show off"),
      ["The console shows no log records"]
    );
    assert_eq!(rig.world.resource::<Console>().log_show, LevelFilter::Off);
    assert_eq!(rig.run("log:show loud").len(), 1);
    assert_eq!(rig.world.resource::<Console>().log_show, LevelFilter::Off);
  }

  #[test]
  fn log_level_names_the_three_defaults() {
    use crate::core::logging::{capture_log, levels_for_test};
    let log = capture_log();
    let mut rig = Rig::new();
    // The defaults are process-wide: hold the levels' turn; dropping it
    // puts every level back, as soon as the lines are run
    let levels = levels_for_test();
    assert_eq!(rig.run("log:level engine debug"), ["engine: debug"]);
    assert_eq!(rig.run("log:level engine"), ["engine: debug"]);
    assert_eq!(
      rig.run("log:level ryo2d::core::console"),
      ["ryo2d::core::console: debug"]
    );
    log::debug!(target: "ryo2d::core::console::level_test", "an engine debug line");
    let listing = rig.run("log:level");
    // Read back only: lowering a default would hide other tests' warnings
    let library = rig.run("log:level library");
    drop(levels);
    assert_eq!(log.containing("an engine debug line"), 1);
    assert!(
      listing.iter().any(|line| line.trim() == "engine: debug"),
      "{listing:?}"
    );
    assert!(
      listing
        .iter()
        .skip_while(|line| line.trim() != "overrides:")
        .all(|line| !line.contains("engine")),
      "the default is no override: {listing:?}"
    );
    assert_eq!(library, ["library: warn"]);
  }

  #[test]
  fn an_unknown_level_prints_the_usage() {
    let mut rig = Rig::new();
    assert_eq!(
      rig.run("log:level engine loud"),
      [
        "\"loud\" is not a level (off, error, warn, info, debug, trace); usage: log:level [target] [level]"
      ]
    );
    assert_eq!(
      rig.run("log:show loud"),
      [
        "\"loud\" is not a level (off, error, warn, info, debug, trace); usage: log:show [level|off]"
      ]
    );
  }

  #[test]
  fn quit_and_exit_ask_to_quit() {
    for name in ["quit", "exit"] {
      let mut rig = Rig::new();
      assert_eq!(rig.run(name), ["Shutting down..."]);
      assert!(rig.quit, "{name}");
    }
  }

  #[test]
  fn clear_empties_the_console() {
    let mut rig = Rig::new();
    rig.run("help");
    rig.run("clear");
    assert_eq!(rig.world.resource::<Console>().lines().count(), 0);
  }
}
