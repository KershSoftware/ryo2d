//! Running a line typed into the console.
//!
//! The line is split into words; the first, lower-cased, names the command.
//! Its entry is cloned out of the registry, so nothing of the registry is
//! borrowed while the handler runs: a handler may add and remove commands,
//! or change scenes, as it likes. A built-in runs with the world and the
//! engine's hooks, a builder's command with the world, and a scene's with
//! its scene and the scene context, through the scene host, as `update` is
//! called (so what it plays is that scene's).

use super::Console;
use super::colors;
use super::command::{Command, split};
use super::registry::{ConsoleCommands, EngineHooks, ErasedSceneHandler, Handler};
use crate::core::ecs::World;
use crate::core::engine::SceneHost;
use crate::core::scene::SceneAction;

/// Run `line` as if typed: the lines its command prints go to the console,
/// then its error in red. A scene action it asks for is applied to the
/// stack; a quit (asked for, or a stack left empty) sets `hooks.quit`. The
/// console echoes the line itself, before this.
pub(crate) fn run_line(world: &mut World, hooks: &mut EngineHooks<'_>, line: &str) {
  let words = split(line);
  let Some(name) = words.first().map(|word| word.to_lowercase()) else {
    return;
  };
  // The line `""` is one empty word
  if name.is_empty() {
    return;
  }
  let Some((spec, handler)) = world
    .get_resource::<ConsoleCommands>()
    .and_then(|commands| commands.find(&name))
  else {
    print(
      world,
      format!("Unknown command: {name}. Type 'help'."),
      colors::ERROR,
    );
    return;
  };

  let mut cmd = Command::new(&spec.name, &words[1..], spec.usage.as_deref());
  let mut from_below = false;
  let result = match handler {
    Handler::Builtin(call) => call(world, hooks, &mut cmd),
    Handler::World(call) => call(world, &mut cmd),
    Handler::Scene { scope, call } => match run_scene(world, scope, &*call, &mut cmd) {
      Ran::Scene { result, top } => {
        from_below = !top;
        result
      }
      Ran::Gone => {
        // An entry its scene did not take with it
        if let Some(mut commands) = world.get_resource_mut::<ConsoleCommands>() {
          commands.clear_scope(scope);
        }
        Err(format!("{name}: the scene that registered it has left"))
      }
      Ran::NoStack => Err(format!("{name}: there is no scene stack")),
    },
  };

  for (text, color) in cmd.take_lines() {
    print(world, text, color.unwrap_or(colors::TEXT));
  }
  if let Err(error) = result {
    print(world, error, colors::ERROR);
  }
  let action = cmd.take_action();
  if !matches!(action, SceneAction::Continue) {
    if from_below {
      // As with `update`: only the top scene changes the stack
      print(
        world,
        format!(
          "{name}: only the top scene's commands change scenes; its {} was dropped",
          action.kind()
        ),
        colors::ERROR,
      );
    } else {
      let kind = action.kind();
      match apply(world, action) {
        Applied::Done => {}
        Applied::Quit => *hooks.quit = true,
        Applied::Waiting => print(
          world,
          format!("{name}: a scene transition is running; its {kind} was dropped"),
          colors::ERROR,
        ),
      }
    }
  }
  if cmd.quit_requested() {
    *hooks.quit = true;
  }
}

/// What running a scene's handler came to
enum Ran {
  Scene {
    result: Result<(), String>,
    top: bool,
  },
  /// No scene on the stack has the scope
  Gone,
  NoStack,
}

/// Call `call` with the scene of `scope`, as that scene's code
fn run_scene(
  world: &mut World,
  scope: u64,
  call: &ErasedSceneHandler,
  cmd: &mut Command<'_>,
) -> Ran {
  let Some(mut host) = world.remove_non_send::<SceneHost>() else {
    return Ran::NoStack;
  };
  let ran = host
    .scenes
    .call_scope(scope, world, |scene, ctx| call(scene, ctx, cmd));
  world.insert_non_send(host);
  match ran {
    Some((result, top)) => Ran::Scene { result, top },
    None => Ran::Gone,
  }
}

/// What applying an action came to
enum Applied {
  Done,
  /// The game should quit: a quit, or a stack left empty
  Quit,
  /// A scene transition is running: the change was dropped, as the UI's is
  Waiting,
}

/// Apply `action` to the stack, as a scene's `update` action is. A scene
/// change waits out a running transition as the UI's does: only a quit gets
/// through.
fn apply(world: &mut World, action: SceneAction) -> Applied {
  let Some(mut host) = world.remove_non_send::<SceneHost>() else {
    return match action {
      SceneAction::Quit => Applied::Quit,
      _ => Applied::Done,
    };
  };
  let scenes = &mut host.scenes;
  let applied = if scenes.is_transitioning() && !matches!(action, SceneAction::Quit) {
    Applied::Waiting
  } else if scenes.handle_action(action, world) {
    scenes.sync_pauses(world);
    Applied::Done
  } else {
    scenes.sync_pauses(world);
    Applied::Quit
  };
  world.insert_non_send(host);
  applied
}

fn print(world: &mut World, text: String, color: crate::core::ui::Color) {
  if let Some(mut console) = world.get_resource_mut::<Console>() {
    console.print_colored(text, color);
  }
}
