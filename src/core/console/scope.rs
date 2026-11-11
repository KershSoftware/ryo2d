//! What `ctx.console()` gives a scene: its commands, and the console's
//! output.

use super::command::{Command, CommandSpec};
use crate::core::ecs::World;
use crate::core::scene::{Scene, SceneContext};
use crate::core::ui::Color;

/// A scene's way into the console, from `ctx.console()`. Its commands run
/// with the scene that registered them and leave with it.
///
/// In a build without the developer tools there is no console: every call
/// compiles and does nothing.
pub struct ConsoleScope<'a> {
  #[cfg(dev_tools)]
  world: &'a mut World,
  #[cfg(dev_tools)]
  scope: u64,
  /// A build without tools has no console to reach
  #[cfg(not(dev_tools))]
  world: std::marker::PhantomData<&'a mut World>,
}

impl<'a> ConsoleScope<'a> {
  /// The console as seen by the scene of `scope`
  #[cfg(dev_tools)]
  pub(crate) fn new(world: &'a mut World, scope: u64) -> Self {
    Self { world, scope }
  }

  /// No console: a build without tools
  #[cfg(not(dev_tools))]
  pub(crate) fn new(_world: &'a mut World, _scope: u64) -> Self {
    Self {
      world: std::marker::PhantomData,
    }
  }
}

impl ConsoleScope<'_> {
  /// Register a command for this scene, of type `S`. Its handler is called
  /// with the scene itself, the scene context `update` gets and the
  /// command, and goes when the scene leaves the stack. Registering a name
  /// this scene registered already replaces its handler. When two scenes on
  /// the stack register one name, the upper one answers, and the lower
  /// one's is back once the upper leaves. A name that is empty or has a
  /// space is refused with a warning.
  ///
  /// ```
  /// use ryo2d::prelude::*;
  ///
  /// struct Game {
  ///   lives: u32,
  /// }
  ///
  /// impl Scene for Game {
  ///   fn on_enter(&mut self, ctx: &mut SceneContext) {
  ///     ctx.console().command(
  ///       CommandSpec::new("game:lives", "Set the lives left").usage("game:lives <count>"),
  ///       |game: &mut Game, _ctx, cmd| {
  ///         game.lives = cmd.arg(0)?;
  ///         cmd.print(format!("{} lives", game.lives));
  ///         Ok(())
  ///       },
  ///     );
  ///   }
  /// }
  /// ```
  pub fn command<S: Scene>(
    &mut self,
    spec: impl Into<CommandSpec>,
    handler: impl Fn(&mut S, &mut SceneContext<'_>, &mut Command<'_>) -> Result<(), String>
    + Send
    + Sync
    + 'static,
  ) {
    #[cfg(dev_tools)]
    {
      use super::registry::{ConsoleCommands, Handler, erase, valid_name};
      let spec = spec.into();
      if !valid_name(&spec.name) {
        return;
      }
      if let Some(mut commands) = self.world.get_resource_mut::<ConsoleCommands>() {
        let call = erase(handler);
        commands.add(
          spec,
          Handler::Scene {
            scope: self.scope,
            call,
          },
        );
      }
    }
    #[cfg(not(dev_tools))]
    let _ = (spec, handler);
  }

  /// Remove the command `name`, whoever registered it (a built-in
  /// included); whether there was one.
  pub fn remove(&mut self, name: &str) -> bool {
    #[cfg(dev_tools)]
    {
      self
        .world
        .get_resource_mut::<super::registry::ConsoleCommands>()
        .is_some_and(|mut commands| commands.remove(name))
    }
    #[cfg(not(dev_tools))]
    {
      let _ = name;
      false
    }
  }

  /// Print a line to the console, at once. From inside a command's handler
  /// it lands before the lines the handler prints through its `Command`,
  /// which go to the console when the handler returns.
  pub fn print(&mut self, text: impl Into<String>) {
    #[cfg(dev_tools)]
    self.print_colored(text, super::colors::TEXT);
    #[cfg(not(dev_tools))]
    let _ = text;
  }

  /// Print a line to the console in `color`, at once (as [`print`](Self::print)).
  pub fn print_colored(&mut self, text: impl Into<String>, color: Color) {
    #[cfg(dev_tools)]
    if let Some(mut console) = self.world.get_resource_mut::<super::Console>() {
      console.print_colored(text, color);
    }
    #[cfg(not(dev_tools))]
    let _ = (text, color);
  }
}
