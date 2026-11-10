//! The console runs commands in the game: a scene's command gets its scene,
//! a builder's command gets the world, and the built-ins use the engine's
//! own state. Driven through `HeadlessEngine::console_run`, which runs a
//! line as if typed and returns what it printed.
#![cfg(dev_tools)]

use ryo2d::prelude::*;
use ryo2d::testing::{HeadlessEngine, StepOutcome};

fn engine(builder: EngineBuilder) -> Option<HeadlessEngine> {
  HeadlessEngine::new_or_skip(builder.resolution(64, 64))
}

/// The commands that change the stack, for the tests that push and pop
fn stack_commands(builder: EngineBuilder) -> EngineBuilder {
  builder
    .console_command(("test:push-upper", "Push Upper"), |_, cmd| {
      cmd.act(SceneAction::Push(Box::new(Upper)));
      Ok(())
    })
    .console_command(("test:push-pause", "Push Pause"), |_, cmd| {
      cmd.act(SceneAction::Push(Box::new(Pause)));
      Ok(())
    })
    .console_command(("test:push-busy", "Push Busy"), |_, cmd| {
      cmd.act(SceneAction::Push(Box::new(Busy)));
      Ok(())
    })
    .console_command(("test:pop", "Pop the top scene"), |_, cmd| {
      cmd.act(SceneAction::Pop);
      Ok(())
    })
    .console_command(("test:switch", "Switch to Plain"), |_, cmd| {
      cmd.act(SceneAction::Switch(Box::new(Plain)));
      Ok(())
    })
}

fn unknown(name: &str) -> Vec<String> {
  vec![format!("Unknown command: {name}. Type 'help'.")]
}

/// A scene with nothing of its own
struct Plain;
impl Scene for Plain {}

#[derive(Component)]
struct Spawned;

/// A scene whose command bumps its own count and spawns an entity
#[derive(Default)]
struct Counter {
  count: u32,
}

impl Scene for Counter {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().command(
      ("game:bump", "Bump the count"),
      |scene: &mut Counter, ctx, cmd| {
        scene.count += 1;
        ctx.world.spawn(Spawned);
        cmd.print(format!("count {}", scene.count));
        Ok(())
      },
    );
  }
}

fn spawned(engine: &mut HeadlessEngine) -> usize {
  let world = engine.world_mut();
  world.query::<&Spawned>().iter(world).count()
}

#[test]
fn a_scene_s_command_calls_its_scene_and_changes_the_world() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Counter::default())) else {
    return;
  };
  e.step().unwrap();
  assert_eq!(e.console_run("game:bump"), ["count 1"]);
  assert_eq!(e.console_run("game:bump"), ["count 2"], "the same scene");
  assert_eq!(spawned(&mut e), 2);
}

#[test]
fn a_name_typed_with_capitals_runs() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Counter::default())) else {
    return;
  };
  assert_eq!(e.console_run("GAME:Bump"), ["count 1"]);
}

#[derive(Resource, Default)]
struct ArrivedUpdates(u32);

struct Leaving;
impl Scene for Leaving {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().command(
      ("game:leave", "Switch to Arrived"),
      |_: &mut Leaving, _, cmd| {
        cmd.act(SceneAction::Switch(Box::new(Arrived)));
        Ok(())
      },
    );
  }
}

struct Arrived;
impl Scene for Arrived {
  fn update(&mut self, ctx: &mut SceneContext, _dt: f32) -> SceneAction {
    ctx.world.resource_mut::<ArrivedUpdates>().0 += 1;
    SceneAction::Continue
  }
}

#[test]
fn a_scene_s_command_can_switch_scene() {
  let Some(mut e) = engine(
    EngineBuilder::new()
      .insert_resource(ArrivedUpdates::default())
      .start_scene(Leaving),
  ) else {
    return;
  };
  e.step().unwrap();
  assert!(e.console_run("game:leave").is_empty());
  e.step().unwrap();
  assert!(
    e.world().resource::<ArrivedUpdates>().0 > 0,
    "the next frame runs the new scene"
  );
  assert_eq!(e.console_run("game:leave"), unknown("game:leave"));
}

/// Prints the type of the scene it was called with
fn who<S: Scene>(
  scene: &mut S,
  _ctx: &mut SceneContext<'_>,
  cmd: &mut Command<'_>,
) -> Result<(), String> {
  cmd.print(scene.name().to_string());
  Ok(())
}

struct Lower;
impl Scene for Lower {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .console()
      .command(("game:who", "Who answers"), who::<Lower>);
  }
}

struct Upper;
impl Scene for Upper {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .console()
      .command(("game:who", "Who answers"), who::<Upper>);
  }
  fn updates_below(&self) -> bool {
    true
  }
}

struct Pause;
impl Scene for Pause {}

#[test]
fn the_upper_scene_answers_and_the_lower_one_returns() {
  let Some(mut e) = engine(stack_commands(EngineBuilder::new()).start_scene(Lower)) else {
    return;
  };
  assert_eq!(e.console_run("game:who"), ["Lower"]);
  e.console_run("test:push-upper");
  assert_eq!(e.console_run("game:who"), ["Upper"]);
  e.console_run("test:push-pause");
  e.step().unwrap();
  assert_eq!(
    e.console_run("game:who"),
    ["Upper"],
    "a pause over them changes nothing"
  );
  e.console_run("test:pop");
  assert_eq!(e.console_run("game:who"), ["Upper"]);
  e.console_run("test:pop");
  e.step().unwrap();
  assert_eq!(
    e.console_run("game:who"),
    ["Lower"],
    "the lower one returns"
  );
}

/// A scene whose commands change the console and pop it while they run
struct Busy;
impl Scene for Busy {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    let mut console = ctx.console();
    console.command(("game:other", "Another"), |_: &mut Busy, _, cmd| {
      cmd.print("other");
      Ok(())
    });
    console.command(
      ("game:add", "Add a command while running"),
      |_: &mut Busy, ctx, _| {
        ctx.console().command(
          ("game:late", "Added while running"),
          |_: &mut Busy, _, cmd| {
            cmd.print("late");
            Ok(())
          },
        );
        Ok(())
      },
    );
    console.command(
      ("game:busy", "Change the console and leave"),
      |_: &mut Busy, ctx, cmd| {
        let removed = ctx.console().remove("game:other");
        ctx.console().command(
          ("game:later", "Added while leaving"),
          |_: &mut Busy, _, _| Ok(()),
        );
        cmd.print(format!("removed {removed}"));
        cmd.act(SceneAction::Pop);
        Ok(())
      },
    );
  }
}

#[test]
fn a_handler_can_change_the_console_and_pop_its_scene() {
  let Some(mut e) = engine(stack_commands(EngineBuilder::new()).start_scene(Plain)) else {
    return;
  };
  e.console_run("test:push-busy");
  assert!(e.console_run("game:add").is_empty());
  assert_eq!(e.console_run("game:late"), ["late"], "added while it ran");
  assert_eq!(e.console_run("game:busy"), ["removed true"]);
  e.step().unwrap();
  for name in [
    "game:other",
    "game:add",
    "game:late",
    "game:busy",
    "game:later",
  ] {
    assert_eq!(
      e.console_run(name),
      unknown(name),
      "{name} left with its scene"
    );
  }
  let scenes = e.console_run("system:scenes").join("\n");
  assert!(
    scenes.contains("Plain") && !scenes.contains("Busy"),
    "{scenes}"
  );
}

#[derive(Resource, Default)]
struct Score(u32);

fn scoring(builder: EngineBuilder) -> EngineBuilder {
  builder.insert_resource(Score::default()).console_command(
    CommandSpec::new("game:score", "Add to the score").usage("game:score <points>"),
    |world, cmd| {
      let points: u32 = cmd.arg(0)?;
      world.resource_mut::<Score>().0 += points;
      cmd.print(format!("score {}", world.resource::<Score>().0));
      Ok(())
    },
  )
}

#[test]
fn a_builder_command_changes_the_world_and_outlives_scenes() {
  let Some(mut e) =
    engine(scoring(stack_commands(EngineBuilder::new())).start_scene(Counter::default()))
  else {
    return;
  };
  assert_eq!(e.console_run("game:score 5"), ["score 5"]);
  e.console_run("test:switch");
  e.step().unwrap();
  assert_eq!(e.console_run("game:score 2"), ["score 7"]);
  assert_eq!(e.world().resource::<Score>().0, 7);
}

#[test]
fn a_bad_argument_prints_the_usage() {
  let Some(mut e) = engine(scoring(EngineBuilder::new()).start_scene(Plain)) else {
    return;
  };
  assert_eq!(
    e.console_run("game:score abc"),
    ["argument 1 (\"abc\") is not a number; usage: game:score <points>"]
  );
  assert_eq!(e.world().resource::<Score>().0, 0);
}

#[test]
fn a_builder_command_replaces_a_built_in_of_its_name() {
  let Some(mut e) = engine(
    EngineBuilder::new()
      .console_command(("help", "The game's help"), |_, cmd| {
        cmd.print("the game's help");
        Ok(())
      })
      .start_scene(Plain),
  ) else {
    return;
  };
  assert_eq!(e.console_run("help"), ["the game's help"]);
}

#[test]
fn removing_a_built_in_takes_it_from_help_too() {
  let Some(mut e) = engine(
    EngineBuilder::new()
      .remove_console_command("system:version")
      .start_scene(Plain),
  ) else {
    return;
  };
  let help = e.console_run("help").join("\n");
  assert!(help.contains("system:timescale"), "{help}");
  assert!(!help.contains("system:version"), "{help}");
  assert_eq!(e.console_run("system:version"), unknown("system:version"));
}

/// Registers names no command may have
struct BadNames;
impl Scene for BadNames {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx
      .console()
      .command(("game:bad name", "Spaced"), |_: &mut BadNames, _, _| Ok(()));
  }
}

#[test]
fn a_name_that_is_empty_or_has_spaces_is_refused() {
  let log = ryo2d::testing::capture_log();
  let Some(mut e) = engine(
    EngineBuilder::new()
      .console_command(("game:two words", "Spaced"), |_, _| Ok(()))
      .console_command(("", "Unnamed"), |_, _| Ok(()))
      .start_scene(BadNames),
  ) else {
    return;
  };
  let warnings = log.warnings();
  for name in ["\"game:two words\"", "\"\"", "\"game:bad name\""] {
    assert_eq!(
      warnings.iter().filter(|w| w.contains(name)).count(),
      1,
      "one warning names {name}: {warnings:?}"
    );
  }
  let help = e.console_run("help").join("\n");
  assert!(
    !help.contains("words") && !help.contains("bad name"),
    "{help}"
  );
  assert_eq!(e.console_run("game:two"), unknown("game:two"));
}

#[test]
fn an_empty_line_does_nothing() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  assert!(e.console_run("").is_empty());
  assert!(e.console_run("   ").is_empty());
  assert!(e.console_run("\"\"").is_empty());
}

#[test]
fn quit_asks_the_engine_to_quit() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  assert_eq!(e.step().unwrap(), StepOutcome::Running);
  assert_eq!(e.console_run("quit"), ["Shutting down..."]);
  assert_eq!(e.step().unwrap(), StepOutcome::Quit);
}

#[test]
fn an_unknown_command_says_so() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  assert_eq!(e.console_run("nope"), unknown("nope"));
  assert_eq!(e.console_run("NoPe now"), unknown("nope"));
}

#[test]
fn timescale_sets_the_clock_and_refuses_nan() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  let speed = |e: &HeadlessEngine| e.world().resource::<Time>().game.speed();
  assert_eq!(
    e.console_run("system:timescale"),
    ["Current time scale: 1.00"]
  );
  assert_eq!(
    e.console_run("system:timescale 0.5"),
    ["Time scale set to 0.50"]
  );
  assert_eq!(speed(&e), 0.5);
  for bad in ["nan", "NaN", "-1", "101", "inf"] {
    let said = e.console_run(&format!("system:timescale {bad}"));
    assert_eq!(said.len(), 1, "{bad}: {said:?}");
    assert_eq!(speed(&e), 0.5, "{bad} is refused");
  }
  assert_eq!(
    e.console_run("system:timescale 100"),
    ["Time scale set to 100.00"]
  );
  assert_eq!(speed(&e), 100.0);
}

#[test]
fn log_level_shows_and_sets() {
  use ryo2d::core::logging::{level, set_level};
  // A target nothing else logs to: levels are process-wide
  let target = "game::console_level_test";
  let before = level(target);
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  assert_eq!(
    e.console_run(&format!("log:level {target} debug")),
    [format!("{target}: debug")]
  );
  assert_eq!(level(target), log::LevelFilter::Debug);
  assert_eq!(
    e.console_run(&format!("log:level {target}")),
    [format!("{target}: debug")]
  );
  let all = e.console_run("log:level");
  let at = |line: &str| {
    all
      .iter()
      .position(|l| l.trim() == line)
      .unwrap_or_else(|| panic!("{line} in {all:#?}"))
  };
  let (defaults, overrides) = (at("defaults:"), at("overrides:"));
  for name in ["game", "engine", "library"] {
    let line = all
      .iter()
      .position(|l| l.trim_start().starts_with(&format!("{name}: ")))
      .unwrap_or_else(|| panic!("{name} in {all:#?}"));
    assert!(
      defaults < line && line < overrides,
      "{name} is a default: {all:#?}"
    );
  }
  let own = at(&format!("{target}: debug"));
  assert!(own > overrides, "{target} is an override: {all:#?}");
  let bad = e.console_run(&format!("log:level {target} loud"));
  assert_eq!(bad.len(), 1, "{bad:?}");
  assert!(
    bad[0].contains("usage: log:level [target] [level]") && bad[0].contains("trace"),
    "{bad:?}"
  );
  assert_eq!(
    level(target),
    log::LevelFilter::Debug,
    "a bad level changes nothing"
  );
  set_level(target, before);
}

#[test]
fn scenes_entities_and_assets_report() {
  let Some(mut e) = engine(stack_commands(EngineBuilder::new()).start_scene(Counter::default()))
  else {
    return;
  };
  e.console_run("test:push-pause");
  let scenes = e.console_run("system:scenes");
  assert_eq!(
    scenes.len(),
    3,
    "a heading and a line per scene: {scenes:?}"
  );
  assert!(scenes[1].contains("Pause"), "top first: {scenes:?}");
  assert!(scenes[2].contains("Counter"), "{scenes:?}");
  assert!(
    scenes[1].contains("updates") && scenes[2].contains("stopped"),
    "{scenes:?}"
  );

  let count = |e: &mut HeadlessEngine| -> usize {
    let said = e.console_run("system:entities");
    said[0]
      .rsplit(' ')
      .next()
      .and_then(|n| n.parse().ok())
      .unwrap_or_else(|| panic!("{said:?}"))
  };
  let before = count(&mut e);
  e.console_run("test:pop");
  e.console_run("game:bump");
  e.console_run("game:bump");
  assert_eq!(count(&mut e), before + 2);

  let assets = e.console_run("system:assets");
  assert!(assets[0].starts_with("Assets"), "{assets:?}");
  assert!(
    assets.iter().any(|line| line.contains("Image")),
    "{assets:?}"
  );
}

#[test]
fn no_command_names_an_ip() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Plain)) else {
    return;
  };
  let help = e.console_run("help");
  assert!(help.len() > 10, "{help:?}");
  assert!(help.iter().all(|line| !line.contains(":ip")), "{help:?}");
  assert_eq!(e.console_run("system:ip:lan"), unknown("system:ip:lan"));
}

#[test]
fn a_change_of_scene_asked_for_during_a_transition_says_it_was_dropped() {
  let Some(mut e) = engine(
    stack_commands(EngineBuilder::new())
      .console_command(("test:fade", "Fade to Pause"), |_, cmd| {
        cmd.act(SceneAction::PushWith(
          Box::new(Pause),
          Transition::fade(std::time::Duration::from_secs(5)),
        ));
        Ok(())
      })
      .start_scene(Plain),
  ) else {
    return;
  };
  assert!(e.console_run("test:fade").is_empty());
  assert_eq!(
    e.console_run("test:push-upper"),
    ["test:push-upper: a scene transition is running; its Push was dropped"]
  );
  let scenes = e.console_run("system:scenes").join("\n");
  assert!(!scenes.contains("Upper"), "{scenes}");
}

/// Prints through the console and through the command
struct Printer;
impl Scene for Printer {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    ctx.console().command(
      ("game:print", "Print both ways"),
      |_: &mut Printer, ctx, cmd| {
        cmd.print("through the command");
        ctx.console().print("through the console");
        Ok(())
      },
    );
  }
}

#[test]
fn a_line_printed_through_the_console_comes_before_the_command_s_own() {
  let Some(mut e) = engine(EngineBuilder::new().start_scene(Printer)) else {
    return;
  };
  assert_eq!(
    e.console_run("game:print"),
    ["through the console", "through the command"]
  );
}

/// Removes a built-in as it enters, and reports removing an unknown name
struct Remover;
impl Scene for Remover {
  fn on_enter(&mut self, ctx: &mut SceneContext) {
    assert!(ctx.console().remove("system:version"));
    ctx.console().command(
      ("game:remove-nothing", "Remove an unknown name"),
      |_: &mut Remover, ctx, cmd| {
        cmd.print(format!("removed {}", ctx.console().remove("game:nothing")));
        Ok(())
      },
    );
  }
}

#[test]
fn a_scene_can_remove_a_built_in_and_removing_an_unknown_name_is_false() {
  let Some(mut e) = engine(
    EngineBuilder::new()
      .remove_console_command("game:never-added")
      .start_scene(Remover),
  ) else {
    return;
  };
  assert_eq!(e.console_run("system:version"), unknown("system:version"));
  let help = e.console_run("help").join("\n");
  assert!(!help.contains("system:version"), "{help}");
  assert!(
    help.contains("system:timescale"),
    "the builder's unknown removal took nothing: {help}"
  );
  assert_eq!(e.console_run("game:remove-nothing"), ["removed false"]);
}

#[test]
fn a_command_that_pops_the_last_scene_quits() {
  let Some(mut e) = engine(stack_commands(EngineBuilder::new()).start_scene(Plain)) else {
    return;
  };
  assert_eq!(e.step().unwrap(), StepOutcome::Running);
  assert!(e.console_run("test:pop").is_empty());
  assert_eq!(e.step().unwrap(), StepOutcome::Quit);
}
