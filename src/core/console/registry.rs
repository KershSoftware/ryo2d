//! The console's commands: per name, a stack of entries, the newest last.
//!
//! A scene's entries carry its scope, so a scene that leaves takes only its
//! own (`clear_scope`), and an earlier scene's entry of the same name answers
//! again. Handlers are `Arc`s, cloned out by `find`, so a handler runs with
//! the registry free to change under it.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::command::{Command, CommandSpec, WorldHandler};
use crate::core::capture::CaptureTarget;
use crate::core::diagnostics::{DebugConfig, OverlayMode};
use crate::core::ecs::{Resource, World};
use crate::core::scene::{Scene, SceneContext};

/// A built-in command's handler: the world and the engine's own state.
pub(crate) type BuiltinHandler =
  dyn Fn(&mut World, &mut EngineHooks<'_>, &mut Command<'_>) -> Result<(), String> + Send + Sync;

/// A scene's handler with its scene type erased (see [`erase`]).
pub(crate) type ErasedSceneHandler = dyn Fn(&mut dyn Scene, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String>
  + Send
  + Sync;

/// What runs a command.
#[derive(Clone)]
pub(crate) enum Handler {
  /// An engine command, with the engine's hooks
  #[expect(dead_code, reason = "the built-ins are not moved over yet")]
  Builtin(Arc<BuiltinHandler>),
  /// A command registered on the builder, for the whole run
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "the builder cannot register commands yet")
  )]
  World(Arc<WorldHandler>),
  /// A scene's command, leaving with the scene of `scope`
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "scenes cannot register commands yet")
  )]
  Scene {
    scope: u64,
    call: Arc<ErasedSceneHandler>,
  },
}

impl Handler {
  /// The scene scope an entry belongs to; `None` for run-long ones.
  fn scope(&self) -> Option<u64> {
    match self {
      Self::Scene { scope, .. } => Some(*scope),
      Self::Builtin(_) | Self::World(_) => None,
    }
  }
}

/// Wrap a handler for scenes of type `S` so it can be called with any
/// scene; called with a scene of another type it refuses, with
/// `"<name> was registered for another scene type"`.
#[cfg_attr(
  not(test),
  expect(dead_code, reason = "scenes cannot register commands yet")
)]
pub(crate) fn erase<S: Scene>(
  handler: impl Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String>
  + Send
  + Sync
  + 'static,
) -> Arc<ErasedSceneHandler> {
  Arc::new(
    move |scene: &mut dyn Scene, ctx: &mut SceneContext<'_>, cmd: &mut Command<'_>| {
      let scene: &mut dyn Any = scene;
      match scene.downcast_mut::<S>() {
        Some(scene) => handler(scene, ctx, cmd),
        None => Err(format!(
          "{} was registered for another scene type",
          cmd.name()
        )),
      }
    },
  )
}

struct Entry {
  spec: CommandSpec,
  handler: Handler,
}

/// Every console command, by lower-case name.
#[derive(Resource, Default)]
pub(crate) struct ConsoleCommands {
  /// Per name, the entries that registered it, the newest (answering) last
  by_name: BTreeMap<String, Vec<Entry>>,
}

impl ConsoleCommands {
  /// Add a command; its name is lower-cased. A name's entries are kept in
  /// scope order: the run-long one (built-in or builder) at the bottom, then
  /// the scenes' by scope id, which grows with each scene made, so a scene
  /// higher on the stack answers over one below it whichever registered
  /// first. An entry of a scope the name already has (one run-long entry
  /// per name) replaces it.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "commands are not registered yet")
  )]
  pub(crate) fn add(&mut self, mut spec: CommandSpec, handler: Handler) {
    spec.name = spec.name.to_lowercase();
    let scope = handler.scope();
    let entries = self.by_name.entry(spec.name.clone()).or_default();
    let entry = Entry { spec, handler };
    // `None` (run-long) sorts below every scene scope
    match entries.binary_search_by_key(&scope, |e| e.handler.scope()) {
      Ok(at) => entries[at] = entry,
      Err(at) => entries.insert(at, entry),
    }
  }

  /// Remove every entry of `name`; whether there was one.
  #[cfg_attr(not(test), expect(dead_code, reason = "commands are not removed yet"))]
  pub(crate) fn remove(&mut self, name: &str) -> bool {
    self.by_name.remove(&name.to_lowercase()).is_some()
  }

  /// Remove the entries of scene `scope`; the names it covered answer to
  /// the entries below, or go.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "scenes cannot register commands yet")
  )]
  pub(crate) fn clear_scope(&mut self, scope: u64) {
    self.by_name.retain(|_, entries| {
      entries.retain(|e| e.handler.scope() != Some(scope));
      !entries.is_empty()
    });
  }

  /// The answering (newest) entry of `name`, cloned out.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "the console does not run these commands yet")
  )]
  pub(crate) fn find(&self, name: &str) -> Option<(CommandSpec, Handler)> {
    let entry = self.by_name.get(&name.to_lowercase())?.last()?;
    Some((entry.spec.clone(), entry.handler.clone()))
  }

  /// The answering entry's spec of each name, sorted by name.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "help does not list these commands yet")
  )]
  pub(crate) fn list(&self) -> Vec<CommandSpec> {
    self
      .by_name
      .values()
      .filter_map(|entries| entries.last().map(|e| e.spec.clone()))
      .collect()
  }

  /// The names starting with `prefix` (in any case), sorted.
  #[cfg_attr(
    not(test),
    expect(dead_code, reason = "the console does not complete names yet")
  )]
  pub(crate) fn completions(&self, prefix: &str) -> Vec<String> {
    let prefix = prefix.to_lowercase();
    self
      .by_name
      .range(prefix.clone()..)
      .map(|(name, _)| name)
      .take_while(|name| name.starts_with(&prefix))
      .cloned()
      .collect()
  }
}

/// The engine's own state a built-in command may change; game commands
/// never see it.
#[expect(dead_code, reason = "the built-ins are not moved over yet")]
pub(crate) struct EngineHooks<'a> {
  pub(crate) debug: &'a mut DebugConfig,
  pub(crate) overlay: &'a mut OverlayMode,
  pub(crate) capture: &'a mut Option<CaptureTarget>,
  pub(crate) quit: &'a mut bool,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::input::Input;
  use crate::core::scene::SceneSlot;
  use crate::core::time::Time;

  #[derive(Default)]
  struct Menu {
    opened: u32,
  }
  impl Scene for Menu {}

  struct Level;
  impl Scene for Level {}

  fn world_handler(text: &'static str) -> Handler {
    Handler::World(Arc::new(
      move |_world: &mut World, cmd: &mut Command<'_>| {
        cmd.print(text);
        Ok(())
      },
    ))
  }

  /// A scene handler of `scope` that prints `text`
  fn scene_handler(scope: u64, text: &'static str) -> Handler {
    Handler::Scene {
      scope,
      call: erase(
        move |_menu: &mut Menu, _ctx: &mut SceneContext<'_>, cmd: &mut Command<'_>| {
          cmd.print(text);
          Ok(())
        },
      ),
    }
  }

  /// Call an erased handler with `scene`, and give its result and printed text
  fn call_scene(
    call: &ErasedSceneHandler,
    scene: &mut dyn Scene,
    name: &str,
  ) -> (Result<(), String>, Vec<String>) {
    let mut world = World::new();
    world.insert_resource(Time::new(60));
    world.insert_resource(Input::new());
    let mut slot = SceneSlot::new(Box::new(Level), 1);
    let mut cmd = Command::new(name, &[], None);
    let result = SceneContext::scoped(&mut world, &mut slot, |ctx| call(scene, ctx, &mut cmd));
    let lines = cmd.take_lines().into_iter().map(|(text, _)| text).collect();
    (result, lines)
  }

  /// Run the answering entry of `name` (a world or a menu's) and give what it printed
  fn run(commands: &ConsoleCommands, name: &str) -> Vec<String> {
    let Some((_, handler)) = commands.find(name) else {
      return vec!["(none)".to_string()];
    };
    match handler {
      Handler::World(call) => {
        let mut cmd = Command::new(name, &[], None);
        let result = call(&mut World::new(), &mut cmd);
        assert_eq!(result, Ok(()));
        cmd.take_lines().into_iter().map(|(text, _)| text).collect()
      }
      Handler::Scene { call, .. } => {
        let (result, lines) = call_scene(&*call, &mut Menu::default(), name);
        assert_eq!(result, Ok(()));
        lines
      }
      Handler::Builtin(_) => vec!["(builtin)".to_string()],
    }
  }

  fn scope_of(commands: &ConsoleCommands, name: &str) -> Option<u64> {
    match commands.find(name)?.1 {
      Handler::Scene { scope, .. } => Some(scope),
      _ => None,
    }
  }

  fn names(commands: &ConsoleCommands) -> Vec<String> {
    commands.list().into_iter().map(|spec| spec.name).collect()
  }

  #[test]
  fn names_are_lower_cased_so_any_spelling_runs() {
    let mut commands = ConsoleCommands::default();
    commands.add(
      CommandSpec::new("Game:Spawn", "spawn"),
      world_handler("spawned"),
    );
    assert_eq!(run(&commands, "game:spawn"), ["spawned"]);
    assert_eq!(run(&commands, "GAME:SPAWN"), ["spawned"]);
    assert_eq!(names(&commands), ["game:spawn"]);
    assert_eq!(
      commands
        .find("game:spawn")
        .map(|(spec, _)| spec.name)
        .as_deref(),
      Some("game:spawn")
    );
    assert!(commands.remove("GAME:spawn"));
    assert!(!commands.remove("game:spawn"));
    assert!(commands.find("game:spawn").is_none());
  }

  #[test]
  fn two_scopes_adding_one_name_stack_and_the_earlier_returns() {
    let mut commands = ConsoleCommands::default();
    commands.add(
      CommandSpec::new("game:reset", "lower"),
      scene_handler(1, "lower"),
    );
    commands.add(
      CommandSpec::new("game:reset", "upper"),
      scene_handler(2, "upper"),
    );
    assert_eq!(run(&commands, "game:reset"), ["upper"]);
    assert_eq!(scope_of(&commands, "game:reset"), Some(2));
    assert_eq!(commands.list()[0].description, "upper");

    commands.clear_scope(2);
    assert_eq!(run(&commands, "game:reset"), ["lower"]);
    assert_eq!(scope_of(&commands, "game:reset"), Some(1));
    assert_eq!(commands.list()[0].description, "lower");

    commands.clear_scope(1);
    assert!(commands.find("game:reset").is_none());
    assert!(commands.list().is_empty());
    assert!(commands.completions("").is_empty());

    // The scene below registering after the one above does not take over,
    // and a builder's command of the name sits under both
    commands.add(
      CommandSpec::new("game:reset", "upper"),
      scene_handler(4, "upper"),
    );
    commands.add(
      CommandSpec::new("game:reset", "lower"),
      scene_handler(3, "lower"),
    );
    commands.add(CommandSpec::new("game:reset", "run"), world_handler("run"));
    assert_eq!(run(&commands, "game:reset"), ["upper"]);
    commands.clear_scope(4);
    assert_eq!(run(&commands, "game:reset"), ["lower"]);
    commands.clear_scope(3);
    assert_eq!(run(&commands, "game:reset"), ["run"]);
  }

  #[test]
  fn a_scope_re_adding_its_own_name_replaces_it() {
    let mut commands = ConsoleCommands::default();
    commands.add(
      CommandSpec::new("game:paint", "first"),
      scene_handler(1, "first"),
    );
    commands.add(
      CommandSpec::new("GAME:PAINT", "second"),
      scene_handler(1, "second"),
    );
    assert_eq!(run(&commands, "game:paint"), ["second"]);
    assert_eq!(commands.list().len(), 1);
    commands.clear_scope(1);
    assert!(
      commands.find("game:paint").is_none(),
      "no earlier copy is left behind"
    );
  }

  #[test]
  fn clearing_a_scope_takes_only_its_own() {
    let mut commands = ConsoleCommands::default();
    commands.add(CommandSpec::new("a", "a"), scene_handler(1, "a"));
    commands.add(CommandSpec::new("b", "b"), scene_handler(2, "b"));
    commands.add(CommandSpec::new("c", "c"), world_handler("c"));
    commands.add(CommandSpec::new("shared", "two"), scene_handler(2, "two"));
    commands.add(CommandSpec::new("shared", "one"), scene_handler(1, "one"));
    commands.clear_scope(1);
    assert_eq!(names(&commands), ["b", "c", "shared"]);
    assert_eq!(run(&commands, "b"), ["b"]);
    assert_eq!(run(&commands, "c"), ["c"]);
    assert_eq!(run(&commands, "shared"), ["two"]);
  }

  #[test]
  fn the_list_shows_each_name_once_sorted() {
    let mut commands = ConsoleCommands::default();
    commands.add(CommandSpec::new("zeta", "z"), world_handler("z"));
    commands.add(
      CommandSpec::new("alpha", "lower"),
      scene_handler(1, "lower"),
    );
    commands.add(
      CommandSpec::new("alpha", "upper")
        .usage("alpha <n>")
        .help("Longer text."),
      scene_handler(2, "upper"),
    );
    commands.add(("Mid", "m").into(), world_handler("m"));
    let list = commands.list();
    let names: Vec<&str> = list.iter().map(|spec| spec.name.as_str()).collect();
    assert_eq!(names, ["alpha", "mid", "zeta"]);
    assert_eq!(list[0].description, "upper", "the answering entry");
    assert_eq!(list[0].usage.as_deref(), Some("alpha <n>"));
    assert_eq!(list[0].help.as_deref(), Some("Longer text."));
  }

  #[test]
  fn completions_are_the_names_with_the_prefix() {
    let mut commands = ConsoleCommands::default();
    commands.add(CommandSpec::new("game:spawn", ""), world_handler(""));
    commands.add(CommandSpec::new("game:reset", ""), scene_handler(1, ""));
    commands.add(CommandSpec::new("debug:fps", ""), world_handler(""));
    assert_eq!(commands.completions("game:"), ["game:reset", "game:spawn"]);
    assert_eq!(commands.completions("GAME:S"), ["game:spawn"]);
    assert_eq!(commands.completions("").len(), 3);
    assert!(commands.completions("x").is_empty());
  }

  #[test]
  fn an_erased_handler_refuses_another_scene_type() {
    let call = erase(
      |menu: &mut Menu, _ctx: &mut SceneContext<'_>, cmd: &mut Command<'_>| {
        menu.opened += 1;
        cmd.print("opened");
        Ok(())
      },
    );
    let mut menu = Menu::default();
    let (result, lines) = call_scene(&*call, &mut menu, "game:open");
    assert_eq!(result, Ok(()));
    assert_eq!(lines, ["opened"]);
    assert_eq!(menu.opened, 1, "called with the scene it was given");

    let (result, lines) = call_scene(&*call, &mut Level, "game:open");
    assert_eq!(
      result,
      Err("game:open was registered for another scene type".to_string())
    );
    assert!(lines.is_empty());
  }
}
